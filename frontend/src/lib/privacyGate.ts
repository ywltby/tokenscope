/// 隐私同意引导状态（P04/P05）：只发引导 IPC 的状态模块。
///
/// 边界：本模块与其组件链**不得**导入会读 localStorage 的业务模块
/// （composables/theme、composables/tokenColors、lib/settingsPreload、
/// views/*）；导入它们会在首屏就读持久化偏好。业务偏好恢复只发生在
/// App.vue 收到后端 Ready 之后的显式调用里。
import { computed, ref, type ComputedRef, type Ref } from "vue";

export type PrivacyPhase =
  | "checking"
  | "needs_consent"
  | "saving_consent"
  | "starting"
  | "ready"
  | "blocked_error"
  | "exiting";

export interface PolicyInfo {
  title: string;
  date: string;
  markdown: string;
}

export interface PrivacySnapshot {
  phase: PrivacyPhase;
  detail: string | null;
  exitPromptPending: boolean;
  diagnostics: string[];
  policy: PolicyInfo;
}

/** 引导 IPC 的最小调用面（与 @tauri-apps/api/core 的 invoke 同形）。 */
export type InvokeFn = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

const PHASES: readonly PrivacyPhase[] = [
  "checking",
  "needs_consent",
  "saving_consent",
  "starting",
  "ready",
  "blocked_error",
  "exiting",
];

/** 后端不可用/响应异常时的兜底政策（保持标题与空正文，不伪造内容）。 */
const EMPTY_POLICY: PolicyInfo = { title: "TokenScope 隐私政策", date: "", markdown: "" };

function normalize(raw: unknown): PrivacySnapshot {
  const value = (raw ?? {}) as Partial<PrivacySnapshot>;
  const phase = PHASES.includes(value.phase as PrivacyPhase)
    ? (value.phase as PrivacyPhase)
    : "blocked_error";
  const policy = value.policy;
  return {
    phase,
    detail: typeof value.detail === "string" ? value.detail : null,
    exitPromptPending: value.exitPromptPending === true,
    diagnostics: Array.isArray(value.diagnostics) ? value.diagnostics.map(String) : [],
    policy:
      policy && typeof policy.markdown === "string"
        ? {
            title: String(policy.title ?? EMPTY_POLICY.title),
            date: String(policy.date ?? ""),
            markdown: policy.markdown,
          }
        : EMPTY_POLICY,
  };
}

function describe(e: unknown): string {
  if (e instanceof Error) return e.message;
  return typeof e === "string" ? e : String(e);
}

/** 引导门：一次会话一个实例（App.vue 里创建）。 */
export function createPrivacyGate(invoke: InvokeFn) {
  const snapshot = ref<PrivacySnapshot | null>(null);
  /** 保存/检查失败的可展示原因（IPC 层错误）。 */
  const error = ref<string | null>(null);
  const exitPromptOpen = ref(false);
  const exitBusy = ref(false);
  const exitError = ref<string | null>(null);

  let acceptInFlight = false;
  let pendingExit = false;

  const phase = computed<PrivacyPhase>(() => snapshot.value?.phase ?? "checking");
  const policy = computed<PolicyInfo>(() => snapshot.value?.policy ?? EMPTY_POLICY);
  const saving = computed(() => phase.value === "saving_consent" || phase.value === "starting");
  /** 后端明确报告阻断（损坏/不可读设置、保存失败）：允许「重新检查」。 */
  const blocked = computed(
    () => phase.value === "blocked_error" || (phase.value === "checking" && error.value !== null),
  );
  /** 政策弹窗在非检查/非就绪/非退出阶段常驻（不预选同意、遮罩与 Esc 不关闭）。 */
  const consentVisible = computed(
    () => phase.value !== "checking" && phase.value !== "ready" && phase.value !== "exiting",
  );
  const blockedDetail = computed(() => snapshot.value?.detail ?? error.value);

  function apply(next: PrivacySnapshot): void {
    snapshot.value = next;
    syncExitPrompt();
  }

  /** 保存中不允许穿插另一条关闭动作：先记下待处理，落定后再显示确认。 */
  function requestExitPrompt(): void {
    if (phase.value === "saving_consent" || phase.value === "starting") {
      pendingExit = true;
      return;
    }
    if (phase.value === "ready" || phase.value === "exiting") return; // 业务接管/已在退出
    exitError.value = null;
    exitPromptOpen.value = true;
  }

  function syncExitPrompt(): void {
    // 关闭事件早于前端监听：由 bootstrap 响应的标记恢复显示。
    if (snapshot.value?.exitPromptPending) requestExitPrompt();
    if (pendingExit && (phase.value === "needs_consent" || phase.value === "blocked_error")) {
      pendingExit = false;
      exitError.value = null;
      exitPromptOpen.value = true;
    }
    if (phase.value === "ready") {
      // 同意成功：关闭询问自动作废（窗口继续存在），不得留下遮挡或堆积弹窗。
      pendingExit = false;
      exitPromptOpen.value = false;
      exitError.value = null;
    }
  }

  /** 检查必要设置（幂等；失败保持封闭并提供重试）。 */
  async function bootstrap(): Promise<void> {
    try {
      apply(normalize(await invoke("privacy_bootstrap")));
      error.value = null;
    } catch (e) {
      error.value = `无法检查隐私设置：${describe(e)}`;
    }
  }

  /** 只在「同意并继续」按钮被点击时调用；重复点击是单飞的。 */
  async function accept(): Promise<void> {
    if (acceptInFlight) return;
    if (phase.value !== "needs_consent" && phase.value !== "blocked_error") return;
    acceptInFlight = true;
    error.value = null;
    snapshot.value = { ...(snapshot.value ?? normalize(null)), phase: "saving_consent" };
    try {
      apply(normalize(await invoke("privacy_accept")));
    } catch (e) {
      // 保存失败：保留政策可见、显示原因、可重试或退出；业务闸门仍关闭。
      error.value = `保存同意记录失败：${describe(e)}`;
      snapshot.value = { ...(snapshot.value ?? normalize(null)), phase: "blocked_error" };
      syncExitPrompt();
    } finally {
      acceptInFlight = false;
    }
  }

  /** 「不同意」：进入专属退出确认（不记忆、不最小化）。 */
  function reject(): void {
    requestExitPrompt();
  }

  function retry(): void {
    void bootstrap();
  }

  /** 取消退出/返回政策：清除后端待处理标记，继续阻断。 */
  function cancelExit(): void {
    if (exitBusy.value) return;
    exitPromptOpen.value = false;
    exitError.value = null;
    pendingExit = false;
    void invoke("privacy_exit_resolve", { exit: false }).catch(() => {
      // 清除标记失败不影响阻断状态：下一次 bootstrap 仍会恢复询问。
    });
  }

  /** 确认退出：后端直接结束进程（不保存窗口状态、不走已记忆的关闭动作）。 */
  async function confirmExit(): Promise<void> {
    if (exitBusy.value) return;
    exitBusy.value = true;
    exitError.value = null;
    try {
      await invoke("privacy_exit_resolve", { exit: true });
    } catch (e) {
      exitError.value = `退出失败，请重试：${describe(e)}`;
    } finally {
      exitBusy.value = false;
    }
  }

  /** 后端引导期关闭事件（窗口 X / Alt+F4）到达。 */
  function onExitRequested(): void {
    requestExitPrompt();
  }

  return {
    // 状态
    phase: phase as ComputedRef<PrivacyPhase>,
    policy,
    saving,
    blocked,
    blockedDetail,
    consentVisible,
    error: error as Ref<string | null>,
    exitPromptOpen,
    exitBusy,
    exitError,
    // 动作
    bootstrap,
    accept,
    reject,
    retry,
    cancelExit,
    confirmExit,
    onExitRequested,
  };
}
