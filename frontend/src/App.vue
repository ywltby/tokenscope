<script setup lang="ts">
// 应用壳（设计系统 Task 2，苹果风格）：吸顶玻璃导航 + 分段控件页面/主题
// 切换 + 滚动容器。视觉改造不改数据流：回到汇总页仍强制刷新一次（设置页
// 可能重建了缓存）。
import { computed, onBeforeUnmount, onMounted, provide, ref, watch, watchEffect } from "vue";
import {
  dateZhCN,
  zhCN,
  darkTheme,
  NConfigProvider,
  NGlobalStyle,
  NMessageProvider,
  type GlobalTheme,
} from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTheme, type ThemePreference } from "./composables/theme";
import { naiveThemeOverrides } from "./styles/naiveTheme";
import SegmentedControl from "./components/SegmentedControl.vue";
import PricingStatusBanner from "./components/PricingStatusBanner.vue";
import CloseConfirmDialog from "./components/CloseConfirmDialog.vue";
import Dashboard from "./views/Dashboard.vue";
import Settings from "./views/Settings.vue";
import { createSettingsPreload, SETTINGS_PRELOAD } from "./lib/settingsPreload";
import { useTokenColors } from "./composables/tokenColors";

const tokenColors = useTokenColors();
watchEffect(() => {
  for (const [key, color] of Object.entries(tokenColors.colors.value)) {
    document.documentElement.style.setProperty(`--ts-chart-${key.replaceAll("_", "-")}`, color);
  }
});

const settingsPreload = createSettingsPreload(invoke);
provide(SETTINGS_PRELOAD, settingsPreload);
// 首屏先挂载；只预读数据，不提前渲染隐藏的价格表。
onMounted(() => {
  settingsPreload.start();
});

const { preference, mode, setPreference } = useTheme();
const theme = computed<GlobalTheme | null>(() => (mode.value === "dark" ? darkTheme : null));
// Naive UI 覆盖与 CSS token 双轨消费同一语义色；html[data-theme] 驱动
// tokens.css 的深色分支（首渲染前绑定，切换不产生首帧闪烁）。
const themeOverrides = computed(() => naiveThemeOverrides(mode.value));
watchEffect(() => {
  document.documentElement.dataset.theme = mode.value;
});

const page = ref<"summary" | "settings">("summary");
// 回到汇总页时强制刷新一次数据（设置页可能重建了缓存）。
const refreshKey = ref(0);
watch(page, (p) => {
  if (p === "summary") refreshKey.value += 1;
});

const pageOptions = [
  { value: "summary" as const, label: "汇总" },
  { value: "settings" as const, label: "设置" },
];

const themeOptions: { value: ThemePreference; label: string; ariaLabel: string }[] = [
  // UX02：字符图标（☀/☾）不合规范且语义不明——改用通用 SVG 插槽 +
  // 显式语义名称（"浅色模式 / 深色模式 / 跟随系统"）。
  { value: "light", label: "浅色", ariaLabel: "浅色模式" },
  { value: "dark", label: "深色", ariaLabel: "深色模式" },
  { value: "system", label: "跟随系统", ariaLabel: "跟随系统" },
];

// ── 启动诊断（SF06）：日志初始化降级为非阻断通知 ───────────────
// 文件日志不可用（state != ok）时提示用户；读取失败静默（非 Tauri
// 环境/测试环境），绝不因诊断失败递归报错或阻塞。
const logWarning = ref<string | null>(null);
onMounted(async () => {
  try {
    const st = await invoke<{ state: string; message: string | null }>("startup_diagnostics");
    if (st.state !== "ok" && st.message) logWarning.value = st.message;
  } catch {
    // 非 Tauri 环境：静默跳过
  }
});

// ── 关闭确认弹窗（关闭确认与配置文件计划 Task 3）──────────────
// 后端在未记忆默认动作时拦截关窗并 emit close-requested；这里弹窗
// 询问（最小化/退出/取消 + 记忆勾选），决定经 close_resolve 回传。
// AP07：已记忆默认动作的执行失败（隐藏窗口失败等）由后端 emit
// close-action-failed——窗口仍然可见，这里显示原因并保持弹窗打开，
// 用户可重试（再选一次）或取消，与未记忆路径共用同一结果处理。
const closeDialogOpen = ref(false);
type CloseActionFailure = { action?: string; reason?: string };
let unlistenClose: (() => void)[] = [];
onMounted(() => {
  const register = (event: string, handler: (payload: unknown) => void): void => {
    listen(event, handler)
      .then((un) => {
        unlistenClose.push(un);
      })
      .catch(() => {
        // 非 Tauri 环境（浏览器预览/测试）没有事件系统：静默跳过
      });
  };
  register("close-requested", () => {
    // 弹窗已开时忽略重复关闭请求（防止事件叠加）
    if (!closeDialogOpen.value) closeDialogOpen.value = true;
  });
  register("close-action-failed", (ev) => {
    const reason = (ev as { payload?: CloseActionFailure } | null)?.payload?.reason ?? "";
    closeSubmitError.value = `关闭操作失败，请重试或取消：${reason}`;
    closeDialogOpen.value = true;
  });
});
onBeforeUnmount(() => {
  for (const un of unlistenClose) un();
  unlistenClose = [];
});

// R07：关闭决定异步执行——失败（写记忆配置/隐藏/退出前错误）时弹窗
// 保持打开并显示原因，用户可重试或取消；成功后由窗口动作结束。
const closeSubmitting = ref(false);
const closeSubmitError = ref<string | null>(null);

async function onCloseResolve(v: { minimize: boolean; remember: boolean }): Promise<void> {
  if (closeSubmitting.value) return; // 防重复提交
  closeSubmitting.value = true;
  closeSubmitError.value = null;
  try {
    await invoke("close_resolve", { minimize: v.minimize, remember: v.remember });
    closeDialogOpen.value = false;
  } catch (e) {
    closeSubmitError.value = `关闭操作失败，请重试或取消：${e instanceof Error ? e.message : String(e)}`;
  } finally {
    closeSubmitting.value = false;
  }
}

function onCloseCancel(): void {
  if (closeSubmitting.value) return;
  closeDialogOpen.value = false;
}
</script>

<template>
  <NConfigProvider
    :theme="theme"
    :theme-overrides="themeOverrides"
    :locale="zhCN"
    :date-locale="dateZhCN"
  >
    <NGlobalStyle />
    <NMessageProvider>
      <div class="app-shell">
        <div class="scroll-container">
          <!-- R04 审核：导航与内容同一滚动上下文——内容滚动时从导航后方
               经过，玻璃模糊才真正有内容可透（吸顶 header 在滚动容器内） -->
          <header class="app-nav ts-glass">
            <span class="brand">
              <!-- 品牌标记：表盘/示波器意象的线性图标（DESIGN.md §3 禁 emoji 图标） -->
              <svg
                class="brand-mark"
                viewBox="0 0 24 24"
                width="18"
                height="18"
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
              TokenScope
            </span>
            <SegmentedControl v-model="page" :options="pageOptions" aria-label="页面切换" />
            <div class="spacer" />
            <SegmentedControl
              v-model="preference"
              :options="themeOptions"
              icon-only
              aria-label="主题偏好（浅色/深色/跟随系统）"
              @update:model-value="setPreference"
            >
              <!-- 通用 SVG 图标插槽：装饰图形对读屏隐藏，真实名称由按钮
                   aria-label（浅色模式/深色模式/跟随系统）承载 -->
              <template #icon="{ option }">
                <svg
                  v-if="option.value === 'light'"
                  viewBox="0 0 24 24"
                  width="16"
                  height="16"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                >
                  <circle cx="12" cy="12" r="4.2" />
                  <path
                    d="M12 2.8v2.2M12 19v2.2M4.6 4.6l1.6 1.6M17.8 17.8l1.6 1.6M2.8 12H5M19 12h2.2M4.6 19.4l1.6-1.6M17.8 6.2l1.6-1.6"
                  />
                </svg>
                <svg
                  v-else-if="option.value === 'dark'"
                  viewBox="0 0 24 24"
                  width="16"
                  height="16"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M20 14.2A8.2 8.2 0 0 1 9.8 4a8.4 8.4 0 1 0 10.2 10.2z" />
                </svg>
                <svg
                  v-else
                  viewBox="0 0 24 24"
                  width="16"
                  height="16"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <rect x="3" y="4.5" width="18" height="12" rx="2" />
                  <path d="M8 20h8M12 16.5V20" />
                </svg>
              </template>
            </SegmentedControl>
          </header>
          <!-- 全局状态横幅：渲染在内容之前，不遮挡主体 -->
          <div class="banner-slot">
            <div v-if="logWarning" class="ts-notice" role="status">
              文件日志不可用，已退回备用输出：{{ logWarning }}
            </div>
            <PricingStatusBanner />
          </div>
          <main class="app-content">
            <Dashboard
              v-if="page === 'summary'"
              :refresh-key="refreshKey"
              @go-settings="page = 'settings'"
            />
            <KeepAlive>
              <Settings v-if="page === 'settings'" :refresh-key="refreshKey" />
            </KeepAlive>
          </main>
        </div>
        <!-- 关闭确认弹窗：未记忆默认动作时由后端触发 -->
        <CloseConfirmDialog
          :open="closeDialogOpen"
          :submitting="closeSubmitting"
          :error="closeSubmitError"
          @resolve="onCloseResolve"
          @cancel="onCloseCancel"
        />
      </div>
    </NMessageProvider>
  </NConfigProvider>
</template>

<style scoped>
.app-shell {
  height: 100vh;
  /* F06：主滚动容器必须有界——flex 纵向布局把 .scroll-container 约束在
     剩余视口高度内；修复前容器随内容长到全高，内部滚动失效、document
     成为第二条主滚动条，sticky 导航随文档滚走。 */
  display: flex;
  flex-direction: column;
}

.app-nav {
  position: sticky;
  top: 0;
  z-index: 10;
  height: 52px;
  display: flex;
  align-items: center;
  gap: var(--ts-space-4);
  padding: 0 var(--ts-space-8);
}

.brand {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.spacer {
  flex: 1;
}

.scroll-container {
  flex: 1;
  /* F06：min-height:0 允许 flex 子项收缩到内容以下——overflow-y:auto
     才能成为真实滚动口（默认 min-height:auto 会重新撑开容器）。 */
  min-height: 0;
  overflow-y: auto;
}

.banner-slot {
  padding: var(--ts-space-4) var(--ts-space-8) 0;
}

.app-content {
  padding: var(--ts-space-6) var(--ts-space-8);
  max-width: 1200px;
  width: 100%;
  margin: 0 auto;
  box-sizing: border-box;
}

/* 窗口接近最小宽度 980px 时左右边距降为 20px（DESIGN.md §4） */
@media (max-width: 1100px) {
  .app-nav {
    padding: 0 var(--ts-space-5);
  }
  .banner-slot {
    padding: var(--ts-space-4) var(--ts-space-5) 0;
  }
  .app-content {
    padding: var(--ts-space-5);
  }
}
</style>
