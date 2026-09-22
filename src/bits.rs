use crate::Error;

pub fn leb128(mut v: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

fn read_leb128(data: &[u8], pos: &mut usize) -> Result<usize, Error> {
    let mut value = 0usize;
    for i in 0..8 {
        let byte = *data.get(*pos).ok_or(Error::Malformed("truncated OBU size"))?;
        *pos += 1;
        value |= ((byte & 0x7f) as usize) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(Error::Malformed("OBU size too long"))
}

pub const OBU_SEQUENCE_HEADER: u8 = 1;
pub const OBU_TEMPORAL_DELIMITER: u8 = 2;
pub const OBU_METADATA: u8 = 5;
pub const OBU_PADDING: u8 = 15;
pub const OBU_FRAME: u8 = 6;

pub struct Obu<'a> {
    pub kind: u8,
    pub payload: &'a [u8],
}

pub fn parse_obus(data: &[u8]) -> Result<Vec<Obu<'_>>, Error> {
    let mut pos = 0;
    let mut obus = Vec::new();
    while pos < data.len() {
        let header = data[pos];
        pos += 1;
        if header & 0x80 != 0 {
            return Err(Error::Malformed("OBU forbidden bit set"));
        }
        let kind = (header >> 3) & 0xf;
        if header & 0x04 != 0 {
            pos += 1;
        }
        if header & 0x02 == 0 {
            return Err(Error::Malformed("OBU without size field"));
        }
        let size = read_leb128(data, &mut pos)?;
        let payload = data.get(pos..pos + size).ok_or(Error::Malformed("truncated OBU"))?;
        pos += size;
        obus.push(Obu { kind, payload });
    }
    Ok(obus)
}

pub fn obu(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![(kind << 3) | 0x02];
    out.extend(leb128(payload.len()));
    out.extend_from_slice(payload);
    out
}

pub const COLOR_PRIMARIES: u16 = 1;
pub const TRANSFER_CHARACTERISTICS: u16 = 13;
pub const MATRIX_COEFFICIENTS: u16 = 6;

pub struct BitWriter {
    out: Vec<u8>,
    nbits: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self { out: Vec::new(), nbits: 0 }
    }

    pub fn put(&mut self, n: u32, value: u32) {
        for i in (0..n).rev() {
            if self.nbits.is_multiple_of(8) {
                self.out.push(0);
            }
            let last = self.out.len() - 1;
            self.out[last] |= (((value >> i) & 1) as u8) << (7 - self.nbits % 8);
            self.nbits += 1;
        }
    }

    pub fn bits(&self) -> usize {
        self.nbits
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn get(&mut self, n: u32) -> Result<u32, Error> {
        let mut v = 0;
        for _ in 0..n {
            let byte = *self.data.get(self.pos / 8).ok_or(Error::Malformed("truncated bitstream"))?;
            v = (v << 1) | ((byte >> (7 - self.pos % 8)) & 1) as u32;
            self.pos += 1;
        }
        Ok(v)
    }

    pub fn pos(&self) -> usize {
        self.pos
    }
}

/// Frame size from a reduced still-picture sequence header payload.
pub fn sequence_header_dimensions(seq: &[u8]) -> Result<(u32, u32), Error> {
    let mut r = BitReader::new(seq);
    let profile = r.get(3)?;
    let still = r.get(1)?;
    let reduced = r.get(1)?;
    if profile != 0 || still != 1 || reduced != 1 {
        return Err(Error::Malformed("not a profile 0 reduced still picture"));
    }
    r.get(5)?; // seq_level_idx
    let wb = r.get(4)? + 1;
    let hb = r.get(4)? + 1;
    let w = r.get(wb)? + 1;
    let h = r.get(hb)? + 1;
    Ok((w, h))
}

/// The sequence header this crate's encoders emit for a frame of the given size:
/// profile 0, reduced still picture, 128x128 superblocks, filter-intra and intra-edge
/// on, superres/CDEF/restoration off, 8-bit 4:2:0, sRGB primaries and transfer,
/// BT.601 matrix, full range.
pub fn canonical_sequence_header(width: u32, height: u32) -> Vec<u8> {
    let dim_bits = |d: u32| (32 - (d - 1).leading_zeros()).max(1);
    let (wb, hb) = (dim_bits(width), dim_bits(height));
    let mut w = BitWriter::new();
    w.put(3, 0); // seq_profile
    w.put(1, 1); // still_picture
    w.put(1, 1); // reduced_still_picture_header
    w.put(5, 0); // seq_level_idx[0]
    w.put(4, wb - 1);
    w.put(4, hb - 1);
    w.put(wb, width - 1);
    w.put(hb, height - 1);
    w.put(1, 1); // use_128x128_superblock
    w.put(1, 1); // enable_filter_intra
    w.put(1, 1); // enable_intra_edge_filter
    w.put(1, 0); // enable_superres
    w.put(1, 0); // enable_cdef
    w.put(1, 0); // enable_restoration
    w.put(1, 0); // high_bitdepth
    w.put(1, 0); // mono_chrome
    w.put(1, 1); // color_description_present_flag
    w.put(8, COLOR_PRIMARIES as u32);
    w.put(8, TRANSFER_CHARACTERISTICS as u32);
    w.put(8, MATRIX_COEFFICIENTS as u32);
    w.put(1, 1); // color_range
    w.put(2, 0); // chroma_sample_position
    w.put(1, 0); // separate_uv_delta_q
    w.put(1, 0); // film_grain_params_present
    w.put(1, 1); // trailing bit
    w.finish()
}

/// The three fields of an uncompressed frame header that vary between avash
/// frames. Everything else is fixed by the sequence header and the encoder
/// settings, so it is regenerated rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub base_q_idx: u8,
    pub tx_mode_select: bool,
    /// Deblocking strength, applied to all four loop filter levels.
    pub loop_filter_level: u8,
}

/// Writes the uncompressed frame header for a key frame under
/// [`canonical_sequence_header`]: no screen-content tools, one tile, no
/// segmentation or delta quantisers, CDEF and loop restoration absent.
pub fn canonical_frame_header(f: FrameHeader) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(1, 0); // disable_cdf_update
    w.put(1, 0); // allow_screen_content_tools
    w.put(1, 0); // render_and_frame_size_different
    w.put(1, 1); // uniform_tile_spacing_flag: one tile
    w.put(8, f.base_q_idx as u32);
    w.put(1, 0); // DeltaQYDc coded
    w.put(1, 0); // DeltaQUDc coded
    w.put(1, 0); // DeltaQUAc coded
    w.put(1, 0); // using_qmatrix
    w.put(1, 0); // segmentation_enabled
    w.put(1, 0); // delta_q_present
    let lf = f.loop_filter_level as u32;
    w.put(6, lf);
    w.put(6, lf);
    if lf != 0 {
        w.put(6, lf);
        w.put(6, lf);
    }
    w.put(3, 0); // loop_filter_sharpness
    w.put(1, 1); // loop_filter_delta_enabled
    w.put(1, 0); // loop_filter_delta_update
    w.put(1, f.tx_mode_select as u32);
    w.put(1, 0); // reduced_tx_set
    w.finish()
}

/// Reads the uncompressed frame header of an `OBU_FRAME` payload, returning the
/// varying fields and the byte length of the header (the tile group follows,
/// byte aligned). `None` when any field departs from [`canonical_frame_header`],
/// in which case the frame has to be stored verbatim.
pub fn parse_frame_header(frame: &[u8], width: u32, height: u32) -> Option<(FrameHeader, usize)> {
    if width > 128 || height > 128 {
        return None; // a second superblock column or row would add tile bits
    }
    let mut r = BitReader::new(frame);
    let zero = |r: &mut BitReader<'_>| r.get(1).ok().filter(|&b| b == 0);
    zero(&mut r)?; // disable_cdf_update
    zero(&mut r)?; // allow_screen_content_tools
    zero(&mut r)?; // render_and_frame_size_different
    r.get(1).ok().filter(|&b| b == 1)?; // uniform_tile_spacing_flag
    let base_q_idx = r.get(8).ok()? as u8;
    if base_q_idx == 0 {
        return None; // lossless: a different header and tile layout
    }
    for _ in 0..3 {
        zero(&mut r)?; // DeltaQYDc, DeltaQUDc, DeltaQUAc coded
    }
    zero(&mut r)?; // using_qmatrix
    zero(&mut r)?; // segmentation_enabled
    zero(&mut r)?; // delta_q_present
    let level = r.get(6).ok()?;
    let level1 = r.get(6).ok()?;
    if level != 0 || level1 != 0 {
        r.get(6).ok()?; // loop filter levels for U and V
        r.get(6).ok()?;
    }
    zero(&mut r)?; // loop_filter_sharpness is 3 bits
    zero(&mut r)?;
    zero(&mut r)?;
    r.get(1).ok().filter(|&b| b == 1)?; // loop_filter_delta_enabled
    zero(&mut r)?; // loop_filter_delta_update
    let tx_mode_select = r.get(1).ok()? == 1;
    zero(&mut r)?; // reduced_tx_set
    let header = FrameHeader { base_q_idx, tx_mode_select, loop_filter_level: level as u8 };
    let len = r.pos().div_ceil(8);
    (len < frame.len()).then_some((header, len))
}
