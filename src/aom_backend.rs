use crate::Error;
use crate::color::Yuv420;
use libaom_sys::*;
use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_uint};
use std::ptr;

struct Ctx(aom_codec_ctx_t);

impl Ctx {
    fn check(&self, err: aom_codec_err_t, what: &'static str) -> Result<(), Error> {
        if err == AOM_CODEC_OK {
            return Ok(());
        }
        let detail = unsafe {
            let p = aom_codec_error_detail(&self.0);
            if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
        };
        Err(Error::Codec(format!("{what}: {detail}")))
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        unsafe { aom_codec_destroy(&mut self.0) };
    }
}

fn control_int(ctx: &mut Ctx, id: c_int, value: c_int, what: &'static str) -> Result<(), Error> {
    let err = unsafe { aom_codec_control(&mut ctx.0, id, value) };
    ctx.check(err, what)
}

/// Encodes one 8-bit 4:2:0 frame as a still picture, returning the raw OBU stream.
pub fn encode(img: &Yuv420, cq_level: u8) -> Result<Vec<u8>, Error> {
    unsafe {
        let iface = aom_codec_av1_cx();
        let mut cfg = MaybeUninit::<aom_codec_enc_cfg_t>::zeroed();
        if aom_codec_enc_config_default(iface, cfg.as_mut_ptr(), AOM_USAGE_GOOD_QUALITY) != AOM_CODEC_OK {
            return Err(Error::Codec("config_default".into()));
        }
        let mut cfg = cfg.assume_init();
        cfg.g_w = img.width;
        cfg.g_h = img.height;
        cfg.g_limit = 1;
        cfg.g_threads = 1;
        cfg.g_lag_in_frames = 0;
        cfg.g_timebase = aom_rational { num: 1, den: 30 };
        cfg.rc_end_usage = AOM_Q;
        cfg.kf_mode = AOM_KF_DISABLED;
        cfg.kf_max_dist = 0;
        cfg.g_bit_depth = AOM_BITS_8;
        cfg.g_input_bit_depth = 8;

        let mut ctx = Ctx(MaybeUninit::zeroed().assume_init());
        let err = aom_codec_enc_init_ver(&mut ctx.0, iface, &cfg, 0, AOM_ENCODER_ABI_VERSION as c_int);
        ctx.check(err, "enc_init")?;

        control_int(&mut ctx, AOME_SET_CPUUSED as c_int, 0, "cpu-used")?;
        control_int(&mut ctx, AOME_SET_CQ_LEVEL as c_int, cq_level as c_int, "cq-level")?;
        control_int(&mut ctx, AV1E_SET_ENABLE_CDEF as c_int, 0, "cdef")?;
        control_int(&mut ctx, AV1E_SET_ENABLE_RESTORATION as c_int, 0, "restoration")?;
        control_int(&mut ctx, AV1E_SET_ENABLE_SUPERRES as c_int, 0, "superres")?;
        control_int(&mut ctx, AV1E_SET_SUPERBLOCK_SIZE as c_int, AOM_SUPERBLOCK_SIZE_128X128 as c_int, "sb-size")?;
        control_int(&mut ctx, AV1E_SET_COLOR_RANGE as c_int, 1, "color-range")?;
        control_int(&mut ctx, AV1E_SET_COLOR_PRIMARIES as c_int, crate::bits::COLOR_PRIMARIES as c_int, "primaries")?;
        control_int(&mut ctx, AV1E_SET_TRANSFER_CHARACTERISTICS as c_int, crate::bits::TRANSFER_CHARACTERISTICS as c_int, "transfer")?;
        control_int(&mut ctx, AV1E_SET_MATRIX_COEFFICIENTS as c_int, crate::bits::MATRIX_COEFFICIENTS as c_int, "matrix")?;

        let mut raw = MaybeUninit::<aom_image_t>::zeroed();
        let cw = img.chroma_width() as c_int;
        let mut y = img.y.clone();
        let mut u = img.u.clone();
        let mut v = img.v.clone();
        if aom_img_wrap(raw.as_mut_ptr(), AOM_IMG_FMT_I420, img.width, img.height, 1, y.as_mut_ptr()).is_null() {
            return Err(Error::Codec("img_wrap".into()));
        }
        let mut raw = raw.assume_init();
        raw.planes = [y.as_mut_ptr(), u.as_mut_ptr(), v.as_mut_ptr()];
        raw.stride = [img.width as c_int, cw, cw];
        raw.range = AOM_CR_FULL_RANGE;

        let mut out = Vec::new();
        for frame in [Some(&raw), None] {
            let (ptr_, pts, flags) = match frame {
                Some(f) => (f as *const aom_image_t, 0, AOM_EFLAG_FORCE_KF as aom_enc_frame_flags_t),
                None => (ptr::null(), 1, 0),
            };
            let err = aom_codec_encode(&mut ctx.0, ptr_, pts, 1, flags);
            ctx.check(err, "encode")?;
            let mut iter: aom_codec_iter_t = ptr::null_mut();
            loop {
                let pkt = aom_codec_get_cx_data(&mut ctx.0, &mut iter);
                if pkt.is_null() {
                    break;
                }
                if (*pkt).kind == AOM_CODEC_CX_FRAME_PKT {
                    let f = (*pkt).data.frame;
                    out.extend_from_slice(std::slice::from_raw_parts(f.buf as *const u8, f.sz));
                }
            }
        }
        if out.is_empty() {
            return Err(Error::Codec("encoder produced no frame".into()));
        }
        Ok(out)
    }
}

/// Decodes a raw OBU stream containing one still picture.
pub fn decode(obus: &[u8]) -> Result<Yuv420, Error> {
    unsafe {
        let cfg = aom_codec_dec_cfg_t { threads: 1, w: 0, h: 0, allow_lowbitdepth: 1 };
        let mut ctx = Ctx(MaybeUninit::zeroed().assume_init());
        let err = aom_codec_dec_init_ver(&mut ctx.0, aom_codec_av1_dx(), &cfg, 0, AOM_DECODER_ABI_VERSION as c_int);
        ctx.check(err, "dec_init")?;
        let err = aom_codec_decode(&mut ctx.0, obus.as_ptr(), obus.len(), ptr::null_mut());
        ctx.check(err, "decode")?;

        let mut iter: aom_codec_iter_t = ptr::null_mut();
        let img = aom_codec_get_frame(&mut ctx.0, &mut iter);
        if img.is_null() {
            return Err(Error::Codec("decoder produced no frame".into()));
        }
        let img = &*img;
        if img.fmt != AOM_IMG_FMT_I420 {
            return Err(Error::Codec("unexpected pixel format".into()));
        }
        let (w, h) = (img.d_w, img.d_h);
        let plane = |i: usize, pw: c_uint, ph: c_uint| -> Vec<u8> {
            let mut v = Vec::with_capacity((pw * ph) as usize);
            for row in 0..ph as isize {
                let p = img.planes[i].offset(row * img.stride[i] as isize);
                v.extend_from_slice(std::slice::from_raw_parts(p, pw as usize));
            }
            v
        };
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        Ok(Yuv420 { width: w, height: h, y: plane(0, w, h), u: plane(1, cw, ch), v: plane(2, cw, ch) })
    }
}
