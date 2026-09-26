<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api } from "../api/tauri";
import { useProjectStore } from "../stores/project";
import type { ExportPlan, Quality } from "../types/project";
import { basename, fmtMs } from "../utils/time";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();
const store = useProjectStore();

const quality = ref<Quality>("Standard");
const audioOnly = ref(false);
const destination = ref<string>("");
const plan = ref<ExportPlan | null>(null);
const planError = ref<string | null>(null);
const jobId = ref<string | null>(null);
const progress = ref(0);
const result = ref<{ destination: string } | null>(null);
const error = ref<string | null>(null);
const startedAt = ref(0);
const elapsed = ref(0);

const settings = computed(() => ({ destination: destination.value || "/tmp/forge-video-preview.mp4", quality: quality.value, audio_only: audioOnly.value }));

async function refreshPlan() {
  planError.value = null;
  try { plan.value = await api.exportPlan(settings.value); } catch (e) { plan.value = null; planError.value = String(e); }
}
watch([() => props.open, settings, () => store.project], () => { if (props.open) void refreshPlan(); }, { deep: true, immediate: true });
watch(audioOnly, () => { destination.value = ""; });

async function pickDestination() {
  const ext = audioOnly.value ? "m4a" : "mp4";
  const p = await save({ defaultPath: `${store.project?.name ?? "export"}.${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
  if (p) destination.value = p;
}

const unlisten: Array<() => void> = [];
void api.onJobProgress((e) => { if (e.job_id === jobId.value) { progress.value = e.progress; elapsed.value = Date.now() - startedAt.value; } }).then((u) => unlisten.push(u));
void api.onJobDone((e) => { if (e.job_id === jobId.value) { progress.value = 1; result.value = e.result; jobId.value = null; elapsed.value = Date.now() - startedAt.value; } }).then((u) => unlisten.push(u));
void api.onJobError((e) => { if (e.job_id === jobId.value) { error.value = e.error; jobId.value = null; } }).then((u) => unlisten.push(u));
onBeforeUnmount(() => unlisten.forEach((u) => u()));

async function start() {
  if (!destination.value) await pickDestination();
  if (!destination.value) return;
  error.value = null; result.value = null; progress.value = 0; startedAt.value = Date.now(); elapsed.value = 0;
  store.playing = false;
  try { jobId.value = await api.exportStart(settings.value); } catch (e) { error.value = String(e); }
}
async function cancel() { if (jobId.value) await api.jobCancel(jobId.value); }
function close() { if (!jobId.value) { result.value = null; error.value = null; emit("close"); } }

const strategyLabel = computed(() => {
  switch (plan.value?.strategy) {
    case "StreamCopy": return { text: "Fast trim · no re-encode", cls: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40" };
    case "HardwareEncode": return { text: "Render · VideoToolbox H.264", cls: "bg-accent/15 text-accent border-accent/40" };
    case "AudioOnly": return { text: "Audio only · AAC", cls: "bg-accent-2/20 text-blue-300 border-accent-2/40" };
    default: return null;
  }
});
</script>

<template>
  <div v-if="open" class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center" @pointerdown.self="close">
    <div class="w-[440px] rounded-lg bg-panel border border-line shadow-2xl p-4 text-xs">
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-sm font-semibold">Export</h2>
        <button class="text-muted hover:text-fg" :disabled="!!jobId" @click="close">✕</button>
      </div>

      <template v-if="!result">
        <div class="flex gap-1 mb-2">
          <button v-for="q in (['Draft', 'Standard', 'High'] as const)" :key="q" class="flex-1 rounded border px-2 py-1.5" :class="quality === q ? 'border-accent bg-accent/10' : 'border-line'" :disabled="!!jobId || audioOnly" @click="quality = q">{{ q }}</button>
        </div>
        <label class="flex items-center gap-2 mb-3 cursor-pointer"><input type="checkbox" v-model="audioOnly" :disabled="!!jobId" /> Audio only (podcast .m4a)</label>

        <div class="flex items-center gap-2 mb-3">
          <span class="text-muted w-16">Save to</span>
          <span class="flex-1 truncate font-mono" :title="destination">{{ destination ? basename(destination) : '— choose —' }}</span>
          <button class="rounded border border-line px-2 py-1 hover:border-muted" :disabled="!!jobId" @click="pickDestination">Choose…</button>
        </div>

        <div v-if="planError" class="text-danger mb-3">{{ planError }}</div>
        <div v-else-if="plan" class="mb-3 rounded border border-line p-2">
          <div class="flex items-center gap-2">
            <span class="rounded border px-1.5 py-0.5" :class="strategyLabel?.cls">{{ strategyLabel?.text }}</span>
            <span class="text-muted">{{ fmtMs(plan.duration_ms) }}<template v-if="plan.output[0]"> · {{ plan.output[0] }}×{{ plan.output[1] }}</template></span>
          </div>
          <div v-if="plan.strategy === 'StreamCopy'" class="text-muted mt-1">Copies the compressed video directly. Cut points snap to the nearest keyframe, so the start may be up to a couple of seconds early.</div>
          <div v-else-if="plan.reasons.length" class="text-muted mt-1">Re-encoding because of: {{ plan.reasons.join(', ') }}.</div>
        </div>

        <div v-if="jobId" class="mb-3">
          <div class="h-2 rounded bg-line overflow-hidden"><div class="h-full bg-accent transition-[width]" :style="{ width: progress * 100 + '%' }" /></div>
          <div class="flex justify-between text-muted mt-1"><span>{{ Math.round(progress * 100) }}%</span><span>{{ (elapsed / 1000).toFixed(0) }}s</span></div>
        </div>
        <div v-if="error" class="text-danger mb-3 whitespace-pre-wrap max-h-24 overflow-auto">{{ error }}</div>

        <div class="flex justify-end gap-2">
          <button v-if="jobId" class="rounded border border-line px-3 py-1.5" @click="cancel">Cancel</button>
          <button v-else class="rounded bg-accent text-black font-medium px-3 py-1.5 disabled:opacity-40" :disabled="!plan" @click="start">Export</button>
        </div>
      </template>

      <template v-else>
        <div class="text-emerald-300 mb-1">Done in {{ (elapsed / 1000).toFixed(1) }}s</div>
        <div class="font-mono truncate mb-3" :title="result.destination">{{ result.destination }}</div>
        <div class="flex justify-end gap-2">
          <button class="rounded border border-line px-3 py-1.5" @click="revealItemInDir(result!.destination)">Reveal in Finder</button>
          <button class="rounded bg-accent text-black font-medium px-3 py-1.5" @click="close">Close</button>
        </div>
      </template>
    </div>
  </div>
</template>
