// Compact port of BlurHash (https://github.com/woltapp/blurhash, MIT), for the demo only.
const CHARS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%*+,-.:;=?@[]^_{|}~";
const enc83 = (v, n) => { let s = ""; for (let i = 1; i <= n; i++) s += CHARS[Math.floor(v / 83 ** (n - i)) % 83]; return s; };
const dec83 = s => [...s].reduce((a, c) => a * 83 + CHARS.indexOf(c), 0);
const toLin = v => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
const toSrgb = v => { v = Math.max(0, Math.min(1, v)); return Math.round((v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055) * 255); };
const sign = v => (v < 0 ? -1 : 1);
const spow = (v, e) => sign(v) * Math.abs(v) ** e;

export function encode(rgba, w, h, cx, cy) {
  const factors = [];
  for (let y = 0; y < cy; y++) for (let x = 0; x < cx; x++) {
    const norm = x === 0 && y === 0 ? 1 : 2;
    let r = 0, g = 0, b = 0;
    for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) {
      const basis = norm * Math.cos(Math.PI * x * i / w) * Math.cos(Math.PI * y * j / h);
      const p = (j * w + i) * 4;
      r += basis * toLin(rgba[p]); g += basis * toLin(rgba[p + 1]); b += basis * toLin(rgba[p + 2]);
    }
    factors.push([r / (w * h), g / (w * h), b / (w * h)]);
  }
  const [dc, ...ac] = factors;
  const max = ac.length ? Math.max(...ac.flat().map(Math.abs)) : 1;
  const qmax = Math.max(0, Math.min(82, Math.floor(max * 166 - 0.5)));
  const amax = (qmax + 1) / 166;
  let s = enc83(cx - 1 + (cy - 1) * 9, 1) + enc83(qmax, 1);
  s += enc83((toSrgb(dc[0]) << 16) + (toSrgb(dc[1]) << 8) + toSrgb(dc[2]), 4);
  for (const f of ac) {
    const q = v => Math.max(0, Math.min(18, Math.floor(spow(v / amax, 0.5) * 9 + 9.5)));
    s += enc83(q(f[0]) * 19 * 19 + q(f[1]) * 19 + q(f[2]), 2);
  }
  return s;
}

export function decode(hash, w, h) {
  const size = dec83(hash[0]);
  const cx = (size % 9) + 1, cy = Math.floor(size / 9) + 1;
  const amax = (dec83(hash[1]) + 1) / 166;
  const dcv = dec83(hash.slice(2, 6));
  const colors = [[toLin(dcv >> 16), toLin((dcv >> 8) & 255), toLin(dcv & 255)]];
  for (let i = 1; i < cx * cy; i++) {
    const v = dec83(hash.slice(4 + i * 2, 6 + i * 2));
    const q = k => spow((k - 9) / 9, 2) * amax;
    colors.push([q(Math.floor(v / 361)), q(Math.floor(v / 19) % 19), q(v % 19)]);
  }
  const out = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    let r = 0, g = 0, b = 0;
    for (let j = 0; j < cy; j++) for (let i = 0; i < cx; i++) {
      const basis = Math.cos(Math.PI * x * i / w) * Math.cos(Math.PI * y * j / h);
      const c = colors[i + j * cx];
      r += c[0] * basis; g += c[1] * basis; b += c[2] * basis;
    }
    const p = (y * w + x) * 4;
    out[p] = toSrgb(r); out[p + 1] = toSrgb(g); out[p + 2] = toSrgb(b); out[p + 3] = 255;
  }
  return out;
}
