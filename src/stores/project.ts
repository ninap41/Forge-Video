import { defineStore } from "pinia";
import { ref, computed, watch } from "vue";
import { api } from "../api/tauri";
import {
  type AspectPreset, type AudioClip, type AudioTrack, type Crop, type MediaInfo, type Ms, type OverlayClip,
  type Placement, type PoolItem, type Project, type Transition,
  clipDuration, clipEnd, mediaKind, projectDuration,
} from "../types/project";

export interface Thumbs { intervalMs: number; urls: string[] }

/** What the user has selected: a V1 clip, an overlay, or an audio clip (with its track). */
export type Selection = { kind: "clip"; id: string } | { kind: "overlay"; id: string } | { kind: "audio"; id: string; trackId: string };

export type PoolView = "grid" | "list";

function readLocal<T extends string>(key: string, fallback: T): T {
  try { return (localStorage.getItem(key) as T | null) ?? fallback; } catch { return fallback; }
}
function writeLocal(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* private window, blocked storage: ignore */ }
}

export const useProjectStore = defineStore("project", () => {
  const project = ref<Project | null>(null);
  const selected = ref<Selection | null>(null);
  const playhead = ref<Ms>(0);
  const playing = ref(false);
  const dirty = ref(false);
  const error = ref<string | null>(null);
  const thumbs = ref<Record<string, Thumbs>>({}); // keyed by source path
  const waveforms = ref<Record<string, number[]>>({}); // keyed by source path, 1 byte per 10 ms
  const busy = ref<Record<string, boolean>>({});
  /** Media pool item being dragged onto the timeline (pointer-driven, not HTML5 DnD). */
  const poolDrag = ref<{ item: PoolItem; x: number; y: number } | null>(null);
  const poolView = ref<PoolView>(readLocal("forgevideo.poolView", "grid"));
  const timelineHeight = ref(Number(readLocal("forgevideo.timelineHeight", "360")) || 360);
  watch(poolView, (v) => writeLocal("forgevideo.poolView", v));
  watch(timelineHeight, (v) => writeLocal("forgevideo.timelineHeight", String(v)));

  const clips = computed(() => project.value?.clips ?? []);
  const overlays = computed(() => project.value?.overlays ?? []);
  const audioTracks = computed(() => project.value?.audio_tracks ?? []);
  const pool = computed(() => project.value?.pool ?? []);
  const duration = computed(() => (project.value ? projectDuration(project.value) : 0));

  /** V1 selection, kept as a writable id so existing callers and tests keep working. */
  const selectedClipId = computed<string | null>({
    get: () => (selected.value?.kind === "clip" ? selected.value.id : null),
    set: (id) => { selected.value = id ? { kind: "clip", id } : null; },
  });
  const selectedClip = computed(() => clips.value.find((c) => c.id === selectedClipId.value) ?? null);
  const selectedIndex = computed(() => clips.value.findIndex((c) => c.id === selectedClipId.value));
  const selectedOverlay = computed(() => (selected.value?.kind === "overlay" ? overlays.value.find((o) => o.id === selected.value!.id) ?? null : null));
  const selectedAudio = computed<{ clip: AudioClip; track: AudioTrack } | null>(() => {
    const s = selected.value; if (s?.kind !== "audio") return null;
    const track = audioTracks.value.find((t) => t.id === s.trackId); if (!track) return null;
    const c = track.clips.find((c) => c.id === s.id); return c ? { clip: c, track } : null;
  });

  /** Clip under the playhead and matching source time; mirrors timeline::locate in Rust. */
  const current = computed(() => {
    const t = playhead.value;
    for (let i = clips.value.length - 1; i >= 0; i--) {
      const c = clips.value[i];
      if (t >= c.timeline_start) {
        const off = Math.min(t - c.timeline_start, Math.max(clipDuration(c) - 1, 0));
        return { index: i, clip: c, sourceMs: c.source_start + off };
      }
    }
    return null;
  });
  /** Overlays under the playhead, lowest layer first; mirrors timeline::locate_overlays. */
  const currentOverlays = computed<{ clip: OverlayClip; sourceMs: Ms }[]>(() => {
    const t = playhead.value;
    return overlays.value.filter((o) => t >= o.timeline_start && t < clipEnd(o)).map((o) => ({ clip: o, sourceMs: o.source_start + (t - o.timeline_start) }));
  });
  const overlayLayers = computed(() => Math.max(1, project.value?.overlay_layers ?? 1));
  /** Every audio clip that covers the playhead, with mute resolved against its track. */
  const activeAudioClips = computed<{ clip: AudioClip; track: AudioTrack; sourceMs: Ms; silent: boolean }[]>(() => {
    const t = playhead.value;
    const out: { clip: AudioClip; track: AudioTrack; sourceMs: Ms; silent: boolean }[] = [];
    for (const track of audioTracks.value) {
      for (const c of track.clips) {
        if (t >= c.timeline_start && t < clipEnd(c)) out.push({ clip: c, track, sourceMs: c.source_start + (t - c.timeline_start), silent: track.muted || c.muted });
      }
    }
    return out;
  });

  function stillSelected(p: Project, s: Selection | null): boolean {
    if (!s) return false;
    if (s.kind === "clip") return p.clips.some((c) => c.id === s.id);
    if (s.kind === "overlay") return p.overlays.some((o) => o.id === s.id);
    return p.audio_tracks.some((t) => t.id === s.trackId && t.clips.some((c) => c.id === s.id));
  }

  function apply(p: Project, markDirty = true) {
    project.value = p;
    if (markDirty) dirty.value = true;
    if (!stillSelected(p, selected.value)) selected.value = null;
    playhead.value = Math.min(playhead.value, Math.max(projectDuration(p) - 1, 0));
  }

  async function run<T>(label: string, fn: () => Promise<T>): Promise<T | undefined> {
    busy.value[label] = true;
    error.value = null;
    try {
      return await fn();
    } catch (e) {
      error.value = String(e);
      console.error(label, e);
      return undefined;
    } finally {
      busy.value[label] = false;
    }
  }

  function ensureMediaCaches(source: string, media: MediaInfo) {
    const kind = mediaKind(media);
    if (kind !== "Audio" && !thumbs.value[source]) {
      void api.cacheThumbnails(source, media.duration_ms).then((t) => {
        thumbs.value[source] = { intervalMs: t.interval_ms, urls: t.files.map(api.assetUrl) };
      }).catch((e) => console.warn("thumbs", e));
    }
    if (media.has_audio && !waveforms.value[source]) {
      void api.cacheWaveform(source).then((w) => { waveforms.value[source] = w.peaks; }).catch((e) => console.warn("waveform", e));
    }
  }
  function cacheAll(p: Project) {
    p.clips.forEach((c) => ensureMediaCaches(c.source, c.media));
    p.overlays.forEach((o) => ensureMediaCaches(o.source, o.media));
    p.audio_tracks.forEach((t) => t.clips.forEach((c) => ensureMediaCaches(c.source, c.media)));
    p.pool.forEach((i) => ensureMediaCaches(i.path, i.media));
  }

  /** Apply a project-returning command; returns whether it succeeded. */
  async function edit(label: string, fn: () => Promise<Project>): Promise<boolean> {
    const p = await run(label, fn);
    if (p) { apply(p); cacheAll(p); }
    return !!p;
  }

  return {
    project, clips, overlays, audioTracks, pool, duration, selected, selectedClipId, selectedClip, selectedIndex, selectedOverlay, selectedAudio,
    playhead, playing, dirty, error, thumbs, waveforms, busy, current, currentOverlays, overlayLayers, activeAudioClips, poolDrag, poolView, timelineHeight,

    async load() { const p = await api.projectGet(); apply(p, false); cacheAll(p); },
    async newProject() { apply(await api.projectNew("Untitled"), false); dirty.value = false; playhead.value = 0; },
    async open(path: string) {
      await run("open", async () => { const p = await api.projectOpen(path); apply(p, false); cacheAll(p); });
      dirty.value = false;
    },
    async save(path?: string) { const r = await run("save", () => api.projectSave(path)); if (r) dirty.value = false; return r; },

    /** Import: video also lands on V1 (and is selected); audio and images go to the pool only. */
    async importMedia(paths: string[]) {
      for (const path of paths) {
        const before = clips.value.length;
        const p = await run("import", () => api.mediaImport(path));
        if (!p) continue;
        apply(p); cacheAll(p);
        const c = p.clips[p.clips.length - 1];
        if (c && p.clips.length > before) selected.value = { kind: "clip", id: c.id };
      }
    },
    async poolAdd(paths: string[]) { for (const path of paths) await edit("import", () => api.poolAdd(path)); },
    async poolRemove(id: string) { await edit("pool", () => api.poolRemove(id)); },
    async insertClip(path: string, atIndex: number) { await edit("insert", () => api.clipInsert(path, atIndex)); },

    async trim(id: string, s: Ms, e: Ms) { await edit("trim", () => api.clipTrim(id, s, e)); },
    /** ⌘T: split whatever is selected at the playhead, falling back to the V1 clip under it. */
    /**
     * ⌘T. Splits the selected overlay/audio clip if the playhead is inside it; otherwise the V1
     * clip under the playhead. Selects the new right-hand half.
     */
    async splitAtPlayhead() {
      const at = playhead.value;
      const s = selected.value;
      const inside = (c: { timeline_start: Ms; source_start: Ms; source_end: Ms }) => at > c.timeline_start && at < clipEnd(c);
      const target: Selection | null =
        s?.kind === "overlay" && selectedOverlay.value && inside(selectedOverlay.value) ? s
        : s?.kind === "audio" && selectedAudio.value && inside(selectedAudio.value.clip) ? s
        : current.value && inside(current.value.clip) ? { kind: "clip", id: current.value.clip.id }
        : null;
      if (!target) {
        error.value = clips.value.length ? "Move the playhead inside a clip to split it" : "Nothing to split";
        return;
      }
      const res = await run("split", () =>
        target.kind === "overlay" ? api.overlaySplit(target.id, at)
        : target.kind === "audio" ? api.audioClipSplit(target.id, at)
        : api.clipSplit(target.id, at));
      if (!res) return;
      apply(res.project);
      const next: Selection = target.kind === "audio" ? { kind: "audio", id: res.new_id, trackId: target.trackId } : { kind: target.kind, id: res.new_id };
      selected.value = stillSelected(res.project, next) ? next : null;
    },
    async deleteClip(id: string) { await edit("delete", () => api.clipDelete(id)); },
    /** ⌫: delete the selection, whatever kind it is. */
    async deleteSelected() {
      const s = selected.value; if (!s) return;
      if (s.kind === "clip") await edit("delete", () => api.clipDelete(s.id));
      else if (s.kind === "overlay") await edit("delete", () => api.overlayDelete(s.id));
      else await edit("delete", () => api.audioClipDelete(s.id));
    },
    async moveClip(id: string, to: number) { await edit("move", () => api.clipMove(id, to)); },
    async setFades(id: string, fi: Ms, fo: Ms) { await edit("fades", () => api.clipSetFades(id, fi, fo)); },
    async setTransition(id: string, t: Transition) { await edit("transition", () => api.clipSetTransition(id, t)); },
    async setVolume(id: string, v: number, muted: boolean) { await edit("volume", () => api.clipSetVolume(id, v, muted)); },
    async setAspect(a: AspectPreset) { await edit("aspect", () => api.setAspect(a)); },
    async setCrop(c: Crop) { await edit("crop", () => api.setCrop(c)); },
    async setVideoMuted(muted: boolean) { await edit("mute", () => api.setVideoMuted(muted)); },

    async overlayAdd(path: string, at: Ms, layer = 0) {
      if (await edit("overlay", () => api.overlayAdd(path, at, layer))) {
        const o = overlays.value.find((o) => o.source === path && o.layer === layer && o.timeline_start >= at - 1) ?? overlays.value[overlays.value.length - 1];
        if (o) selected.value = { kind: "overlay", id: o.id };
      }
    },
    async overlayMove(id: string, at: Ms, layer: number) { await edit("move", () => api.overlayMove(id, at, layer)); },
    async overlayLayerAdd() { await edit("layer", () => api.overlayLayerAdd()); },
    async overlayLayerRemove(layer: number) { await edit("layer", () => api.overlayLayerRemove(layer)); },
    async overlayTrim(id: string, s: Ms, e: Ms) { await edit("trim", () => api.overlayTrim(id, s, e)); },
    async overlaySetFades(id: string, fi: Ms, fo: Ms) { await edit("fades", () => api.overlaySetFades(id, fi, fo)); },
    async overlaySetPlacement(id: string, p: Placement) { await edit("placement", () => api.overlaySetPlacement(id, p)); },

    async audioTrackAdd(label: string) { await edit("track", () => api.audioTrackAdd(label)); },
    async audioTrackUpdate(id: string, label: string, muted: boolean) { await edit("track", () => api.audioTrackUpdate(id, label, muted)); },
    async audioTrackRemove(id: string) { await edit("track", () => api.audioTrackRemove(id)); },
    async audioClipAdd(trackId: string, path: string, at: Ms) {
      if (await edit("audio", () => api.audioClipAdd(trackId, path, at))) {
        const t = audioTracks.value.find((t) => t.id === trackId);
        const c = t?.clips.find((c) => c.source === path && c.timeline_start >= at - 1) ?? t?.clips[t.clips.length - 1];
        if (c) selected.value = { kind: "audio", id: c.id, trackId };
      }
    },
    async audioClipMove(id: string, trackId: string, at: Ms) {
      if (await edit("move", () => api.audioClipMove(id, trackId, at)) && selected.value?.kind === "audio" && selected.value.id === id) {
        selected.value = { kind: "audio", id, trackId };
      }
    },
    async audioClipTrim(id: string, s: Ms, e: Ms) { await edit("trim", () => api.audioClipTrim(id, s, e)); },
    async audioClipSet(id: string, volume: number, fi: Ms, fo: Ms, muted: boolean) { await edit("audio", () => api.audioClipSet(id, volume, fi, fo, muted)); },

    select(sel: string | Selection | null) { selected.value = typeof sel === "string" ? { kind: "clip", id: sel } : sel; },
    seek(ms: Ms) { playhead.value = Math.max(0, Math.min(ms, Math.max(duration.value - 1, 0))); },
  };
});
