/**
 * Draws a title to a transparent PNG with the webview's canvas. The same function feeds the
 * preview (at preview size) and the export (at output size), so both show the identical box:
 * ffmpeg here has no drawtext, and the webview already has every system font.
 */
import type { TextStyle } from "../types/project";

export interface TextRasterImage { url: string; w: number; h: number }

/** Padding and corner radius of the backdrop, in em. */
export const BACKDROP_PAD_EM = 0.45;

const cache = new Map<string, TextRasterImage>();
const CACHE_MAX = 32;

function hexToRgba(hex: string, alpha: number): string {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  const n = m ? parseInt(m[1], 16) : 0xffffff;
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${Math.max(0, Math.min(1, alpha))})`;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  if (typeof ctx.roundRect === "function") { ctx.roundRect(x, y, w, h, rr); return; }
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

/** The CSS font string used for measuring and drawing at `px` line height. */
export const fontFor = (style: TextStyle, px: number) => `${px}px "${style.font.replace(/"/g, "")}", sans-serif`;

/**
 * Rasterise `style` for a frame of `frameW` × `frameH` pixels. The box hugs the longest line
 * plus padding and is capped at the frame width; lines are centred. Cached by style + size.
 */
export function rasterText(style: TextStyle, frameW: number, frameH: number): TextRasterImage {
  const W = Math.max(1, Math.round(frameW)), H = Math.max(1, Math.round(frameH));
  const key = `${W}x${H}|${JSON.stringify(style)}`;
  const hit = cache.get(key);
  if (hit) return hit;

  const px = Math.max(1, style.size * H);
  const lines = style.text.split("\n");
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  ctx.font = fontFor(style, px);
  const textW = Math.max(1, ...lines.map((l) => ctx.measureText(l).width));
  const pad = BACKDROP_PAD_EM * px;
  const w = Math.max(2, Math.min(W, Math.ceil(textW + 2 * pad)));
  const h = Math.max(2, Math.ceil(lines.length * px * 1.2 + 2 * pad));
  canvas.width = w;
  canvas.height = h;
  ctx.clearRect(0, 0, w, h);
  if (style.backdrop) {
    ctx.fillStyle = hexToRgba(style.backdrop.color, style.backdrop.opacity);
    roundRect(ctx, 0, 0, w, h, pad);
    ctx.fill();
  }
  ctx.font = fontFor(style, px);
  ctx.fillStyle = style.color;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  lines.forEach((l, i) => ctx.fillText(l, w / 2, pad + (i + 0.5) * px * 1.2));
  const out = { url: canvas.toDataURL("image/png"), w, h };
  if (cache.size >= CACHE_MAX) cache.delete(cache.keys().next().value!);
  cache.set(key, out);
  return out;
}

/** The PNG bytes of a raster as base64 (what `export_start` takes). */
export const rasterBase64 = (r: TextRasterImage): string => r.url.replace(/^data:image\/png;base64,/, "");

/** Test hook. */
export const _clearRasterCache = () => cache.clear();
