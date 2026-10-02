<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useProjectStore } from "../stores/project";
import type { Highlight, Range } from "../types/project";
import { sameRange } from "../types/project";
import { fmtMs } from "../utils/time";

const store = useProjectStore();

onMounted(() => void store.refreshAiStatus());

const st = computed(() => store.aiStatus);
/** What is still missing, with the one thing to do about it. */
const setup = computed(() => {
  const s = st.value; if (!s) return [];
  return [
    { ok: !!s.whisper, label: "whisper-cli", fix: "brew install whisper.cpp" },
    { ok: !!s.model, label: "Speech model", fix: `Download ggml-base.en.bin to ${s.model_path}` },
    { ok: !!s.claude, label: "Claude Code", fix: "Install Claude Code (claude.com/claude-code)" },
    { ok: !!s.account, label: "Claude account", fix: s.claude ? "Sign in below" : "Sign in once Claude Code is installed" },
  ];
});
const canTranscribe = computed(() => !!st.value?.whisper && !!st.value?.model);
/** Anything the install script can put right (the account needs a sign-in instead). */
const toolsMissing = computed(() => !!st.value && !(st.value.whisper && st.value.model && st.value.claude));
/** True once every speech source on V1 has been transcribed (even if whisper heard no words). */
const transcribed = computed(() => !!store.project?.transcripts.length && !pending.value);
const captionStatus = computed(() => {
  if (store.cues.length) return `${store.cues.length} captions`;
  if (!store.clips.length) return "Import a recording first";
  if (transcribed.value) return "Transcribed, but no speech was found";
  return "Not transcribed yet";
});

/** V1 sources with speech that have no transcript yet. */
const pending = computed(() => {
  const p = store.project; if (!p) return 0;
  const known = new Set(p.transcripts.map((t) => t.source));
  return new Set(p.clips.filter((c) => c.media.has_audio && !c.media.is_still && !known.has(c.source)).map((c) => c.source)).size;
});

const secs = (ms: number) => `${(ms / 1000).toFixed(ms % 1000 ? 1 : 0)} s`;
const kept = (h: Highlight) => h.keep.reduce((n, r) => n + (r.end - r.start), 0);
/** Claude's suggested cut, as steps to do by hand after exporting the section. */
function steps(h: Highlight): string[] {
  const out: string[] = [];
  const cut = h.end - h.start - kept(h);
  out.push(`Keep ${h.keep.map((r) => `${fmtMs(r.start, false)}–${fmtMs(r.end, false)}`).join(", ")} (${secs(kept(h))})`);
  if (cut > 0) out.push(`Cut ${secs(cut)} of filler`);
  if (h.fade_in || h.fade_out) out.push([h.fade_in ? `Fade in ${secs(h.fade_in)}` : "", h.fade_out ? `Fade out ${secs(h.fade_out)}` : ""].filter(Boolean).join(" · "));
  out.push("Captions come along in the .srt");
  return out;
}
const rangeOf = (h: Highlight): Range => ({ start: h.start, end: h.end });
/** Show: seek to it and select its span, so the yellow band shows what the card refers to. */
function show(h: Highlight) { store.playing = false; store.setRange(rangeOf(h)); store.seek(h.start); }
const looping = (h: Highlight) => store.playing && store.loopOn && sameRange(store.range, rangeOf(h));
const pinned = (h: Highlight) => store.loops.some((l) => sameRange(l, rangeOf(h)));
</script>

<template>
  <aside class="shrink-0 border-l border-line bg-panel overflow-y-auto text-xs" data-testid="ai-panel">
    <section class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">AI · shorts from a long recording</h3>
      <p class="text-muted">Speech is transcribed on this Mac. Claude Code only reads the transcript.</p>
      <ul v-if="setup.some((s) => !s.ok)" class="mt-2 space-y-1" data-testid="ai-setup">
        <li v-for="s in setup" :key="s.label" class="flex gap-1.5">
          <span :class="s.ok ? 'text-accent' : 'text-danger'">{{ s.ok ? '✓' : '✕' }}</span>
          <span class="min-w-0"><span class="text-fg">{{ s.label }}</span><span v-if="!s.ok" class="block text-muted break-words">{{ s.fix }}</span></span>
        </li>
      </ul>
      <div v-if="toolsMissing" class="mt-2" data-testid="ai-install">
        <template v-if="store.aiInstalling">
          <p class="text-muted">Installing in the Terminal window. This may take a few minutes and updates by itself.</p>
          <button class="mt-2 rounded border border-line px-2 py-1 hover:border-muted" @click="store.cancelInstallAi()">Stop waiting</button>
        </template>
        <template v-else>
          <button class="rounded border border-accent px-2 py-1 text-accent hover:bg-accent/10" @click="store.installAi()">Install AI tools…</button>
          <span class="ml-2 text-muted">Opens Terminal; installs Homebrew, whisper.cpp, the speech model and Claude Code.</span>
        </template>
      </div>
    </section>

    <section class="p-3 border-b border-line" data-testid="ai-account">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">Claude account</h3>
      <div v-if="st?.account" class="flex items-center gap-2">
        <span class="min-w-0 flex-1 truncate text-fg" :title="st.account">{{ st.account }}</span>
        <button class="rounded border border-line px-2 py-1 hover:border-muted" @click="store.claudeLogout()">Sign out</button>
      </div>
      <div v-else-if="store.aiSigningIn">
        <p class="text-muted">Finish signing in in the Terminal window and your browser. This updates by itself.</p>
        <button class="mt-2 rounded border border-line px-2 py-1 hover:border-muted" @click="store.cancelClaudeLogin()">Stop waiting</button>
      </div>
      <div v-else class="flex items-center gap-2">
        <button
          class="rounded border border-accent px-2 py-1 text-accent hover:bg-accent/10 disabled:opacity-40 disabled:hover:bg-transparent"
          :disabled="!st?.claude" @click="store.claudeLogin()"
        >Sign in to Claude</button>
        <span class="text-muted">{{ st?.claude ? 'Opens Terminal; uses your Claude subscription.' : 'Install Claude Code first.' }}</span>
      </div>
    </section>

    <section class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">1 · Captions</h3>
      <div class="flex items-center gap-2">
        <button
          class="rounded border border-line px-2 py-1 hover:border-muted disabled:opacity-40 disabled:hover:border-line"
          :disabled="!!store.aiJob || !canTranscribe || !pending" @click="store.transcribe()"
        >{{ store.cues.length && pending ? 'Transcribe new clips' : 'Transcribe' }}</button>
        <span class="text-muted" data-testid="caption-status">{{ captionStatus }}</span>
        <button
          v-if="store.cues.length" class="ml-auto rounded border border-line px-2 py-1 hover:border-muted" data-testid="copy-transcript"
          title="Copy the transcript as plain text, one caption per line" @click="store.copyTranscript()"
        >Copy</button>
      </div>
      <p v-if="store.cues.length" class="text-muted mt-2">Double-click a caption on the timeline to correct it. Export writes an .srt beside the video.</p>
    </section>

    <section class="p-3 border-b border-line">
      <h3 class="uppercase tracking-wide text-[10px] text-muted mb-2">2 · Highlights</h3>
      <button
        class="rounded border border-line px-2 py-1 hover:border-muted disabled:opacity-40 disabled:hover:border-line"
        :disabled="!!store.aiJob || !st?.account || !store.cues.length" @click="store.findHighlights()"
      >{{ store.highlights.length ? 'Find again' : 'Find highlights' }}</button>
      <p v-if="store.highlights.length" class="text-muted mt-2">Finding again replaces this list.</p>
    </section>

    <section v-if="store.aiJob" class="p-3 border-b border-line" data-testid="ai-progress">
      <div class="flex justify-between text-muted mb-1">
        <span class="truncate">{{ store.aiJob.kind === 'transcribe' ? `Transcribing ${store.aiJob.message ?? ''}` : 'Claude is reading the transcript…' }}</span>
        <span v-if="store.aiJob.kind === 'transcribe'">{{ Math.round(store.aiJob.progress * 100) }}%</span>
      </div>
      <div class="h-2 rounded bg-line overflow-hidden">
        <div v-if="store.aiJob.kind === 'transcribe'" class="h-full bg-accent transition-[width]" :style="{ width: store.aiJob.progress * 100 + '%' }" />
        <div v-else class="h-full w-full bg-accent/60 animate-pulse" />
      </div>
      <div class="flex justify-end mt-2"><button class="rounded border border-line px-2 py-1" :disabled="!store.aiJob.id" @click="store.cancelAi()">Cancel</button></div>
    </section>
    <div v-if="store.aiError" class="p-3 border-b border-line text-danger whitespace-pre-wrap break-words" data-testid="ai-error">{{ store.aiError }}</div>

    <section v-for="(h, i) in store.highlights" :key="h.id" class="p-3 border-b border-line" data-testid="highlight">
      <div class="flex items-start gap-2">
        <button class="flex-1 min-w-0 text-left hover:text-accent" title="Show on the timeline" @click="show(h)">
          <span class="text-muted">{{ i + 1 }}.</span> <span class="font-semibold text-fg">{{ h.title }}</span>
          <span class="block font-mono text-muted">{{ fmtMs(h.start, false) }}–{{ fmtMs(h.end, false) }} · {{ secs(h.end - h.start) }}</span>
        </button>
        <button class="w-4 text-center text-muted hover:text-danger" title="Dismiss" :aria-label="`Dismiss ${h.title}`" @click="store.deleteHighlight(h.id)">✕</button>
      </div>
      <p v-if="h.reason" class="mt-1 text-fg/80">{{ h.reason }}</p>
      <ol class="mt-2 space-y-0.5 list-decimal list-inside text-muted" data-testid="plan">
        <li v-for="s in steps(h)" :key="s">{{ s }}</li>
      </ol>
      <ul v-if="h.notes.length" class="mt-2 space-y-0.5 text-muted">
        <li v-for="n in h.notes" :key="n">By hand: {{ n }}</li>
      </ul>
      <div class="flex items-center gap-1 mt-2">
        <button class="rounded bg-accent text-black font-medium px-2 py-1" title="Export just this section of the timeline" data-testid="hl-export" @click="store.exportRange(rangeOf(h))">Export short</button>
        <button
          class="rounded border px-2 py-1" :class="looping(h) ? 'border-yellow-400 text-yellow-400 bg-yellow-400/10' : 'border-line hover:border-muted'" data-testid="hl-play"
          :title="looping(h) ? 'Pause' : 'Play this section on repeat'" :aria-label="`${looping(h) ? 'Pause' : 'Play'} ${h.title}`" @click="store.playRange(rangeOf(h))"
        >{{ looping(h) ? '❚❚' : '▶' }}</button>
        <button
          class="rounded border border-line px-2 py-1 hover:border-muted disabled:opacity-40" data-testid="hl-pin" :disabled="pinned(h)"
          :title="pinned(h) ? 'Already in Pinned loops' : 'Keep this section in Pinned loops'" @click="store.loopAdd(h.title, rangeOf(h))"
        >{{ pinned(h) ? 'Pinned' : 'Pin loop' }}</button>
      </div>
    </section>
  </aside>
</template>
