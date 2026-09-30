import type { Cue, Ms, Project } from "../types/project";

const MIN_CUE_MS = 100;

/** Every cue as it falls on the timeline, in order. Mirrors ai::captions::timeline_cues in Rust. */
export function timelineCues(p: Project): Cue[] {
  const out: Cue[] = [];
  for (const c of p.clips) {
    if (c.media.is_still) continue;
    const t = (p.transcripts ?? []).find((t) => t.source === c.source);
    if (!t) continue;
    for (const cue of t.cues) {
      const start = Math.max(cue.start, c.source_start);
      const end = Math.min(cue.end, c.source_end);
      if (start + MIN_CUE_MS > end) continue;
      out.push({ id: cue.id, start: c.timeline_start + (start - c.source_start), end: c.timeline_start + (end - c.source_start), text: cue.text });
    }
  }
  return out.sort((a, b) => a.start - b.start);
}

/** The cue on screen at `t`, if any. */
export const cueAt = (cues: Cue[], t: Ms): Cue | null => cues.find((c) => t >= c.start && t < c.end) ?? null;

/**
 * The transcript as plain text for the clipboard: one cue per line in timeline order, so a paste
 * into notes or a doc reads as prose. Timestamps are left out on purpose; Export writes the .srt.
 */
export function transcriptText(cues: Cue[]): string {
  return cues.map((c) => c.text.trim()).filter(Boolean).join("\n");
}
