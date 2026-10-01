<script lang="ts">
/** localStorage flag: set once the first-launch setup has been finished or skipped. */
export const WELCOME_KEY = "forgevideo.welcomed";
export function welcomeSeen(): boolean {
  try { return localStorage.getItem(WELCOME_KEY) === "1"; } catch { return true; }
}
export function markWelcomeSeen() {
  try { localStorage.setItem(WELCOME_KEY, "1"); } catch { /* blocked storage: show it again next time */ }
}
</script>

<script setup lang="ts">
import { computed, watch } from "vue";
import { useProjectStore } from "../stores/project";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();
const store = useProjectStore();

// Only ask Rust about the tools while the dialog is actually showing.
watch(() => props.open, (o) => { if (o) void store.refreshAiStatus(); }, { immediate: true });

const st = computed(() => store.aiStatus);
/** Anything the install script can put right (the account needs a sign-in instead). */
const toolsMissing = computed(() => !!st.value && !(st.value.whisper && st.value.model && st.value.claude));
const signedIn = computed(() => !!st.value?.account);

function finish() { markWelcomeSeen(); emit("close"); }
</script>

<template>
  <div v-if="open" class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center">
    <div class="w-[460px] rounded-lg bg-panel border border-line shadow-2xl p-4 text-xs" role="dialog" aria-label="Welcome to ForgeVideo" data-testid="welcome">
      <h2 class="text-sm font-semibold">Welcome to ForgeVideo</h2>
      <p class="text-muted mt-1">
        Cut, trim and export right away. To let AI mode transcribe recordings and find highlights for shorts,
        set up the tools and your Claude account now — or later from the <b class="text-fg">✦ AI</b> panel.
      </p>

      <section class="mt-4 flex items-start gap-2" data-testid="welcome-tools">
        <span class="w-4 text-center" :class="st && !toolsMissing ? 'text-accent' : 'text-muted'">{{ st && !toolsMissing ? '✓' : '1' }}</span>
        <div class="min-w-0 flex-1">
          <div class="text-fg font-semibold">AI tools</div>
          <p v-if="!st" class="text-muted">Checking…</p>
          <p v-else-if="!toolsMissing" class="text-muted">whisper.cpp, the speech model and Claude Code are installed.</p>
          <template v-else-if="store.aiInstalling">
            <p class="text-muted">Installing in the Terminal window. This may take a few minutes and updates by itself.</p>
            <button class="mt-2 rounded border border-line px-2 py-1 hover:border-muted" @click="store.cancelInstallAi()">Stop waiting</button>
          </template>
          <template v-else>
            <p class="text-muted">Opens Terminal; installs Homebrew, ffmpeg, whisper.cpp, the speech model and Claude Code.</p>
            <button class="mt-2 rounded border border-accent px-2 py-1 text-accent hover:bg-accent/10" @click="store.installAi()">Install AI tools…</button>
          </template>
        </div>
      </section>

      <section class="mt-4 flex items-start gap-2" data-testid="welcome-account">
        <span class="w-4 text-center" :class="signedIn ? 'text-accent' : 'text-muted'">{{ signedIn ? '✓' : '2' }}</span>
        <div class="min-w-0 flex-1">
          <div class="text-fg font-semibold">Claude account</div>
          <p v-if="signedIn" class="text-muted">Signed in as <span class="text-fg" :title="st!.account!">{{ st!.account }}</span>.</p>
          <template v-else-if="store.aiSigningIn">
            <p class="text-muted">Finish signing in in the Terminal window and your browser. This updates by itself.</p>
            <button class="mt-2 rounded border border-line px-2 py-1 hover:border-muted" @click="store.cancelClaudeLogin()">Stop waiting</button>
          </template>
          <template v-else>
            <p class="text-muted">{{ st?.claude ? 'Opens Terminal on claude auth login; uses your Claude subscription.' : 'Available once Claude Code is installed.' }}</p>
            <button
              class="mt-2 rounded border border-accent px-2 py-1 text-accent hover:bg-accent/10 disabled:opacity-40 disabled:hover:bg-transparent"
              :disabled="!st?.claude" @click="store.claudeLogin()"
            >Sign in to Claude</button>
          </template>
        </div>
      </section>

      <p v-if="store.aiError" class="mt-3 text-danger whitespace-pre-wrap break-words" data-testid="welcome-error">{{ store.aiError }}</p>

      <div class="flex justify-end gap-2 mt-5">
        <button v-if="!signedIn" class="rounded border border-line px-3 py-1.5 hover:border-muted" @click="finish">Skip for now</button>
        <button class="rounded bg-accent text-black font-medium px-3 py-1.5" @click="finish">{{ signedIn ? 'Start editing' : 'Done' }}</button>
      </div>
    </div>
  </div>
</template>
