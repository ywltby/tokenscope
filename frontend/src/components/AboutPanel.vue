<script setup lang="ts">
// 设置 → 关于：只读展示正在运行的 TokenScope 版本。
//
// 版本必须来自 Tauri 的只读 `getVersion()`（运行中的程序），不能写死，也不
// 能用 package.json、环境变量或远端 Release 冒充。首次挂载读取一次；成功
// 结果保留在组件生命周期内，切分区/切语言不重复读取。失败显示本地化原因并
// 提供重试；同一时刻最多一条读取，卸载后的迟到结果不写状态。
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { NButton } from "naive-ui";
import { useT } from "../lib/i18n";

const t = useT();
const version = ref<string | null>(null);
// 错误保存**类型**而不是读取时翻译好的字符串：语言切换后原因必须跟着翻译。
// `reason` 保留后端原始诊断（DESIGN.md：后端诊断原文不翻译），外层文案由模板翻译。
type AboutError = { kind: "unavailable" } | { kind: "reason"; message: string };
const error = ref<AboutError | null>(null);
const errorMessage = computed(() => {
  const current = error.value;
  if (!current) return "";
  return current.kind === "unavailable" ? t("settings.aboutVersionUnavailable") : current.message;
});
const loading = ref(true);

let mounted = true;
let inFlight = false;
let readSeq = 0;

async function load(): Promise<void> {
  if (inFlight) return; // 并发抑制：重试/重复触发不并发第二条读取
  inFlight = true;
  loading.value = true;
  error.value = null;
  const seq = ++readSeq;
  try {
    const raw = await getVersion();
    if (!mounted || seq !== readSeq) return; // 卸载或更新一轮读取后：丢弃迟到结果
    const value = typeof raw === "string" ? raw.trim() : "";
    if (!value) {
      // 空白/非字符串按不可用处理，不把空行伪装成成功版本
      version.value = null;
      error.value = { kind: "unavailable" };
      return;
    }
    version.value = value;
  } catch (e) {
    if (!mounted || seq !== readSeq) return;
    version.value = null;
    error.value = { kind: "reason", message: e instanceof Error ? e.message : String(e) };
  } finally {
    if (mounted && seq === readSeq) {
      loading.value = false;
      inFlight = false;
    }
  }
}

onMounted(() => {
  void load();
});
onUnmounted(() => {
  mounted = false;
});
</script>

<template>
  <section class="ts-card settings-card about-card">
    <div class="about-brand">
      <!-- 复用主界面导航的品牌标记（DESIGN.md §3：线性图标，不用 emoji） -->
      <svg
        class="about-mark"
        viewBox="0 0 24 24"
        width="40"
        height="40"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9" />
        <path d="M12 12 17.2 6.8" />
        <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
      </svg>
      <div class="about-name">TokenScope</div>
    </div>
    <div class="setting-row about-version-row" role="status" aria-live="polite">
      <div class="setting-main">
        <div class="setting-label">{{ t("settings.aboutVersion") }}</div>
        <div class="setting-help">{{ t("settings.aboutVersionHelp") }}</div>
      </div>
      <span v-if="loading" class="about-version is-pending">{{ t("settings.aboutLoading") }}</span>
      <span v-else-if="version" class="about-version is-selectable ts-num" data-about-version>{{
        version
      }}</span>
      <span v-else class="about-version" aria-hidden="true">—</span>
    </div>
    <div v-if="error" class="ts-notice ts-notice-inline about-error" role="alert">
      <svg
        class="ts-notice-icon is-error"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9" />
        <path d="m9 9 6 6M15 9l-6 6" />
      </svg>
      <span class="ts-notice-content">{{
        t("settings.aboutReadFailed", { message: errorMessage })
      }}</span>
      <NButton size="tiny" :loading="loading" :disabled="loading" @click="load">{{
        t("settings.retry")
      }}</NButton>
    </div>
  </section>
</template>

<style scoped>
/* Settings.vue 的 scoped 样式不会作用到子组件内部；这里复用同一组设置行样式，
   保证版本行的 flex 对齐、行内间距与 12px 辅助文字层级一致。 */
.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ts-space-4);
  padding: var(--ts-space-3) 0;
  border-bottom: 1px solid var(--ts-separator);
}

.setting-main {
  min-width: 0;
}

.setting-label {
  font-size: 14px;
  color: var(--ts-text);
}

.setting-help {
  font-size: 12px;
  line-height: 1.5;
  color: var(--ts-text-muted);
  margin: var(--ts-space-1) 0 var(--ts-space-2);
}

.about-brand {
  display: flex;
  align-items: center;
  gap: var(--ts-space-3);
  padding: 0 0 var(--ts-space-3);
  border-bottom: 1px solid var(--ts-separator);
}

.about-mark {
  color: var(--ts-accent);
}

.about-name {
  font-size: 17px;
  font-weight: 600;
  line-height: 1.3;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.about-version-row {
  border-bottom: none;
}

.about-version {
  font-size: 14px;
  font-weight: 600;
  color: var(--ts-text);
}

.about-version.is-pending {
  font-weight: 400;
  color: var(--ts-text-secondary);
}

.about-version.is-selectable {
  user-select: text;
  -webkit-user-select: text;
}

.about-error {
  margin-top: var(--ts-space-3);
}
</style>
