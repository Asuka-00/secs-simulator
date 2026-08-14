<script setup lang="ts">
/**
 * Shell: session list + header + active SessionView.
 * 外壳：会话列表 + 顶栏 + 当前 SessionView。
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import zhCn from "element-plus/es/locale/lang/zh-cn";
import en from "element-plus/es/locale/lang/en";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSessionStore } from "./stores/session";
import SessionView from "./components/SessionView.vue";
import type { SessionConfig, SessionEvent } from "./types/session";
import { setAppLocale, type AppLocale } from "./i18n";
import {
  getAppTheme,
  setAppTheme,
  type AppTheme,
} from "./theme";

const { t, locale } = useI18n();
const store = useSessionStore();
let unlistenError: UnlistenFn | null = null;

const localeModel = computed({
  get: () => locale.value as AppLocale,
  set: (v: AppLocale) => setAppLocale(v),
});

const themeModel = ref<AppTheme>(getAppTheme());

function onThemeChange(v: AppTheme) {
  setAppTheme(v);
  themeModel.value = v;
}

const epLocale = computed(() => (locale.value === "zh-CN" ? zhCn : en));

onMounted(async () => {
  try {
    await store.startEventListen();
    unlistenError = await listen<SessionEvent>("session-event", (e) => {
      if (e.payload.type === "error" && e.payload.message) {
        ElMessage.error(`[${e.payload.sessionId.slice(0, 8)}] ${e.payload.message}`);
      } else if (e.payload.type === "send_done" && e.payload.message) {
        ElMessage.success(`[${e.payload.sessionId.slice(0, 8)}] ${e.payload.message}`);
      }
    });
    await store.loadVersion();
    await store.refreshList();
    if (store.sessions.length === 0) {
      await store.createSession({
        name: "Equip",
        role: "equipment",
        mode: "passive",
        port: 5000,
        sessionId: 10,
        linktestEnabled: false,
      });
      await store.createSession({
        name: "Host",
        role: "host",
        mode: "active",
        port: 5000,
        sessionId: 10,
        linktestEnabled: false,
      });
      try {
        await store.persistState();
      } catch {
        /* first-run persist optional / 首次持久化可失败 */
      }
    } else if (!store.activeSessionId && store.sessions[0]) {
      await store.selectSession(store.sessions[0].id);
    }
  } catch (e) {
    ElMessage.error(t("msg.initFailed", { error: String(e) }));
  }
});

onUnmounted(() => {
  unlistenError?.();
});

async function onAdd() {
  try {
    await store.createSession();
    await store.persistState();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onAddLoopback() {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const list = await invoke<{ id: string }[]>("session_create_loopback_pair", {
      port: 5000,
    });
    await store.refreshList();
    if (list[0]) await store.selectSession(list[0].id);
    await store.persistState();
    ElMessage.success(t("msg.pairCreated"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onRemove(id: string) {
  try {
    await ElMessageBox.confirm(t("msg.removeSession"), t("msg.confirm"), {
      type: "warning",
    });
    await store.removeSession(id);
    await store.persistState();
  } catch {
    /* cancelled / 用户取消 */
  }
}

async function onSaveConfig(config: SessionConfig) {
  if (!store.activeSessionId) return;
  try {
    await store.saveConfig(store.activeSessionId, config);
    await store.persistState();
    ElMessage.success(t("msg.configApplied"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onOpen() {
  if (!store.activeSessionId) return;
  try {
    await store.openSession(store.activeSessionId);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onClose() {
  if (!store.activeSessionId) return;
  try {
    await store.closeSession(store.activeSessionId);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onClearLogs() {
  if (!store.activeSessionId) return;
  try {
    await store.clearLogs(store.activeSessionId);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function onRefreshLogs() {
  if (!store.activeSessionId) return;
  try {
    await store.loadLogs(store.activeSessionId);
  } catch {
    /* live events usually enough / 实时事件通常已覆盖 */
  }
}

async function onPersist() {
  try {
    await store.persistState();
  } catch {
    /* ignore */
  }
}
</script>

<template>
  <el-config-provider :locale="epLocale">
    <div class="app-shell">
      <header class="app-header">
        <div class="brand">
          <span class="brand-title">{{ t("app.title") }}</span>
          <span class="brand-sub">{{ t("app.subtitle") }}</span>
        </div>
        <div class="header-meta">
          <el-select
            v-model="themeModel"
            size="small"
            class="theme-select"
            :title="t('app.theme')"
            @change="onThemeChange"
          >
            <el-option :label="t('app.themeLight')" value="light" />
            <el-option :label="t('app.themeDark')" value="dark" />
          </el-select>
          <el-select
            v-model="localeModel"
            size="small"
            class="lang-select"
            :title="t('app.lang')"
          >
            <el-option :label="t('app.zh')" value="zh-CN" />
            <el-option :label="t('app.en')" value="en-US" />
          </el-select>
          <el-tag v-if="store.secs4rsVersion" type="success" size="small" effect="dark">
            secs4rs {{ store.secs4rsVersion }}
          </el-tag>
        </div>
      </header>

      <div class="app-body">
        <aside class="sidebar">
          <div class="sidebar-head">
            <span>{{ t("app.sessions") }}</span>
            <div class="sidebar-actions">
              <el-button
                size="small"
                :title="t('app.pairTitle')"
                @click="onAddLoopback"
              >
                {{ t("app.pair") }}
              </el-button>
              <el-button size="small" type="primary" :loading="store.busy" @click="onAdd">
                +
              </el-button>
            </div>
          </div>
          <div class="session-list">
            <button
              v-for="s in store.sessions"
              :key="s.id"
              type="button"
              class="session-item"
              :class="{ active: s.id === store.activeSessionId }"
              @click="store.selectSession(s.id)"
            >
              <span
                class="dot"
                :class="{
                  on: s.open && s.hsmsState === 'Selected',
                  wait: s.open && s.hsmsState !== 'Selected',
                }"
              />
              <span class="name">{{ s.name }}</span>
              <span class="state">{{ s.hsmsState }}</span>
              <button type="button" class="rm" @click.stop="onRemove(s.id)">×</button>
            </button>
            <div v-if="store.sessions.length === 0" class="sidebar-empty">
              {{ t("app.noSessions") }}
            </div>
          </div>
        </aside>

        <main class="main-panel">
          <SessionView
            v-if="store.activeSession && store.activeConfig"
            :key="store.activeSession.id"
            :summary="store.activeSession"
            :config="store.activeConfig"
            :logs="store.activeLogs"
            :busy="store.busy"
            @save="onSaveConfig"
            @open="onOpen"
            @close="onClose"
            @clear-logs="onClearLogs"
            @refresh-logs="onRefreshLogs"
            @persist="onPersist"
          />
          <div v-else class="empty-main">{{ t("app.emptyMain") }}</div>
        </main>
      </div>
    </div>
  </el-config-provider>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
  color: var(--text);
}

.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--header-bg);
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.brand-title {
  font-weight: 600;
  font-size: 14px;
}

.brand-sub {
  color: var(--muted);
  font-size: 11px;
}

.header-meta {
  display: flex;
  align-items: center;
  gap: 8px;
}

.lang-select {
  width: 110px;
}

.theme-select {
  width: 100px;
}

.app-body {
  display: flex;
  flex: 1;
  min-height: 0;
}

.sidebar {
  width: 200px;
  border-right: 1px solid var(--border);
  background: var(--panel);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}

.sidebar-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--muted);
}

.sidebar-actions {
  display: flex;
  gap: 4px;
}

.session-list {
  flex: 1;
  overflow: auto;
  padding: 6px;
}

.session-item {
  width: 100%;
  display: grid;
  grid-template-columns: 10px 1fr auto auto;
  align-items: center;
  gap: 6px;
  padding: 8px;
  margin-bottom: 4px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  cursor: pointer;
  text-align: left;
  font: inherit;
}

.session-item:hover {
  background: var(--surface-hover);
}

.session-item.active {
  background: var(--surface-active);
  border-color: var(--surface-active-border);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--status-off-fg);
}

.dot.on {
  background: var(--ar-dot);
  box-shadow: 0 0 6px color-mix(in srgb, var(--ar-dot) 55%, transparent);
}

.dot.wait {
  background: var(--status-wait-fg);
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.state {
  color: var(--muted);
  font-size: 10px;
}

.rm {
  border: none;
  background: transparent;
  color: var(--danger);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0 2px;
}

.sidebar-empty {
  padding: 16px 8px;
  color: var(--muted);
  font-size: 12px;
}

.main-panel {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.empty-main {
  margin: auto;
  color: var(--muted);
}
</style>
