<script setup lang="ts">
/**
 * Transaction log list + detail pane.
 * 事务日志列表与详情面板。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import type { LogEntry } from "../types/session";

const props = defineProps<{
  entries: LogEntry[];
  sessionName?: string;
}>();

const emit = defineEmits<{
  clear: [];
}>();

const { t } = useI18n();
const filter = ref("");
const dir = ref<"all" | "tx" | "rx" | "system">("all");
const selectedId = ref<string | null>(null);

const filtered = computed(() => {
  const q = filter.value.trim().toLowerCase();
  return props.entries.filter((e) => {
    if (dir.value !== "all" && e.direction !== dir.value) return false;
    if (!q) return true;
    const hay = `${e.summary} ${e.sml ?? ""} ${e.hex ?? ""}`.toLowerCase();
    return hay.includes(q);
  });
});

const selected = computed(
  () => props.entries.find((e) => e.id === selectedId.value) ?? null,
);

function time(ms: number) {
  const d = new Date(ms);
  return d.toLocaleTimeString(undefined, { hour12: false }) +
    "." +
    String(d.getMilliseconds()).padStart(3, "0");
}

function dirLabel(d: string) {
  if (d === "tx") return t("log.tx");
  if (d === "rx") return t("log.rx");
  if (d === "system") return t("log.sys");
  return d;
}

/** Title without trailing " W" / 去掉末尾 W 后的标题 */
function summaryTitle(e: LogEntry): string {
  return e.summary.replace(/\s+W\b/, "").trim();
}

function hasWbit(e: LogEntry): boolean {
  return e.wbit === true || /\sW\b/.test(e.summary);
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    /* ignore */
  }
}
</script>

<template>
  <div class="log">
    <header class="log-head">
      <span class="title">{{ t("log.title") }}</span>
      <span class="count">{{ filtered.length }}</span>
      <el-select v-model="dir" size="small" style="width: 100px">
        <el-option :label="t('log.all')" value="all" />
        <el-option :label="t('log.tx')" value="tx" />
        <el-option :label="t('log.rx')" value="rx" />
        <el-option :label="t('log.sys')" value="system" />
      </el-select>
      <el-input
        v-model="filter"
        size="small"
        clearable
        :placeholder="t('log.search')"
        style="width: 160px"
      />
      <el-button size="small" text type="danger" @click="emit('clear')">
        {{ t("log.clear") }}
      </el-button>
    </header>

    <div class="log-body" :class="{ 'has-detail': !!selected }">
      <div class="rows">
        <button
          v-for="e in filtered"
          :key="e.id"
          type="button"
          class="row"
          :class="[e.direction, { active: e.id === selectedId }]"
          @click="selectedId = e.id === selectedId ? null : e.id"
        >
          <span class="t">{{ time(e.timestampMs) }}</span>
          <span class="d">{{ dirLabel(e.direction) }}</span>
          <span class="s">{{ e.summary }}</span>
        </button>
        <div v-if="filtered.length === 0" class="empty">{{ t("log.empty") }}</div>
      </div>
      <div v-if="selected" class="detail">
        <div class="detail-head">
          <div class="detail-summary">
            <span class="sf">{{ summaryTitle(selected) }}</span>
            <span v-if="hasWbit(selected)" class="w-badge">W</span>
          </div>
          <div class="detail-actions">
            <el-button
              v-if="selected.sml"
              size="small"
              text
              @click="copy(selected.sml!)"
            >
              {{ t("log.copySml") }}
            </el-button>
            <el-button
              v-if="selected.hex"
              size="small"
              text
              @click="copy(selected.hex!)"
            >
              {{ t("log.copyHex") }}
            </el-button>
          </div>
        </div>
        <pre
          v-if="!selected.sml && !selected.hex"
          class="pre"
        >{{ selected.summary }}</pre>
        <pre v-if="selected.sml" class="pre">{{ selected.sml }}</pre>
        <pre v-if="selected.hex" class="pre hex">{{ selected.hex }}</pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  border-top: 1px solid var(--border);
  background: var(--panel);
}

.log-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
}

.title {
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--muted);
}

.count {
  font-size: 11px;
  color: var(--muted);
  margin-right: auto;
}

.log-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 1fr;
  grid-template-rows: 1fr;
}

.log-body.has-detail {
  grid-template-rows: minmax(0, 1fr) minmax(140px, 40%);
}

.rows {
  overflow: auto;
  font-size: 11px;
}

.row {
  width: 100%;
  display: grid;
  grid-template-columns: 96px 36px 1fr;
  gap: 8px;
  padding: 3px 10px;
  border: none;
  border-bottom: 1px solid var(--log-row-border);
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
  font: inherit;
}

.row:hover,
.row.active {
  background: var(--surface-hover);
}

.row.tx .d { color: var(--status-ok-fg); }
.row.rx .d { color: var(--arr-h2e-fg); }
.row.system .d { color: var(--sys-color); }

.t { color: var(--muted); }
.s {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail {
  border-top: 1px solid var(--border);
  overflow: auto;
  padding: 8px 10px;
  background: var(--code-bg);
  min-height: 0;
}

.detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 12px;
}

.detail-summary {
  display: flex;
  align-items: flex-start;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
  flex: 1 1 auto;
  font-weight: 600;
}

.detail-summary .sf {
  white-space: pre-wrap;
  word-break: break-word;
}

.w-badge {
  flex-shrink: 0;
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
  padding: 2px 5px;
  border-radius: 3px;
  background: var(--w-badge-bg);
  color: var(--w-badge-fg);
  letter-spacing: 0.02em;
}

.detail-actions {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.pre {
  margin: 0 0 8px;
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 11px;
  color: var(--pre-color);
}

.pre.hex {
  color: var(--pre-hex-color);
}

.empty {
  padding: 16px;
  color: var(--muted);
  font-size: 12px;
}
</style>
