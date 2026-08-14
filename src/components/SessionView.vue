<script setup lang="ts">
/**
 * Active session workspace: connection + message tree + log + dialogs.
 * 当前会话工作区：连接 + 消息树 + 日志 + 弹窗。
 */
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { invoke } from "@tauri-apps/api/core";
import type {
  LogEntry,
  MessageCatalog,
  PrefabMessage,
  SessionConfig,
  SessionSummary,
} from "../types/session";
import { emptyCatalog, sxFy } from "../types/session";
import ConnectionBar from "./ConnectionBar.vue";
import MessageLibrary from "./MessageLibrary.vue";
import MessageLog from "./MessageLog.vue";
import MessagePropertyDialog from "./MessagePropertyDialog.vue";
import MessageBodyDialog from "./MessageBodyDialog.vue";
import { isHostToEquip, messageLeafLabel } from "../utils/messageTree";

const { t } = useI18n();

const props = defineProps<{
  summary: SessionSummary;
  config: SessionConfig;
  logs: LogEntry[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [config: SessionConfig];
  open: [];
  close: [];
  clearLogs: [];
  refreshLogs: [];
  persist: [];
}>();

const catalog = ref<MessageCatalog>(emptyCatalog());
const selectedId = ref<string | null>(null);
const localBusy = ref(false);

const propOpen = ref(false);
const bodyOpen = ref(false);
const editing = ref<PrefabMessage | null>(null);

const selected = computed(
  () => catalog.value.messages.find((m) => m.id === selectedId.value) ?? null,
);

const canSend = computed(
  () => props.summary.open && props.summary.hsmsState === "Selected",
);

async function loadCatalog() {
  try {
    catalog.value = await invoke<MessageCatalog>("session_get_catalog", {
      id: props.summary.id,
    });
    if (
      selectedId.value &&
      !catalog.value.messages.some((m) => m.id === selectedId.value)
    ) {
      selectedId.value = catalog.value.messages[0]?.id ?? null;
    } else if (!selectedId.value && catalog.value.messages[0]) {
      selectedId.value = catalog.value.messages[0].id;
    }
  } catch (e) {
    ElMessage.error(String(e));
  }
}

watch(
  () => props.summary.id,
  async () => {
    selectedId.value = null;
    await loadCatalog();
  },
);

onMounted(loadCatalog);

async function onImportSmd(file: File) {
  localBusy.value = true;
  try {
    const xml = await file.text();
    catalog.value = await invoke<MessageCatalog>("session_import_smd", {
      id: props.summary.id,
      xml,
      source: file.name,
    });
    selectedId.value = catalog.value.messages[0]?.id ?? null;
    emit("persist");
    ElMessage.success(
      t("msg.imported", {
        count: catalog.value.messages.length,
        file: file.name,
      }),
    );
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    localBusy.value = false;
  }
}

async function onAdd() {
  try {
    const blank = await invoke<PrefabMessage>("session_new_blank_message");
    catalog.value = await invoke<MessageCatalog>("session_upsert_message", {
      id: props.summary.id,
      message: blank,
    });
    selectedId.value = blank.id;
    editing.value = blank;
    propOpen.value = true;
    emit("persist");
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function onEditProperty(message: PrefabMessage) {
  editing.value = { ...message };
  propOpen.value = true;
}

function onEditBody(message: PrefabMessage) {
  editing.value = { ...message };
  bodyOpen.value = true;
}

async function onSaveMessage(message: PrefabMessage) {
  localBusy.value = true;
  try {
    catalog.value = await invoke<MessageCatalog>("session_upsert_message", {
      id: props.summary.id,
      message,
    });
    selectedId.value = message.id;
    editing.value = message;
    emit("persist");
    ElMessage.success(t("msg.messageSaved"));
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    localBusy.value = false;
  }
}

async function onSendMessage(message: PrefabMessage) {
  if (!canSend.value) {
    ElMessage.warning(t("msg.sendNeedSelected"));
    return;
  }
  try {
    catalog.value = await invoke<MessageCatalog>("session_upsert_message", {
      id: props.summary.id,
      message,
    });
    const res = await invoke<{
      summary: string;
      waiting?: boolean;
      reply?: { summary: string };
    }>("session_send_message", { id: props.summary.id, message });
    emit("refreshLogs");
    if (res.waiting) {
      ElMessage.info(t("msg.sentWaiting", { summary: res.summary }));
    } else if (res.reply) {
      ElMessage.success(
        t("msg.sentReply", { summary: res.summary, reply: res.reply.summary }),
      );
    } else {
      ElMessage.success(t("msg.sent", { summary: res.summary }));
    }
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onCopyMessage(message: PrefabMessage) {
  try {
    const blank = await invoke<PrefabMessage>("session_new_blank_message");
    const copy: PrefabMessage = {
      ...message,
      id: blank.id,
      messageName: `${message.messageName || sxFy(message)}_Copy`,
    };
    catalog.value = await invoke<MessageCatalog>("session_upsert_message", {
      id: props.summary.id,
      message: copy,
    });
    selectedId.value = copy.id;
    emit("persist");
    ElMessage.success(t("msg.messageCopied"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onRemoveMessage(id: string) {
  try {
    await ElMessageBox.confirm(t("msg.deleteMessage"), t("msg.confirm"), {
      type: "warning",
    });
    catalog.value = await invoke<MessageCatalog>("session_remove_message", {
      id: props.summary.id,
      messageId: id,
    });
    if (selectedId.value === id) {
      selectedId.value = catalog.value.messages[0]?.id ?? null;
    }
    emit("persist");
  } catch {
    /* cancelled / 用户取消 */
  }
}

async function onToggleAutoReply(id: string, value: boolean) {
  const msg = catalog.value.messages.find((m) => m.id === id);
  if (!msg) return;
  try {
    catalog.value = await invoke<MessageCatalog>("session_upsert_message", {
      id: props.summary.id,
      message: { ...msg, autoReply: value },
    });
    emit("persist");
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function onSaveConfig(config: SessionConfig) {
  emit("save", config);
}
</script>

<template>
  <div class="session-view">
    <ConnectionBar
      :config="config"
      :open="summary.open"
      :hsms-state="summary.hsmsState"
      :busy="busy || localBusy"
      @save="onSaveConfig"
      @open="emit('open')"
      @close="emit('close')"
    />

    <div class="workspace">
      <aside class="msg-col">
        <MessageLibrary
          :messages="catalog.messages"
          :selected-id="selectedId"
          :source="catalog.source"
          :can-send="canSend"
          @select="selectedId = $event"
          @import-smd="onImportSmd"
          @add="onAdd"
          @edit-property="onEditProperty"
          @edit-body="onEditBody"
          @send="onSendMessage"
          @copy="onCopyMessage"
          @remove="onRemoveMessage"
          @toggle-auto-reply="onToggleAutoReply"
        />
      </aside>

      <section class="right-col">
        <div v-if="selected" class="selection-bar">
          <span
            class="arr"
            :class="isHostToEquip(selected.direction) ? 'arr-h2e' : 'arr-e2h'"
          >
            {{ isHostToEquip(selected.direction) ? "→" : "←" }}
          </span>
          <span class="sel-label" :title="messageLeafLabel(selected)">
            {{ messageLeafLabel(selected) }}
          </span>
          <span v-if="selected.autoReply" class="ar-tag">AR</span>
          <span v-if="selected.wait" class="w-tag">W</span>
          <div class="sel-actions">
            <el-button size="small" @click="onEditProperty(selected)">
              {{ t("selection.property") }}
            </el-button>
            <el-button size="small" @click="onEditBody(selected)">
              {{ t("selection.body") }}
            </el-button>
            <el-button
              size="small"
              type="primary"
              :disabled="!canSend || busy || localBusy"
              :loading="localBusy"
              @click="onSendMessage(selected)"
            >
              {{ t("selection.send") }}
            </el-button>
          </div>
        </div>
        <div v-else class="selection-bar muted">
          {{ t("selection.hint") }}
        </div>

        <div class="log-col">
          <MessageLog
            :entries="logs"
            :session-name="summary.name"
            @clear="emit('clearLogs')"
          />
        </div>
      </section>
    </div>

    <MessagePropertyDialog
      v-model="propOpen"
      :message="editing"
      @save="onSaveMessage"
    />
    <MessageBodyDialog
      v-model="bodyOpen"
      :message="editing"
      @save="onSaveMessage"
    />
  </div>
</template>

<style scoped>
.session-view {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  height: 100%;
  min-height: 0;
}

.workspace {
  display: grid;
  grid-template-columns: minmax(300px, 380px) minmax(0, 1fr);
  min-height: 0;
  overflow: hidden;
}

.msg-col {
  min-height: 0;
  min-width: 0;
  overflow: hidden;
}

.right-col {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  min-height: 0;
  min-width: 0;
  overflow: hidden;
}

.selection-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
  flex-shrink: 0;
  min-height: 40px;
}

.selection-bar.muted {
  color: var(--muted);
  font-size: 12px;
}

.arr {
  width: 18px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 3px;
  font-weight: 700;
  flex-shrink: 0;
}

.arr-h2e {
  color: var(--arr-h2e-fg);
  background: var(--arr-h2e-bg);
}

.arr-e2h {
  color: var(--arr-e2h-fg);
  background: var(--arr-e2h-bg);
}

.sel-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-family: ui-monospace, monospace;
}

.ar-tag,
.w-tag {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 3px;
  flex-shrink: 0;
}

.ar-tag {
  background: var(--status-ok-bg);
  color: var(--status-ok-fg);
}

.w-tag {
  background: var(--w-badge-bg);
  color: var(--w-badge-fg);
}

.sel-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.log-col {
  min-height: 0;
  overflow: hidden;
}

.log-col :deep(.log) {
  border-top: none;
  height: 100%;
}
</style>
