export function fmtMs(ms: number, showTenths = true): string {
  const total = Math.max(0, Math.floor(Number.isFinite(ms) ? ms : 0));
  const m = Math.floor(total / 60000);
  const s = Math.floor((total % 60000) / 1000);
  const t = Math.floor((total % 1000) / 100);
  return `${m}:${String(s).padStart(2, "0")}${showTenths ? "." + t : ""}`;
}
export const basename = (p: string) => p.split("/").pop() ?? p;
export const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
