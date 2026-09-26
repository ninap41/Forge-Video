/** Draw cached peaks (1 byte per 10 ms) for a source range onto a canvas. Shared by every clip block. */
export function drawPeaks(
  c: HTMLCanvasElement, peaks: number[] | undefined, sourceStart: number, sourceEnd: number,
  widthPx: number, heightPx: number, volume: number, color: string,
) {
  const w = Math.max(1, Math.floor(widthPx)), h = heightPx;
  c.width = w; c.height = h;
  const ctx = c.getContext("2d")!; ctx.clearRect(0, 0, w, h);
  if (!peaks) return;
  ctx.fillStyle = color;
  const s0 = sourceStart / 10, s1 = sourceEnd / 10;
  const bucketsPerPx = (s1 - s0) / w;
  for (let x = 0; x < w; x++) {
    let m = 0;
    const a = Math.floor(s0 + x * bucketsPerPx), b = Math.max(a + 1, Math.floor(s0 + (x + 1) * bucketsPerPx));
    for (let i = a; i < b && i < peaks.length; i++) m = Math.max(m, peaks[i]);
    const bh = Math.max(1, (m / 255) * h * Math.min(1, volume));
    ctx.fillRect(x, (h - bh) / 2, 1, bh);
  }
}
