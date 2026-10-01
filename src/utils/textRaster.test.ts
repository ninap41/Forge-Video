import { beforeEach, describe, expect, it } from "vitest";
import { _clearRasterCache, rasterBase64, rasterText } from "./textRaster";
import { TEXT_STYLE_DEFAULT } from "../types/project";
import { ctx2d, FAKE_PNG_URL } from "../test/setup";

beforeEach(() => { _clearRasterCache(); ctx2d.fillText.mockClear(); ctx2d.roundRect.mockClear(); ctx2d.fill.mockClear(); });

describe("rasterText", () => {
  it("sizes the box from the widest line plus padding, at the frame's font size, and caps it at the frame width", () => {
    // 1080 p frame, 8 % → 86.4 px line height; the stub measures 10 px per character
    const r = rasterText({ ...TEXT_STYLE_DEFAULT, text: "Hi\nHello there" }, 1920, 1080);
    const px = 0.08 * 1080, pad = 0.45 * px;
    expect(r.w).toBe(Math.ceil(11 * 10 + 2 * pad));
    expect(r.h).toBe(Math.ceil(2 * px * 1.2 + 2 * pad));
    expect(r.url).toBe(FAKE_PNG_URL);
    expect(ctx2d.fillText).toHaveBeenCalledTimes(2);
    expect(ctx2d.fillText).toHaveBeenLastCalledWith("Hello there", r.w / 2, expect.any(Number));
    expect(ctx2d.font).toContain('"Quicksand", sans-serif');
    expect(ctx2d.fillStyle).toBe("#ffffff");
    expect(ctx2d.roundRect).not.toHaveBeenCalled();
    const wide = rasterText({ ...TEXT_STYLE_DEFAULT, text: "x".repeat(400) }, 640, 360);
    expect(wide.w).toBe(640);
  });

  it("draws the backdrop as a rounded rectangle in the chosen colour and opacity before the text", () => {
    rasterText({ ...TEXT_STYLE_DEFAULT, text: "Go", color: "#ff0000", backdrop: { color: "#0000ff", opacity: 0.5 } }, 800, 450);
    expect(ctx2d.roundRect).toHaveBeenCalledTimes(1);
    expect(ctx2d.fill).toHaveBeenCalledTimes(1);
    const [x, y, w, h, r] = ctx2d.roundRect.mock.calls[0];
    expect([x, y]).toEqual([0, 0]);
    expect(w).toBeGreaterThan(0); expect(h).toBeGreaterThan(0);
    expect(r).toBeCloseTo(0.45 * 0.08 * 450, 3);
    // the backdrop fill was set before the text colour replaced it
    expect(ctx2d.fillStyle).toBe("#ff0000");
  });

  it("caches by style and frame size, and strips the data: prefix for Rust", () => {
    const a = rasterText(TEXT_STYLE_DEFAULT, 800, 450);
    const b = rasterText({ ...TEXT_STYLE_DEFAULT }, 800, 450);
    expect(b).toBe(a);
    expect(rasterText(TEXT_STYLE_DEFAULT, 1920, 1080)).not.toBe(a);
    expect(rasterText({ ...TEXT_STYLE_DEFAULT, size: 0.1 }, 800, 450)).not.toBe(a);
    expect(rasterBase64(a)).toBe("iVBORw0KGgo=");
  });
});
