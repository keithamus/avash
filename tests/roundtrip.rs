fn gradient(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255]);
        }
    }
    v
}

#[test]
fn encode_decode_preserves_shape_and_colour() {
    let (w, h) = (640, 360);
    let hash = avash::encode(&gradient(w, h), w, h, &avash::Options::default()).unwrap();
    assert!(hash.bytes().all(|b| avash::ALPHABET.contains(&b)), "alphabet");
    assert!(!hash.contains(['"', '&', '<', '>', '\\', '\'']));
    assert_eq!(avash::to_av1(&hash).unwrap()[..2], [0x12, 0], "temporal delimiter leads the rebuilt stream");
    assert_eq!(avash::dimensions(&hash).unwrap(), (48, 28));

    let img = avash::decode(&hash).unwrap();
    assert_eq!((img.width, img.height), (48, 28));
    let px = |x: u32, y: u32| &img.rgba[((y * img.width + x) * 4) as usize..][..3];
    let (tl, br) = (px(1, 1), px(46, 26));
    assert!(tl[0] < 60 && tl[1] < 60, "top-left should be dark red/green: {tl:?}");
    assert!(br[0] > 180 && br[1] > 180, "bottom-right should be bright: {br:?}");
}

#[test]
fn packing_the_rebuilt_stream_reproduces_the_bytes() {
    let hash = avash::encode(&gradient(256, 256), 256, 256, &avash::Options { size: 48, quality: 13, ..Default::default() }).unwrap();
    let av1 = avash::to_av1(&hash).unwrap();
    let stored = avash::from_base88(&hash).unwrap();
    assert_eq!(stored[0] >> 6, avash::VERSION);
    assert_eq!((stored[0] >> 4) & 3, 1, "rav1e output should pack with both headers regenerated");
    assert_eq!(stored, avash::pack(&av1).unwrap());
    assert_eq!(avash::to_av1(&avash::to_base88(&stored)).unwrap(), av1);
}

#[test]
fn rejects_garbage() {
    assert!(avash::decode("").is_err());
    assert!(avash::decode("AQ").is_err());
    assert!(avash::decode("!!!!").is_err());
    assert!(avash::decode("!!!!!!!!!!!!!!").is_err());
    assert!(avash::decode("<<<<").is_err());
}

#[test]
fn avif_wrapper_is_well_formed() {
    let hash = avash::encode(&gradient(64, 64), 64, 64, &avash::Options { size: 16, quality: 13, ..Default::default() }).unwrap();
    let avif = avash::to_avif(&hash).unwrap();
    assert_eq!(&avif[4..12], b"ftypavif");
    let mdat = avif.windows(4).position(|w| w == b"mdat").unwrap() + 4;
    let iloc = avif.windows(4).position(|w| w == b"iloc").unwrap();
    let offset = u32::from_be_bytes(avif[iloc + 18..iloc + 22].try_into().unwrap()) as usize;
    assert_eq!(offset, mdat, "iloc extent offset must point at mdat payload");
    let mut av1 = avash::to_av1(&hash).unwrap();
    av1.drain(..2);
    assert_eq!(&avif[mdat..], &av1[..], "mdat holds the OBU stream minus the temporal delimiter");
}
