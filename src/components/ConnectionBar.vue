<script setup lang="ts">
/**
 * Connection toolbar: role/mode/IP/port + open/close.
 * 连接工具栏：角色/模式/IP/端口 + 打开/关闭。
 */
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { SessionConfig } from "../types/session";

const props = defineProps<{
  config: SessionConfig;
  open: boolean;
  hsmsState: string;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [config: SessionConfig];
  open: [];
  close: [];
}>();

const { t } = useI18n();
const draft = reactive<SessionConfig>({ ...props.config });
const advanced = ref(false);

watch(
  () => props.config,
  (c) => Object.assign(draft, c),
  { deep: true },
);

const stateClass = computed(() => {
  if (props.open && props.hsmsState === "Selected") return "ok";
  if (props.open) return "wait";
  return "off";
});

function onSave() {
  emit("save", { ...draft });
}
</script>

<template>
  <div class="conn">
    <div class="row">
      <el-input
        v-model="draft.name"
        size="small"
        class="w-name"
        :placeholder="t('conn.name')"
        :disabled="open"
      />
      <el-select v-model="draft.role" size="small" class="w-role" :disabled="open">
        <el-option :label="t('conn.equipment')" value="equipment" />
        <el-option :label="t('conn.host')" value="host" />
      </el-select>
      <el-select v-model="draft.mode" size="small" class="w-mode" :disabled="open">
        <el-option :label="t('conn.passive')" value="passive" />
        <el-option :label="t('conn.active')" value="active" />
      </el-select>
      <el-input v-model="draft.ip" size="small" class="w-ip" :disabled="open" />
      <el-input-number
        v-model="draft.port"
        size="small"
        :min="1"
        :max="65535"
        controls-position="right"
        :disabled="open"
      />
      <el-input-number
        v-model="draft.sessionId"
        size="small"
        :min="0"
        :max="32767"
        controls-position="right"
        :disabled="open"
        :title="t('conn.sessionId')"
      />
      <span class="status" :class="stateClass">
        {{ open ? t("conn.open") : t("conn.closed") }} · {{ hsmsState }}
      </span>
      <div class="actions">
        <el-button size="small" :disabled="open || busy" @click="advanced = !advanced">
          {{ advanced ? t("conn.less") : t("conn.more") }}
        </el-button>
        <el-button size="small" :disabled="open || busy" @click="onSave">
          {{ t("conn.apply") }}
        </el-button>
        <el-button
          size="small"
          type="success"
          :disabled="open || busy"
          :loading="busy && !open"
          @click="emit('open')"
        >
          {{ t("conn.openBtn") }}
        </el-button>
        <el-button
          size="small"
          type="danger"
          :disabled="!open || busy"
          :loading="busy && open"
          @click="emit('close')"
        >
          {{ t("conn.closeBtn") }}
        </el-button>
      </div>
    </div>
    <div v-if="advanced" class="row more">
      <label>T3 <el-input-number v-model="draft.t3" size="small" :min="0.1" :disabled="open" /></label>
      <label>T5 <el-input-number v-model="draft.t5" size="small" :min="0.1" :disabled="open" /></label>
      <label>T6 <el-input-number v-model="draft.t6" size="small" :min="0.1" :disabled="open" /></label>
      <label>T7 <el-input-number v-model="draft.t7" size="small" :min="0.1" :disabled="open" /></label>
      <label>
        {{ t("conn.linktest") }}
        <el-switch v-model="draft.linktestEnabled" size="small" :disabled="open" />
      </label>
      <el-input-number
        v-model="draft.linktestSeconds"
        size="small"
        :min="1"
        :disabled="open || !draft.linktestEnabled"
      />
    </div>
  </div>
</template>

<style scoped>
.conn {
  flex-shrink: 0;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.more {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px dashed var(--border);
}

.more label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--muted);
  font-size: 11px;
}

.w-name { width: 120px; }
.w-role { width: 120px; }
.w-mode { width: 110px; }
.w-ip { width: 120px; }

.status {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--status-off-bg);
  color: var(--status-off-fg);
}

.status.ok {
  background: var(--status-ok-bg);
  color: var(--status-ok-fg);
}

.status.wait {
  background: var(--status-wait-bg);
  color: var(--status-wait-fg);
}

.actions {
  margin-left: auto;
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
</style>
