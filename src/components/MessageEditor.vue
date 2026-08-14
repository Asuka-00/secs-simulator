<script setup lang="ts">
import { reactive, watch } from "vue";
import type { PrefabMessage } from "../types/session";
import { sxFy } from "../types/session";

const props = defineProps<{
  message: PrefabMessage | null;
  canSend: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [message: PrefabMessage];
  send: [message: PrefabMessage];
  remove: [id: string];
}>();

const draft = reactive({
  id: "",
  messageName: "",
  description: "",
  pairName: "",
  stream: 1,
  function: 1,
  direction: "H->E",
  wait: true,
  autoReply: false,
  noLogging: false,
  bodySml: "",
});

watch(
  () => props.message,
  (m) => {
    if (!m) return;
    Object.assign(draft, m);
  },
  { immediate: true, deep: true },
);

function snapshot(): PrefabMessage {
  return {
    id: draft.id,
    messageName: draft.messageName,
    description: draft.description,
    pairName: draft.pairName || sxFy(draft),
    stream: Number(draft.stream),
    function: Number(draft.function),
    direction: draft.direction,
    wait: draft.wait,
    autoReply: draft.autoReply,
    noLogging: draft.noLogging,
    bodySml: draft.bodySml,
  };
}

function onSave() {
  if (!props.message) return;
  emit("save", snapshot());
}

function onSend() {
  if (!props.message) return;
  emit("send", snapshot());
}
</script>

<template>
  <div class="editor">
    <template v-if="message">
      <header class="ed-head">
        <div class="sf">{{ sxFy(draft) }}</div>
        <div class="name">{{ draft.messageName || "—" }}</div>
        <div class="actions">
          <el-button size="small" :disabled="busy" @click="onSave">Save</el-button>
          <el-button
            size="small"
            type="primary"
            :disabled="!canSend || busy"
            :loading="busy"
            @click="onSend"
          >
            Send
          </el-button>
          <el-button
            size="small"
            type="danger"
            text
            :disabled="busy"
            @click="emit('remove', draft.id)"
          >
            Delete
          </el-button>
        </div>
      </header>

      <div class="fields">
        <label>
          Name
          <el-input v-model="draft.messageName" size="small" />
        </label>
        <label>
          Description
          <el-input v-model="draft.description" size="small" />
        </label>
        <label>
          Stream
          <el-input-number v-model="draft.stream" size="small" :min="0" :max="127" />
        </label>
        <label>
          Function
          <el-input-number v-model="draft.function" size="small" :min="0" :max="255" />
        </label>
        <label>
          Direction
          <el-select v-model="draft.direction" size="small">
            <el-option label="H→E (Host→Equip)" value="H->E" />
            <el-option label="H←E (Equip→Host)" value="H<-E" />
          </el-select>
        </label>
        <label>
          Pair
          <el-input v-model="draft.pairName" size="small" placeholder="S1F1" />
        </label>
        <label class="switch">
          <span>Wait (W-bit)</span>
          <el-switch v-model="draft.wait" size="small" />
        </label>
        <label class="switch">
          <span>AutoReply</span>
          <el-switch v-model="draft.autoReply" size="small" />
        </label>
      </div>

      <div class="body-block">
        <div class="body-label">
          Body (SML)
          <span class="hint">empty = no body · e.g. &lt;L &lt;A "x"&gt; &lt;A "y"&gt;&gt;</span>
        </div>
        <textarea
          v-model="draft.bodySml"
          class="body"
          spellcheck="false"
          placeholder="(empty)"
        />
      </div>
    </template>
    <div v-else class="placeholder">
      Select a message from the list, or import a <code>.SMD</code> file.
    </div>
  </div>
</template>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding: 10px 12px;
  background: var(--bg);
}

.ed-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  flex-shrink: 0;
}

.sf {
  font-weight: 700;
  color: #93c5fd;
  font-size: 14px;
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text);
}

.actions {
  display: flex;
  gap: 6px;
}

.fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 12px;
  flex-shrink: 0;
  margin-bottom: 10px;
}

.fields label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 11px;
  color: var(--muted);
}

.fields label.switch {
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
  padding-top: 18px;
}

.body-block {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.body-label {
  font-size: 11px;
  color: var(--muted);
  margin-bottom: 4px;
  display: flex;
  gap: 8px;
  align-items: baseline;
}

.hint {
  font-size: 10px;
  opacity: 0.7;
}

.body {
  flex: 1;
  min-height: 120px;
  resize: none;
  width: 100%;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #12141a;
  color: #e5e7eb;
  font-family: inherit;
  font-size: 12px;
  line-height: 1.45;
  padding: 10px;
}

.body:focus {
  outline: none;
  border-color: #3b82f6;
}

.placeholder {
  margin: auto;
  color: var(--muted);
  font-size: 13px;
  text-align: center;
  line-height: 1.6;
}

.placeholder code {
  color: #93c5fd;
}
</style>
