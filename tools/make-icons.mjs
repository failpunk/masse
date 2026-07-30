// Generates the app icon PNGs with no dependencies.
// Mark: a dark rounded square holding one circle split into three wedges, in the
// account rail's own three colours. Run: node tools/make-icons.mjs
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const OUT = process.env.ICON_OUT || join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'icons');
// The mark: one circle divided into three wedges, in the same three colours the
// account rail uses. Meaning: several accounts, one object. Chosen because it
// survives 16px, where overlapping circles or a wordmark turn to mush.
const BG = [11, 13, 15];
const WEDGES = [
  [99, 102, 241],  // indigo
  [236, 72, 153],  // pink
  [245, 158, 11],  // amber
];
const SS = 4; // supersample factor, box-downsampled for antialiasing

function icon(size) {
  const n = size * SS;
  const hi = new Float32Array(n * n * 4);
  const radius = n * 0.22;

  const cx = n / 2;
  const cy = n / 2;
  const ring = n * 0.30;          // radius of the mark
  const gap = n * 0.018;          // dark separation between wedges

  for (let y = 0; y < n; y++) {
    for (let x = 0; x < n; x++) {
      const px = x + 0.5;
      const py = y + 0.5;
      let color = null;
      let alpha = 0;

      if (insideRoundRect(px, py, 0, 0, n, n, radius)) {
        color = BG;
        alpha = 1;
        const dx = px - cx;
        const dy = py - cy;
        const dist = Math.hypot(dx, dy);
        if (dist <= ring) {
          // Angle from straight up, clockwise, so one wedge points at 12 o'clock.
          let a = Math.atan2(dx, -dy);
          if (a < 0) a += Math.PI * 2;
          const third = (Math.PI * 2) / 3;
          const index = Math.min(2, Math.floor(a / third));
          // Leave the background showing along each wedge boundary.
          const offset = a - index * third;
          const nearEdge = dist * Math.min(offset, third - offset) < gap;
          if (!nearEdge) color = WEDGES[index];
        }
      }

      const o = (y * n + x) * 4;
      hi[o] = color ? color[0] : 0;
      hi[o + 1] = color ? color[1] : 0;
      hi[o + 2] = color ? color[2] : 0;
      hi[o + 3] = alpha * 255;
    }
  }

  // Box downsample SS x SS -> 1.
  const out = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (let dy = 0; dy < SS; dy++) {
        for (let dx = 0; dx < SS; dx++) {
          const o = ((y * SS + dy) * n + (x * SS + dx)) * 4;
          const w = hi[o + 3] / 255;
          r += hi[o] * w;
          g += hi[o + 1] * w;
          b += hi[o + 2] * w;
          a += hi[o + 3];
        }
      }
      const count = SS * SS;
      const weight = a / 255 || 1; // premultiplied average, guard fully transparent
      const o = (y * size + x) * 4;
      out[o] = Math.round(r / weight);
      out[o + 1] = Math.round(g / weight);
      out[o + 2] = Math.round(b / weight);
      out[o + 3] = Math.round(a / count);
    }
  }
  return png(size, size, out);
}

function insideRoundRect(px, py, x0, y0, x1, y1, r) {
  if (px < x0 || px > x1 || py < y0 || py > y1) return false;
  // Clamp into the inset rect: dead centre gives distance 0, corners get the arc test.
  const cx = Math.min(Math.max(px, x0 + r), x1 - r);
  const cy = Math.min(Math.max(py, y0 + r), y1 - r);
  return (px - cx) ** 2 + (py - cy) ** 2 <= r * r;
}

function png(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter: none
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body) >>> 0);
  return Buffer.concat([len, body, crc]);
}

const TABLE = Array.from({ length: 256 }, (_, i) => {
  let c = i;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

mkdirSync(OUT, { recursive: true });
const SIZES = process.argv.slice(2).map(Number).filter(Boolean);
for (const size of (SIZES.length ? SIZES : [16, 32, 48, 128])) {
  writeFileSync(join(OUT, `icon${size}.png`), icon(size));
}
console.log('wrote icon16/32/48/128.png to', OUT);
