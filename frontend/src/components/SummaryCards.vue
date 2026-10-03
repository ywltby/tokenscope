<script setup lang="ts">
import { computed } from "vue";
import { NCard, NGrid, NStatistic } from "naive-ui";
import { fmtCost, fmtNum, type Group } from "../types";

const props = defineProps<{ totals: Group }>();

const cards = computed(() => [
  { label: "请求数", value: fmtNum(props.totals.requests) },
  { label: "输入 token", value: fmtNum(props.totals.tokens.input) },
  { label: "输出 token", value: fmtNum(props.totals.tokens.output) },
  { label: "缓存写 token", value: fmtNum(props.totals.tokens.cache_write) },
  { label: "缓存读 token", value: fmtNum(props.totals.tokens.cache_read) },
  {
    label: props.totals.unknown_pricing ? "估算费用†" : "估算费用",
    value: `$${fmtCost(props.totals.cost_usd)}`,
  },
]);
</script>

<template>
  <NGrid :cols="6" :x-gap="12" item-responsive responsive="screen">
    <NCard v-for="c in cards" :key="c.label" size="small">
      <NStatistic :label="c.label" :value="c.value" />
    </NCard>
  </NGrid>
</template>
