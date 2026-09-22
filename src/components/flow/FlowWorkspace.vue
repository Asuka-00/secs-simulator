<script setup lang="ts">
import { computed, markRaw, nextTick, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { invoke } from "@tauri-apps/api/core";
import { VueFlow, useVueFlow, type Connection } from "@vue-flow/core";
import { Background } from "@vue-flow/background";
import { Controls } from "@vue-flow/controls";
import "@vue-flow/core/dist/style.css";
import "@vue-flow/core/dist/theme-default.css";
import "@vue-flow/controls/dist/style.css";
import type { FlowDef, FlowNodeType } from "../../types/flow";
import { FLOW_DND } from "../../types/flow";
import type { PrefabMessage } from "../../types/session";
import { useSessionStore } from "../../stores/session";
import { sxFy } from "../../types/session";
import { blankFlow, buildPreset, type PresetKind } from "./presets";
import FlowInspector from "./FlowInspector.vue";
import TriggerNode from "./nodes/TriggerNode.vue";
import SendNode from "./nodes/SendNode.vue";
import DelayNode from "./nodes/DelayNode.vue";
import WaitNode from "./nodes/WaitNode.vue";
import BranchNode from "./nodes/BranchNode.vue";

const props = defineProps<{
  sessionId: string;
  catalog: PrefabMessage[];
  canRun: boolean;
}>();

const emit = defineEmits<{ persist: [] }>();
const { t } = useI18n();
const store = useSessionStore();

const nodeTypes = {
  trigger: markRaw(TriggerNode),
  send: markRaw(SendNode),
  delay: markRaw(DelayNode),
  wait: markRaw(WaitNode),
  branch: markRaw(BranchNode),
} as any;

const flows = ref<FlowDef[]>([]);
const activeId = ref<string | null>(null);
const nodes = ref<any[]>([]);
const edges = ref<any[]>([]);
const filter = ref("");
const saving = ref(false);
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let skipWatch = false;

const flowCanvasId = computed(() => `flow-${props.sessionId}`);
const { screenToFlowCoordinate, addNodes, addEdges, onConnect, removeEdges } = useVueFlow({
  id: `flow-${props.sessionId}`,
});

function isDefaultOutlet(handle: string | null | undefined): boolean {
  return handle == null || handle === "" || handle === "source";
}

function sameSourceOutlet(
  a: { source: string; sourceHandle?: string | null },
  b: { source: string; sourceHandle?: string | null },
): boolean {
  if (a.source !== b.source) return false;
  const ah = a.sourceHandle ?? null;
  const bh = b.sourceHandle ?? null;
  if (ah === bh) return true;
  return isDefaultOutlet(ah) && isDefaultOutlet(bh);
}

onConnect((c: Connection) => {
  const stale = edges.value.filter((e) => sameSourceOutlet(e, c)).map((e) => e.id);
  if (stale.length) removeEdges(stale);
  addEdges(c);
});

const active = computed(() => flows.value.find((f) => f.id === activeId.value) ?? null);
const selectedEdge = computed(() => edges.value.find((e) => e.selected) ?? null);
const selected = computed(() =>
  selectedEdge.value ? null : (nodes.value.find((n) => n.selected) ?? null),
);
const isRunning = computed(
  () =>
    !!activeId.value && store.isFlowRunning(props.sessionId, activeId.value),
);

function mapErr(e: unknown): string {
  const s = String(e);
  if (s.includes("flow already running")) return t("flow.errRunning");
  if (s.includes("session is not open") || s.includes("session closed")) {
    return t("flow.errNotOpen");
  }
  if (s.includes("flow not found")) return t("flow.errNotFound");
  return s;
}

const ancestors = computed(() => {
  const cur = selected.value;
  if (!cur || cur.type !== "branch") return [];
  const seen = new Set<string>();
  const q = [cur.id];
  const out: any[] = [];
  while (q.length) {
    const id = q.pop()!;
    for (const e of edges.value.filter((x) => x.target === id)) {
      if (seen.has(e.source)) continue;
      seen.add(e.source);
      const n = nodes.value.find((x) => x.id === e.source);
      if (n && ["send", "wait", "trigger"].includes(n.type ?? "")) {
        out.push(n);
      }
      q.push(e.source);
    }
  }
  return out;
});

const paletteTypes = computed(() => [
  { type: "trigger" as const, label: t("flow.nodeTrigger") },
  { type: "send" as const, label: t("flow.nodeSend") },
  { type: "wait" as const, label: t("flow.nodeWait") },
  { type: "delay" as const, label: t("flow.nodeDelay") },
  { type: "branch" as const, label: t("flow.nodeBranch") },
]);

const paletteMsgs = computed(() => {
  const q = filter.value.trim().toLowerCase();
  return props.catalog.filter((m) => {
    if (!q) return true;
    return (
      sxFy(m).toLowerCase().includes(q) ||
      (m.messageName || "").toLowerCase().includes(q) ||
      (m.description || "").toLowerCase().includes(q)
    );
  }).slice(0, 80);
});

function toPersist(): FlowDef[] {
  return flows.value.map((f) =>
    f.id !== activeId.value
      ? f
      : {
          ...f,
          nodes: nodes.value.map((n) => ({
            id: n.id,
            type: (n.type ?? "send") as FlowNodeType,
            position: n.position,
            data: { ...(n.data as Record<string, unknown>) },
          })),
          edges: edges.value.map((e) => ({
            id: e.id,
            source: e.source,
            target: e.target,
            sourceHandle: e.sourceHandle,
            targetHandle: e.targetHandle,
          })),
        },
  );
}

function applyFlow(f: FlowDef | null) {
  skipWatch = true;
  if (!f) {
    nodes.value = [];
    edges.value = [];
  } else {
    nodes.value = f.nodes.map((n) => ({ ...n }));
    edges.value = f.edges.map((e) => ({
      ...e,
      selectable: true,
      deletable: true,
    }));
  }
  nextTick(() => {
    skipWatch = false;
  });
}

function scheduleSave() {
  if (skipWatch) return;
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void flushSave(), 400);
}

async function flushSave() {
  saving.value = true;
  try {
    const next = toPersist();
    flows.value = next;
    await invoke("session_set_flows", { id: props.sessionId, flows: next });
    emit("persist");
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    saving.value = false;
  }
}

watch([nodes, edges], scheduleSave, { deep: true });

watch(activeId, (id, prev) => {
  if (prev) {
    const idx = flows.value.findIndex((f) => f.id === prev);
    if (idx >= 0) flows.value[idx] = toPersist().find((f) => f.id === prev) ?? flows.value[idx];
  }
  applyFlow(flows.value.find((f) => f.id === id) ?? null);
});

watch(
  () => store.flowTick,
  (tick) => {
    if (!tick || tick.sessionId !== props.sessionId) return;
    if (tick.type === "flow_done") {
      if (!tick.flowId || tick.flowId === activeId.value) {
        nodes.value = nodes.value.map((n) => ({ ...n, class: "" }));
      }
      return;
    }
    if (tick.flowId && tick.flowId !== activeId.value) return;
    nodes.value = nodes.value.map((n) => ({
      ...n,
      class: n.id === tick.nodeId ? `is-${tick.message ?? "running"}` : "",
    }));
  },
);

async function load() {
  try {
    flows.value = await invoke<FlowDef[]>("session_get_flows", { id: props.sessionId });
    activeId.value = flows.value[0]?.id ?? null;
    applyFlow(flows.value[0] ?? null);
    await store.syncRunningFlows(props.sessionId);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

onMounted(load);
watch(() => props.sessionId, load);

function onName(v: string) {
  if (!active.value) return;
  active.value.name = v;
  scheduleSave();
}

function onEnabled(v: boolean) {
  if (!active.value) return;
  active.value.enabled = v;
  scheduleSave();
}

async function addBlank() {
  const f = blankFlow(t("flow.untitled"));
  flows.value = [...flows.value, f];
  activeId.value = f.id;
  applyFlow(f);
  await flushSave();
}

async function addPreset(kind: PresetKind) {
  const f = buildPreset(kind, props.catalog);
  f.name = t(`flow.preset.${kind}`);
  flows.value = [...flows.value, f];
  activeId.value = f.id;
  applyFlow(f);
  await flushSave();
}

async function removeFlow() {
  if (!activeId.value) return;
  flows.value = flows.value.filter((f) => f.id !== activeId.value);
  activeId.value = flows.value[0]?.id ?? null;
  applyFlow(flows.value[0] ?? null);
  await flushSave();
}

async function run() {
  if (!activeId.value) return;
  store.markFlowRunning(props.sessionId, activeId.value, true);
  try {
    await invoke("flow_run", { id: props.sessionId, flowId: activeId.value });
  } catch (e) {
    if (!String(e).includes("flow already running")) {
      store.markFlowRunning(props.sessionId, activeId.value, false);
    }
    ElMessage.error(mapErr(e));
  }
}

async function stop() {
  if (!activeId.value) return;
  try {
    await invoke("flow_stop", { id: props.sessionId, flowId: activeId.value });
  } catch (e) {
    ElMessage.error(mapErr(e));
  }
}

type DropPayload = {
  kind: string;
  type?: FlowNodeType;
  messageId?: string;
  stream?: number;
  function?: number;
  direction?: string;
  messageName?: string;
};

function setDrag(ev: DragEvent, payload: DropPayload) {
  if (!ev.dataTransfer) return;
  ev.dataTransfer.setData("text/plain", JSON.stringify(payload));
  ev.dataTransfer.effectAllowed = "copy";
}

function dragType(ev: DragEvent, type: FlowNodeType) {
  setDrag(ev, { kind: "node", type });
}

function dragMsg(ev: DragEvent, m: PrefabMessage) {
  setDrag(ev, {
    kind: "send",
    messageId: m.id,
    stream: m.stream,
    function: m.function,
    direction: m.direction,
    messageName: m.messageName,
  });
}

function dropPos(ev?: DragEvent) {
  if (ev) {
    const p = screenToFlowCoordinate({ x: ev.clientX, y: ev.clientY });
    if (p && Number.isFinite(p.x) && Number.isFinite(p.y)) return p;
  }
  return { x: 200, y: 40 + nodes.value.length * 100 };
}

function nodeData(type: FlowNodeType): Record<string, unknown> {
  if (type === "trigger") return { kind: "manual" };
  if (type === "delay") return { ms: 1000 };
  if (type === "wait") return { stream: 1, function: 1, timeoutMs: 45000 };
  if (type === "branch") {
    return { combinator: "and", clauses: [{ field: "ack", op: "eq", value: "0" }] };
  }
  return {};
}

async function ensureFlow() {
  if (activeId.value) return;
  await addBlank();
}

function placeNode(partial: {
  type: string;
  position: { x: number; y: number };
  data: Record<string, unknown>;
}) {
  addNodes({ id: crypto.randomUUID(), ...partial });
}

async function addType(type: FlowNodeType, ev?: DragEvent) {
  await ensureFlow();
  placeNode({ type, position: dropPos(ev), data: nodeData(type) });
}

async function addSend(p: DropPayload, ev?: DragEvent) {
  await ensureFlow();
  placeNode({
    type: "send",
    position: dropPos(ev),
    data: {
      messageId: p.messageId,
      stream: p.stream,
      function: p.function,
      direction: p.direction,
      messageName: p.messageName,
    },
  });
}

async function onDrop(ev: DragEvent) {
  ev.preventDefault();
  ev.stopPropagation();
  const raw =
    ev.dataTransfer?.getData("text/plain") ||
    ev.dataTransfer?.getData(FLOW_DND) ||
    "";
  if (!raw) return;
  let payload: DropPayload;
  try {
    payload = JSON.parse(raw) as DropPayload;
  } catch {
    return;
  }
  if (payload.kind === "send") {
    await addSend(payload, ev);
    return;
  }
  await addType(payload.type ?? "delay", ev);
}

function patchNode(data: Record<string, unknown>) {
  const id = selected.value?.id;
  if (!id) return;
  nodes.value = nodes.value.map((n) => (n.id === id ? { ...n, data } : n));
}

function removeNode() {
  const id = selected.value?.id;
  if (!id) return;
  nodes.value = nodes.value.filter((n) => n.id !== id);
  edges.value = edges.value.filter((e) => e.source !== id && e.target !== id);
}

function removeEdge() {
  const id = selectedEdge.value?.id;
  if (!id) return;
  removeEdges(id);
}

function onEdgeClick() {
  nodes.value = nodes.value.map((n) => (n.selected ? { ...n, selected: false } : n));
}
</script>

<template>
  <div class="flow-ws">
    <header class="bar">
      <el-select
        v-model="activeId"
        size="small"
        class="flow-sel"
        :placeholder="t('flow.noFlow')"
      >
        <el-option v-for="f in flows" :key="f.id" :label="f.name || t('flow.untitled')" :value="f.id" />
      </el-select>
      <el-input
        v-if="active"
        :model-value="active.name"
        size="small"
        class="name"
        @input="onName"
      />
      <el-tooltip v-if="active" :content="t('flow.autoHint')" placement="bottom">
        <el-switch
          :model-value="active.enabled"
          size="small"
          :active-text="t('flow.auto')"
          @change="(v: boolean) => onEnabled(v)"
        />
      </el-tooltip>
      <el-tag v-if="active" size="small" :type="isRunning ? 'success' : 'info'" effect="plain">
        {{ isRunning ? t("flow.statusRunning") : t("flow.statusIdle") }}
      </el-tag>
      <el-button size="small" type="primary" :disabled="!active || !canRun || isRunning" @click="run">
        {{ t("flow.run") }}
      </el-button>
      <el-button size="small" :disabled="!isRunning" @click="stop">{{ t("flow.stop") }}</el-button>
      <el-button size="small" @click="addBlank">{{ t("flow.new") }}</el-button>
      <el-dropdown size="small" @command="addPreset">
        <el-button size="small">{{ t("flow.presetBtn") }}</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="online">{{ t("flow.preset.online") }}</el-dropdown-item>
            <el-dropdown-item command="alarm">{{ t("flow.preset.alarm") }}</el-dropdown-item>
            <el-dropdown-item command="recipe">{{ t("flow.preset.recipe") }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
      <el-button size="small" text type="danger" :disabled="!active" @click="removeFlow">
        {{ t("flow.delete") }}
      </el-button>
      <span v-if="saving" class="muted">{{ t("flow.saving") }}</span>
    </header>

    <div class="body">
      <aside class="pal">
        <div class="sec">{{ t("flow.palette") }}</div>
        <div
          v-for="item in paletteTypes"
          :key="item.type"
          class="chip"
          draggable="true"
          @dragstart="dragType($event, item.type)"
          @click="addType(item.type)"
        >
          {{ item.label }}
        </div>
        <div class="sec">{{ t("flow.catalog") }}</div>
        <el-input v-model="filter" size="small" :placeholder="t('library.filter')" />
        <div class="msgs">
          <div
            v-for="m in paletteMsgs"
            :key="m.id"
            class="msg"
            draggable="true"
            @dragstart="dragMsg($event, m)"
            @click="addSend({ kind: 'send', messageId: m.id, stream: m.stream, function: m.function, direction: m.direction, messageName: m.messageName })"
          >
            <span class="sf">{{ sxFy(m) }}</span>
            <span class="mn">{{ m.messageName }}</span>
          </div>
        </div>
      </aside>

      <div class="canvas" @dragover.prevent @drop="onDrop">
        <VueFlow
          :id="flowCanvasId"
          v-model:nodes="nodes"
          v-model:edges="edges"
          :node-types="nodeTypes"
          fit-view-on-init
          :delete-key-code="['Backspace', 'Delete']"
          :default-edge-options="{ type: 'smoothstep', selectable: true, deletable: true }"
          @edge-click="onEdgeClick"
          @dragover.prevent
          @drop="onDrop"
        >
          <Background :gap="16" />
          <Controls />
        </VueFlow>
      </div>

      <aside class="side">
        <FlowInspector
          :node="selected"
          :edge="selectedEdge"
          :ancestors="ancestors"
          @patch="patchNode"
          @remove="removeNode"
          @remove-edge="removeEdge"
        />
      </aside>
    </div>
  </div>
</template>

<style scoped>
.flow-ws {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  min-height: 0;
  height: 100%;
  background: var(--panel);
}

.bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border);
  flex-wrap: wrap;
}

.flow-sel {
  width: 160px;
}

.name {
  width: 160px;
}

.bar :deep(.el-switch__label) {
  font-size: 12px;
  color: var(--muted);
}

.muted {
  color: var(--muted);
  font-size: 11px;
}

.body {
  display: grid;
  grid-template-columns: 180px minmax(0, 1fr) 240px;
  min-height: 0;
}

.pal,
.side {
  border-right: 1px solid var(--border);
  min-height: 0;
  overflow: auto;
  background: var(--panel);
}

.side {
  border-right: none;
  border-left: 1px solid var(--border);
}

.sec {
  padding: 8px 10px 4px;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--muted);
}

.chip {
  display: block;
  width: calc(100% - 16px);
  margin: 4px 8px;
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--btn-bg);
  color: inherit;
  cursor: grab;
  text-align: left;
  font: inherit;
  font-size: 12px;
  user-select: none;
}

.msgs {
  padding: 4px 6px 8px;
}

.msg {
  display: flex;
  gap: 6px;
  padding: 4px 6px;
  border-radius: 4px;
  cursor: grab;
  font-size: 11px;
}

.msg:hover {
  background: var(--surface-hover);
}

.sf {
  color: var(--sf-color);
  font-family: ui-monospace, monospace;
}

.mn {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--muted);
}

.canvas {
  min-width: 0;
  min-height: 0;
}

.canvas :deep(.vue-flow) {
  background: var(--surface-2);
}

.canvas :deep(.vue-flow__controls) {
  box-shadow: none;
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
}

.canvas :deep(.vue-flow__edge-path) {
  stroke: var(--muted);
}

.canvas :deep(.vue-flow__edge.selected .vue-flow__edge-path) {
  stroke: var(--arr-h2e-fg);
  stroke-width: 2.5;
}

.canvas :deep(.fn) {
  min-width: 140px;
  padding: 8px 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  box-shadow: var(--shadow);
  font-size: 12px;
}

.canvas :deep(.fn .k) {
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--muted);
}

.canvas :deep(.fn .v) {
  font-weight: 600;
  margin-top: 2px;
}

.canvas :deep(.fn .sub) {
  color: var(--muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.canvas :deep(.fn.trigger) {
  border-color: var(--arr-h2e-fg);
}

.canvas :deep(.fn.branch) {
  border-color: var(--sys-color);
}

.canvas :deep(.fn .handles) {
  display: flex;
  justify-content: space-between;
  margin-top: 6px;
  font-size: 10px;
  color: var(--muted);
}

.canvas :deep(.vue-flow__node.is-running .fn),
.canvas :deep(.vue-flow__node.is-then .fn) {
  outline: 2px solid var(--arr-h2e-fg);
}

.canvas :deep(.vue-flow__node.is-ok .fn) {
  outline: 2px solid var(--ar-dot);
}

.canvas :deep(.vue-flow__node.is-else .fn) {
  outline: 2px solid var(--arr-e2h-fg);
}
</style>
