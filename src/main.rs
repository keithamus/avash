use std::process::exit;

const USAGE: &str = "usage:
  avash encode <image> [--size N] [--quality Q]   print avash for an image (png/jpeg/webp)
  avash decode <hash> -o <out.png> [--width W]    decode to PNG, optionally upscaled
  avash avif <hash> -o <out.avif>                 wrap as a standalone AVIF
  avash av1 <hash> -o <out.obu>                   raw AV1 OBU stream
  avash info <hash>                               dimensions and byte counts";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("avash: {e}");
        exit(1);
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, target) = match (args.first(), args.get(1)) {
        (Some(c), Some(t)) => (c.as_str(), t.as_str()),
        _ => return Err(USAGE.into()),
    };
    match cmd {
        "encode" => {
            let img = image::open(target)?.into_rgba8();
            let mut opts = avash::Options::default();
            if let Some(s) = flag(args, "--size") {
                opts.size = s.parse()?;
            }
            if let Some(q) = flag(args, "--quality") {
                opts.quality = q.parse()?;
            }
            println!("{}", avash::encode(img.as_raw(), img.width(), img.height(), &opts)?);
        }
        "decode" => {
            let out = flag(args, "-o").ok_or("decode needs -o")?;
            let img = avash::decode(target)?;
            let buf = image::RgbaImage::from_raw(img.width, img.height, img.rgba).ok_or("bad buffer")?;
            match flag(args, "--width") {
                Some(w) => {
                    let w: u32 = w.parse()?;
                    let h = (w as u64 * img.height as u64 / img.width as u64) as u32;
                    image::imageops::resize(&buf, w, h.max(1), image::imageops::FilterType::Lanczos3).save(out)?
                }
                None => buf.save(out)?,
            }
        }
        "avif" => std::fs::write(flag(args, "-o").ok_or("avif needs -o")?, avash::to_avif(target)?)?,
        "av1" => std::fs::write(flag(args, "-o").ok_or("av1 needs -o")?, avash::to_av1(target)?)?,
        "info" => {
            let (w, h) = avash::dimensions(target)?;
            let av1 = avash::to_av1(target)?;
            println!("{w}x{h}, {} chars, {} bytes as raw AV1, {} bytes as AVIF", target.len(), av1.len(), avash::to_avif(target)?.len());
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
