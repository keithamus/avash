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
    assert_eq!(avash::to_av1(&hash).unwrap()[2..4], [0x0a, 9], "canonical 9-byte sequence header regenerated");
    assert_eq!(avash::dimensions(&hash).unwrap(), (48, 28));

    let img = avash::decode(&hash).unwrap();
    assert_eq!((img.width, img.height), (48, 28));
    let px = |x: u32, y: u32| &img.rgba[((y * img.width + x) * 4) as usize..][..3];
    let (tl, br) = (px(1, 1), px(46, 26));
    assert!(tl[0] < 60 && tl[1] < 60, "top-left should be dark red/green: {tl:?}");
    assert!(br[0] > 180 && br[1] > 180, "bottom-right should be bright: {br:?}");
}

#[test]
fn canonical_mode_stores_no_headers() {
    let hash = avash::encode(&gradient(256, 256), 256, 256, &avash::Options { size: 48, quality: 50 }).unwrap();
    let av1 = avash::to_av1(&hash).unwrap();
    let frame = av1.len() - 13; // temporal delimiter (2) + sequence header OBU (11)
    let stored = avash::from_base88(&hash).unwrap().len();
    assert!(stored + 4 <= 3 + frame, "frame header should be stripped: {stored} stored vs {frame} frame bytes");
    assert_eq!(stored, avash::pack(&av1).unwrap().len(), "packing the rebuilt stream reproduces the avash bytes");
}

#[test]
fn foreign_stream_falls_back_to_verbatim_frame() {
    let hash = avash::encode(&gradient(128, 128), 128, 128, &avash::Options { size: 32, quality: 50 }).unwrap();
    let mut av1 = avash::to_av1(&hash).unwrap();
    let seq_end = 4 + av1[3] as usize;
    av1[seq_end - 1] ^= 1; // a padding bit: still a valid header, no longer the canonical one
    let packed = avash::pack(&av1).unwrap();
    assert_eq!(packed[0] >> 6, avash::VERSION);
    assert_eq!((packed[0] >> 4) & 3, 2, "non-canonical sequence header must be stored");
    assert_eq!(avash::to_av1(&avash::to_base88(&packed)).unwrap(), av1, "verbatim mode round trips the original stream");
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
    let hash = avash::encode(&gradient(64, 64), 64, 64, &avash::Options { size: 16, quality: 50 }).unwrap();
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

/// rav1e signals 64x64 superblocks, no filter-intra and a separate UV delta q,
/// none of which the canonical sequence header can express, so its streams take
/// the verbatim path and run about 9 bytes longer than libaom's.
#[cfg(feature = "rav1e")]
#[test]
fn rav1e_streams_round_trip_verbatim() {
    let hash = avash::encode_rav1e(&gradient(256, 256), 256, 256, &avash::Options { size: 48, quality: 50 }).unwrap();
    let bytes = avash::from_base88(&hash).unwrap();
    assert_eq!((bytes[0] >> 4) & 3, 2, "rav1e needs its own sequence header");
    let img = avash::decode(&hash).unwrap();
    assert_eq!((img.width, img.height), (48, 48));
}

#[test]
fn uncanonical_frame_header_keeps_the_frame_whole() {
    let hash = avash::encode(&gradient(128, 128), 128, 128, &avash::Options { size: 32, quality: 50 }).unwrap();
    let mut av1 = avash::to_av1(&hash).unwrap();
    let frame = 4 + av1[3] as usize + 2; // temporal delimiter, sequence header OBU, frame OBU header
    av1[frame + 1] ^= 1; // using_qmatrix, bit 15 of the uncompressed frame header
    let packed = avash::pack(&av1).unwrap();
    assert_eq!((packed[0] >> 4) & 3, 1, "frame must be stored whole when its header is not canonical");
    assert_eq!(avash::to_av1(&avash::to_base88(&packed)).unwrap(), av1);
}
