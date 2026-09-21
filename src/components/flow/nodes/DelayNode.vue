<script setup lang="ts">
import { Handle, Position } from "@vue-flow/core";
import { useI18n } from "vue-i18n";

defineProps<{ data: { ms?: number } }>();
const { t } = useI18n();
</script>

<template>
  <div class="fn delay">
    <Handle type="target" :position="Position.Top" />
    <div class="k">{{ t("flow.nodeDelay") }}</div>
    <div class="v">{{ data.ms ?? 0 }} ms</div>
    <Handle type="source" :position="Position.Bottom" />
  </div>
</template>
