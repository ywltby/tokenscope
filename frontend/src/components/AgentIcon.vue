<script setup lang="ts">
import { computed } from "vue";
import claudeSvg from "../assets/icons/claude.svg?raw";
import openaiSvg from "../assets/icons/openai.svg?raw";
import googlegeminiSvg from "../assets/icons/googlegemini.svg?raw";

/** 品牌图标（Simple Icons，fill=currentColor 随主题）；all 为通用四宫格。 */
const ICONS: Record<string, string> = {
  claude: claudeSvg,
  openai: openaiSvg,
  googlegemini: googlegeminiSvg,
  all: `<svg viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><rect x="3" y="3" width="8" height="8" rx="2"/><rect x="13" y="3" width="8" height="8" rx="2"/><rect x="3" y="13" width="8" height="8" rx="2"/><rect x="13" y="13" width="8" height="8" rx="2"/></svg>`,
};

const props = defineProps<{ name: string; size?: number }>();

const svg = computed(() => ICONS[props.name] ?? ICONS.all);
const box = computed(() => `${props.size ?? 20}px`);
</script>

<template>
  <span class="agent-icon" :style="{ width: box, height: box }" v-html="svg" />
</template>

<style scoped>
.agent-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.agent-icon :deep(svg) {
  width: 100%;
  height: 100%;
  display: block;
}
</style>
