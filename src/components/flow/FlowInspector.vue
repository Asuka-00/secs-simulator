<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { BranchClause, FlowNodeType } from "../../types/flow";

interface InspNode {
  id: string;
  type?: string;
  data: Record<string, unknown>;
}

interface InspEdge {
  id: string;
  source: string;
  target: string;
}

const props = defineProps<{
  node: InspNode | null;
  edge?: InspEdge | null;
  ancestors: InspNode[];
}>();

const emit = defineEmits<{
  patch: [data: Record<string, unknown>];
  remove: [];
  removeEdge: [];
}>();

const { t } = useI18n();

const ty = computed(() => (props.node?.type ?? "") as FlowNodeType | "");

function set(key: string, value: unknown) {
  if (!props.node) return;
  emit("patch", { ...props.node.data, [key]: value });
}

const clauses = computed<BranchClause[]>(() => {
  const raw = props.node?.data?.clauses;
  return Array.isArray(raw) ? (raw as BranchClause[]) : [];
});

function patchClause(i: number, next: BranchClause) {
  const list = clauses.value.slice();
  list[i] = next;
  set("clauses", list);
}

function addClause() {
  set("clauses", [
    ...clauses.value,
    { field: "ack", op: "eq", value: "0" } satisfies BranchClause,
  ]);
}

function rmClause(i: number) {
  set(
    "clauses",
    clauses.value.filter((_, idx) => idx !== i),
  );
}

function ancestorLabel(n: InspNode): string {
  const d = n.data as { messageName?: string; stream?: number; function?: number };
  const sx =
    d.stream != null && d.function != null ? `S${d.stream}F${d.function}` : n.type;
  return `${sx} (${n.id.slice(0, 6)})`;
}
</script>

<template>
  <div class="insp">
    <template v-if="edge">
      <div class="hd">
        <span>{{ t("flow.inspEdge") }}</span>
        <el-button size="small" text type="danger" @click="emit('removeEdge')">
          {{ t("flow.deleteEdge") }}
        </el-button>
      </div>
    </template>
    <template v-else-if="node">
      <div class="hd">
        <span>{{
          ty === "trigger"
            ? t("flow.nodeTrigger")
            : ty === "send"
              ? t("flow.nodeSend")
              : ty === "delay"
                ? t("flow.nodeDelay")
                : ty === "wait"
                  ? t("flow.nodeWait")
                  : t("flow.nodeBranch")
        }}</span>
        <el-button size="small" text type="danger" @click="emit('remove')">
          {{ t("flow.deleteNode") }}
        </el-button>
      </div>

      <template v-if="ty === 'trigger'">
        <el-select
          :model-value="node.data.kind ?? 'manual'"
          size="small"
          @change="(v: string) => set('kind', v)"
        >
          <el-option :label="t('flow.kindManual')" value="manual" />
          <el-option :label="t('flow.kindSelected')" value="onSelected" />
          <el-option :label="t('flow.kindInbound')" value="onInbound" />
          <el-option :label="t('flow.kindInterval')" value="interval" />
        </el-select>
        <template v-if="node.data.kind === 'onInbound'">
          <div class="row">
            <el-input-number
              :model-value="(node.data.stream as number) ?? 1"
              size="small"
              :min="0"
              :max="127"
              controls-position="right"
              @change="(v: number | undefined) => set('stream', v ?? 1)"
            />
            <el-input-number
              :model-value="(node.data.function as number) ?? 1"
              size="small"
              :min="0"
              :max="255"
              controls-position="right"
              @change="(v: number | undefined) => set('function', v ?? 1)"
            />
          </div>
        </template>
        <div
          v-if="node.data.kind && node.data.kind !== 'manual'"
          class="hint"
        >
          {{ t("flow.triggerAutoHint") }}
        </div>
        <el-input-number
          v-if="node.data.kind === 'interval'"
          :model-value="(node.data.intervalMs as number) ?? 5000"
          size="small"
          :min="200"
          :step="500"
          controls-position="right"
          @change="(v: number | undefined) => set('intervalMs', v ?? 5000)"
        />
      </template>

      <template v-else-if="ty === 'send'">
        <div class="send-drop">
          <div class="hint">{{ t("flow.sendHint") }}</div>
          <div class="mono">
            S{{ node.data.stream ?? "?" }}F{{ node.data.function ?? "?" }}
            <span v-if="node.data.messageName"> · {{ node.data.messageName }}</span>
          </div>
        </div>
      </template>

      <template v-else-if="ty === 'delay'">
        <el-input-number
          :model-value="(node.data.ms as number) ?? 0"
          size="small"
          :min="0"
          :step="100"
          controls-position="right"
          @change="(v: number | undefined) => set('ms', v ?? 0)"
        />
      </template>

      <template v-else-if="ty === 'wait'">
        <div class="row">
          <el-input-number
            :model-value="(node.data.stream as number) ?? 1"
            size="small"
            :min="0"
            :max="127"
            controls-position="right"
            @change="(v: number | undefined) => set('stream', v ?? 1)"
          />
          <el-input-number
            :model-value="(node.data.function as number) ?? 1"
            size="small"
            :min="0"
            :max="255"
            controls-position="right"
            @change="(v: number | undefined) => set('function', v ?? 1)"
          />
        </div>
        <el-input-number
          :model-value="(node.data.timeoutMs as number) ?? 45000"
          size="small"
          :min="100"
          :step="1000"
          controls-position="right"
          @change="(v: number | undefined) => set('timeoutMs', v ?? 45000)"
        />
      </template>

      <template v-else-if="ty === 'branch'">
        <el-select
          :model-value="(node.data.combinator as string) ?? 'and'"
          size="small"
          @change="(v: string) => set('combinator', v)"
        >
          <el-option label="AND" value="and" />
          <el-option label="OR" value="or" />
        </el-select>
        <div v-for="(c, i) in clauses" :key="i" class="clause">
          <el-select
            :model-value="c.nodeId ?? ''"
            size="small"
            clearable
            :placeholder="t('flow.lastResult')"
            @change="(v: string) => patchClause(i, { ...c, nodeId: v || undefined })"
          >
            <el-option
              v-for="a in ancestors"
              :key="a.id"
              :label="ancestorLabel(a)"
              :value="a.id"
            />
          </el-select>
          <el-select
            :model-value="c.field"
            size="small"
            @change="(v: BranchClause['field']) => patchClause(i, { ...c, field: v })"
          >
            <el-option label="ACK" value="ack" />
            <el-option label="SxFy" value="sxFy" />
            <el-option :label="t('flow.bodyPath')" value="bodyPath" />
            <el-option label="SML" value="sml" />
          </el-select>
          <el-input
            v-if="c.field === 'bodyPath'"
            :model-value="c.path ?? ''"
            size="small"
            placeholder="0.1"
            @input="(v: string) => patchClause(i, { ...c, path: v })"
          />
          <div class="row">
            <el-select
              :model-value="c.op"
              size="small"
              style="width: 90px"
              @change="(v: BranchClause['op']) => patchClause(i, { ...c, op: v })"
            >
              <el-option label="=" value="eq" />
              <el-option label="≠" value="ne" />
              <el-option label=">" value="gt" />
              <el-option label="<" value="lt" />
              <el-option :label="t('flow.contains')" value="contains" />
            </el-select>
            <el-input
              :model-value="c.value"
              size="small"
              @input="(v: string) => patchClause(i, { ...c, value: v })"
            />
          </div>
          <el-button size="small" text type="danger" @click="rmClause(i)">×</el-button>
        </div>
        <el-button size="small" @click="addClause">{{ t("flow.addClause") }}</el-button>
      </template>
    </template>
    <div v-else class="empty">{{ t("flow.inspHint") }}</div>
  </div>
</template>

<style scoped>
.insp {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  font-size: 12px;
  min-height: 0;
  overflow: auto;
}

.hd {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
}

.empty,
.hint,
.sub {
  color: var(--muted);
  font-size: 11px;
}

.row {
  display: flex;
  gap: 6px;
}

.mono {
  font-family: ui-monospace, monospace;
}

.clause {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-2);
}

.send-drop {
  min-height: 72px;
  padding: 8px;
  border: 1px dashed var(--border);
  border-radius: 6px;
  background: var(--surface-2);
}
</style>
