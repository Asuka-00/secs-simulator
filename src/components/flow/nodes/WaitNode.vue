<script setup lang="ts">
import { computed } from "vue";
import { Handle, Position } from "@vue-flow/core";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  data: { stream?: number; function?: number; timeoutMs?: number };
}>();
const { t } = useI18n();
const sx = computed(() => `S${props.data.stream ?? "?"}F${props.data.function ?? "?"}`);
</script>

<template>
  <div class="fn wait" @dragover.prevent>
    <Handle type="target" :position="Position.Top" />
    <div class="k">{{ t("flow.nodeWait") }}</div>
    <div class="v">{{ sx }}</div>
    <div class="sub">{{ data.timeoutMs ?? 45000 }} ms</div>
    <Handle type="source" :position="Position.Bottom" />
  </div>
</template>
