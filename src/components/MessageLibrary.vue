<script setup lang="ts">
/**
 * Message catalog tree + context menu.
 * 消息目录树 + 右键菜单。
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { PrefabMessage } from "../types/session";
import {
  buildMessageTree,
  filterTree,
  isHostToEquip,
  type MsgTreeNode,
} from "../utils/messageTree";

const { t } = useI18n();

const props = defineProps<{
  messages: PrefabMessage[];
  selectedId: string | null;
  source?: string;
  canSend?: boolean;
}>();

const emit = defineEmits<{
  select: [id: string];
  importSmd: [file: File];
  add: [];
  editProperty: [message: PrefabMessage];
  editBody: [message: PrefabMessage];
  send: [message: PrefabMessage];
  copy: [message: PrefabMessage];
  remove: [id: string];
  toggleAutoReply: [id: string, value: boolean];
}>();

const filter = ref("");
const treeRef = ref<{ setCurrentKey?: (k: string | null) => void } | null>(null);

const treeData = computed(() => {
  const all = buildMessageTree(props.messages);
  return filterTree(all, filter.value);
});

const defaultExpanded = ref<string[]>([]);

watch(
  () => props.messages.length,
  () => {
    // Expand nothing by default when huge catalog (default.SMD 600+)
    if (props.messages.length <= 40) {
      defaultExpanded.value = treeData.value.map((n) => n.id);
    }
  },
  { immediate: true },
);

watch(
  () => props.selectedId,
  async (id) => {
    await nextTick();
    treeRef.value?.setCurrentKey?.(id);
  },
);

// Context menu / 右键菜单状态
const menu = ref({
  visible: false,
  x: 0,
  y: 0,
  message: null as PrefabMessage | null,
  groupKey: null as string | null,
});

function hideMenu() {
  menu.value.visible = false;
}

function showMenu(e: MouseEvent, msg: PrefabMessage | null, groupKey?: string) {
  e.preventDefault();
  e.stopPropagation();
  menu.value = {
    visible: true,
    x: e.clientX,
    y: e.clientY,
    message: msg,
    groupKey: groupKey ?? null,
  };
}

function onTreeContext(e: Event, data: MsgTreeNode) {
  const me = e as MouseEvent;
  if (data.kind === "message" && data.message) {
    showMenu(me, data.message);
    emit("select", data.message.id);
  } else if (data.kind === "body" && data.message) {
    showMenu(me, data.message);
    emit("select", data.message.id);
  } else if (data.kind === "group") {
    showMenu(me, data.message ?? null, data.groupKey);
  }
}

function onNodeClick(data: MsgTreeNode) {
  if (data.message) {
    emit("select", data.message.id);
  }
}

function onNodeDblClick(data: MsgTreeNode) {
  if (data.kind === "message" && data.message) {
    emit("select", data.message.id);
    emit("editProperty", data.message);
  } else if (data.kind === "body" && data.message) {
    emit("select", data.message.id);
    emit("editBody", data.message);
  }
}

function menuAction(action: string) {
  const m = menu.value.message;
  hideMenu();
  switch (action) {
    case "edit":
      if (m) emit("editProperty", m);
      break;
    case "body":
      if (m) emit("editBody", m);
      break;
    case "send":
      if (m) emit("send", m);
      break;
    case "copy":
      if (m) emit("copy", m);
      break;
    case "delete":
      if (m) emit("remove", m.id);
      break;
    case "toggleAr":
      if (m) emit("toggleAutoReply", m.id, !m.autoReply);
      break;
    case "add":
      emit("add");
      break;
  }
}

function onFile(ev: Event) {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (file) emit("importSmd", file);
}

function onDocClick() {
  hideMenu();
}

onMounted(() => document.addEventListener("click", onDocClick));
onUnmounted(() => document.removeEventListener("click", onDocClick));

function arrowClass(m: PrefabMessage) {
  return isHostToEquip(m.direction) ? "arr-h2e" : "arr-e2h";
}
</script>

<template>
  <div class="lib">
    <header class="lib-head">
      <div class="title-row">
        <span class="title">{{ t("library.title") }}</span>
        <span class="count">{{ messages.length }}</span>
      </div>
      <div class="tools">
        <el-input
          v-model="filter"
          size="small"
          clearable
          :placeholder="t('library.filter')"
        />
      </div>
      <div class="btns">
        <label class="file-btn">
          {{ t("library.importSmd") }}
          <input type="file" accept=".smd,.xml,text/xml" hidden @change="onFile" />
        </label>
        <el-button size="small" @click="emit('add')">+</el-button>
      </div>
      <div v-if="source" class="source" :title="source">{{ source }}</div>
    </header>

    <div class="tree-wrap" @contextmenu.prevent>
      <el-tree
        ref="treeRef"
        :data="treeData"
        node-key="id"
        :props="{ label: 'label', children: 'children' }"
        highlight-current
        :expand-on-click-node="false"
        :default-expanded-keys="defaultExpanded"
        @node-click="onNodeClick"
        @node-contextmenu="onTreeContext"
      >
        <template #default="{ data }">
          <div
            class="node"
            :class="[data.kind, { selected: data.message?.id === selectedId }]"
            @dblclick.stop="onNodeDblClick(data)"
            @contextmenu="onTreeContext($event, data)"
          >
            <template v-if="data.kind === 'group'">
              <span class="g-icon">▣</span>
              <span class="g-label">{{ data.label }}</span>
            </template>
            <template v-else-if="data.kind === 'message' && data.message">
              <span class="arr" :class="arrowClass(data.message)">
                {{ isHostToEquip(data.message.direction) ? "→" : "←" }}
              </span>
              <span v-if="data.message.autoReply" class="ar-dot" :title="t('prop.autoReply')" />
              <span class="m-label">{{ data.label }}</span>
            </template>
            <template v-else>
              <span class="body-label">{{ data.label }}</span>
            </template>
          </div>
        </template>
      </el-tree>
      <div v-if="treeData.length === 0" class="empty">
        {{ messages.length === 0 ? t("library.emptyImport") : t("library.noMatch") }}
      </div>
    </div>

    <!-- Context menu / 右键菜单 -->
    <teleport to="body">
      <ul
        v-if="menu.visible"
        class="ctx-menu"
        :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
        @click.stop
      >
        <li
          :class="{ disabled: !menu.message }"
          @click="menu.message && menuAction('edit')"
        >
          {{ t("library.editProperty") }}
        </li>
        <li
          :class="{ disabled: !menu.message }"
          @click="menu.message && menuAction('body')"
        >
          {{ t("library.editBody") }}
        </li>
        <li class="sep" />
        <li
          :class="{ disabled: !menu.message || !canSend }"
          @click="menu.message && canSend && menuAction('send')"
        >
          {{ t("library.send") }}
        </li>
        <li
          :class="{ disabled: !menu.message }"
          @click="menu.message && menuAction('toggleAr')"
        >
          {{ menu.message?.autoReply ? t("library.disableAr") : t("library.enableAr") }}
        </li>
        <li class="sep" />
        <li
          :class="{ disabled: !menu.message }"
          @click="menu.message && menuAction('copy')"
        >
          {{ t("library.copy") }}
        </li>
        <li
          :class="{ disabled: !menu.message }"
          @click="menu.message && menuAction('delete')"
        >
          {{ t("library.delete") }}
        </li>
        <li class="sep" />
        <li @click="menuAction('add')">{{ t("library.newMessage") }}</li>
      </ul>
    </teleport>
  </div>
</template>

<style scoped>
.lib {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--panel);
  border-right: 1px solid var(--border);
}

.lib-head {
  flex-shrink: 0;
  padding: 10px;
  border-bottom: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.title-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}

.title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--muted);
}

.count {
  font-size: 11px;
  color: var(--muted);
}

.tools .el-input {
  width: 100%;
}

.btns {
  display: flex;
  gap: 6px;
}

.file-btn {
  flex: 1;
  text-align: center;
  padding: 5px 8px;
  font-size: 12px;
  border-radius: 4px;
  border: 1px solid var(--border-strong);
  background: var(--btn-bg);
  color: var(--text);
  cursor: pointer;
}

.file-btn:hover {
  border-color: var(--muted);
}

.source {
  font-size: 10px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tree-wrap {
  flex: 1;
  overflow: auto;
  padding: 4px 0;
}

.tree-wrap :deep(.el-tree) {
  background: transparent;
  color: var(--text);
  --el-tree-node-hover-bg-color: var(--surface-hover);
  --el-tree-text-color: var(--text);
}

.tree-wrap :deep(.el-tree-node__content) {
  height: auto;
  min-height: 26px;
  padding: 2px 0;
}

.tree-wrap :deep(.el-tree-node.is-current > .el-tree-node__content) {
  background: var(--surface-active);
}

.node {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  min-width: 0;
  padding-right: 8px;
  font-size: 12px;
  line-height: 1.35;
}

.g-icon {
  color: var(--sf-color);
  font-size: 10px;
  flex-shrink: 0;
}

.g-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}

.arr {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 3px;
  font-size: 12px;
  font-weight: 700;
}

.arr-h2e {
  color: var(--arr-h2e-fg);
  background: var(--arr-h2e-bg);
}

.arr-e2h {
  color: var(--arr-e2h-fg);
  background: var(--arr-e2h-bg);
}

.ar-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--ar-dot);
  flex-shrink: 0;
  box-shadow: 0 0 6px color-mix(in srgb, var(--ar-dot) 55%, transparent);
}

.m-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.body-label {
  color: var(--muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, monospace;
}

.empty {
  padding: 24px 12px;
  text-align: center;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.5;
}
</style>

<style>
/* Context menu must be global (teleport) / 右键菜单需全局样式（teleport） */
.ctx-menu {
  position: fixed;
  z-index: 9999;
  min-width: 180px;
  margin: 0;
  padding: 4px 0;
  list-style: none;
  background: var(--ctx-bg);
  border: 1px solid var(--ctx-border);
  border-radius: 6px;
  box-shadow: var(--shadow);
  font-size: 12px;
  color: var(--ctx-text);
}

.ctx-menu li {
  padding: 6px 14px;
  cursor: pointer;
  user-select: none;
}

.ctx-menu li:hover:not(.disabled):not(.sep) {
  background: var(--ctx-hover);
}

.ctx-menu li.disabled {
  opacity: 0.4;
  cursor: default;
}

.ctx-menu li.sep {
  height: 1px;
  padding: 0;
  margin: 4px 8px;
  background: var(--ctx-border);
  cursor: default;
}
</style>
