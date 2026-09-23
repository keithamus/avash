# avash

Tiny AVIF image placeholders. Add an `avash="..."` attribute to your images and
the avash library will render placeholder image while the bigger image
downloads. Like BlurHash, but with AVIF.

An avash is a short string holding one AV1 still picture with the container and
header data stripped off (the container, OBU framing, sequence header and
uncompressed frame header). This means with a small JS library, it's possible to
read the avash string and re-convert it back into an AVIF image for decoding in
the browsers native decoder.

AVIF images tend to have more recognisable features than a BlurHash at the
equivalent size, and the quality is fine-tunable depending on exactly how long
you want the hash to be, making it a scalable replacement for BlurHash or
equivalent hash methods.

## Technical Details: Format

The bytes are a bitstream, big-endian:

```
2  version = 1
2  mode
mode 0, canonical: both headers regenerated, only tile data stored
  6  width / 2 - 1          (even, 2..=128)
  6  height / 2 - 1
  6  quantiser index / 4    (63 means 255)
  1  tx_mode_select
  1  loop filter present
  6  loop filter level      (only if present)
  .. pad to a byte, then the tile group bytes verbatim
mode 1, canonical sequence header, frame stored whole (odd or large frames)
  4  padding, 8 width - 1, 8 height - 1, then the OBU_FRAME payload
mode 2, foreign encoder: sequence header stored too
  4  padding, 8 sequence header length, the header, then the OBU_FRAME payload
```

Mode 0 keeps a three byte prefix (four when the frame keeps deblocking) where a
raw stream carries a 9 byte sequence header and a 5 to 7 byte frame header.
Stripping the frame header alone takes 6 to 7 bytes off every hash: 45% of a
16px placeholder, 30% of a 24 px one, 12% at 48 px and 5% at 64 px. The
regenerated sequence header is profile 0, reduced still picture, 128x128
superblocks, filter-intra and intra-edge on, superres/CDEF/restoration off,
8-bit 4:2:0, sRGB/BT.601 full range; the regenerated frame header is a key frame
with one tile, no screen-content tools, no segmentation and no delta quantisers.
The four loop filter levels collapse to one value, which is why the
reconstruction is not bit-identical to the encoder's own stream (mean SSIM
difference over Kodak 24: 0.0003).

The bytes are written as one big-endian integer in base 88 over printable ASCII
minus space, `"`, `&`, `'`, `<`, `>` and `\`, so the string drops into HTML
attributes and JSON unescaped at 6.46 bits per character (base64 is 6,
BlurHash's base83 is 6.38; the largest attribute-safe ASCII alphabet would only
buy another 0.5%). Any AV1 encoder producing a reduced still-picture header and
a single frame OBU can emit an avash (`avash::pack` takes a raw OBU stream), but
only libaom's tool set matches the canonical headers. rav1e signals 64x64
superblocks, no filter-intra and a separate UV delta q, so the wasm encoder
falls back to mode 2 and its strings run about 11 characters longer. Decoding
rebuilds the OBU stream or a minimal AVIF; browsers decode the AVIF natively.

## Rust

```rust
// size 2..=128, quality 0..=63
let hash = avash::encode(&rgba, width, height, &avash::Options::default())?;

// e.g. (48, 32); both always even
let (w, h) = avash::dimensions(&hash)?;

// RGBA8 at native size, via libaom
let img = avash::decode(&hash)?;
// standalone .avif bytes
let avif = avash::to_avif(&hash)?;

// and avash::to_base88 / from_base88
let bytes = avash::pack(&raw_obu_stream)?;
```

Features: `aom` (default, libaom encode + decode, built from source), `rav1e`
(pure Rust encode, mode 2, ~9 bytes longer), `wasm` (rav1e + wasm-bindgen
exports `encode` and `toAvif`), `cli`.

## CLI

```
avash encode photo.jpg [--size 48] [--quality 23]
avash decode <hash> -o out.png [--width 384]
avash avif <hash> -o out.avif
avash info <hash>
```

## Browser

```
npm install avashjs
```

```html
<script type="module" src="https://unpkg.com/avashjs"></script>
<img
  avash="#/BXnHmUmisJm0-cON6z#@7ewu]gV|naU9FTdh8gG(8l|2?6O,xyX9|4zo_*mA%u!:h?PtIkd3(]ty!kc{S]$F4MwV"
  src="photo.avif"
  width="1200"
  height="800"
  alt="..."
/>
```

`js/avash.js` (no dependencies, no wasm, 2.4 kB minified / 1.4 kB gzipped)
paints the placeholder as the image's background until `load`, and watches for
`img[avash]` added later. Importing the package does the same, and also exports
`toAvif(hash)`, `toObjectURL(hash)`, `decode(hash)` and `dimensions(hash)`.
Browser-side encoding uses the wasm build in `js/pkg` (`wasm-pack build --target
web --no-default-features --features wasm`, not published to npm); see
`index.html`.
