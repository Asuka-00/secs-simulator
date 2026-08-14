<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ElMessage } from "element-plus";
import { invoke } from "@tauri-apps/api/core";

const props = defineProps<{
  sessionId: string;
  open: boolean;
  selected: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  sent: [];
}>();

const sml = ref("S1F1.");
const parseError = ref<string | null>(null);
const parseOk = ref<string | null>(null);
const sending = ref(false);
const lastReply = ref<string | null>(null);

const presets = [
  { label: "S1F1 (no W)", value: "S1F1." },
  { label: "S1F1 W (AreYouThere)", value: "S1F1 W." },
  { label: "S1F13 W (Establish)", value: "S1F13 W." },
  { label: "S1F17 W (Online)", value: "S1F17 W." },
  { label: "S2F17 W (DateTimeReq)", value: "S2F17 W." },
  {
    label: "S2F41 sample",
    value: 'S2F41 W <L <A "START"> <L> >.',
  },
];

const canSend = computed(
  () =>
    props.open &&
    props.selected &&
    !sending.value &&
    !props.busy &&
    sml.value.trim().length > 0 &&
    !parseError.value,
);

let parseTimer: ReturnType<typeof setTimeout> | null = null;

watch(
  sml,
  (text) => {
    parseError.value = null;
    parseOk.value = null;
    if (parseTimer) clearTimeout(parseTimer);
    parseTimer = setTimeout(async () => {
      const t = text.trim();
      if (!t) return;
      try {
        const p = await invoke<{
          stream: number;
          function: number;
          wbit: boolean;
          summary: string;
        }>("sml_parse", { text: t });
        parseOk.value = p.summary + (p.wbit ? " · will wait reply (T3)" : "");
        parseError.value = null;
      } catch (e) {
        parseOk.value = null;
        parseError.value = String(e);
      }
    }, 250);
  },
  { immediate: true },
);

function applyPreset(v: string) {
  sml.value = v;
}

async function onSend() {
  if (!canSend.value) return;
  sending.value = true;
  lastReply.value = null;
  try {
    const res = await invoke<{
      stream: number;
      function: number;
      wbit: boolean;
      summary: string;
      waiting?: boolean;
      reply?: { stream: number; function: number; summary: string } | null;
    }>("session_send_sml", { id: props.sessionId, sml: sml.value });
    if (res.waiting) {
      ElMessage.info(`Sent ${res.summary}, waiting for reply`);
      lastReply.value = "Waiting for reply…";
    } else {
      ElMessage.success(`Sent ${res.summary}`);
      if (res.reply) {
        lastReply.value = `Reply ${res.reply.summary}`;
      }
    }
    emit("sent");
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <div class="sml-editor">
    <div class="presets">
      <el-select
        size="small"
        placeholder="Presets"
        style="width: 100%"
        @change="(v: string) => applyPreset(v)"
      >
        <el-option v-for="p in presets" :key="p.value" :label="p.label" :value="p.value" />
      </el-select>
    </div>

    <el-input
      v-model="sml"
      type="textarea"
      :rows="10"
      resize="vertical"
      class="sml-input"
      placeholder='S1F1 W.  or  S2F41 W <L <A "START"> <L> >.'
      spellcheck="false"
    />

    <div class="status">
      <span v-if="parseError" class="err">{{ parseError }}</span>
      <span v-else-if="parseOk" class="ok">{{ parseOk }}</span>
      <span v-else class="muted">Enter SML ending with '.'</span>
    </div>

    <div v-if="lastReply" class="reply">{{ lastReply }}</div>

    <div class="actions">
      <el-button
        type="primary"
        size="small"
        :disabled="!canSend"
        :loading="sending"
        @click="onSend"
      >
        Send
      </el-button>
      <span v-if="!open" class="hint">Open session first</span>
      <span v-else-if="!selected" class="hint">Wait until Selected</span>
    </div>
  </div>
</template>

<style scoped>
.sml-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  min-height: 0;
}

.presets {
  flex-shrink: 0;
}

.sml-input :deep(textarea) {
  font-family: "SF Mono", Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
  line-height: 1.4;
  background: #0f1012;
  color: #e5e7eb;
}

.status {
  min-height: 18px;
  font-size: 11px;
}

.err {
  color: #f87171;
}

.ok {
  color: #4ade80;
}

.muted {
  color: #6b7280;
}

.reply {
  font-size: 12px;
  color: #93c5fd;
  padding: 4px 6px;
  background: #0f172a;
  border-radius: 4px;
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.hint {
  color: #6b7280;
  font-size: 11px;
}
</style>
