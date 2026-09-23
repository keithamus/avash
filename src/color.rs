pub struct Yuv420 {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl Yuv420 {
    pub fn chroma_width(&self) -> u32 {
        self.width.div_ceil(2)
    }
}

/// Box-filter downsample of RGBA to `dw`x`dh`, returning RGB.
pub fn downsample(rgba: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<f32> {
    let mut out = vec![0f32; (dw * dh * 3) as usize];
    for dy in 0..dh {
        let y0 = (dy * sh / dh) as usize;
        let y1 = (((dy + 1) * sh).div_ceil(dh) as usize).max(y0 + 1);
        for dx in 0..dw {
            let x0 = (dx * sw / dw) as usize;
            let x1 = (((dx + 1) * sw).div_ceil(dw) as usize).max(x0 + 1);
            let mut acc = [0f32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * sw as usize + x) * 4;
                    let a = rgba[i + 3] as f32 / 255.0;
                    for c in 0..3 {
                        acc[c] += rgba[i + c] as f32 * a + 255.0 * (1.0 - a);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as f32;
            let o = ((dy * dw + dx) * 3) as usize;
            for c in 0..3 {
                out[o + c] = acc[c] / n;
            }
        }
    }
    out
}

fn clamp(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

pub fn rgb_to_yuv420(rgb: &[f32], width: u32, height: u32) -> Yuv420 {
    let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
    let mut y = vec![0u8; (width * height) as usize];
    let mut u = vec![0u8; (cw * ch) as usize];
    let mut v = vec![0u8; (cw * ch) as usize];
    let mut cb_acc = vec![0f32; (cw * ch) as usize];
    let mut cr_acc = vec![0f32; (cw * ch) as usize];
    let mut cnt = vec![0f32; (cw * ch) as usize];
    for py in 0..height {
        for px in 0..width {
            let i = ((py * width + px) * 3) as usize;
            let (r, g, b) = (rgb[i], rgb[i + 1], rgb[i + 2]);
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            y[(py * width + px) as usize] = clamp(luma);
            let ci = ((py / 2) * cw + px / 2) as usize;
            cb_acc[ci] += (b - luma) / 1.772;
            cr_acc[ci] += (r - luma) / 1.402;
            cnt[ci] += 1.0;
        }
    }
    for i in 0..cb_acc.len() {
        u[i] = clamp(128.0 + cb_acc[i] / cnt[i]);
        v[i] = clamp(128.0 + cr_acc[i] / cnt[i]);
    }
    Yuv420 { width, height, y, u, v }
}

#[cfg(feature = "dav1d")]
pub fn yuv420_to_rgba(img: &Yuv420) -> Vec<u8> {
    let cw = img.chroma_width();
    let mut out = vec![255u8; (img.width * img.height * 4) as usize];
    for py in 0..img.height {
        for px in 0..img.width {
            let ci = ((py / 2) * cw + px / 2) as usize;
            let luma = img.y[(py * img.width + px) as usize] as f32;
            let cb = img.u[ci] as f32 - 128.0;
            let cr = img.v[ci] as f32 - 128.0;
            let o = ((py * img.width + px) * 4) as usize;
            out[o] = clamp(luma + 1.402 * cr);
            out[o + 1] = clamp(luma - 0.344136 * cb - 0.714136 * cr);
            out[o + 2] = clamp(luma + 1.772 * cb);
        }
    }
    out
}
