# avash

Tiny AV1 image placeholders. A BlurHash replacement that shows real shapes, not a gradient, in ~150 to 250 bytes.

An avash is a short string (~190 to 270 characters at the defaults) holding one AV1 still picture with everything a decoder can regenerate stripped
off. The default encode is 48 px on the long edge at constant quality 40, 4:2:0, no CDEF or loop restoration:
the settings that won blind tests against BlurHash, ThumbHash and JPEG XL at matched bytes
([keithamus/blurhash-alt](https://github.com/keithamus/blurhash-alt)).

## Format

```
byte 0        high nibble: version = 1; low nibble: flags (bit 0 = explicit sequence header)
canonical (flag clear, what libaom emits with this crate's settings):
  byte 1      width - 1
  byte 2      height - 1
  bytes 3..   OBU_FRAME payload (frame header + tile group), verbatim
explicit (flag set, any other encoder):
  byte 1      n, length of the sequence header payload
  bytes 2..   OBU_SEQUENCE_HEADER payload (profile 0, reduced_still_picture_header = 1), then the frame
```

The bytes are written as one big-endian integer in base 88 over printable ASCII minus space, `"`, `&`, `'`,
`<`, `>` and `\`, so the string drops into HTML attributes and JSON unescaped at 6.46 bits per character
(base64 is 6). The canonical sequence header is regenerated on decode: profile 0, 128x128 superblocks,
filter-intra and intra-edge on, superres/CDEF/restoration off, 8-bit 4:2:0, sRGB/BT.601 full range. Any AV1
encoder producing a reduced still-picture header and a single frame OBU can emit an avash (`avash::pack`
takes a raw OBU stream). Decoding rebuilds the OBU stream or a minimal AVIF; browsers decode the AVIF natively.

## Rust

```rust
let hash = avash::encode(&rgba, width, height, &avash::Options::default())?;
let (w, h) = avash::dimensions(&hash)?;      // e.g. (48, 32)
let img = avash::decode(&hash)?;             // RGBA8 at native size, via libaom
let avif = avash::to_avif(&hash)?;           // standalone .avif bytes
```

Features: `aom` (default, libaom encode + decode, built from source), `rav1e` (pure Rust encode, ~same bytes),
`wasm` (rav1e + wasm-bindgen exports `encode` and `toAvif`), `cli`.

## CLI

```
avash encode photo.jpg [--size 48] [--quality 40]
avash decode <hash> -o out.png [--width 384]
avash avif <hash> -o out.avif
avash info <hash>
```

## Browser

```html
<script type="module" src="avash.js"></script>
<img avash="_#1?ZE$k2.r?m4N$Dwe@gdwiMIsQ06,O;drJ6HKC5Zx..." src="photo.avif" width="1200" height="800" alt="...">
```

`js/avash.js` (no dependencies, no wasm) paints the placeholder as the image's background until `load`, and
watches for `img[avash]` added later. Also exports `toAvif(hash)`, `toObjectURL(hash)`, `decode(hash)` and
`dimensions(hash)`. Browser-side encoding uses the wasm build in `js/pkg` (`wasm-pack build --target web
--no-default-features --features wasm`); see `index.html`.
