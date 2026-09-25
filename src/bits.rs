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

    pub fn align(&mut self) {
        while !self.nbits.is_multiple_of(8) {
            self.put(1, 0);
        }
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

    pub fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
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

/// The sequence header rav1e emits for a still picture of the given size: profile
/// 0, reduced still picture, level 31, 64x64 superblocks, intra-edge filter on,
/// filter-intra/superres/CDEF off, loop restoration per `restoration`, 8-bit
/// 4:2:0, sRGB primaries and transfer, BT.601 matrix, full range, separate UV
/// delta q.
pub fn canonical_sequence_header(width: u32, height: u32, restoration: bool) -> Vec<u8> {
    let dim_bits = |d: u32| (32 - (d - 1).leading_zeros()).max(1);
    let (wb, hb) = (dim_bits(width), dim_bits(height));
    let mut w = BitWriter::new();
    w.put(3, 0); // seq_profile
    w.put(1, 1); // still_picture
    w.put(1, 1); // reduced_still_picture_header
    w.put(5, 31); // seq_level_idx[0]
    w.put(4, wb - 1);
    w.put(4, hb - 1);
    w.put(wb, width - 1);
    w.put(hb, height - 1);
    w.put(1, 0); // use_128x128_superblock
    w.put(1, 0); // enable_filter_intra
    w.put(1, 1); // enable_intra_edge_filter
    w.put(1, 0); // enable_superres
    w.put(1, 0); // enable_cdef
    w.put(1, restoration as u32); // enable_restoration
    w.put(1, 0); // high_bitdepth
    w.put(1, 0); // mono_chrome
    w.put(1, 1); // color_description_present_flag
    w.put(8, COLOR_PRIMARIES as u32);
    w.put(8, TRANSFER_CHARACTERISTICS as u32);
    w.put(8, MATRIX_COEFFICIENTS as u32);
    w.put(1, 1); // color_range
    w.put(2, 0); // chroma_sample_position
    w.put(1, 1); // separate_uv_delta_q
    w.put(1, 0); // film_grain_params_present
    w.put(1, 1); // trailing bit
    w.finish()
}

/// Fields of the frame header that vary between rav1e still pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameParams {
    pub base_q_idx: u8,
    /// DeltaQYDc, DeltaQUDc, DeltaQUAc, DeltaQVDc, DeltaQVAc.
    pub delta_q: [i8; 5],
    /// Deblocking levels: luma vertical, luma horizontal, U, V.
    pub loop_filter: [u8; 4],
}

/// The uncompressed frame header rav1e emits for a still picture, byte aligned
/// so the tile data follows directly: CDF updates on, no screen content tools,
/// no render size, one uniform tile, `params`, no quantiser matrices,
/// segmentation or delta q, sharpness 0, no filter deltas, Wiener restoration
/// on every plane in one 256 pixel unit when `restoration`, tx mode select on,
/// full transform set.
pub fn canonical_frame_header(width: u32, height: u32, restoration: bool, params: &FrameParams) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(1, 0); // disable_cdf_update
    w.put(1, 0); // allow_screen_content_tools
    w.put(1, 0); // render_and_frame_size_different
    w.put(1, 1); // uniform_tile_spacing_flag
    if width > 64 {
        w.put(1, 0); // increment_tile_cols_log2
    }
    if height > 64 {
        w.put(1, 0); // increment_tile_rows_log2
    }
    w.put(8, params.base_q_idx as u32);
    let [ydc, udc, uac, vdc, vac] = params.delta_q;
    let delta_q = |w: &mut BitWriter, d: i8| {
        w.put(1, (d != 0) as u32); // delta_coded
        if d != 0 {
            w.put(7, (d as u8 & 0x7f) as u32);
        }
    };
    delta_q(&mut w, ydc);
    let diff_uv = udc != vdc || uac != vac;
    w.put(1, diff_uv as u32); // diff_uv_delta
    delta_q(&mut w, udc);
    delta_q(&mut w, uac);
    if diff_uv {
        delta_q(&mut w, vdc);
        delta_q(&mut w, vac);
    }
    w.put(1, 0); // using_qmatrix
    w.put(1, 0); // segmentation_enabled
    w.put(1, 0); // delta_q_present
    let lf = params.loop_filter;
    w.put(6, lf[0] as u32);
    w.put(6, lf[1] as u32);
    if lf[0] | lf[1] != 0 {
        w.put(6, lf[2] as u32);
        w.put(6, lf[3] as u32);
    }
    w.put(3, 0); // loop_filter_sharpness
    w.put(1, 0); // loop_filter_delta_enabled
    if restoration {
        w.put(6, 0b101010); // lr_type: RESTORE_WIENER x3
        w.put(1, 1); // lr_unit_shift
        w.put(1, 1); // lr_unit_extra_shift
        w.put(1, 0); // lr_uv_shift
    }
    w.put(1, 1); // tx_mode_select
    w.put(1, 0); // reduced_tx_set
    w.align();
    w.finish()
}

/// Reads the variable fields out of an OBU_FRAME payload laid out like
/// [`canonical_frame_header`], returning them with the tile data offset. The
/// caller regenerates and compares; constant bits are not checked here.
pub fn canonical_frame_params(frame: &[u8], width: u32, height: u32, restoration: bool) -> Result<(FrameParams, usize), Error> {
    let mut r = BitReader::new(frame);
    r.get(4 + (width > 64) as u32 + (height > 64) as u32)?;
    let base_q_idx = r.get(8)? as u8;
    if base_q_idx == 0 {
        return Err(Error::Malformed("lossless frame"));
    }
    let delta_q = |r: &mut BitReader| -> Result<i8, Error> { Ok(if r.get(1)? == 1 { ((r.get(7)? as u8) << 1) as i8 >> 1 } else { 0 }) };
    let ydc = delta_q(&mut r)?;
    let diff_uv = r.get(1)? == 1;
    let (udc, uac) = (delta_q(&mut r)?, delta_q(&mut r)?);
    let (vdc, vac) = if diff_uv { (delta_q(&mut r)?, delta_q(&mut r)?) } else { (udc, uac) };
    r.get(3)?;
    let mut lf = [r.get(6)? as u8, r.get(6)? as u8, 0, 0];
    if lf[0] | lf[1] != 0 {
        lf[2] = r.get(6)? as u8;
        lf[3] = r.get(6)? as u8;
    }
    r.get(4 + if restoration { 9 } else { 0 } + 2)?;
    r.align();
    Ok((FrameParams { base_q_idx, delta_q: [ydc, udc, uac, vdc, vac], loop_filter: lf }, r.pos() / 8))
}
