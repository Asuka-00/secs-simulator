<script setup lang="ts">
import { computed } from "vue";
import { Handle, Position } from "@vue-flow/core";

const props = defineProps<{
  data: {
    stream?: number;
    function?: number;
    messageName?: string;
  };
}>();

const sx = computed(() =>
  props.data.stream != null && props.data.function != null
    ? `S${props.data.stream}F${props.data.function}`
    : "S?F?",
);
</script>

<template>
  <div class="fn send" @dragover.prevent>
    <Handle type="target" :position="Position.Top" />
    <div class="v">{{ sx }}</div>
    <div v-if="data.messageName" class="sub">{{ data.messageName }}</div>
    <Handle type="source" :position="Position.Bottom" />
  </div>
</template>
