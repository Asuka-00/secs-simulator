<script setup lang="ts">
import { onMounted, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";

export interface RuleMatch {
  stream?: number | null;
  function?: number | null;
  wbit?: boolean | null;
}

export type RuleAction =
  | { type: "builtin"; handler: string; params?: unknown }
  | { type: "sml_reply"; body: string; delayMs?: number }
  | { type: "sml_primary"; sml: string; delayMs?: number }
  | { type: "drop" }
  | { type: "log_only" };

export interface Rule {
  id: string;
  enabled: boolean;
  match: RuleMatch;
  action: RuleAction;
  continueMatch?: boolean;
}

export interface RuleSet {
  version: number;
  rules: Rule[];
}

const props = defineProps<{
  sessionId: string;
}>();

const rules = ref<Rule[]>([]);
const loading = ref(false);

const draft = reactive({
  id: "",
  enabled: true,
  stream: 2 as number | null,
  function: 41 as number | null,
  wbit: true as boolean | null,
  actionType: "sml_reply" as RuleAction["type"],
  body: '<L <B 0x00> <A "OK"> >',
  sml: "S6F11 W <L <U4 1> <U4 100> <L> >.",
  handler: "s1f14",
  continueMatch: false,
});

async function load() {
  loading.value = true;
  try {
    const set = await invoke<RuleSet>("session_get_rules", { id: props.sessionId });
    rules.value = set.rules ?? [];
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    loading.value = false;
  }
}

async function saveAll(next: Rule[]) {
  try {
    await invoke("session_set_rules", {
      id: props.sessionId,
      rules: { version: 1, rules: next },
    });
    rules.value = next;
    ElMessage.success("Rules saved");
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function actionLabel(a: RuleAction): string {
  switch (a.type) {
    case "sml_reply":
      return `sml_reply ${a.body.slice(0, 24)}…`;
    case "sml_primary":
      return `sml_primary ${a.sml.slice(0, 24)}…`;
    case "builtin":
      return `builtin:${a.handler}`;
    case "drop":
      return "drop";
    case "log_only":
      return "log_only";
  }
}

function matchLabel(m: RuleMatch): string {
  const s = m.stream != null ? `S${m.stream}` : "S*";
  const f = m.function != null ? `F${m.function}` : "F*";
  const w = m.wbit == null ? "" : m.wbit ? " W" : " ~W";
  return `${s}${f}${w}`;
}

function buildAction(): RuleAction {
  switch (draft.actionType) {
    case "sml_reply":
      return { type: "sml_reply", body: draft.body, delayMs: 0 };
    case "sml_primary":
      return { type: "sml_primary", sml: draft.sml, delayMs: 0 };
    case "builtin":
      return { type: "builtin", handler: draft.handler };
    case "drop":
      return { type: "drop" };
    case "log_only":
      return { type: "log_only" };
  }
}

async function addRule() {
  const id = draft.id.trim() || `rule-${Date.now().toString(36)}`;
  if (rules.value.some((r) => r.id === id)) {
    ElMessage.warning("Rule id already exists");
    return;
  }
  const rule: Rule = {
    id,
    enabled: draft.enabled,
    match: {
      stream: draft.stream ?? undefined,
      function: draft.function ?? undefined,
      wbit: draft.wbit ?? undefined,
    },
    action: buildAction(),
    continueMatch: draft.continueMatch,
  };
  await saveAll([...rules.value, rule]);
  draft.id = "";
}

async function removeRule(id: string) {
  await saveAll(rules.value.filter((r) => r.id !== id));
}

async function toggleRule(id: string, enabled: boolean) {
  await saveAll(
    rules.value.map((r) => (r.id === id ? { ...r, enabled } : r)),
  );
}

onMounted(load);
watch(() => props.sessionId, load);
</script>

<template>
  <div class="rules-panel" v-loading="loading">
    <div class="list">
      <div v-if="rules.length === 0" class="empty">No custom rules</div>
      <div v-for="r in rules" :key="r.id" class="rule-row">
        <el-switch
          :model-value="r.enabled"
          size="small"
          @change="(v: boolean) => toggleRule(r.id, v)"
        />
        <div class="meta">
          <div class="id">{{ r.id }}</div>
          <div class="sub">{{ matchLabel(r.match) }} · {{ actionLabel(r.action) }}</div>
        </div>
        <el-button size="small" text type="danger" @click="removeRule(r.id)">×</el-button>
      </div>
    </div>

    <div class="form">
      <div class="sec-title">Add rule</div>
      <el-input v-model="draft.id" size="small" placeholder="id (optional)" />
      <div class="row">
        <el-input-number v-model="draft.stream" size="small" :min="0" :max="127" controls-position="right" />
        <el-input-number v-model="draft.function" size="small" :min="0" :max="255" controls-position="right" />
        <el-select v-model="draft.wbit" size="small" style="width: 90px" clearable placeholder="W">
          <el-option label="W" :value="true" />
          <el-option label="~W" :value="false" />
        </el-select>
      </div>
      <el-select v-model="draft.actionType" size="small" style="width: 100%">
        <el-option label="sml_reply" value="sml_reply" />
        <el-option label="sml_primary" value="sml_primary" />
        <el-option label="builtin" value="builtin" />
        <el-option label="drop" value="drop" />
        <el-option label="log_only" value="log_only" />
      </el-select>
      <el-input
        v-if="draft.actionType === 'sml_reply'"
        v-model="draft.body"
        type="textarea"
        :rows="3"
        size="small"
        placeholder='Body: <L <B 0x00> <A "OK"> >'
      />
      <el-input
        v-if="draft.actionType === 'sml_primary'"
        v-model="draft.sml"
        type="textarea"
        :rows="3"
        size="small"
        placeholder="Full SML primary ending with ."
      />
      <el-input
        v-if="draft.actionType === 'builtin'"
        v-model="draft.handler"
        size="small"
        placeholder="handler e.g. s1f14"
      />
      <el-checkbox v-model="draft.continueMatch" size="small" label="continue after hit" />
      <el-button type="primary" size="small" @click="addRule">Add</el-button>
    </div>
  </div>
</template>

<style scoped>
.rules-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  font-size: 12px;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 180px;
  overflow: auto;
}

.rule-row {
  display: grid;
  grid-template-columns: auto 1fr auto;
  gap: 8px;
  align-items: center;
  padding: 6px 8px;
  background: #0f1012;
  border: 1px solid #2c2e33;
  border-radius: 6px;
}

.id {
  font-weight: 600;
  color: #e5e7eb;
}

.sub {
  color: #6b7280;
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty {
  color: #6b7280;
  padding: 8px;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-top: 1px solid #2c2e33;
  padding-top: 8px;
}

.sec-title {
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: #9aa0a6;
  font-size: 11px;
}

.row {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.form :deep(textarea) {
  font-family: "SF Mono", Menlo, Monaco, Consolas, monospace;
  font-size: 11px;
}
</style>
