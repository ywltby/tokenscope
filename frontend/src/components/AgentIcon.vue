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
  // WB06：两个 WorkBuddy 应用没有可用的品牌图标——仓库自绘的中性字母
  // 徽标（笔画式 path，fill/stroke=currentColor 随主题；不用 <text>，
  // 使"纯图标无文字"的契约保持成立；不引入第三方商标素材）。
  workbuddy: `<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="1.5" y="1.5" width="21" height="21" rx="5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M4.2 8 L6.7 16 L8.9 10.6 L11.1 16 L13.6 8" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/><path d="M15.4 8 h2.3 a1.5 1.5 0 0 1 0 3 h-2.3 z M15.4 11.5 h2.6 a1.5 1.5 0 0 1 0 3 h-2.6 z" fill="currentColor"/><path d="M15.4 8 v6.5" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>`,
  "workbuddy-ai": `<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="1.5" y="1.5" width="21" height="21" rx="5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M4.2 8 L6.7 16 L8.9 10.6 L11.1 16 L13.6 8" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/><path d="M15 15.6 L17.1 8.4 L19.2 15.6 M15.9 13.1 h2.4" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
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
  vertical-align: middle;
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
