use crate::Error;
use crate::color::Yuv420;
use rav1e::config::SpeedSettings;
use rav1e::prelude::*;

/// Encodes one 8-bit 4:2:0 frame as a still picture, returning the raw OBU stream.
pub fn encode(img: &Yuv420, cq_level: u8) -> Result<Vec<u8>, Error> {
    let mut speed = SpeedSettings::from_preset(0);
    speed.cdef = false;
    speed.lrf = false;
    let enc = EncoderConfig {
        width: img.width as usize,
        height: img.height as usize,
        bit_depth: 8,
        chroma_sampling: ChromaSampling::Cs420,
        pixel_range: PixelRange::Full,
        color_description: Some(ColorDescription {
            color_primaries: ColorPrimaries::BT709,
            transfer_characteristics: TransferCharacteristics::SRGB,
            matrix_coefficients: MatrixCoefficients::BT601,
        }),
        still_picture: true,
        low_latency: true,
        min_key_frame_interval: 0,
        max_key_frame_interval: 1,
        quantizer: (cq_level as usize * 255 + 31) / 63,
        speed_settings: speed,
        ..Default::default()
    };
    let cfg = Config::new().with_encoder_config(enc).with_threads(1);
    let mut ctx: Context<u8> = cfg.new_context().map_err(|e| Error::Codec(format!("rav1e: {e}")))?;

    let mut frame = ctx.new_frame();
    let cw = img.chroma_width() as usize;
    frame.planes[0].copy_from_raw_u8(&img.y, img.width as usize, 1);
    frame.planes[1].copy_from_raw_u8(&img.u, cw, 1);
    frame.planes[2].copy_from_raw_u8(&img.v, cw, 1);
    ctx.send_frame(frame).map_err(|e| Error::Codec(format!("rav1e send: {e}")))?;
    ctx.flush();

    let mut out = Vec::new();
    loop {
        match ctx.receive_packet() {
            Ok(p) => out.extend_from_slice(&p.data),
            Err(EncoderStatus::Encoded) | Err(EncoderStatus::NeedMoreData) => {}
            Err(EncoderStatus::LimitReached) => break,
            Err(e) => return Err(Error::Codec(format!("rav1e: {e}"))),
        }
    }
    if out.is_empty() {
        return Err(Error::Codec("rav1e produced no frame".into()));
    }
    Ok(out)
}
