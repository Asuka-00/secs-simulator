import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { FlowTick } from "../types/flow";
import {
  defaultSessionConfig,
  type LogEntry,
  type SessionConfig,
  type SessionEvent,
  type SessionSummary,
} from "../types/session";

export const useSessionStore = defineStore("session", () => {
  const sessions = ref<SessionSummary[]>([]);
  const configs = ref<Record<string, SessionConfig>>({});
  /** Per-session log buffer (live from events + initial pull). */
  const logsBySession = ref<Record<string, LogEntry[]>>({});
  const activeSessionId = ref<string | null>(null);
  const busy = ref(false);
  const flowTick = ref<FlowTick | null>(null);
  /** Backend-owned run set; survives Flow tab unmount. */
  const runningBySession = ref<Record<string, string[]>>({});
  let unlisten: UnlistenFn | null = null;

  const activeSession = computed(
    () => sessions.value.find((s) => s.id === activeSessionId.value) ?? null,
  );

  const activeConfig = computed(() => {
    const id = activeSessionId.value;
    if (!id) return null;
    return configs.value[id] ?? null;
  });

  const activeLogs = computed(() => {
    const id = activeSessionId.value;
    if (!id) return [] as LogEntry[];
    return logsBySession.value[id] ?? [];
  });

  function applySummary(summary: SessionSummary) {
    const idx = sessions.value.findIndex((s) => s.id === summary.id);
    if (idx >= 0) {
      sessions.value[idx] = summary;
    } else {
      sessions.value.push(summary);
    }
  }

  function appendLog(sessionId: string, entry: LogEntry) {
    const cur = logsBySession.value[sessionId] ?? [];
    // Dedup by id if event + pull race
    if (cur.some((e) => e.id === entry.id)) return;
    logsBySession.value[sessionId] = [...cur, entry];
  }

  function applyEvent(ev: SessionEvent) {
    if (ev.type === "state") {
      const idx = sessions.value.findIndex((s) => s.id === ev.sessionId);
      if (idx < 0) return;
      const cur = sessions.value[idx];
      sessions.value[idx] = {
        ...cur,
        open: ev.open ?? cur.open,
        hsmsState: ev.hsms ?? cur.hsmsState,
      };
    } else if (ev.type === "log" && ev.entry) {
      appendLog(ev.sessionId, ev.entry);
    } else if (ev.type === "flow_progress" || ev.type === "flow_done") {
      if (ev.flowId) {
        markFlowRunning(ev.sessionId, ev.flowId, ev.type === "flow_progress");
      }
      flowTick.value = {
        sessionId: ev.sessionId,
        type: ev.type,
        flowId: ev.flowId,
        nodeId: ev.nodeId,
        message: ev.message,
      };
    }
  }

  async function startEventListen() {
    if (unlisten) return;
    unlisten = await listen<SessionEvent>("session-event", (e) => {
      applyEvent(e.payload);
    });
  }

  async function refreshList() {
    sessions.value = await invoke<SessionSummary[]>("session_list");
    if (
      activeSessionId.value &&
      !sessions.value.some((s) => s.id === activeSessionId.value)
    ) {
      activeSessionId.value = sessions.value[0]?.id ?? null;
    }
  }

  async function loadLogs(id: string) {
    const entries = await invoke<LogEntry[]>("session_get_logs", { id });
    logsBySession.value[id] = entries;
  }

  async function clearLogs(id: string) {
    await invoke("session_clear_logs", { id });
    logsBySession.value[id] = [];
  }

  async function createSession(partial?: Partial<SessionConfig>) {
    busy.value = true;
    try {
      const config = defaultSessionConfig({
        name: `Session ${sessions.value.length + 1}`,
        ...partial,
      });
      if (partial?.role === "host" || config.role === "host") {
        config.mode = partial?.mode ?? "active";
      }
      const summary = await invoke<SessionSummary>("session_create", { config });
      await refreshList();
      configs.value[summary.id] = await invoke<SessionConfig>("session_get_config", {
        id: summary.id,
      });
      logsBySession.value[summary.id] = [];
      activeSessionId.value = summary.id;
      return summary;
    } finally {
      busy.value = false;
    }
  }

  async function removeSession(id: string) {
    busy.value = true;
    try {
      await invoke("session_remove", { id });
      delete configs.value[id];
      delete logsBySession.value[id];
      delete runningBySession.value[id];
      if (activeSessionId.value === id) {
        activeSessionId.value = null;
      }
      await refreshList();
      if (!activeSessionId.value && sessions.value.length > 0) {
        await selectSession(sessions.value[0].id);
      }
    } finally {
      busy.value = false;
    }
  }

  async function selectSession(id: string) {
    activeSessionId.value = id;
    if (!configs.value[id]) {
      configs.value[id] = await invoke<SessionConfig>("session_get_config", { id });
    }
    // Pull history (events may have been missed while another tab was active)
    try {
      await loadLogs(id);
    } catch {
      /* ignore */
    }
  }

  async function saveConfig(id: string, config: SessionConfig) {
    busy.value = true;
    try {
      const summary = await invoke<SessionSummary>("session_update_config", {
        id,
        config,
      });
      configs.value[id] = config;
      applySummary(summary);
      return summary;
    } finally {
      busy.value = false;
    }
  }

  async function openSession(id: string) {
    busy.value = true;
    try {
      const summary = await invoke<SessionSummary>("session_open", { id });
      applySummary(summary);
      await loadLogs(id);
      return summary;
    } finally {
      busy.value = false;
    }
  }

  async function closeSession(id: string) {
    busy.value = true;
    try {
      const summary = await invoke<SessionSummary>("session_close", { id });
      applySummary(summary);
      runningBySession.value = { ...runningBySession.value, [id]: [] };
      await loadLogs(id);
      return summary;
    } finally {
      busy.value = false;
    }
  }

  function setActive(id: string | null) {
    activeSessionId.value = id;
  }

  function markFlowRunning(sessionId: string, flowId: string, on: boolean) {
    const cur = new Set(runningBySession.value[sessionId] ?? []);
    if (on) cur.add(flowId);
    else cur.delete(flowId);
    runningBySession.value = { ...runningBySession.value, [sessionId]: [...cur] };
  }

  function isFlowRunning(sessionId: string, flowId: string) {
    return (runningBySession.value[sessionId] ?? []).includes(flowId);
  }

  async function syncRunningFlows(sessionId: string) {
    try {
      const ids = await invoke<string[]>("session_running_flows", { id: sessionId });
      runningBySession.value = { ...runningBySession.value, [sessionId]: ids };
    } catch {
      /* session closed / not open */
    }
  }

  async function persistState() {
    await invoke("app_save_state");
  }

  async function exportScenario(name?: string): Promise<string> {
    return invoke<string>("scenario_export", { name: name ?? "secs-scenario" });
  }

  async function importScenarioJson(json: string) {
    busy.value = true;
    try {
      await invoke<number>("scenario_import", { json });
      configs.value = {};
      logsBySession.value = {};
      runningBySession.value = {};
      activeSessionId.value = null;
      await refreshList();
      // Reload configs for all sessions
      for (const s of sessions.value) {
        configs.value[s.id] = await invoke<SessionConfig>("session_get_config", {
          id: s.id,
        });
        logsBySession.value[s.id] = [];
      }
      if (sessions.value[0]) {
        await selectSession(sessions.value[0].id);
      }
    } finally {
      busy.value = false;
    }
  }

  return {
    sessions,
    configs,
    logsBySession,
    activeSessionId,
    activeSession,
    activeConfig,
    activeLogs,
    busy,
    flowTick,
    runningBySession,
    markFlowRunning,
    isFlowRunning,
    syncRunningFlows,
    startEventListen,
    refreshList,
    loadLogs,
    clearLogs,
    createSession,
    removeSession,
    selectSession,
    saveConfig,
    openSession,
    closeSession,
    setActive,
    persistState,
    exportScenario,
    importScenarioJson,
  };
});
