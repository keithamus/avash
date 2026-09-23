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
mode 1, canonical sequence header regenerated, frame stored whole
  1  loop restoration enabled
  3  padding, 8 width - 1, 8 height - 1, then the OBU_FRAME payload
mode 2, foreign encoder: sequence header stored too
  4  padding, 8 sequence header length, the header, then the OBU_FRAME payload
```

Mode 1 keeps a three byte prefix where a raw stream carries an 8 to 10 byte
sequence header, so it saves 6 to 8 bytes per hash. The regenerated header is
the one rav1e emits: profile 0, reduced still picture, level 31, 64x64
superblocks, intra-edge filter on, filter-intra/superres/CDEF off, loop
restoration when a blur is applied, 8-bit 4:2:0, sRGB/BT.601 full range,
separate UV delta q. The frame header (11 to 12 bytes) is stored verbatim:
rav1e derives its chroma delta quantisers from tables that a decoder side would
have to carry to regenerate it.

The bytes are written as one big-endian integer in base 88 over printable ASCII
minus space, `"`, `&`, `'`, `<`, `>` and `\`, so the string drops into HTML
attributes and JSON unescaped at 6.46 bits per character (base64 is 6,
BlurHash's base83 is 6.38; the largest attribute-safe ASCII alphabet would only
buy another 0.5%). Any AV1 encoder producing a reduced still-picture header and
a single frame OBU can emit an avash (`avash::pack` takes a raw OBU stream);
a sequence header other than rav1e's is stored whole (mode 2), about 10
characters longer.
Decoding rebuilds the OBU stream or a minimal AVIF; browsers decode the AVIF
natively.

## Rust

```rust
// size 2..=128, quality 0..=63
let hash = avash::encode(&rgba, width, height, &avash::Options::default())?;

// e.g. (48, 32); both always even
let (w, h) = avash::dimensions(&hash)?;

// RGBA8 at native size, via dav1d
let img = avash::decode(&hash)?;
// standalone .avif bytes
let avif = avash::to_avif(&hash)?;

// and avash::to_base88 / from_base88
let bytes = avash::pack(&raw_obu_stream)?;
```

Features: `dav1d` (default, pixel decode via system libdav1d), `wasm`
(wasm-bindgen exports `encode` and `toAvif`), `cli` (implies `dav1d`). Encoding
is always rav1e, pure Rust.

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
  avash="Tfw{%6Y_]t:y9cP:EKP]{(,=n%TL6Fl%qkUuzWfq)Oqjp8|69[vJCLsVS99yAfjUfA%v5zFOvES,Hn!!3bi[Ov9b;$_I{x}F54X-b`BQ1=rQD,wg,$)Cf`hZa%ZIW+N!==?gZ-YlS4Ld3v(f)@^YVq4"
  src="photo.avif"
  width="1200"
  height="800"
  alt="..."
/>
```

`js/avash.js` (no dependencies, no wasm, 1.9 kB minified / 1.1 kB gzipped /
1.0 kB brotli) paints the placeholder as the image's background until `load`
(a failed load keeps it), and watches for `img[avash]` added later. Importing
the package does the same, and also exports `toAvif(hash)`, `toObjectURL(hash)`,
`decode(hash)` and `dimensions(hash)`.
Browser-side encoding uses the wasm build in `js/pkg` (`wasm-pack build --target
web --no-default-features --features wasm`, not published to npm); see
`index.html`.
