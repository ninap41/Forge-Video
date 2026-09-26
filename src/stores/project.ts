import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { api } from "../api/tauri";
import {
  type AspectPreset, type AudioTrack, type Clip, type Crop, type Ms, type Project, type Transition,
  clipDuration, projectDuration,
} from "../types/project";

export interface Thumbs { intervalMs: number; urls: string[] }

export const useProjectStore = defineStore("project", () => {
  const project = ref<Project | null>(null);
  const selectedClipId = ref<string | null>(null);
  const playhead = ref<Ms>(0);
  const playing = ref(false);
  const dirty = ref(false);
  const error = ref<string | null>(null);
  const thumbs = ref<Record<string, Thumbs>>({}); // keyed by source path
  const waveforms = ref<Record<string, number[]>>({}); // keyed by source path, 1 byte per 10 ms
  const busy = ref<Record<string, boolean>>({});

  const clips = computed(() => project.value?.clips ?? []);
  const duration = computed(() => (project.value ? projectDuration(project.value) : 0));
  const selectedClip = computed(() => clips.value.find((c) => c.id === selectedClipId.value) ?? null);
  const selectedIndex = computed(() => clips.value.findIndex((c) => c.id === selectedClipId.value));

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

  function apply(p: Project, markDirty = true) {
    project.value = p;
    if (markDirty) dirty.value = true;
    if (selectedClipId.value && !p.clips.some((c) => c.id === selectedClipId.value)) selectedClipId.value = null;
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

  async function ensureMediaCaches(c: Clip) {
    if (!thumbs.value[c.source]) {
      void api.cacheThumbnails(c.id).then((t) => {
        thumbs.value[c.source] = { intervalMs: t.interval_ms, urls: t.files.map(api.assetUrl) };
      }).catch((e) => console.warn("thumbs", e));
    }
    if (c.media.has_audio && !waveforms.value[c.source]) {
      void api.cacheWaveform(c.source).then((w) => { waveforms.value[c.source] = w.peaks; }).catch((e) => console.warn("waveform", e));
    }
  }

  return {
    project, clips, duration, selectedClipId, selectedClip, selectedIndex, playhead, playing, dirty, error, thumbs, waveforms, busy, current,

    async load() { const p = await api.projectGet(); apply(p, false); p.clips.forEach(ensureMediaCaches); },
    async newProject() { apply(await api.projectNew("Untitled"), false); dirty.value = false; playhead.value = 0; },
    async open(path: string) {
      await run("open", async () => { const p = await api.projectOpen(path); apply(p, false); p.clips.forEach(ensureMediaCaches); });
      dirty.value = false;
    },
    async save(path?: string) { const r = await run("save", () => api.projectSave(path)); if (r) dirty.value = false; return r; },

    async importMedia(paths: string[]) {
      for (const path of paths) {
        const p = await run("import", () => api.mediaImport(path));
        if (p) { apply(p); const c = p.clips[p.clips.length - 1]; ensureMediaCaches(c); selectedClipId.value = c.id; }
      }
    },
    async trim(id: string, s: Ms, e: Ms) { const p = await run("trim", () => api.clipTrim(id, s, e)); if (p) apply(p); },
    async splitAtPlayhead() {
      const cur = current.value; if (!cur) return;
      const r = await run("split", () => api.clipSplit(cur.clip.id, playhead.value));
      if (r) { apply(r.project); selectedClipId.value = r.new_id; }
    },
    async deleteClip(id: string) { const p = await run("delete", () => api.clipDelete(id)); if (p) apply(p); },
    async moveClip(id: string, to: number) { const p = await run("move", () => api.clipMove(id, to)); if (p) apply(p); },
    async setFades(id: string, fi: Ms, fo: Ms) { const p = await run("fades", () => api.clipSetFades(id, fi, fo)); if (p) apply(p); },
    async setTransition(id: string, t: Transition) { const p = await run("transition", () => api.clipSetTransition(id, t)); if (p) apply(p); },
    async setVolume(id: string, v: number, muted: boolean) { const p = await run("volume", () => api.clipSetVolume(id, v, muted)); if (p) apply(p); },
    async setAspect(a: AspectPreset) { const p = await run("aspect", () => api.setAspect(a)); if (p) apply(p); },
    async setCrop(c: Crop) { const p = await run("crop", () => api.setCrop(c)); if (p) apply(p); },
    async setMusic(path: string | null) {
      const p = await run("music", () => api.musicSet(path)); if (!p) return; apply(p);
      if (p.music && !waveforms.value[p.music.source]) {
        void api.cacheWaveform(p.music.source).then((w) => { waveforms.value[p.music!.source] = w.peaks; }).catch(() => {});
      }
    },
    async updateMusic(t: AudioTrack) { const p = await run("music", () => api.musicUpdate(t)); if (p) apply(p); },
    select(id: string | null) { selectedClipId.value = id; },
    seek(ms: Ms) { playhead.value = Math.max(0, Math.min(ms, Math.max(duration.value - 1, 0))); },
  };
});
