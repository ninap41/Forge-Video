import { describe, expect, it } from "vitest";
import { clip, cue, media, project, stillMedia } from "../test/fixtures";
import { cueAt, timelineCues, transcriptText } from "./captions";

const podcast = () => project(
  [clip({ id: "a", source: "/v/a.mp4", media: media({ duration_ms: 60_000 }) }), clip({ id: "b", source: "/v/b.mp4", media: media({ duration_ms: 30_000 }) })],
  { transcripts: [
    { source: "/v/a.mp4", cues: [0, 1, 2, 3, 4, 5].map((i) => cue(i * 10_000, i * 10_000 + 9_000, `a${i}`)) },
    { source: "/v/b.mp4", cues: [0, 1, 2].map((i) => cue(i * 10_000, i * 10_000 + 9_000, `b${i}`)) },
  ] },
);
const spans = (p: ReturnType<typeof project>) => timelineCues(p).map((c) => [c.start, c.end, c.text]);

describe("timelineCues", () => {
  it("offsets each clip's cues by where the clip sits", () => {
    const s = spans(podcast());
    expect(s).toHaveLength(9);
    expect(s[0]).toEqual([0, 9_000, "a0"]);
    expect(s[6]).toEqual([60_000, 69_000, "b0"]);
  });

  it("follows trims and reorders, shortening cues that are cut", () => {
    const p = podcast();
    const [a, b] = p.clips;
    const trimmed = project([{ ...a, source_start: 15_000, source_end: 32_000 }, b], { transcripts: p.transcripts });
    expect(spans(trimmed).slice(0, 4)).toEqual([[0, 4_000, "a1"], [5_000, 14_000, "a2"], [15_000, 17_000, "a3"], [17_000, 26_000, "b0"]]);
    const swapped = project([{ ...b }, { ...a }], { transcripts: p.transcripts });
    expect(spans(swapped)[0]).toEqual([0, 9_000, "b0"]);
    expect(spans(swapped)[3]).toEqual([30_000, 39_000, "a0"]);
  });

  it("keeps the cue id, so a correction reaches the transcript", () => {
    const p = podcast();
    expect(timelineCues(p)[0].id).toBe(p.transcripts[0].cues[0].id);
  });

  it("ignores stills, untranscribed sources and slivers", () => {
    const p = podcast();
    expect(timelineCues(project([clip({ source: "/v/other.mp4" })], { transcripts: p.transcripts }))).toEqual([]);
    expect(timelineCues(project([clip({ source: "/v/a.mp4", media: stillMedia() })], { transcripts: p.transcripts }))).toEqual([]);
    const sliver = project([clip({ source: "/v/a.mp4", media: media({ duration_ms: 60_000 }), source_start: 8_950, source_end: 10_050 })], { transcripts: p.transcripts });
    expect(timelineCues(sliver)).toEqual([]);
    expect(timelineCues(project([]))).toEqual([]);
  });
});

describe("cueAt", () => {
  it("finds the cue on screen, with an exclusive end", () => {
    const cues = timelineCues(podcast());
    expect(cueAt(cues, 0)?.text).toBe("a0");
    expect(cueAt(cues, 8_999)?.text).toBe("a0");
    expect(cueAt(cues, 9_000)).toBeNull();
    expect(cueAt(cues, 61_000)?.text).toBe("b0");
    expect(cueAt([], 10)).toBeNull();
  });
});

describe("transcriptText", () => {
  it("joins the cue texts one per line, skipping blanks", () => {
    expect(transcriptText([cue(0, 1000, " So here is the thing "), cue(1000, 2000, "   "), cue(2000, 3000, "nobody tells you this")])).toBe("So here is the thing\nnobody tells you this");
    expect(transcriptText([])).toBe("");
  });
});
