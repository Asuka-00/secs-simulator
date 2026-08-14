<script setup lang="ts">
/**
 * SECS message property dialog (Name / SxFy / Wait / AR / Direction).
 * SECS 消息属性弹窗（名称 / SxFy / Wait / 自动应答 / 方向）。
 */
import { computed, reactive, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { PrefabMessage } from "../types/session";
import { sxFy } from "../types/session";
import { isHostToEquip, messageLeafLabel } from "../utils/messageTree";

const props = defineProps<{
  modelValue: boolean;
  message: PrefabMessage | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  save: [message: PrefabMessage];
}>();

const { t } = useI18n();

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
  bodyTree: [] as PrefabMessage["bodyTree"],
});

watch(
  () => [props.modelValue, props.message] as const,
  ([open, m]) => {
    if (open && m) Object.assign(draft, m);
  },
  { immediate: true },
);

const title = computed(() => {
  if (!props.message) return t("prop.title");
  return messageLeafLabel({ ...draft } as PrefabMessage);
});

const dirHost = computed({
  get: () => isHostToEquip(draft.direction),
  set: (v: boolean) => {
    draft.direction = v ? "H->E" : "H<-E";
  },
});

function close() {
  emit("update:modelValue", false);
}

function onOk() {
  const msg: PrefabMessage = {
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
    bodyTree: draft.bodyTree,
  };
  emit("save", msg);
  close();
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    width="420px"
    class="prop-dialog"
    append-to-body
    destroy-on-close
    :close-on-click-modal="false"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <template #header>
      <div class="dlg-title">{{ t("prop.title") }}</div>
      <div class="dlg-sub">{{ title }}</div>
    </template>

    <div class="form">
      <label class="row">
        <span class="lbl">{{ t("prop.name") }}</span>
        <el-input v-model="draft.messageName" size="small" />
      </label>
      <label class="row">
        <span class="lbl">{{ t("prop.description") }}</span>
        <el-input v-model="draft.description" size="small" />
      </label>
      <div class="row two">
        <label>
          <span class="lbl">{{ t("prop.stream") }}</span>
          <el-input-number v-model="draft.stream" size="small" :min="0" :max="127" controls-position="right" />
        </label>
        <label>
          <span class="lbl">{{ t("prop.function") }}</span>
          <el-input-number v-model="draft.function" size="small" :min="0" :max="255" controls-position="right" />
        </label>
      </div>
      <label class="row">
        <span class="lbl">{{ t("prop.pair") }}</span>
        <el-input v-model="draft.pairName" size="small" placeholder="S2F5" />
      </label>

      <div class="checks">
        <el-checkbox v-model="draft.wait">{{ t("prop.wait") }}</el-checkbox>
        <el-checkbox v-model="draft.autoReply">{{ t("prop.autoReply") }}</el-checkbox>
        <el-checkbox v-model="draft.noLogging">{{ t("prop.noLogging") }}</el-checkbox>
      </div>

      <div class="dirs">
        <el-radio-group v-model="dirHost" size="small">
          <el-radio :value="true">{{ t("prop.hostToEqp") }}</el-radio>
          <el-radio :value="false">{{ t("prop.eqpToHost") }}</el-radio>
        </el-radio-group>
      </div>
    </div>

    <template #footer>
      <el-button size="small" @click="close">{{ t("prop.close") }}</el-button>
      <el-button size="small" type="primary" @click="onOk">{{ t("prop.ok") }}</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.dlg-title {
  font-weight: 600;
  font-size: 14px;
}
.dlg-sub {
  margin-top: 4px;
  font-size: 11px;
  color: var(--muted);
  font-family: ui-monospace, monospace;
  word-break: break-all;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.row.two {
  flex-direction: row;
  gap: 16px;
}
.row.two label {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.lbl {
  font-size: 12px;
  color: var(--muted);
}
.checks {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}
.dirs {
  padding-top: 4px;
}
</style>
