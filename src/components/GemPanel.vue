<script setup lang="ts">
import { onMounted, reactive, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import type { SessionConfig } from "../types/session";

export interface BuiltinHandlers {
  s1f13: boolean;
  s1f17: boolean;
  s1f15: boolean;
  s1f1: boolean;
  s2f17: boolean;
  s6f11: boolean;
  s5f1: boolean;
}

const props = defineProps<{
  sessionId: string;
  config: SessionConfig;
  open: boolean;
}>();

const handlers = reactive<BuiltinHandlers>({
  s1f13: true,
  s1f17: true,
  s1f15: true,
  s1f1: true,
  s2f17: true,
  s6f11: true,
  s5f1: true,
});

const equipOnly = [
  { key: "s1f13" as const, label: "S1F13 → S1F14 (COMMACK)" },
  { key: "s1f17" as const, label: "S1F17 → S1F18 (ONLACK)" },
  { key: "s1f15" as const, label: "S1F15 → S1F16 (OFLACK)" },
  { key: "s1f1" as const, label: "S1F1 → S1F2 (MDLN/SOFTREV)" },
  { key: "s2f17" as const, label: "S2F17 → S2F18 (Clock)" },
];

const anyRole = [
  { key: "s6f11" as const, label: "S6F11 → S6F12 (ACKC6)" },
  { key: "s5f1" as const, label: "S5F1 → S5F2 (ACKC5)" },
];

async function load() {
  try {
    const h = await invoke<BuiltinHandlers>("session_get_handlers", {
      id: props.sessionId,
    });
    Object.assign(handlers, h);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function save() {
  try {
    await invoke("session_set_handlers", {
      id: props.sessionId,
      handlers: { ...handlers },
    });
    ElMessage.success("GEM handlers saved");
  } catch (e) {
    ElMessage.error(String(e));
  }
}

onMounted(load);
watch(() => props.sessionId, load);
</script>

<template>
  <div class="gem-panel">
    <div class="identity">
      <div class="row">
        <span class="k">Role</span>
        <span class="v">{{ config.role }}</span>
      </div>
      <div class="row">
        <span class="k">MDLN</span>
        <span class="v">{{ config.mdln }}</span>
      </div>
      <div class="row">
        <span class="k">SOFTREV</span>
        <span class="v">{{ config.softrev }}</span>
      </div>
      <div class="row">
        <span class="k">Clock</span>
        <span class="v">{{ config.clockType }}</span>
      </div>
      <p class="hint">
        MDLN/SOFTREV/Clock 在 ConnectionBar 编辑（{{ open ? "open 时锁定" : "可改" }}）。
      </p>
    </div>

    <div class="section">
      <div class="sec-title">Equip builtins</div>
      <div v-for="item in equipOnly" :key="item.key" class="toggle-row">
        <el-switch v-model="handlers[item.key]" size="small" />
        <span>{{ item.label }}</span>
      </div>
    </div>

    <div class="section">
      <div class="sec-title">Any role</div>
      <div v-for="item in anyRole" :key="item.key" class="toggle-row">
        <el-switch v-model="handlers[item.key]" size="small" />
        <span>{{ item.label }}</span>
      </div>
    </div>

    <el-button type="primary" size="small" @click="save">Apply</el-button>
  </div>
</template>

<style scoped>
.gem-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  font-size: 12px;
}

.identity {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px;
  background: #0f1012;
  border-radius: 6px;
  border: 1px solid #2c2e33;
}

.row {
  display: flex;
  gap: 8px;
}

.k {
  color: #6b7280;
  width: 64px;
}

.v {
  color: #e5e7eb;
  font-family: "SF Mono", Menlo, monospace;
}

.hint {
  margin: 4px 0 0;
  color: #6b7280;
  font-size: 11px;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.sec-title {
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: #9aa0a6;
  font-size: 11px;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
