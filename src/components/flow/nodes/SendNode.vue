<script setup lang="ts">
import { computed } from "vue";
import { Handle, Position } from "@vue-flow/core";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  data: {
    stream?: number;
    function?: number;
    messageName?: string;
  };
}>();

const { t } = useI18n();
const sx = computed(() =>
  props.data.stream != null && props.data.function != null
    ? `S${props.data.stream}F${props.data.function}`
    : "S?F?",
);
</script>

<template>
  <div class="fn send">
    <Handle type="target" :position="Position.Top" />
    <div class="k">{{ t("flow.nodeSend") }}</div>
    <div class="v">{{ sx }}</div>
    <div v-if="data.messageName" class="sub">{{ data.messageName }}</div>
    <Handle type="source" :position="Position.Bottom" />
  </div>
</template>
