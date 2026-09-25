//! avash: tiny AV1 image placeholders.
//!
//! An avash is a string wrapping a single AV1 still picture with the container,
//! OBU framing and every header field a decoder can regenerate stripped off. The
//! bytes are written as one big-endian integer in base 88 over an alphabet that
//! is safe inside HTML attributes and JSON strings. The bytes are a bitstream:
//!
//! ```text
//! 2  version = 2
//! 2  mode
//! mode 1, rav1e's sequence and frame headers regenerated:
//!   1  loop restoration enabled
//!   8  width - 1
//!   8  height - 1
//!   8  base_q_idx
//!   7  DeltaQYDc, 7 DeltaQUDc, 7 DeltaQUAc, 7 DeltaQVDc, 7 DeltaQVAc
//!   6  loop_filter_level[0], 6 loop_filter_level[1]
//!   6  loop_filter_level[2], 6 loop_filter_level[3], only if either above is nonzero
//!   .. padding to a byte, then the tile data verbatim
//! mode 2, foreign encoder: both headers stored
//!   4  padding
//!   8  sequence header length
//!   .. OBU_SEQUENCE_HEADER payload, then the OBU_FRAME payload verbatim
//! ```
//!
//! The regenerated headers are [`bits::canonical_sequence_header`] and
//! [`bits::canonical_frame_header`], the ones rav1e emits; mode 1 stores their
//! 35 or so bytes in 10 or 11. [`to_av1`] and [`to_avif`] rebuild a stream stock
//! decoders accept.

pub mod bits;
mod color;
#[cfg(feature = "dav1d")]
mod dav1d_backend;
mod rav1e_backend;

use std::fmt;

pub const VERSION: u8 = 2;

const MODE_CANONICAL: u8 = 1;
const MODE_EXPLICIT: u8 = 2;

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
    Codec(String),
    InvalidInput(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed avash: {m}"),
            Error::Codec(m) => write!(f, "codec error: {m}"),
            Error::InvalidInput(m) => write!(f, "invalid input: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Wiener taps `[luma, chroma]`: the outer three of a symmetric 7-tap filter
/// summing to 128. Baked into the bitstream, so any AV1 decoder applies them.
pub const BLUR_LIGHT: [[i8; 3]; 2] = [[0, 0, 16], [0, 0, 16]];
pub const BLUR_MEDIUM: [[i8; 3]; 2] = [[0, 8, 32], [0, 8, 32]];
pub const BLUR_STRONG: [[i8; 3]; 2] = [[6, 8, 26], [0, 8, 32]];

/// Encoder settings.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Long edge of the encoded frame in pixels, 1..=256. Default 48.
    pub size: u32,
    /// Encoding quality, 0 (smallest) ..= 63 (best). Default 23.
    pub quality: u8,
    /// Loop-restoration blur hiding block edges; `None` for no filter. Default [`BLUR_MEDIUM`].
    pub blur: Option<[[i8; 3]; 2]>,
}

impl Default for Options {
    fn default() -> Self {
        Self { size: 48, quality: 23, blur: Some(BLUR_MEDIUM) }
    }
}

/// Decoded avash pixels at the encoded frame size.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Encodes an RGBA8 image into an avash string.
pub fn encode(rgba: &[u8], width: u32, height: u32, opts: &Options) -> Result<String, Error> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(Error::InvalidInput("rgba length does not match dimensions"));
    }
    if !(2..=128).contains(&opts.size) {
        return Err(Error::InvalidInput("size must be 2..=128"));
    }
    if opts.quality > 63 {
        return Err(Error::InvalidInput("quality must be 0..=63"));
    }
    let scale = |a: u32, b: u32| ((a as u64 * opts.size as u64 + b as u64 / 2) / b as u64).max(1) as u32;
    let (dw, dh) = if width >= height { (opts.size.min(width), scale(height, width)) } else { (scale(width, height), opts.size.min(height)) };
    let even = |d: u32| (d + 1) & !1;
    let (dw, dh) = (even(dw), even(dh));
    let rgb = color::downsample(rgba, width, height, dw, dh);
    let yuv = color::rgb_to_yuv420(&rgb, dw, dh);
    let obus = rav1e_backend::encode(&yuv, 63 - opts.quality, opts.blur)?;
    Ok(to_base88(&pack(&obus)?))
}

/// Decodes an avash string to RGBA8 pixels at its native (tiny) size.
#[cfg(feature = "dav1d")]
pub fn decode(hash: &str) -> Result<Image, Error> {
    let yuv = dav1d_backend::decode(&to_av1(hash)?)?;
    Ok(Image { width: yuv.width, height: yuv.height, rgba: color::yuv420_to_rgba(&yuv) })
}

/// Encoded frame dimensions without decoding pixels.
pub fn dimensions(hash: &str) -> Result<(u32, u32), Error> {
    let p = split(&from_base88(hash)?)?;
    bits::sequence_header_dimensions(&p.seq)
}

/// Reconstructs a complete raw AV1 OBU stream (temporal delimiter, sequence header, frame).
pub fn to_av1(hash: &str) -> Result<Vec<u8>, Error> {
    let p = split(&from_base88(hash)?)?;
    let mut out = bits::obu(bits::OBU_TEMPORAL_DELIMITER, &[]);
    out.extend(bits::obu(bits::OBU_SEQUENCE_HEADER, &p.seq));
    out.extend(bits::obu(bits::OBU_FRAME, &p.frame));
    Ok(out)
}

/// Wraps the frame in a minimal AVIF container, decodable by any AVIF-capable image pipeline.
pub fn to_avif(hash: &str) -> Result<Vec<u8>, Error> {
    let p = split(&from_base88(hash)?)?;
    let (w, h) = bits::sequence_header_dimensions(&p.seq)?;
    let seq_obu = bits::obu(bits::OBU_SEQUENCE_HEADER, &p.seq);
    let item = [seq_obu.clone(), bits::obu(bits::OBU_FRAME, &p.frame)].concat();

    let ftyp = bx(b"ftyp", &[&b"avif"[..], &0u32.to_be_bytes(), b"avif", b"mif1", b"miaf"].concat());
    let hdlr = fullbox(b"hdlr", 0, &[&[0u8; 4][..], b"pict", &[0u8; 12], &[0u8]].concat());
    let pitm = fullbox(b"pitm", 0, &1u16.to_be_bytes());
    let infe = fullbox(b"infe", 2, &[&1u16.to_be_bytes()[..], &0u16.to_be_bytes(), b"av01", &[0u8]].concat());
    let iinf = fullbox(b"iinf", 0, &[&1u16.to_be_bytes()[..], &infe].concat());
    let ispe = fullbox(b"ispe", 0, &[w.to_be_bytes(), h.to_be_bytes()].concat());
    let pixi = fullbox(b"pixi", 0, &[3, 8, 8, 8]);
    let av1c = bx(b"av1C", &[&[0x81, 0x00, 0x0c, 0x00][..], &seq_obu].concat());
    let colr = bx(b"colr", &[b"nclx", &bits::COLOR_PRIMARIES.to_be_bytes()[..], &bits::TRANSFER_CHARACTERISTICS.to_be_bytes(), &bits::MATRIX_COEFFICIENTS.to_be_bytes(), &[0x80]].concat());
    let ipco = bx(b"ipco", &[ispe, pixi, av1c, colr].concat());
    let ipma = fullbox(b"ipma", 0, &[&1u32.to_be_bytes()[..], &1u16.to_be_bytes(), &[4, 0x01, 0x02, 0x83, 0x04]].concat());
    let iprp = bx(b"iprp", &[ipco, ipma].concat());
    let iloc = |offset: u32| fullbox(b"iloc", 0, &[&[0x44, 0x00][..], &1u16.to_be_bytes(), &1u16.to_be_bytes(), &0u16.to_be_bytes(), &1u16.to_be_bytes(), &offset.to_be_bytes(), &(item.len() as u32).to_be_bytes()].concat());
    let meta_len = fullbox(b"meta", 0, &[hdlr.clone(), pitm.clone(), iloc(0), iinf.clone(), iprp.clone()].concat()).len();
    let offset = (ftyp.len() + meta_len + 8) as u32;
    let meta = fullbox(b"meta", 0, &[hdlr, pitm, iloc(offset), iinf, iprp].concat());
    Ok([ftyp, meta, bx(b"mdat", &item)].concat())
}

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [&((payload.len() + 8) as u32).to_be_bytes()[..], kind, payload].concat()
}

fn fullbox(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[version, 0, 0, 0][..], payload].concat())
}

struct Parts {
    seq: Vec<u8>,
    frame: Vec<u8>,
}

fn split(bytes: &[u8]) -> Result<Parts, Error> {
    if bytes.len() < 4 {
        return Err(Error::Malformed("too short"));
    }
    let version = bytes[0] >> 6;
    if version == 0 || version > VERSION {
        return Err(Error::Malformed("unsupported version"));
    }
    let (seq, frame) = match (bytes[0] >> 4) & 3 {
        // version 1 stored the frame whole after a byte-padded prefix
        MODE_CANONICAL if version == 1 => {
            let restoration = (bytes[0] >> 3) & 1 == 1;
            (bits::canonical_sequence_header(bytes[1] as u32 + 1, bytes[2] as u32 + 1, restoration), bytes[3..].to_vec())
        }
        MODE_CANONICAL => {
            let mut r = bits::BitReader::new(bytes);
            r.get(4)?;
            let restoration = r.get(1)? == 1;
            let (w, h) = (r.get(8)? + 1, r.get(8)? + 1);
            let base_q_idx = r.get(8)? as u8;
            let mut delta_q = [0i8; 5];
            for d in &mut delta_q {
                *d = ((r.get(7)? as u8) << 1) as i8 >> 1;
            }
            let mut lf = [r.get(6)? as u8, r.get(6)? as u8, 0, 0];
            if lf[0] | lf[1] != 0 {
                lf[2] = r.get(6)? as u8;
                lf[3] = r.get(6)? as u8;
            }
            r.align();
            let params = bits::FrameParams { base_q_idx, delta_q, loop_filter: lf };
            let mut frame = bits::canonical_frame_header(w, h, restoration, &params);
            let tile = bytes.get(r.pos() / 8..).ok_or(Error::Malformed("truncated header"))?;
            if tile.is_empty() {
                return Err(Error::Malformed("missing tile data"));
            }
            frame.extend_from_slice(tile);
            (bits::canonical_sequence_header(w, h, restoration), frame)
        }
        MODE_EXPLICIT => {
            let n = bytes[1] as usize;
            let seq = bytes.get(2..2 + n).ok_or(Error::Malformed("truncated sequence header"))?;
            (seq.to_vec(), bytes[2 + n..].to_vec())
        }
        _ => return Err(Error::Malformed("unknown mode")),
    };
    if frame.is_empty() {
        return Err(Error::Malformed("missing frame"));
    }
    Ok(Parts { seq, frame })
}

/// Packs a raw OBU stream (from any conforming encoder) into avash bytes.
pub fn pack(obus: &[u8]) -> Result<Vec<u8>, Error> {
    let mut seq = None;
    let mut frame = None;
    for obu in bits::parse_obus(obus)? {
        match obu.kind {
            bits::OBU_TEMPORAL_DELIMITER | bits::OBU_PADDING | bits::OBU_METADATA => {}
            bits::OBU_SEQUENCE_HEADER if seq.is_none() => seq = Some(obu.payload),
            bits::OBU_FRAME if frame.is_none() => frame = Some(obu.payload),
            _ => return Err(Error::Codec("unexpected OBU in stream".into())),
        }
    }
    let seq = seq.ok_or(Error::Codec("no sequence header".into()))?;
    let frame = frame.ok_or(Error::Codec("no frame".into()))?;
    if seq.len() > 255 {
        return Err(Error::Codec("sequence header too long".into()));
    }
    let (w, h) = bits::sequence_header_dimensions(seq)?;
    if let Some((restoration, params, tile)) = canonical(seq, frame, w, h) {
        let mut out = bits::BitWriter::new();
        out.put(2, VERSION as u32);
        out.put(2, MODE_CANONICAL as u32);
        out.put(1, restoration as u32);
        out.put(8, w - 1);
        out.put(8, h - 1);
        out.put(8, params.base_q_idx as u32);
        for d in params.delta_q {
            out.put(7, (d as u8 & 0x7f) as u32);
        }
        let lf = params.loop_filter;
        out.put(6, lf[0] as u32);
        out.put(6, lf[1] as u32);
        if lf[0] | lf[1] != 0 {
            out.put(6, lf[2] as u32);
            out.put(6, lf[3] as u32);
        }
        out.align();
        let mut out = out.finish();
        out.extend_from_slice(tile);
        return Ok(out);
    }
    let mut out = vec![(VERSION << 6) | (MODE_EXPLICIT << 4), seq.len() as u8];
    out.extend_from_slice(seq);
    out.extend_from_slice(frame);
    Ok(out)
}

/// Restoration flag, frame parameters and tile data when both headers are the
/// canonical rav1e ones and so can be regenerated.
fn canonical<'a>(seq: &[u8], frame: &'a [u8], w: u32, h: u32) -> Option<(bool, bits::FrameParams, &'a [u8])> {
    if w > 256 || h > 256 {
        return None;
    }
    let restoration = [false, true].into_iter().find(|&r| seq == bits::canonical_sequence_header(w, h, r))?;
    let (params, tile_start) = bits::canonical_frame_params(frame, w, h, restoration).ok()?;
    if tile_start >= frame.len() || frame[..tile_start] != bits::canonical_frame_header(w, h, restoration, &params) {
        return None;
    }
    Some((restoration, params, &frame[tile_start..]))
}

/// Printable ASCII minus space, `"`, `&`, `'`, `<`, `>` and `\`: safe in HTML attributes and JSON strings.
pub const ALPHABET: &[u8; 88] = b"!#$%()*+,-./0123456789:;=?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_`abcdefghijklmnopqrstuvwxyz{|}~";

/// Writes avash bytes as a base-88 string.
pub fn to_base88(bytes: &[u8]) -> String {
    let mut digits: Vec<u32> = bytes.iter().map(|&b| b as u32).collect();
    let mut out = Vec::with_capacity(bytes.len() * 5 / 4 + 1);
    while !digits.is_empty() {
        let mut rem = 0u32;
        let mut next = Vec::with_capacity(digits.len());
        for &d in &digits {
            let cur = (rem << 8) | d;
            let q = cur / 88;
            rem = cur % 88;
            if q != 0 || !next.is_empty() {
                next.push(q);
            }
        }
        out.push(ALPHABET[rem as usize]);
        digits = next;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// Reads avash bytes back out of a base-88 string.
pub fn from_base88(s: &str) -> Result<Vec<u8>, Error> {
    let mut bytes: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c).ok_or(Error::Malformed("bad character"))? as u32;
        let mut carry = v;
        for b in bytes.iter_mut().rev() {
            let cur = *b * 88 + carry;
            *b = cur & 255;
            carry = cur >> 8;
        }
        while carry != 0 {
            bytes.insert(0, carry & 255);
            carry >>= 8;
        }
    }
    Ok(bytes.into_iter().map(|b| b as u8).collect())
}

#[cfg(feature = "wasm")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn encode(rgba: &[u8], width: u32, height: u32, size: u32, quality: u8, blur: u8) -> Result<String, JsError> {
        let blur = [None, Some(super::BLUR_LIGHT), Some(super::BLUR_MEDIUM), Some(super::BLUR_STRONG)].get(blur as usize).copied().ok_or_else(|| JsError::new("blur must be 0..=3"))?;
        super::encode(rgba, width, height, &super::Options { size, quality, blur }).map_err(|e| JsError::new(&e.to_string()))
    }

    #[wasm_bindgen(js_name = toAvif)]
    pub fn to_avif(hash: &str) -> Result<Vec<u8>, JsError> {
        super::to_avif(hash).map_err(|e| JsError::new(&e.to_string()))
    }
}
