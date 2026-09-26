import { describe, expect, it } from "vitest";
import { ASPECT_PRESETS, clipDuration, projectDuration, transitionMs } from "./project";
import { clip, project } from "../test/fixtures";

describe("aspect presets", () => {
  it("mirror the Rust AspectPreset::dimensions table", () => {
    expect(ASPECT_PRESETS.map((p) => [p.id, p.w, p.h])).toEqual([
      ["YouTube16x9", 1920, 1080], ["Shorts9x16", 1080, 1920], ["Square1x1", 1080, 1080], ["LinkedIn4x5", 1080, 1350],
    ]);
    for (const p of ASPECT_PRESETS) { expect(p.label).toBeTruthy(); expect(p.sub).toMatch(/^\d+:\d+$/); }
  });
});

describe("duration helpers", () => {
  it("clipDuration is out minus in", () => {
    expect(clipDuration(clip({ source_start: 1000, source_end: 3500 }))).toBe(2500);
    expect(clipDuration(clip())).toBe(5000);
  });
  it("projectDuration is the last clip's end, honouring transition overlap from layout", () => {
    expect(projectDuration(project([]))).toBe(0);
    const a = clip({ transition_out: { type: "CrossDissolve", ms: 1000 } });
    const b = clip({ source_end: 4000 });
    const p = project([a, b]);
    expect(b.timeline_start).toBe(4000);
    expect(projectDuration(p)).toBe(8000);
  });
  it("transitionMs is zero for a cut", () => {
    expect(transitionMs({ type: "None" })).toBe(0);
    expect(transitionMs({ type: "CrossDissolve", ms: 750 })).toBe(750);
    expect(transitionMs({ type: "DipToBlack", ms: 300 })).toBe(300);
  });
});
