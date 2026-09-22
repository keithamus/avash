// avash: tiny AV1 image placeholders. Decoding needs no wasm: the string is
// rewrapped as an AVIF and handed to the browser's own image decoder.
//
// Everything here is a string of "0"/"1": an avash string is a big integer,
// AV1 headers are bit fields, ISOBMFF boxes are byte fields, and only the
// finished file is turned into bytes. Comments carry the field names the bit
// literals stand for; they cost nothing once minified.

const u = (v, n) => v.toString(2).padStart(n, 0);
const align = s => s.padEnd(s.length + 7 & -8, 0);
const ascii = s => [...s].map(c => u(c.charCodeAt(), 8)).join("");
const bytes = s => Uint8Array.from(s.match(/.{8}/g), b => parseInt(b, 2));

// seq_profile 0, reduced still picture, 128x128 superblocks, filter-intra and
// intra-edge on, superres/CDEF/restoration off, 8-bit 4:2:0, sRGB primaries and
// transfer, BT.601 matrix, full range.
const sequenceHeader = (w, h) => {
  // frame_width_bits/frame_height_bits, then the dimensions themselves
  const width = d => 32 - Math.clz32(d - 1) || 1;
  const wb = width(w), hb = width(h);
  return align("000" + "1" + "1" + "00000" + u(wb - 1, 4) + u(hb - 1, 4) + u(w - 1, wb) + u(h - 1, hb) +
    // filter_intra, intra_edge_filter, then color_config
    "111" + "000" + "001" + "00000001" + "00001101" + "00000110" + "1" + "00" + "0" + "0" + "1");
};

// Key frame under that sequence header: no screen-content tools, one tile, no
// segmentation or delta quantisers, CDEF and loop restoration absent.
const frameHeader = (q, tx, lf) =>
  align("0" + "0" + "0" + "1" + u(q, 8) + "000000" + u(lf, 6).repeat(lf ? 4 : 2) + "000" + "1" + "0" + tx + "0");

// base 88: printable ASCII from "!" to "~" less the six characters that would
// need escaping in an HTML attribute or a JS string (" & ' < > \), so a
// character's digit value is its code less the skipped ones below it. Then
// version(2) mode(2), and per the mode: canonical (dimensions and the three
// varying frame header fields), canonical sequence header only, or both headers
// stored whole.
const parse = hash => {
  let n = 0n;
  for (let c of hash) c = c.charCodeAt(), n = n * 88n + BigInt(c - 33 - (c > 34) - (c > 38) - (c > 39) - (c > 60) - (c > 62) - (c > 92));
  let s = n.toString(2);
  s = s.padStart(s.length + 7 & -8, 0);
  let p = 0, w, h, seq, frame;
  const g = k => parseInt(s.slice(p, p += k), 2);
  g(2);
  const mode = g(2);
  if (!mode) {
    w = (g(6) + 1) * 2;
    h = (g(6) + 1) * 2;
    const q = g(6);
    frame = frameHeader(q == 63 ? 255 : q * 4, g(1), g(1) ? g(6) : 0) + s.slice(p + 7 & -8);
    seq = sequenceHeader(w, h);
  } else if (mode == 1) {
    g(4);
    w = g(8) + 1;
    h = g(8) + 1;
    seq = sequenceHeader(w, h);
    frame = s.slice(p);
  } else {
    g(4);
    const end = p + 8 + g(8) * 8;
    seq = s.slice(p, end);
    frame = s.slice(end);
    p += 10;
    const wb = g(4) + 1, hb = g(4) + 1;
    w = g(wb) + 1;
    h = g(hb) + 1;
  }
  return [seq, frame, w, h];
};

const leb = v => v > 127 ? u(v % 128 + 128, 8) + leb(v >> 7) : u(v, 8);
const obu = (kind, payload) => u(kind * 8 + 2, 8) + leb(payload.length / 8) + payload;
const box = (kind, ...parts) => (parts = parts.join(""), u(parts.length / 8 + 8, 32) + ascii(kind) + parts);
const fullbox = (kind, ...parts) => box(kind, u(0, 32), ...parts);

/** Frame dimensions encoded in an avash string. */
export function dimensions(hash) {
  const [, , w, h] = parse(hash);
  return { width: w, height: h };
}

/** Rebuild a standalone AVIF file from an avash string. */
export function toAvif(hash) {
  const [seq, frame, w, h] = parse(hash);
  const seqObu = obu(1, seq);
  const item = seqObu + obu(6, frame);
  const ftyp = box("ftyp", ascii("avif"), u(0, 32), ascii("avifmif1miaf"));
  const meta = offset => fullbox("meta",
    fullbox("hdlr", u(0, 32) + ascii("pict") + u(0, 104)),
    fullbox("pitm", u(1, 16)),
    // one item, one extent, 4-byte offset and length (offset_size/length_size 4,4)
    fullbox("iloc", u(0x44000001, 32) + u(0x000100000001, 48) + u(offset, 32) + u(item.length / 8, 32)),
    fullbox("iinf", u(1, 16) + box("infe", u(0x02000000, 32) + u(0x00010000, 32) + ascii("av01") + u(0, 8))),
    box("iprp",
      box("ipco",
        fullbox("ispe", u(w, 32) + u(h, 32)),
        fullbox("pixi", u(0x03080808, 32)),
        box("av1C", u(0x81000c00, 32) + seqObu),
        // nclx: color_primaries 1, transfer 13, matrix 6, full range
        box("colr", ascii("nclx") + u(0x0001000d000680, 56))),
      // ispe, pixi, av1C (essential) and colr apply to item 1
      fullbox("ipma", u(0x000000010001, 48) + u(0x0401028304, 40))));
  return bytes(ftyp + meta(ftyp.length / 8 + meta(0).length / 8 + 8) + box("mdat", item));
}

const blob = hash => new Blob([toAvif(hash)], { type: "image/avif" });

/** Object URL for the placeholder image. Caller revokes. */
export const toObjectURL = hash => URL.createObjectURL(blob(hash));

/** Decode to an ImageBitmap at native (tiny) size. */
export const decode = hash => createImageBitmap(blob(hash));

const shown = new WeakSet();

/** Show the avash as a placeholder behind an <img avash="..."> until its real source loads. */
export function apply(img) {
  if (shown.has(img) || (shown.add(img), img.complete && img.naturalWidth)) return;
  const url = toObjectURL(img.getAttribute("avash"));
  const prev = img.style.background;
  img.style.background = `url("${url}")center/100% 100% no-repeat`;
  const done = () => {
    img.style.background = prev;
    URL.revokeObjectURL(url);
  };
  for (const e of ["load", "error"]) img.addEventListener(e, done, { once: true });
}

/** Apply to every img[avash] under root and keep watching for new ones. */
export function observe(root = document) {
  const scan = node => {
    if (node.matches?.("img[avash]")) apply(node);
    node.querySelectorAll?.("img[avash]").forEach(apply);
  };
  scan(root);
  new MutationObserver(l => l.forEach(m => m.addedNodes.forEach(scan))).observe(root, { childList: true, subtree: true });
}

// Module scripts are deferred, and anything parsed after this point arrives as
// a mutation, so there is nothing to wait for.
if (typeof document < "u") observe();
