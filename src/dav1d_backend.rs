use crate::Error;
use crate::color::Yuv420;
use dav1d::{Decoder, PixelLayout, PlanarImageComponent, Settings};

/// Decodes a raw OBU stream containing one still picture.
pub fn decode(obus: &[u8]) -> Result<Yuv420, Error> {
    let codec = |e: dav1d::Error| Error::Codec(format!("dav1d: {e}"));
    let mut settings = Settings::new();
    settings.set_n_threads(1);
    settings.set_max_frame_delay(1);
    let mut dec = Decoder::with_settings(&settings).map_err(codec)?;
    dec.send_data(obus.to_vec(), None, None, None).map_err(codec)?;
    let pic = loop {
        match dec.get_picture() {
            Err(dav1d::Error::Again) => dec.send_pending_data().map_err(codec)?,
            r => break r.map_err(codec)?,
        }
    };
    if pic.pixel_layout() != PixelLayout::I420 || pic.bit_depth() != 8 {
        return Err(Error::Codec("unexpected pixel format".into()));
    }
    let (w, h) = (pic.width(), pic.height());
    let plane = |c: PlanarImageComponent, pw: u32, ph: u32| -> Vec<u8> {
        let (data, stride) = (pic.plane(c), pic.stride(c) as usize);
        let mut v = Vec::with_capacity((pw * ph) as usize);
        for row in 0..ph as usize {
            v.extend_from_slice(&data[row * stride..][..pw as usize]);
        }
        v
    };
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    Ok(Yuv420 { width: w, height: h, y: plane(PlanarImageComponent::Y, w, h), u: plane(PlanarImageComponent::U, cw, ch), v: plane(PlanarImageComponent::V, cw, ch) })
}
