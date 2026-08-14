<script setup lang="ts">
/**
 * Tree editor for SECS-II body (ItemName + values).
 * SECS-II 正文树形编辑器（ItemName + 值）。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { BodyItem, PrefabMessage } from "../types/session";
import { sxFy } from "../types/session";
import {
  ITEM_TYPES,
  addChild,
  bodyItemLabel,
  bodyTreeToSml,
  ensureBodyTree,
  newLeaf,
  newList,
  removeNode,
} from "../utils/secsBody";

const props = defineProps<{
  modelValue: boolean;
  message: PrefabMessage | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  save: [message: PrefabMessage];
}>();

const { t } = useI18n();
const roots = ref<BodyItem[]>([]);
const selectedId = ref<string | null>(null);
const showSml = ref(false);
const expanded = ref<string[]>([]);

const selected = computed(() => {
  if (!selectedId.value) return null;
  return findById(roots.value, selectedId.value);
});

const treeData = computed(() => roots.value.map(toElNode));

function toElNode(n: BodyItem): {
  id: string;
  label: string;
  item: BodyItem;
  children?: ReturnType<typeof toElNode>[];
} {
  return {
    id: n.id,
    label: bodyItemLabel(n),
    item: n,
    children: (n.children ?? []).map(toElNode),
  };
}

function findById(items: BodyItem[], id: string): BodyItem | null {
  for (const n of items) {
    if (n.id === id) return n;
    const c = findById(n.children ?? [], id);
    if (c) return c;
  }
  return null;
}

function collectIds(items: BodyItem[], out: string[] = [], depth = 0): string[] {
  for (const n of items) {
    if (depth < 3) out.push(n.id);
    if (n.children?.length) collectIds(n.children, out, depth + 1);
  }
  return out;
}

watch(
  () => [props.modelValue, props.message] as const,
  ([open, m]) => {
    if (open && m) {
      roots.value = ensureBodyTree(m.bodyTree, m.bodySml ?? "");
      expanded.value = collectIds(roots.value);
      selectedId.value = roots.value[0]?.id ?? null;
      showSml.value = false;
    }
  },
  { immediate: true },
);

function onNodeClick(data: { id: string }) {
  selectedId.value = data.id;
}

function onAddRoot() {
  roots.value = [...roots.value, newList("ROOT", [])];
  selectedId.value = roots.value[roots.value.length - 1].id;
}

function onAddChild() {
  const parentId = selectedId.value;
  const parent = parentId ? findById(roots.value, parentId) : null;
  if (parent && parent.type.toUpperCase() === "L") {
    roots.value = addChild(roots.value, parentId, newLeaf("U2", "", "0"));
    expanded.value = [...new Set([...expanded.value, parentId!])];
  } else if (!parentId) {
    onAddRoot();
  } else {
    // Add sibling under same parent by converting selection path: append under list root
    roots.value = addChild(roots.value, null, newLeaf("U2", "", "0"));
  }
  // re-select last added is hard; keep parent selected
}

function onAddListChild() {
  const parentId = selectedId.value;
  const parent = parentId ? findById(roots.value, parentId) : null;
  if (parent && parent.type.toUpperCase() === "L") {
    roots.value = addChild(roots.value, parentId, newList("", []));
    expanded.value = [...new Set([...expanded.value, parentId!])];
  } else {
    roots.value = [...roots.value, newList("", [])];
  }
}

function onDelete() {
  if (!selectedId.value) return;
  const id = selectedId.value;
  roots.value = removeNode(roots.value, id);
  selectedId.value = roots.value[0]?.id ?? null;
}

function onTypeChange() {
  // force tree label refresh
  roots.value = JSON.parse(JSON.stringify(roots.value));
}

function close() {
  emit("update:modelValue", false);
}

function onOk() {
  if (!props.message) return;
  const bodyTree = JSON.parse(JSON.stringify(roots.value)) as BodyItem[];
  const bodySml = bodyTreeToSml(bodyTree);
  emit("save", {
    ...props.message,
    bodyTree,
    bodySml,
  });
  close();
}

const smlPreview = computed(() => bodyTreeToSml(roots.value));
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    width="720px"
    class="body-dlg"
    append-to-body
    destroy-on-close
    :close-on-click-modal="false"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <template #header>
      <div class="dlg-title">{{ t("body.title") }}</div>
      <div v-if="message" class="dlg-sub">
        {{ sxFy(message) }} · {{ message.messageName }}
      </div>
    </template>

    <div class="toolbar">
      <el-button size="small" @click="onAddRoot">{{ t("body.addRootL") }}</el-button>
      <el-button size="small" @click="onAddListChild">{{ t("body.addChildL") }}</el-button>
      <el-button size="small" @click="onAddChild">{{ t("body.addChildLeaf") }}</el-button>
      <el-button size="small" type="danger" plain :disabled="!selectedId" @click="onDelete">
        {{ t("body.delete") }}
      </el-button>
      <el-button size="small" text @click="showSml = !showSml">
        {{ showSml ? t("body.hideSml") : t("body.showSml") }}
      </el-button>
    </div>

    <div class="workspace">
      <div class="tree-pane">
        <el-tree
          v-if="treeData.length"
          :data="treeData"
          node-key="id"
          highlight-current
          :expand-on-click-node="false"
          :default-expanded-keys="expanded"
          :current-node-key="selectedId ?? undefined"
          @node-click="onNodeClick"
        >
          <template #default="{ data }">
            <span class="tree-label" :class="{ list: data.item.type === 'L' }">
              <span class="ty">{{ data.item.type }}</span>
              <span class="txt">{{ data.label }}</span>
            </span>
          </template>
        </el-tree>
        <div v-else class="empty">{{ t("body.emptyTree") }}</div>
      </div>

      <div class="edit-pane">
        <template v-if="selected">
          <label>
            {{ t("body.type") }}
            <el-select v-model="selected.type" size="small" @change="onTypeChange">
              <el-option v-for="ty in ITEM_TYPES" :key="ty" :label="ty" :value="ty" />
            </el-select>
          </label>
          <label>
            {{ t("body.itemName") }}
            <el-input v-model="selected.name" size="small" placeholder="e.g. DATAID" @input="onTypeChange" />
          </label>
          <label v-if="selected.type.toUpperCase() !== 'L'">
            {{ t("body.value") }}
            <el-input
              v-model="selected.value"
              size="small"
              type="textarea"
              :rows="3"
              :placeholder="selected.type === 'A' ? t('body.phAscii') : t('body.phNum')"
              @input="onTypeChange"
            />
          </label>
          <div v-else class="list-hint">
            {{ t("body.listHint", { count: selected.children?.length ?? 0 }) }}
          </div>
          <div class="preview-label">{{ t("body.label") }}</div>
          <code class="preview">{{ bodyItemLabel(selected) }}</code>
        </template>
        <div v-else class="empty">{{ t("body.selectNode") }}</div>
      </div>
    </div>

    <div v-if="showSml" class="sml-box">
      <div class="sml-title">{{ t("body.smlAuto") }}</div>
      <pre>{{ smlPreview || t("body.empty") }}</pre>
    </div>

    <template #footer>
      <el-button size="small" @click="close">{{ t("body.close") }}</el-button>
      <el-button size="small" type="primary" @click="onOk">{{ t("body.ok") }}</el-button>
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
  color: var(--muted, #8b919a);
  font-family: ui-monospace, monospace;
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 10px;
}
.workspace {
  display: grid;
  grid-template-columns: 1fr 240px;
  gap: 12px;
  min-height: 320px;
  max-height: 420px;
}
.tree-pane {
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: auto;
  background: var(--tree-bg);
  padding: 6px 0;
}
.tree-pane :deep(.el-tree) {
  background: transparent;
  color: var(--text);
  --el-tree-node-hover-bg-color: var(--surface-hover);
}
.tree-pane :deep(.el-tree-node.is-current > .el-tree-node__content) {
  background: var(--surface-active);
}
.tree-label {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-family: ui-monospace, monospace;
  min-width: 0;
}
.tree-label .ty {
  color: var(--sf-color);
  font-weight: 600;
  min-width: 28px;
}
.tree-label.list .ty {
  color: var(--list-ty-color);
}
.tree-label .txt {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.edit-pane {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  background: var(--panel);
  overflow: auto;
}
.edit-pane label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 11px;
  color: var(--muted);
}
.list-hint {
  font-size: 12px;
  color: var(--muted);
}
.preview-label {
  font-size: 11px;
  color: var(--muted);
}
.preview {
  font-size: 11px;
  color: var(--sf-color);
  word-break: break-all;
}
.empty {
  padding: 24px 12px;
  text-align: center;
  color: var(--muted);
  font-size: 12px;
}
.sml-box {
  margin-top: 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px;
  background: var(--code-bg);
}
.sml-title {
  font-size: 11px;
  color: var(--muted);
  margin-bottom: 4px;
}
.sml-box pre {
  margin: 0;
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--pre-color);
  max-height: 120px;
  overflow: auto;
}
</style>
