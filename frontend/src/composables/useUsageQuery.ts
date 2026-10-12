// U04（UI 调整计划 2026-10-13）：汇总页查询状态归属。
//
// 集中持有：查询会话（含批次复用守卫）、汇总、明细、分页、快照、错误态、
// 来源状态、刷新调度。页面上**同时显示的**汇总与明细永远属于同一批次、
// 同一 `query_id`——任何时候都不拼接两个会话的数据，也不持久化这种组合。
//
// 与页面（`Dashboard.vue`）的分工：
// - 这里持有状态与动作；页面只做模板渲染与 UI 局部状态（通知展开等）；
// - 筛选偏好（来源 / 维度 / 时间预设 / 日粒度参与方式 / 下钻）是查询口径的
//   唯一来源，启动时同步读取、变化即写盘；视图快照只在「偏好 + 时区」逐项
//   相等时用于首帧展示，从不反向改写偏好或时区；
// - 可见性由页面用 `onActivated` / `onDeactivated` 调 `setVisible(bool)` 传入
//   （见文件末尾的说明）。
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  AGENT_LABEL,
  type AgentFilter,
  type Dim,
  type EventDrill,
  type EventList,
  type RangeView,
  type SourceStatus,
  type SummaryReport,
} from "../types";
import {
  normalizePreset,
  presetKey,
  presetToQueryParams,
  type RangePreset,
} from "../lib/rangePreset";
import { SNAPSHOT_VERSION, enqueueSnapshotSave } from "../lib/viewSnapshot";
import { errorText, isQueryExpired } from "../lib/queryError";
import {
  loadViewFilters,
  normalizeDrill,
  saveViewFilters,
  viewFiltersEqual,
  type RollupsMode,
  type ViewFilters,
} from "../lib/viewFilters";
import { useTimezone } from "./timezone";

export type { RollupsMode };

/// SF04：查询会话句柄（query_begin 返回）——同一批次的汇总/明细/分页
/// 都绑定同一 query_id；价格修订随会话冻结。
type QueryHandle = {
  queryId: string;
  generation: number;
  pricingRevision: string;
  timezone: string;
  asOf: string;
  /** U01：后端冻结的时间范围（全部时间为 null/缺省）。 */
  range?: RangeView | null;
};

/// 主筛选身份（`drill: null` = 主批次身份；带 `drill` = 明细子查询身份）。
type MainIdentity = {
  by: Dim;
  agent: AgentFilter;
  preset: RangePreset | null;
  drill: EventDrill | null;
};

/// RC02：**成功首页身份**——只有它存在时"加载更多"才有分页资格。它记录
/// 本进程内一次真实成功的首页响应所属的 query_id/价格修订/身份/批次 epoch；
/// 磁盘恢复的旧视图**不赋予**该身份，因此其旧游标（属于保存时的旧
/// query_id）不得用于当前后端会话续页。
type LiveFirstPage = {
  queryId: string;
  pricingRevision: string;
  epoch: number;
  filters: MainIdentity;
};

/// 快照落盘载荷（v12：时间以预设保存，快照不含时区）。
type SnapshotPayload = {
  v: number;
  saved_at: string;
  filters: {
    by: Dim;
    agent: AgentFilter;
    preset: RangePreset | null;
    drill: EventDrill | null;
  };
  rollups_mode: RollupsMode;
  report: SummaryReport;
  events: EventList;
};

/// 合并通知的严重级别（排序决定折叠态展示哪一条）。
export type NoticeSeverity = "error" | "warning" | "info";

export interface NoticeAction {
  label: string;
  disabled?: boolean;
  run: () => void;
}

/** 合并通知的一条：文本 + 自己的动作（动作永远留在各自的条目上）。 */
export interface NoticeItem {
  id: string;
  severity: NoticeSeverity;
  text: string;
  /** 展开后逐条列出的细节（采集警告、逐条来源异常等）。 */
  details?: string[];
  actions: NoticeAction[];
}

/** 折叠态头部：最高严重级别的文本 + 条数 + 可展开性。 */
export interface NoticeHeadline {
  severity: NoticeSeverity;
  count: number;
  text: string;
  expandable: boolean;
  actions: NoticeAction[];
}

export interface UsageQueryOptions {
  /** 程序触发的刷新计数（价格同步、设置保存成功等）。 */
  refreshKey: () => number;
  /** 来源异常条目的「去设置 ›」动作（由页面接到应用壳）。 */
  goSettings?: () => void;
}

const SEVERITY_ORDER: Record<NoticeSeverity, number> = { error: 0, warning: 1, info: 2 };

/// 下钻提示用**中文类型名**（不再是 `model: xxx`）。
const DRILL_TYPE_LABEL: Record<EventDrill["type"], string> = {
  day: "日期",
  model: "模型",
  project: "项目",
};

/** 时区身份：`local` 解析为本机 IANA 名后再与快照的统计时区比较。 */
function resolvedTzId(value: string): string {
  if (value !== "local") return value;
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "local";
  } catch {
    return "local";
  }
}

export function useUsageQuery(opts: UsageQueryOptions) {
  // ── 筛选偏好（决定"查什么"）────────────────────────────────
  const initial: ViewFilters = loadViewFilters();
  const by = ref<Dim>(initial.by);
  const agent = ref<AgentFilter>(initial.agent);
  const preset = ref<RangePreset | null>(initial.preset);
  const rollupsMode = ref<RollupsMode>(initial.rollupsMode);
  const drill = ref<EventDrill | null>(initial.drill);
  /// U01：本批次后端回传的冻结范围——日期按钮的标签/悬停提示与汇总数字同源。
  const batchRange = ref<RangeView | null>(null);
  const { tz } = useTimezone();

  // ── 数据与状态 ──────────────────────────────────────────
  const report = ref<SummaryReport | null>(null);
  const events = ref<EventList | null>(null);
  const loading = ref(false);
  const eventsLoading = ref(false);
  const moreLoading = ref(false);
  const summaryError = ref<string | null>(null);
  const eventsError = ref<string | null>(null);
  /// G-P3-4：明细失败发生在首页还是追加页——追加失败的原地重试必须仍走
  /// 追加，不把已累积分页重置回首页。
  const eventsFailureWasAppend = ref(false);
  const stale = ref(false);
  const cachedAt = ref<string | null>(null);
  const currentQuery = ref<QueryHandle | null>(null);
  /// RC02：当前批次是否已成功建立后端会话（begin 成功）。
  const sessionReady = ref(false);
  const liveFirstPage = ref<LiveFirstPage | null>(null);

  const hasMore = computed(() => !!events.value && events.value.rows.length < events.value.total);

  // F01/F03：汇总与明细各自持有请求代次（曾共用一个 runSeq，两个 immediate
  // watcher 依次启动使后者作废前者，首屏永远转圈或停在旧快照）；refreshEpoch
  // 是共享刷新批次，两条查询携带同一 epoch，请求代次只负责丢弃旧响应。
  let summarySeq = 0;
  let eventsSeq = 0;
  let refreshEpoch = 0;
  let summaryBatchEpoch = -1;
  let eventsBatchEpoch = -1;
  // SF05/SF04：每批次共享一次 begin_query Promise，汇总/明细并发读取同一会话。
  let beginPromise: Promise<QueryHandle | null> | null = null;
  let beginEpoch = -1;
  /// H08：本批次捕获的日粒度参与方式（与 begin 同一时刻读取）。
  let batchRollups: RollupsMode = "auto";
  // R04：实例 disposal——卸载后晚到的响应不得更新状态或落盘快照。
  let disposed = false;
  // R04：汇总/明细各自的**请求时捕获身份**（不可变）——响应返回时不再读当前
  // 筛选冒充请求身份（混代数据保存的根因）。
  let summaryKey: MainIdentity | null = null;
  const eventsKey = ref<MainIdentity | null>(null);

  // ── 可见性与刷新调度（U04）────────────────────────────────
  /// 汇总页是否可见（KeepAlive 停用期间为 false）。
  const visible = ref(true);
  /// 待刷新标记：隐藏期间的所有刷新触发都只置位，回到页面时合并成一批。
  const pendingRefresh = ref(false);
  /// U06 接入点：最近一次收到的历史库代次（`usage-updated` 与查询句柄取最大）。
  const latestGeneration = ref(0);

  // ── 来源状态（AP06）───────────────────────────────────────
  const sourceStatus = ref<SourceStatus[]>([]);
  const sourceError = ref<string | null>(null);
  const sourcesLoading = ref(false);
  let sourcesInflight: Promise<void> | null = null;
  let sourcesAgain = false;

  function mainIdentity(): MainIdentity {
    return { by: by.value, agent: agent.value, preset: preset.value, drill: null };
  }

  function eventIdentity(): MainIdentity {
    return {
      by: by.value,
      agent: agent.value,
      preset: preset.value,
      drill: drill.value ? { ...drill.value } : null,
    };
  }

  function preferenceSnapshot(): ViewFilters {
    return {
      by: by.value,
      agent: agent.value,
      preset: preset.value,
      rollupsMode: rollupsMode.value,
      drill: drill.value ? { ...drill.value } : null,
    };
  }

  function sameMainIdentity(a: MainIdentity, b: MainIdentity): boolean {
    // U01：时间按**预设**比较（快照不再保存固定日期区间）；时区不在快照里，
    // 由 useTimezone 单独持有，不作为筛选身份的一部分。
    // 注意：这里**只比主身份**（by/agent/preset）——下钻是明细侧的子查询身份，
    // 汇总侧没有这个概念；保存快照时明细身份带 drill（见 saveSnapshot），
    // 分页守卫另有 drill 的显式比较（见 loadEvents）。
    return a.by === b.by && a.agent === b.agent && presetKey(a.preset) === presetKey(b.preset);
  }

  function sameDrill(a: EventDrill | null, b: EventDrill | null): boolean {
    return (a?.type ?? null) === (b?.type ?? null) && (a?.key ?? null) === (b?.key ?? null);
  }

  function persistFilters(): void {
    saveViewFilters(preferenceSnapshot());
  }

  // ── 查询会话与两条查询 ────────────────────────────────────
  /// SF04：创建（或复用）当前批次的查询会话。同一 epoch 只发起一次
  /// begin_query，refresh/loadEvents 共享该 Promise；晚到的旧 begin 不得
  /// 接管新批次（epoch/queryId 双重守卫）。
  function beginQueryForCurrentEpoch(): Promise<QueryHandle | null> {
    const epoch = refreshEpoch;
    if (beginPromise && beginEpoch === epoch) return beginPromise;
    beginEpoch = epoch;
    sessionReady.value = false;
    batchRollups = rollupsMode.value;
    // 参数在 async 函数体首个 await 之前同步求值，与 epoch 一一对应。
    // **U06 接入点**：自动刷新要用的 `collect: false` 加在这里——按批次来源
    // （启动/手动/自动）决定是否先采集；缓存缺失时后端会退回采集。
    beginPromise = (async () => {
      try {
        const h = await invoke<QueryHandle>("query_begin", {
          by: by.value,
          agent: agent.value,
          // U01：只发时间预设（days / rollingHours / startAt+endAt / untilNow）——
          // 四组互斥由后端判定，前端不做边界计算、不再发 from/to。
          ...presetToQueryParams(preset.value),
          tz: tz.value,
          // H08：日粒度历史参与方式（`detail_only` = 只看请求明细）。
          rollups: batchRollups === "detail_only" ? "detail_only" : null,
        });
        if (epoch !== refreshEpoch || disposed) return null; // 旧 begin 晚到不接管
        currentQuery.value = h;
        // U01：范围的唯一来源是后端冻结结果（标签只做展示，不参与查询）。
        batchRange.value = h.range ?? null;
        sessionReady.value = true;
        return h;
      } catch (e) {
        // 会话建立失败：保留旧视图（不伪装已更新），错误在汇总侧可见。
        if (epoch === refreshEpoch && !disposed) {
          summaryError.value = errorText(e);
          // RC02：失败的 Promise 不得留在缓存里——否则同一 epoch 的"重试"
          // 会复用已失败的 begin，永远不重发（failed_begin_can_be_retried）。
          if (beginEpoch === epoch) {
            beginPromise = null;
            beginEpoch = -1;
          }
          sessionReady.value = false;
        }
        return null;
      }
    })();
    return beginPromise;
  }

  async function refresh(): Promise<void> {
    const seq = ++summarySeq;
    const epoch = refreshEpoch;
    const captured = mainIdentity();
    loading.value = true;
    summaryError.value = null;
    try {
      const h = await beginQueryForCurrentEpoch();
      if (seq !== summarySeq || disposed || epoch !== refreshEpoch) return;
      if (!h) return; // begin 失败已写入 summaryError；保留旧视图
      const fresh = await invoke<SummaryReport>("query_summary", { queryId: h.queryId });
      if (seq !== summarySeq || disposed || epoch !== refreshEpoch) return;
      if (h.queryId !== currentQuery.value?.queryId) return; // 会话已被新批次替换
      report.value = fresh;
      stale.value = false;
      summaryBatchEpoch = epoch;
      summaryKey = captured;
      saveSnapshot();
    } catch (e) {
      if (seq === summarySeq && !disposed) {
        summaryError.value = errorText(e);
        // RC02：会话已失效 → 本批次不再有 live 会话，恢复动作必须新建批次。
        if (isQueryExpired(e)) sessionReady.value = false;
      }
    } finally {
      if (seq === summarySeq && !disposed) loading.value = false;
    }
  }

  async function loadEvents(append = false): Promise<void> {
    if (append) {
      // F03：追加页仅允许在所属明细首批完成后执行，且锚点必须与当前显示
      // 的明细同主身份——旧筛选的游标不得用于新筛选，新筛选首批也不得被
      // 追加响应作废。
      if (eventsLoading.value || !eventsKey.value) return;
      const live = liveFirstPage.value;
      if (!live || live.queryId !== currentQuery.value?.queryId) return;
      const current = eventIdentity();
      if (!sameMainIdentity(live.filters, current)) return;
      if (!sameDrill(live.filters.drill, current.drill)) return;
      if (!sameMainIdentity(eventsKey.value, current)) return;
      if (!sameDrill(eventsKey.value.drill, current.drill)) return;
    } else {
      // RC02：新的首页请求开始即作废旧首页身份——成功前不得续页。
      liveFirstPage.value = null;
    }
    const seq = ++eventsSeq;
    const epoch = refreshEpoch;
    const captured = eventIdentity();
    const anchor = append ? (events.value?.rows.at(-1)?.cursor ?? null) : null;
    if (!append) eventsLoading.value = true;
    moreLoading.value = append;
    eventsError.value = null;
    try {
      // SF04：共享本批次的会话；下钻/翻页在同一 query_id 上执行。
      const h = await beginQueryForCurrentEpoch();
      if (seq !== eventsSeq || disposed || epoch !== refreshEpoch) return;
      if (!h) {
        // 会话建立失败：保留旧明细（不伪装已更新），错误在汇总侧可见。
        eventsError.value = summaryError.value;
        return;
      }
      const list = await invoke<EventList>("query_events", {
        queryId: h.queryId,
        model: captured.drill?.type === "model" ? captured.drill.key : null,
        project: captured.drill?.type === "project" ? captured.drill.key : null,
        day: captured.drill?.type === "day" ? captured.drill.key : null,
        limit: 200,
        before: anchor,
      });
      if (seq !== eventsSeq || disposed) return;
      if (h.queryId !== currentQuery.value?.queryId) return; // 旧会话响应不落地
      if (append && events.value) {
        events.value = { ...list, rows: [...events.value.rows, ...list.rows] };
      } else {
        events.value = list;
        eventsKey.value = captured;
        eventsBatchEpoch = epoch;
        // RC02：成功首页才赋予 live 分页资格（恢复视图永不进入本分支）。
        liveFirstPage.value = {
          queryId: h.queryId,
          pricingRevision: list.pricing_revision,
          epoch,
          filters: captured,
        };
      }
      saveSnapshot();
    } catch (e) {
      if (seq === eventsSeq && !disposed) {
        eventsError.value = errorText(e);
        eventsFailureWasAppend.value = append;
        // RC02：会话失效 → 恢复动作改为新建批次（局部重试无意义）。
        if (isQueryExpired(e)) sessionReady.value = false;
      }
    } finally {
      if (seq === eventsSeq && !disposed) {
        eventsLoading.value = false;
        moreLoading.value = false;
      }
    }
  }

  function loadMoreEvents(): void {
    void loadEvents(true);
  }

  function saveSnapshot(): void {
    // F03：保存门槛 = 汇总与明细均成功且属于**当前批次**（epoch 一致），
    // 主键完全一致（含 by），且 report.by 与键一致。任一侧失败/旧批次 →
    // 保留上一份一致快照，不由另一侧成功写新旧拼接；分页追加只延长同
    // 批次明细，drill 是明细侧子查询身份（汇总无 drill 概念）。
    if (!report.value || !events.value || !eventsKey.value || !summaryKey) return;
    if (summaryBatchEpoch !== refreshEpoch || eventsBatchEpoch !== refreshEpoch) return;
    const sk = summaryKey;
    const ek = eventsKey.value;
    if (!sameMainIdentity(sk, ek)) return;
    if (report.value.by !== sk.by) return;
    // SF04：两侧必须来自同一后端会话（query_id/价格修订一致）——
    // 跨会话拼接的 report/events 不落盘。
    if (report.value.query_id !== events.value.query_id) return;
    if (report.value.pricing_revision !== events.value.pricing_revision) return;
    const payload: SnapshotPayload = {
      v: SNAPSHOT_VERSION,
      saved_at: new Date().toISOString(),
      // 复核修订：下钻标签写**当前汇总解析出的**展示名，避免把过期标签带进快照。
      filters: {
        by: ek.by,
        agent: ek.agent,
        preset: ek.preset,
        drill: ek.drill ? { ...ek.drill, label: resolvedDrillLabel(ek.drill) } : null,
      },
      rollups_mode: rollupsMode.value,
      report: report.value,
      events: events.value,
    };
    enqueueSnapshotSave(payload);
  }

  // ── 刷新批次调度 ─────────────────────────────────────────
  // F03：主刷新协调入口——同一 epoch 传给汇总与明细两条查询。
  function startRefreshBatch(): void {
    refreshEpoch++;
    // RC02：新批次 = 新会话——旧首页身份与"会话已就绪"标记立即作废。
    liveFirstPage.value = null;
    sessionReady.value = false;
    void refresh();
    void loadEvents();
  }

  /// U04：可见时立刻发起一批；隐藏时只置待刷新标记（回到页面时合并成一批）。
  function scheduleBatchRefresh(): void {
    if (!visible.value) {
      pendingRefresh.value = true;
      return;
    }
    startRefreshBatch();
  }

  /// C4/F03：手动刷新——以当前筛选重跑两条查询；也是用户操作。
  /// AP06：同时重查来源状态（目录修好后旧通知必须消失）。
  function manualRefresh(): void {
    void loadSources();
    startRefreshBatch();
  }

  /// H08：切换"只看请求明细"——桶选择在建快照时固定，必须重建查询会话，
  /// 所以与其它筛选项一样经偏好 watcher 走同一刷新路径。
  function setRollupsMode(mode: RollupsMode): void {
    rollupsMode.value = mode;
  }

  /// RC02：统一错误恢复入口。
  /// - 会话级失败（begin 失败、query_expired、会话被淘汰/外会话游标）→
  ///   建立**一个新批次**：一次 query_begin，汇总与首页共享同一新 query_id；
  /// - 普通当前会话请求失败 → 局部重试（只重发失败的那一侧）。
  function retryAfterFailure(scope: "summary" | "events"): void {
    const err = scope === "summary" ? summaryError.value : eventsError.value;
    if (!sessionReady.value || isQueryExpired(err)) {
      startRefreshBatch();
    } else if (scope === "summary") {
      void refresh();
    } else if (eventsFailureWasAppend.value && events.value?.rows.at(-1)?.cursor) {
      // G-P3-4：追加页失败的重试仍走追加（首页身份与锚点均未失效）。
      void loadEvents(true);
    } else {
      void loadEvents();
    }
  }

  /// RC02：恢复视图 / 新批次首页未成功时没有分页资格——按钮禁用并说明原因，
  /// 不允许"点击无反馈"。
  const moreBlockedHint = computed<string | undefined>(() =>
    hasMore.value && !liveFirstPage.value ? "刷新完成后可继续加载" : undefined,
  );

  // ── 下钻 ────────────────────────────────────────────────
  function onSummaryRowClick(key: string): void {
    if (by.value === "agent") {
      // Agent 行：切换顶部 agent 过滤，不进入明细下钻
      agent.value = key === "claude-code" ? "claude" : key === "codex" ? "codex" : "all";
      drill.value = null;
      return;
    }
    const row = report.value?.groups.find((g) => g.key === key);
    // MP04：筛选键始终是后端身份 key；展示名另存 label（模型维度 = 友好名）。
    drill.value = {
      type: by.value as "day" | "model" | "project",
      key,
      label: row?.label ?? key,
    };
  }

  /// U04：合计行由 `UsageTable` 的 `total-row` 事件单独通知——这里不再靠
  /// 字面量「合计」判断（合计不是维度值，不得生成同名字面过滤）。
  function onTotalRowClick(): void {
    if (by.value === "agent") agent.value = "all";
    drill.value = null;
  }

  function clearDrill(): void {
    drill.value = null;
  }

  /// 复核修订：下钻标签**跟随当前汇总**——价格同步/模型展示名更新后，表格与
  /// 筛选提示必须同时显示新名称；汇总里找不到时回退磁盘快照里的 label，再退回
  /// 身份键。项目维度保留完整路径，避免同名不同路径产生歧义。
  function resolvedDrillLabel(d: EventDrill): string {
    if (d.type === "project") return d.key;
    return report.value?.groups.find((g) => g.key === d.key)?.label ?? d.label ?? d.key;
  }

  const drillLabel = (d: EventDrill): string =>
    `${DRILL_TYPE_LABEL[d.type] ?? d.type}：${resolvedDrillLabel(d)}`;

  // ── 来源状态 ────────────────────────────────────────────
  // AP06：来源检测独立状态。读取失败**不**清空已成功的汇总数据，只标注
  // 来源状态可能过期；在途期间合并连点，只记一次补读意图。
  async function loadSources(): Promise<void> {
    if (sourcesInflight) {
      sourcesAgain = true;
      await sourcesInflight;
      return;
    }
    const round = (async () => {
      sourcesLoading.value = true;
      try {
        const fresh = await invoke<SourceStatus[]>("source_status");
        if (disposed) return;
        sourceStatus.value = fresh;
        sourceError.value = null;
      } catch (e) {
        if (disposed) return;
        sourceError.value = errorText(e);
      } finally {
        sourcesLoading.value = false;
      }
    })();
    sourcesInflight = round;
    try {
      await round;
    } finally {
      sourcesInflight = null;
      if (sourcesAgain && !disposed) {
        sourcesAgain = false;
        await loadSources();
      }
    }
  }

  // ── 状态胶囊与通知 ──────────────────────────────────────
  // 任务 3：标题行状态胶囊——stale / 刷新中 / 刷新失败 / 已更新四态可见。
  const statusPill = computed<{ cls: string; text: string }>(() => {
    // R04 状态机：stale + 失败 = 旧数据/刷新失败（不能谎称"后台刷新中"）；
    // stale + 刷新中才显示后台刷新。
    if (stale.value && summaryError.value)
      return { cls: "ts-pill-warning", text: "缓存数据 · 刷新失败" };
    if (stale.value && loading.value)
      return { cls: "ts-pill-warning", text: "缓存数据 · 后台刷新中" };
    // 首载失败且无任何数据可恢复：必须显示失败而非"刷新中"（G-P2-1）。
    if (!report.value && summaryError.value)
      return { cls: "ts-pill-warning", text: "刷新失败 · 暂无数据" };
    if (loading.value || !report.value) return { cls: "ts-pill-info", text: "刷新中…" };
    if (summaryError.value) return { cls: "ts-pill-warning", text: "刷新失败 · 显示上次数据" };
    return { cls: "ts-pill-success", text: "已更新" };
  });

  // R08（Task 8）：采集诊断——空结果/部分结果都必须可见，不依赖表格挂载。
  // 分开命名：warnings（重叠/缓存降级等）、坏行、IO 错误——不声称所有
  // warning 都表示丢数据；干净空区间不误报。
  const collectionDiagnostics = computed(() => {
    // H06：日汇总的时区不匹配是「当前视图看不到这部分历史」的可恢复提示，
    // 由通知列表单独成条（不计入采集警告计数，免得把时区提示说成采集问题）。
    const warns = [...(report.value?.warnings ?? [])];
    const timezoneMismatch = report.value?.rollup_coverage?.timezone_mismatch ?? null;
    const instantOnly = report.value?.rollup_coverage?.rollups_excluded_for_instant_range === true;
    const statsList = report.value?.sources.map((s) => s.stats) ?? [];
    const ioErrors = statsList.reduce((a, st) => a + (st.io_errors ?? 0), 0);
    const badLines = statsList.reduce((a, st) => a + (st.bad_lines ?? 0), 0);
    // H06：含只有日粒度的历史时，费用按「未知」披露（不是模型未收录）。
    const rollupOnly = report.value?.totals.unknown_reason === "rollup_only";
    const clean = warns.length === 0 && ioErrors === 0 && badLines === 0 && !rollupOnly;
    const summaryText = clean
      ? null
      : [
          warns.length > 0 ? `${warns.length} 条采集警告` : null,
          ioErrors > 0 ? `${ioErrors} 个文件读取失败` : null,
          badLines > 0 ? `${badLines} 行解析失败（已跳过）` : null,
          rollupOnly ? "含只有日粒度的历史（这部分费用按未知披露）" : null,
        ]
          .filter(Boolean)
          .join("，");
    return {
      clean,
      summaryText,
      warns,
      ioErrors,
      badLines,
      rollupOnly,
      timezoneMismatch,
      instantOnly,
    };
  });

  /// U01：时刻模式提示——当前范围精确到时刻时不取日汇总，必须让用户知道
  /// "这部分历史看不到"（进入合并通知，由界面写文案）。
  const instantRangeHint = computed<string | null>(() =>
    report.value?.rollup_coverage?.rollups_excluded_for_instant_range === true
      ? "当前时间范围精确到时刻，只统计请求明细，不含只有日粒度的历史"
      : null,
  );

  const problemSources = computed(() => sourceStatus.value.filter((x) => x.state !== "ready"));

  function sourceNoticeText(s: SourceStatus): string {
    if (s.state === "disabled")
      return `${AGENT_LABEL[s.agent] ?? s.agent} 已在设置中停用，不参与统计。`;
    if (s.state === "missing")
      return `${AGENT_LABEL[s.agent] ?? s.agent} 数据目录不存在（${s.dir}）。`;
    return `${AGENT_LABEL[s.agent] ?? s.agent} 目录存在但没有发现会话日志（${s.dir}）。`;
  }

  /// U04 通知合并：页面上**只显示一条通知**（原来最多七条并存），展示最高
  /// 严重级别与条数，展开后逐条列出。动作留在各自条目上：
  /// 重试（汇总/明细失败、来源检测失败）、去设置（来源异常）、
  /// 只看请求明细（时区不匹配）、恢复自动参与（仅明细模式）、详情（采集诊断）。
  const notices = computed<NoticeItem[]>(() => {
    const list: NoticeItem[] = [];
    if (summaryError.value) {
      list.push({
        id: "summary-error",
        severity: "error",
        text: `汇总加载失败：${summaryError.value}`,
        actions: [{ label: "重试", run: () => retryAfterFailure("summary") }],
      });
    }
    if (eventsError.value) {
      list.push({
        id: "events-error",
        severity: "error",
        text: `明细加载失败：${eventsError.value}`,
        actions: [{ label: "重试", run: () => retryAfterFailure("events") }],
      });
    }
    if (sourceError.value) {
      list.push({
        id: "source-error",
        severity: "error",
        text: `来源目录检测失败${
          sourceStatus.value.length > 0 ? "（下方来源状态可能已过期）" : ""
        }：${sourceError.value}`,
        actions: [
          {
            label: sourcesLoading.value ? "重试中…" : "重试",
            disabled: sourcesLoading.value,
            run: () => void loadSources(),
          },
        ],
      });
    }
    const problems = problemSources.value;
    if (problems.length > 0) {
      list.push({
        id: "source-problem",
        severity: "warning",
        text:
          problems.length === 1
            ? sourceNoticeText(problems[0])
            : `${problems.length} 个来源异常，统计可能不完整。`,
        details: problems.length > 1 ? problems.map((s) => sourceNoticeText(s)) : undefined,
        actions:
          problems.some((s) => s.state !== "disabled") && opts.goSettings
            ? [{ label: "去设置 ›", run: () => opts.goSettings?.() }]
            : [],
      });
    }
    if (!collectionDiagnostics.value.clean) {
      list.push({
        id: "collection",
        severity: "warning",
        text: `本轮采集存在部分问题（${collectionDiagnostics.value.summaryText}）；已采集数据仍有效。`,
        details:
          collectionDiagnostics.value.warns.length > 0
            ? [...collectionDiagnostics.value.warns]
            : undefined,
        actions: [],
      });
    }
    if (instantRangeHint.value) {
      list.push({
        id: "instant-range",
        severity: "warning",
        text: instantRangeHint.value,
        actions: [],
      });
    }
    if (rollupsMode.value === "auto" && collectionDiagnostics.value.timezoneMismatch) {
      list.push({
        id: "rollup-timezone",
        severity: "warning",
        text: collectionDiagnostics.value.timezoneMismatch,
        actions: [{ label: "只看请求明细", run: () => setRollupsMode("detail_only") }],
      });
    } else if (rollupsMode.value === "detail_only") {
      list.push({
        id: "rollup-detail-only",
        severity: "info",
        text: "当前只统计请求明细（不含只有日粒度的历史）；按日视图不受来源时区影响。",
        actions: [{ label: "恢复自动参与", run: () => setRollupsMode("auto") }],
      });
    }
    // 稳定排序：严重级别优先，同级别保持声明顺序。
    return list.sort((a, b) => SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity]);
  });

  const noticeHeadline = computed<NoticeHeadline | null>(() => {
    const list = notices.value;
    if (list.length === 0) return null;
    const top = list[0];
    return {
      severity: top.severity,
      count: list.length,
      text: list.length === 1 ? top.text : `${top.text}（共 ${list.length} 条通知）`,
      expandable: list.length > 1 || (top.details?.length ?? 0) > 0,
      // 折叠态保留**最高严重级别那条**的动作（"重试"不必先展开才能点到）；
      // 其余条目的动作在展开后各自出现。
      actions: top.actions,
    };
  });

  // ── 可见性（U04）────────────────────────────────────────
  /// 页面用 `onActivated` / `onDeactivated` 调用。选择理由：
  /// - 汇总页由 `KeepAlive` 保留，`v-if` 为假时父级不会再为它生成 vnode，
  ///   props 更新不可靠；
  /// - `onActivated` / `onDeactivated` 是 `KeepAlive` 明确保证的生命周期信号，
  ///   不会有"隐藏了却以为可见"的窗口；
  /// - 独立 mount 该组合式函数（无 KeepAlive）时两个钩子都不触发，
  ///   `visible` 保持默认 true，行为与"页面一直可见"一致。
  function setVisible(next: boolean): void {
    if (next === visible.value) return;
    visible.value = next;
    if (!next) return;
    // 激活帧内可能还有其它触发要落地（KeepAlive 复用实例时 props 会在这一帧
    // 补齐，比如隐藏期间外部递增的 refreshKey）——统一推迟一帧再合并处理，
    // 保证"回到页面只发起一批"，不依赖 activated 钩子与 watcher 的先后顺序。
    void nextTick(() => {
      if (disposed || !visible.value || !pendingRefresh.value) return;
      pendingRefresh.value = false;
      // 回到页面：只发起一批刷新，筛选、下钻与滚动位置都不动；顺带重查来源
      // 状态（隐藏期间可能改过来源目录）。
      void loadSources();
      startRefreshBatch();
    });
  }

  /// **U06 接入点**：自动刷新（`usage-updated` 事件、时间窗口到期、窗口恢复）
  /// 的唯一入口。当前实现只做可见性合并：可见时发起一批，隐藏时只置标记。
  /// U06 在这里补 `collect: false`、批次在途合并（`latestGeneration` 补刷）、
  /// 以及"已加载更多时不自动刷新，改为提示"的翻页保护。
  function requestAutoRefresh(): void {
    if (!visible.value) {
      pendingRefresh.value = true;
      return;
    }
    startRefreshBatch();
  }

  /// **U06 接入点**：`usage-updated` 事件处理器（事件监听由 U06 接线）。
  function notifyUsageUpdated(generation: number): void {
    if (typeof generation === "number" && Number.isFinite(generation)) {
      latestGeneration.value = Math.max(latestGeneration.value, generation);
    }
    requestAutoRefresh();
  }

  // ── 启动 ────────────────────────────────────────────────
  /// U04：偏好变化 = 查询口径变化 → 写盘 + 按可见性刷新（可见时同一批次两条
  /// 查询；隐藏时只标记）。
  watch([by, agent, preset, rollupsMode], () => {
    persistFilters();
    scheduleBatchRefresh();
  });
  /// 下钻是同一 query_id 内的明细子查询：可见时重扫明细，隐藏时合并到待刷新。
  watch(drill, () => {
    persistFilters();
    if (!visible.value) {
      pendingRefresh.value = true;
      return;
    }
    void loadEvents();
  });
  /// 时区不是偏好（唯一来源是 settings.toml），但影响统计口径。
  watch(tz, () => scheduleBatchRefresh());
  /// refreshKey 是程序触发（价格同步、设置保存成功等）：新批次但不算用户操作。
  watch(
    () => opts.refreshKey(),
    () => {
      void loadSources();
      scheduleBatchRefresh();
    },
  );

  onUnmounted(() => {
    disposed = true;
  });

  // 视图快照：只在"偏好 + 时区"逐项相等时用于首帧展示（标记 stale），
  // 不触发筛选 watcher、不写偏好、不改写时区。
  async function loadViewCache(): Promise<void> {
    try {
      const cached = await invoke<SnapshotPayload | null>("view_cache_load");
      // R04：晚到的缓存不得覆盖已落地的新结果；卸载实例同样不恢复。
      if (disposed || report.value || events.value) return;
      // 只接受当前版快照；旧版本可能含混代数据，一律走正常加载。
      if (!cached || cached.v !== SNAPSHOT_VERSION || !cached.report || !cached.filters) return;
      const restored: ViewFilters = {
        by: cached.filters.by,
        agent: cached.filters.agent,
        preset: normalizePreset(cached.filters.preset),
        rollupsMode: cached.rollups_mode === "detail_only" ? "detail_only" : "auto",
        drill: normalizeDrill(cached.filters.drill),
      };
      // 偏好（来源/维度/预设/日粒度参与方式/下钻）与时区逐项相等才可用：
      // 快照的统计时区取自它的 report（v12 起 filters 不再保存时区）。
      if (!viewFiltersEqual(restored, preferenceSnapshot())) return;
      if (!cached.report.timezone || cached.report.timezone !== resolvedTzId(tz.value)) return;
      report.value = cached.report;
      events.value = cached.events ?? null;
      eventsKey.value = {
        by: restored.by,
        agent: restored.agent,
        preset: restored.preset,
        drill: restored.drill,
      };
      // RC02：恢复的旧视图没有当前后端会话的 live 首页身份——旧游标只能
      // 作 stale 展示，续页必须等本批次首页成功。
      liveFirstPage.value = null;
      cachedAt.value = cached.saved_at;
      stale.value = true;
    } catch {
      // 视图缓存损坏：静默忽略，走正常加载
    }
  }

  void loadSources();
  void loadViewCache();
  // 启动首批查询（偏好决定"查什么"）。
  startRefreshBatch();

  return {
    // 筛选偏好
    by,
    agent,
    preset,
    rollupsMode,
    drill,
    batchRange,
    // 数据
    report,
    events,
    loading,
    eventsLoading,
    moreLoading,
    hasMore,
    stale,
    cachedAt,
    summaryError,
    eventsError,
    sessionReady,
    liveFirstPage,
    currentQuery,
    // 来源
    sourceStatus,
    sourceError,
    sourcesLoading,
    problemSources,
    // 派生
    statusPill,
    collectionDiagnostics,
    instantRangeHint,
    notices,
    noticeHeadline,
    moreBlockedHint,
    drillLabel,
    // 动作
    refresh,
    manualRefresh,
    startRefreshBatch,
    retryAfterFailure,
    loadEvents,
    loadMoreEvents,
    setRollupsMode,
    onSummaryRowClick,
    onTotalRowClick,
    clearDrill,
    loadSources,
    saveSnapshot,
    // 可见性与刷新调度（U06 接入点见 requestAutoRefresh / notifyUsageUpdated）
    visible,
    pendingRefresh,
    latestGeneration,
    setVisible,
    requestAutoRefresh,
    notifyUsageUpdated,
  };
}

export type UsageQueryApi = ReturnType<typeof useUsageQuery>;
