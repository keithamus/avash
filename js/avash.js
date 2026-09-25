// avash: tiny AV1 image placeholders. Decoding needs no wasm: the string is
// rewrapped as an AVIF and handed to the browser's own image decoder.
//
// Everything here is a string of "0"/"1": an avash string is a big integer,
// AV1 headers are bit fields, ISOBMFF boxes are byte fields, and only the
// finished file is turned into bytes. Comments carry the field names the bit
// literals stand for; they cost nothing once minified.

const u = (value, width) => value.toString(2).padStart(width, 0);
const align = bits => bits.padEnd(bits.length + 7 & -8, 0);
const ascii = text => text.replace(/./g, c => u(c.charCodeAt(), 8));
const bytes = bits => Uint8Array.from(bits.match(/.{8}/g), byte => +("0b" + byte));

// The sequence header rav1e emits: seq_profile 0, reduced still picture, level
// 31, 64x64 superblocks, intra-edge filter on, filter-intra/superres/CDEF off,
// loop restoration as flagged, 8-bit 4:2:0, sRGB primaries and transfer, BT.601
// matrix, full range, separate UV delta q.
const sequenceHeader = (w, h, restoration) => {
  // max_frame_width_minus_1 / max_frame_height_minus_1 in as few bits as they
  // need; frame_width_bits_minus_1 / frame_height_bits_minus_1 say how many
  const wm = (w - 1).toString(2), hm = (h - 1).toString(2);
  return align("000" + "1" + "1" + "11111" + u(wm.length - 1, 4) + u(hm.length - 1, 4) + wm + hm +
    // use_128x128_superblock, enable_filter_intra, enable_intra_edge_filter, superres, cdef, restoration, then color_config
    "001" + "00" + restoration + "001" + "00000001" + "00001101" + "00000110" + "1" + "00" + "1" + "0" + "1");
};

// The frame header rav1e emits, around the fields the avash stores (read in
// order): CDF updates on, no screen content tools, no render size, one uniform
// tile, base_q_idx, the five delta qs, no quantiser matrices, segmentation or
// delta q, the deblocking levels, sharpness 0, no filter deltas, Wiener
// restoration on every plane in one 256 pixel unit when flagged, tx mode
// select on, full transform set. Byte aligned, so the tile data follows.
const frameHeader = (w, h, restoration, read) => {
  // stored as 7-bit two's complement, which is AV1's su(1+6) behind delta_coded
  const dq = () => { const d = read(7); return d ? "1" + u(d, 7) : "0"; };
  const q = u(read(8), 8), y = dq(), uv = dq() + dq(), v = dq() + dq();
  let lf = u(read(6), 6) + u(read(6), 6);
  if (+lf) lf += u(read(6), 6) + u(read(6), 6);
  // disable_cdf_update, allow_screen_content_tools, render_and_frame_size_different, uniform_tile_spacing_flag, increment_tile_{cols,rows}_log2
  return align("0" + "0" + "0" + "1" + (w > 64 ? "0" : "") + (h > 64 ? "0" : "") +
    // base_q_idx, DeltaQYDc, diff_uv_delta with the U (and V) deltas, using_qmatrix, segmentation_enabled, delta_q_present
    q + y + (uv == v ? "0" + uv : "1" + uv + v) + "0" + "0" + "0" +
    // loop_filter_level[], loop_filter_sharpness, loop_filter_delta_enabled
    lf + "000" + "0" +
    // lr_type x3 (Wiener), lr_unit_shift, lr_unit_extra_shift, lr_uv_shift
    (restoration ? "101010" + "1" + "1" + "0" : "") +
    // tx_mode_select, reduced_tx_set
    "1" + "0");
};

// base 88: printable ASCII from "!" to "~" less the six characters that would
// need escaping in an HTML attribute or a JS string (" & ' < > \), so a
// character's digit value is its code less the skipped ones below it. Then
// version(2) mode(2), and per the mode: 1 both headers regenerated (restoration
// flag, dimensions minus 1 in 8 bits each, the frame header fields, padding to a
// byte, the tile data), 2 both headers stored (4 padding, sequence header
// length, sequence header, frame).
const parse = hash => {
  let n = 0n;
  for (let c of hash) c = c.charCodeAt(), n = n * 88n + BigInt(c - 33 - (c > 34) - 2 * (c > 39) - (c > 60) - (c > 62) - (c > 92));
  // version 1 hashes start with a 0 bit the integer cannot keep; put it back
  let s = n.toString(2);
  if (s.length % 8) s = "0" + s;
  let p = 0;
  const read = k => +("0b" + s.slice(p, p += k));
  const v1 = read(2) < 2, stored = read(2) > 1, restoration = read(1);
  // version 1 padded the mode 1 prefix to a byte and stored the frame whole
  if (v1) p = 8;
  let end;
  // stored sequence header spans bits 16..end; skip its seq_profile, still_picture,
  // reduced_still_picture_header and seq_level_idx to reach the dimension fields
  if (stored) p = 8, end = 16 + read(8) * 8, p = 26;
  const bitsFor = () => stored ? read(4) + 1 : 8;
  const wb = bitsFor(), hb = bitsFor();
  const width = read(wb) + 1, height = read(hb) + 1;
  if (stored) return [s.slice(16, end), s.slice(end), { width, height }];
  const seq = sequenceHeader(width, height, restoration);
  if (v1) return [seq, s.slice(24), { width, height }];
  return [seq, frameHeader(width, height, restoration, read) + s.slice(p + 7 & -8), { width, height }];
};

const leb = v => v > 127 ? u(v % 128 + 128, 8) + leb(v >> 7) : u(v, 8);
const obu = (kind, payload) => u(kind * 8 + 2, 8) + leb(payload.length / 8) + payload;
const box = (kind, body) => u(body.length / 8 + 8, 32) + ascii(kind) + body;
const fullbox = (kind, body) => box(kind, u(0, 32) + body);

/** Frame dimensions encoded in an avash string. */
export const dimensions = hash => parse(hash)[2];

/** Rebuild a standalone AVIF file from an avash string. */
export function toAvif(hash) {
  const [seq, frame, size] = parse(hash);
  const item = obu(1, seq) + obu(6, frame);
  return bytes(
    // major brand avif, minor_version 0, compatible brands
    box("ftyp", ascii("avif\0\0\0\0avifmif1miaf")) +
    fullbox("meta",
      fullbox("hdlr", ascii("\0\0\0\0pict") + u(0, 104)) +
      fullbox("pitm", u(1, 16)) +
      // iloc version 1: no offsets, 4-byte length, one item with one extent in idat
      box("iloc", u(0x01000000_0400_0001_0001_0001_0000_0001n, 128) + u(item.length / 8, 32)) +
      // infe version 2: item 1, unprotected, type av01, empty name
      fullbox("iinf", u(1, 16) + box("infe", u(0x02000000_00010000n, 64) + ascii("av01\0"))) +
      box("iprp",
        box("ipco",
          fullbox("ispe", u(size.width, 32) + u(size.height, 32)) +
          fullbox("pixi", u(0x03080808, 32)) +
          // seq_profile 0, level 0, 8-bit 4:2:0; the sequence header stays in the item
          box("av1C", u(0x81000c00, 32))) +
        // ispe, pixi and av1C (essential) apply to item 1
        fullbox("ipma", u(0x00000001_0001_03_01_02_83n, 80))) +
      box("idat", item)));
}

const blob = hash => new Blob([toAvif(hash)], { type: "image/avif" });

/** Object URL for the placeholder image. Caller revokes. */
export const toObjectURL = hash => URL.createObjectURL(blob(hash));

/** Decode to an ImageBitmap at native (tiny) size. */
export const decode = hash => createImageBitmap(blob(hash));

const shown = new WeakSet();

/** Show the avash as a placeholder behind an <img avash="..."> until its real source loads. A failed load keeps it. */
export function apply(img) {
  if (shown.has(img) || (shown.add(img), img.complete && img.naturalWidth)) return;
  const url = toObjectURL(img.getAttribute("avash")), style = img.style, prev = style.background;
  style.background = `url(${url})0/100% 100%`;
  img.addEventListener("load", () => {
    style.background = prev;
    URL.revokeObjectURL(url);
  });
}

/** Apply to every img[avash] under root and keep watching for new ones. */
export function observe(root = document) {
  const scan = () => root.querySelectorAll("img[avash]").forEach(apply);
  scan();
  new MutationObserver(scan).observe(root, { childList: true, subtree: true });
}

// Module scripts are deferred, and anything parsed after this point arrives as
// a mutation, so there is nothing to wait for.
if (globalThis.document) observe();
