<script setup lang="ts">
import { computed } from "vue";
import { Handle, Position } from "@vue-flow/core";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  data: {
    combinator?: string;
    clauses?: Array<{ field?: string; op?: string; value?: string }>;
  };
}>();
const { t } = useI18n();
const summary = computed(() => {
  const c = props.data.clauses?.[0];
  if (!c) return t("flow.branchEmpty");
  return `${c.field ?? "ack"} ${c.op ?? "eq"} ${c.value ?? ""}`;
});
</script>

<template>
  <div class="fn branch">
    <Handle type="target" :position="Position.Top" />
    <div class="k">{{ t("flow.nodeBranch") }}</div>
    <div class="v">{{ summary }}</div>
    <div class="handles">
      <span class="then">then</span>
      <span class="else">else</span>
    </div>
    <Handle id="then" type="source" :position="Position.Bottom" :style="{ left: '28%' }" />
    <Handle id="else" type="source" :position="Position.Bottom" :style="{ left: '72%' }" />
  </div>
</template>
