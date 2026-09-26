// Browser APIs the components touch that happy-dom does not implement.
import { vi } from "vitest";

export const resizeObservers: Array<{ cb: ResizeObserverCallback; targets: Element[] }> = [];
class RO {
  targets: Element[] = [];
  constructor(public cb: ResizeObserverCallback) { resizeObservers.push(this); }
  observe(el: Element) { this.targets.push(el); }
  unobserve(el: Element) { this.targets = this.targets.filter((t) => t !== el); }
  disconnect() { this.targets = []; }
}
(globalThis as any).ResizeObserver = RO;

/** Fire every registered ResizeObserver with the given size. */
export function resizeAll(width: number, height: number) {
  for (const ro of resizeObservers) {
    for (const t of ro.targets) ro.cb([{ target: t, contentRect: { width, height } } as any], ro as any);
  }
}

export const ctx2d = {
  clearRect: vi.fn(), fillRect: vi.fn(), fillStyle: "",
};
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", { value: () => ctx2d, configurable: true });

for (const m of ["play", "pause", "load"]) {
  Object.defineProperty(HTMLMediaElement.prototype, m, { value: vi.fn(() => Promise.resolve()), configurable: true, writable: true });
}
Object.defineProperty(HTMLMediaElement.prototype, "readyState", { value: 1, configurable: true });

(globalThis as any).requestAnimationFrame = (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 0) as unknown as number;
(globalThis as any).cancelAnimationFrame = (id: number) => clearTimeout(id);
if (!Element.prototype.setPointerCapture) Element.prototype.setPointerCapture = () => {};
if (!Element.prototype.releasePointerCapture) Element.prototype.releasePointerCapture = () => {};

// happy-dom's localStorage is not a usable Storage here; a Map-backed stand-in is enough for persistence tests.
const mem = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (k: string) => mem.get(k) ?? null,
    setItem: (k: string, v: string) => { mem.set(k, String(v)); },
    removeItem: (k: string) => { mem.delete(k); },
    clear: () => mem.clear(),
    key: (i: number) => [...mem.keys()][i] ?? null,
    get length() { return mem.size; },
  },
});
