<script setup lang="ts">
import { computed } from "vue";
import { Handle, Position } from "@vue-flow/core";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  data: {
    kind?: string;
    stream?: number;
    function?: number;
    intervalMs?: number;
  };
}>();

const { t } = useI18n();

const label = computed(() => {
  switch (props.data.kind) {
    case "onSelected":
      return t("flow.kindSelected");
    case "onInbound":
      return `S${props.data.stream ?? "?"}F${props.data.function ?? "?"}`;
    case "interval":
      return `${props.data.intervalMs ?? 5000} ms`;
    default:
      return t("flow.kindManual");
  }
});
</script>

<template>
  <div class="fn trigger">
    <div class="k">{{ t("flow.nodeTrigger") }}</div>
    <div class="v">{{ label }}</div>
    <Handle type="source" :position="Position.Bottom" />
  </div>
</template>
