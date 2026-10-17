// PR2（2026-10-15 计划 F03）：真实采集状态与最短显示期。
//
// 职责边界：这里只维护**读取侧**状态——真实采集生命周期（后端 collection_status
// 快照与 collection-status 事件）+ 最短 1 秒显示期 + 独立的采集错误通道。
// 统计数据与 query_id 继续由 `useUsageQuery` 管理，不复制第二套统计快照；
// 更高的 latestGeneration 通过页面注入的回调交给既有的"有新数据"自动刷新
// 合并入口，本组合式绝不直接 query_begin。
//
// 与 useUsageQuery 的分工（由页面 Dashboard.vue 桥接）：
// - `pillOut`：读取侧状态胶囊视图；useUsageQuery 的 statusPill 在查询侧无
//   在途/失败/stale 状态时采用它（查询在途时"刷新中…"优先）；
// - `noticeOut`：采集错误通知（failed / 通道不可用），独立于查询错误——
//   只有下一次成功采集能清除采集错误，成功查询清除查询错误；
// - `visible`：页面 + 宿主综合可见性（useUsageQuery 的 visible ref）。
//
// 显示期规则（计划 §2 / §6.2）：
// - 动画仅由真实 start（新 runId 的 running）触发，从可见 start 起最少
//   1000ms；实际读取更久则到真实结束才停止；相同 runId 的重复快照不延长；
// - 计时用可注入单调时钟，不用系统日期算持续时间；
// - failed / 通道不可用立即清除显示计时（不用最短期掩盖异常）；
// - 隐藏时清除显示计时器，不补播已完成轮次；恢复仍在 running 则按当前
//   运行状态展示；卸载清理全部 timer 与订阅，晚到的 IPC 不得更新。
import { onUnmounted, ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { NoticeItem } from "./useUsageQuery";

/** 后端 CollectionStatus DTO（camelCase，与内核 serde 约定一致）。 */
export interface CollectionStatusPayload {
  revision: number;
  runId: number;
  phase: "idle" | "running" | "succeeded" | "failed";
  trigger: "startup" | "manual" | "auto" | null;
  latestGeneration: number | null;
  changed: boolean;
  hasWarnings: boolean;
  lastCompletedAt: string | null;
  error: string | null;
}

/** 读取侧状态胶囊视图（useUsageQuery 的 statusPill 合并时采用）。 */
export interface CollectionPillView {
  cls: string;
  text: string;
  spin: boolean;
  /** 静态绿点（正常态）；动画期间由 spin 承担视觉反馈。 */
  dot: boolean;
  /** 悬停提示的追加诊断（"；最近读取 N 秒前"），可为空。 */
  recent?: string;
}

export interface CollectionStatusOptions {
  /** 汇总页综合可见性（页面 + 宿主窗口，由 useUsageQuery 提供）。 */
  visible: Ref<boolean>;
  /** 更高 latestGeneration 的既有自动刷新合并入口（useUsageQuery）。 */
  onGeneration: (generation: number) => void;
  /** 读取失败通知里的"手动刷新"动作（等同用户手动刷新，重新采集）。 */
  onManualRefresh: () => void;
  /** 读取侧胶囊输出（桥接到 useUsageQuery 的 statusPill 合并）。 */
  pillOut: Ref<CollectionPillView | null>;
  /** 采集错误通知输出（独立通道；null = 无采集错误）。 */
  noticeOut: Ref<NoticeItem | null>;
  /** 可注入单调时钟（毫秒）；缺省 performance.now。 */
  nowMs?: () => number;
  /** 可注入状态查询；缺省 invoke("collection_status")。 */
  queryStatus?: () => Promise<CollectionStatusPayload>;
  /** 可注入订阅；缺省 listen("collection-status")，返回取消监听函数。 */
  subscribe?: (handler: (status: CollectionStatusPayload) => void) => Promise<() => void>;
}

const MIN_DISPLAY_MS = 1000;
/// 可见期间的内存状态校验周期：纠正漏失的结束事件（计划 §3.2）。
const STATUS_VERIFY_INTERVAL_MS = 30_000;

/** 非 Tauri 环境（浏览器预览 / 单测之外的宿主）没有事件通道：静默降级，
 *  不显示"读取状态不可用"（该提示只用于真实宿主里通道确实失败的情形）。 */
function hasTauriHost(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function useCollectionStatus(opts: CollectionStatusOptions) {
  const nowMs = opts.nowMs ?? (() => performance.now());
  const queryStatus =
    opts.queryStatus ?? (() => invoke<CollectionStatusPayload>("collection_status"));
  const subscribe =
    opts.subscribe ??
    ((handler: (status: CollectionStatusPayload) => void) =>
      listen<CollectionStatusPayload>("collection-status", (event) => handler(event.payload)));

  /// 最新真实状态（revision 最大者）。
  const latestStatus = ref<CollectionStatusPayload | null>(null);
  /// 状态通道不可用（订阅失败或查询失败）：不得静默显示正常状态。
  const statusUnavailable = ref(false);
  /// 读取反馈是否在展示（running 或最短显示期未满）。
  const reading = ref(false);

  let disposed = false;
  let lastRevision = -1;
  let lastSeenRunId = -1;
  /// 已转发给自动刷新入口的最高代次（同一已覆盖代次不重复建查询）。
  let forwardedGeneration = -1;
  let unlisten: (() => void) | null = null;
  let subscribing = false;
  let displayUntil = 0;
  let displayTimer: ReturnType<typeof setTimeout> | null = null;
  let verifyTimer: ReturnType<typeof setInterval> | null = null;

  function stopDisplayTimer(): void {
    if (displayTimer !== null) {
      clearTimeout(displayTimer);
      displayTimer = null;
    }
  }

  function clearReading(): void {
    stopDisplayTimer();
    displayUntil = 0;
    reading.value = false;
    updateOutputs();
  }

  /// 新 start 只延长截止：displayUntil = max(旧截止, now + 1000ms)。
  function extendDisplay(): void {
    const now = nowMs();
    displayUntil = Math.max(displayUntil, now + MIN_DISPLAY_MS);
    reading.value = true;
    stopDisplayTimer();
    displayTimer = setTimeout(
      () => {
        displayTimer = null;
        if (disposed) return;
        // 到期时只有真实截止已过且阶段不再是 running 才停——running 期间的
        // 读取反馈由真实状态保持（"running 或 now < displayUntil：保留"）。
        if (nowMs() >= displayUntil && latestStatus.value?.phase !== "running") {
          reading.value = false;
          updateOutputs();
        }
      },
      Math.max(0, displayUntil - now),
    );
    updateOutputs();
  }

  function recentLabel(): string | undefined {
    const s = latestStatus.value;
    if (!s?.lastCompletedAt) return undefined;
    const ms = Date.parse(s.lastCompletedAt);
    if (!Number.isFinite(ms)) return undefined;
    const secs = Math.max(0, Math.round((Date.now() - ms) / 1000));
    return `；最近读取 ${secs} 秒前`;
  }

  /** 从当前内存状态重建胶囊视图与错误通知（所有状态变化点的唯一出口）。 */
  function updateOutputs(): void {
    if (disposed) return;
    const s = latestStatus.value;
    if (statusUnavailable.value) {
      opts.noticeOut.value = {
        id: "collection-status-unavailable",
        severity: "warning",
        text: "读取状态不可用，无法确认后台采集是否正常；统计显示的是最近一次成功读取的数据。",
        actions: [{ label: "重试", run: () => void resync() }],
      };
      opts.pillOut.value = {
        cls: "ts-pill-warning",
        text: "读取状态不可用",
        spin: false,
        dot: false,
      };
      return;
    }
    opts.noticeOut.value = null;
    if (!s) {
      // 初始状态尚未获取：静态"正在确认状态"，不假称扫描中。
      opts.pillOut.value = {
        cls: "ts-pill-info",
        text: "正在确认状态",
        spin: false,
        dot: false,
      };
      return;
    }
    if (s.phase === "failed") {
      opts.noticeOut.value = {
        id: "collection-failed",
        severity: "error",
        text: `读取失败：${s.error ?? "未知错误"}（统计显示的是上次成功读取的数据）`,
        actions: [{ label: "手动刷新", run: () => opts.onManualRefresh() }],
      };
      opts.pillOut.value = {
        cls: "ts-pill-warning",
        text: "读取失败",
        spin: false,
        dot: false,
        recent: recentLabel(),
      };
      return;
    }
    if ((reading.value || s.phase === "running") && opts.visible.value) {
      // 隐藏期间不播放动画（真实状态照常更新，恢复后按当前运行状态展示）。
      opts.pillOut.value = {
        cls: "ts-pill-info",
        text: "正在读取",
        spin: true,
        dot: false,
      };
      return;
    }
    if (s.hasWarnings) {
      opts.pillOut.value = {
        cls: "ts-pill-warning",
        text: "已更新 · 部分异常",
        spin: false,
        dot: true,
        recent: recentLabel(),
      };
      return;
    }
    opts.pillOut.value = {
      cls: "ts-pill-success",
      text: "已更新",
      spin: false,
      dot: true,
      recent: recentLabel(),
    };
  }

  /** 应用一份状态：只接受较新 revision（迟到的 IPC 回复与旧事件被忽略）。 */
  function applyStatus(s: CollectionStatusPayload | null | undefined): void {
    if (disposed || !s || typeof s.revision !== "number" || !Number.isFinite(s.revision)) return;
    if (s.revision <= lastRevision) return;
    lastRevision = s.revision;
    latestStatus.value = s;
    statusUnavailable.value = false;
    // 代次转发：更高的 latestGeneration 走既有自动刷新合并入口。隐藏期间
    // 也转发（合并入口自己负责"只记待刷新"）。
    if (
      typeof s.latestGeneration === "number" &&
      Number.isFinite(s.latestGeneration) &&
      s.latestGeneration > forwardedGeneration
    ) {
      forwardedGeneration = s.latestGeneration;
      opts.onGeneration(s.latestGeneration);
    }
    if (!opts.visible.value) {
      // 隐藏：不开始显示计时（已完成轮次不补播），只更新真实状态。
      clearReading();
      return;
    }
    if (s.phase === "failed") {
      // 失败优先立即停止，不用最短期掩盖异常。
      clearReading();
      return;
    }
    if (s.phase === "running") {
      if (!reading.value || s.runId !== lastSeenRunId) {
        // 新轮次（或恢复可见后的同一轮次）：从现在起满足最短期；相同
        // runId 的重复快照在反馈持续期间不延长。
        lastSeenRunId = s.runId;
        extendDisplay();
      }
      return;
    }
    // 终态：显示期内保留读取反馈到截止（由计时器停）；已过截止则停。
    if (reading.value && displayTimer === null && nowMs() >= displayUntil) {
      reading.value = false;
    }
    updateOutputs();
  }

  async function ensureSubscribed(): Promise<void> {
    if (unlisten || subscribing || disposed) return;
    subscribing = true;
    try {
      const un = await subscribe(onEvent);
      if (disposed) {
        un();
        return;
      }
      unlisten = un;
    } catch {
      if (disposed) return;
      if (hasTauriHost()) {
        // 真实宿主里订阅失败：不得静默显示正常状态。
        statusUnavailable.value = true;
        clearReading();
      }
    } finally {
      subscribing = false;
    }
  }

  function onEvent(s: CollectionStatusPayload): void {
    applyStatus(s);
  }

  /// 重查内存状态：纠正漏失的结束事件；失败时提示状态不可用（不把超时
  /// 假判为采集失败）。30 秒校验计时器与恢复可见时共用。
  async function resync(): Promise<void> {
    if (disposed) return;
    await ensureSubscribed();
    try {
      const s = await queryStatus();
      applyStatus(s);
    } catch {
      if (disposed) return;
      if (hasTauriHost()) {
        statusUnavailable.value = true;
        clearReading();
      }
    }
  }

  function startVerifyTimer(): void {
    if (verifyTimer !== null || disposed) return;
    verifyTimer = setInterval(() => {
      if (!disposed && opts.visible.value) void resync();
    }, STATUS_VERIFY_INTERVAL_MS);
  }

  function stopVerifyTimer(): void {
    if (verifyTimer !== null) {
      clearInterval(verifyTimer);
      verifyTimer = null;
    }
  }

  function dispose(): void {
    disposed = true;
    clearReading();
    stopVerifyTimer();
    if (unlisten) {
      try {
        unlisten();
      } catch {
        /* 通道已关闭 */
      }
      unlisten = null;
    }
  }

  watch(
    () => opts.visible.value,
    (visible) => {
      if (disposed) return;
      if (visible) {
        startVerifyTimer();
        // 恢复：仍在 running 的轮次按当前运行状态展示（从现在起满足最短期，
        // 不重播隐藏期间已完成的轮次）；再重查内存状态补齐漏失的转换。
        if (latestStatus.value?.phase === "running" && !reading.value) {
          lastSeenRunId = latestStatus.value.runId;
          extendDisplay();
        }
        void resync();
      } else {
        stopVerifyTimer();
        clearReading();
      }
    },
  );

  onUnmounted(dispose);

  // 初始输出：状态尚未获取时静态"正在确认状态"，不假称扫描中。
  updateOutputs();

  // 启动顺序：先订阅，再查询快照（订阅先就位才能收到间隙里的转换）。
  void (async () => {
    await ensureSubscribed();
    if (!disposed) await resync();
  })();

  return {
    latestStatus,
    statusUnavailable,
    reading,
    dispose,
  };
}

export type CollectionStatusApi = ReturnType<typeof useCollectionStatus>;
