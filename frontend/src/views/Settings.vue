<script setup lang="ts">
import {
  computed,
  h,
  inject,
  onActivated,
  onDeactivated,
  onMounted,
  onUnmounted,
  ref,
  watch,
  isRef,
  type Ref,
  type VNode,
} from "vue";
import { priceSourceLine } from "../lib/statsView";
import { invoke } from "@tauri-apps/api/core";
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NDataTable,
  NInput,
  NSelect,
  NSwitch,
  NTag,
  useMessage,
  type DataTableColumn,
} from "naive-ui";
import {
  AGENT_LABEL,
  fmtNum,
  type CacheInfo,
  type CcsImportDefaults,
  type CcsImportPreview,
  type CcsImportReport,
  type PricingEntry,
  type PricingView,
  type SourcePathRow,
} from "../types";
import { TZ_OPTIONS, useTimezone } from "../composables/timezone";
import {
  COLLECT_INTERVAL_OPTIONS,
  adoptCollectInterval,
  useCollectInterval,
} from "../composables/collectInterval";
import { totalTokens } from "../lib/tokenUsage";
import { fmtPriceOrUnknown, formatTieredPricing } from "../lib/tieredPrice";
import {
  TOKEN_BUCKETS,
  UNIT_PRICE_DENOMINATOR,
  UNIT_PRICE_SUFFIX,
  tokenBucketLabel,
  type TokenBucketKey,
} from "../lib/tokenDisplay";
import HelpTooltip from "../components/HelpTooltip.vue";
import ScrollList from "../components/ScrollList.vue";
import SegmentedControl from "../components/SegmentedControl.vue";
import { SETTINGS_PRELOAD } from "../lib/settingsPreload";
import { useTokenColors } from "../composables/tokenColors";

const {
  colors: tokenColors,
  setColor: setTokenColor,
  resetColors,
  persistenceError: colorSaveError,
} = useTokenColors();

const preload = inject(SETTINGS_PRELOAD, null);
const readInitial = preload ? preload.read : invoke;

const props = defineProps<{ refreshKey: number }>();
const emit = defineEmits<{ (e: "data-changed"): void }>();
const msg = useMessage();

// ── U05：分区导航 ────────────────────────────────────────────────
// 一次只显示一个分区（v-show，未选中的分区保留挂载，未保存草稿不丢）；
// 窗口 ≤ 1100px 时导航改为顶部分段控件（与 DESIGN.md §4 的边距断点一致）。
const SECTIONS = [
  { id: "general", label: "通用" },
  { id: "sources", label: "数据源" },
  { id: "usage", label: "用量数据" },
  { id: "pricing", label: "价格" },
  { id: "appearance", label: "外观" },
  { id: "advanced", label: "高级" },
] as const;
type SectionId = (typeof SECTIONS)[number]["id"];
const sectionOptions: { value: SectionId; label: string }[] = SECTIONS.map((s) => ({
  value: s.id,
  label: s.label,
}));

const activeSection = ref<SectionId>("general");
/** 首次进入「价格」分区才挂载价格表并发起首次读取（进入前不发价格表请求）。 */
const pricingVisited = ref(false);

const NARROW_NAV_QUERY = "(max-width: 1100px)";
const narrowNav = ref(false);
const navMode = computed<"side" | "top">(() => (narrowNav.value ? "top" : "side"));
let narrowMedia: MediaQueryList | null = null;

function onNarrowChange(e: MediaQueryListEvent): void {
  narrowNav.value = e.matches;
}

/** U05：切换分区；首次进入「价格」分区时挂载并读取价格表。 */
function selectSection(id: SectionId): void {
  activeSection.value = id;
  if (id === "pricing" && !pricingVisited.value) {
    pricingVisited.value = true;
    void loadPricing();
  }
}

const sourceRows = ref<SourcePathRow[]>([]);
const cache = ref<CacheInfo | null>(null);
const pricing = ref<PricingView | null>(null);
const syncing = ref(false);
const rebuilding = ref(false);
// H05：CCS 手动导入——默认路径只做**路径解析**（不打开来源库）：进入设置页、
// 显示默认路径或恢复上次选择都不会读取 CCS 用量；只有点击按钮才只读打开。
const ccsDefaults = ref<CcsImportDefaults | null>(null);
/** 本次导入使用的来源库路径（默认取本机 CCS 库；用户可改选其它副本）。 */
const ccsPath = ref<string>("");
const ccsPreview = ref<CcsImportPreview | null>(null);
const ccsReport = ref<CcsImportReport | null>(null);
const ccsBusy = ref(false);
const ccsError = ref<string | null>(null);
const ccsTakeSourceRollups = ref(false);
/// H05：重叠候选（同时间同用量但身份不足）的处理——默认不导入，需用户显式确认。
const ccsAllowOverlap = ref(false);
/// H05：日汇总的来源统计时区（默认本机；来源库用 localtime 生成日键）。
const ccsSourceTz = ref<string>("local");
const CCS_TZ_OPTIONS = [
  { label: "本机时区（默认）", value: "local" },
  ...TZ_OPTIONS.filter((o) => o.value !== "local"),
];
/** 预览/结果里的净新增 token 合计（四桶之和，展示用）。 */
const netNewTokens = computed(() =>
  ccsPreview.value ? totalTokens(ccsPreview.value.net_new_tokens) : 0,
);
const reportNetTokens = computed(() =>
  ccsReport.value ? totalTokens(ccsReport.value.net_new_tokens) : 0,
);
// U03：时区的权威来源是 settings.toml——下拉的读写走串行保存队列
// （保存中/失败状态与迁移提示都在「通用」分区的时区行展示）。
const {
  tz,
  saving: tzSaving,
  error: tzError,
  notice: tzNotice,
  clearNotice: clearTzNotice,
} = useTimezone();
// U06：自动读取间隔的唯一来源也是 settings.toml——下拉读写走同一条串行保存
// 队列（`lib/settingQueue`）；权威值由下方 loadSettings 的 settings_get 灌入，
// 不额外发一次设置读取（设置首读失败/挂起场景依赖"首读发生在设置页"）。
const { intervalSecs, saving: intervalSaving, error: intervalError } = useCollectInterval();
const autostart = ref<boolean | null>(null);
const autostartBusy = ref(false);
const autoSync = ref<boolean | null>(null);
const autoSyncBusy = ref(false);
// 关闭确认与配置文件计划 Task 4：关闭窗口默认动作（ask = 每次询问）。
const closeAction = ref<"ask" | "minimize" | "quit">("ask");
// RC03：关闭动作是否已由一次**成功**读取确认。读取失败时为 false——
// 显示"未知"、控件禁用，不把 ask 伪装成可操作默认值。
const closeActionKnown = ref(false);
const CLOSE_ACTION_OPTIONS: { label: string; value: "ask" | "minimize" | "quit" }[] = [
  { label: "每次询问", value: "ask" },
  { label: "最小化到托盘", value: "minimize" },
  { label: "直接退出", value: "quit" },
];

// RC03：设置读取/写入协调——统一读取一次，写入期间作废在途读取。
// - settingsReadSeq：丢弃晚到的旧读取响应；
// - mutationEpoch：任一相关写入开始即推进；旧读取即使 seq 仍最新也不得
//   提交（**保存成功 ≠ 读取的数据更新**）；
// - sourceEditRev：来源行编辑版本；保存成功只清除"提交时版本一致"的
//   dirty，保存期间的新编辑保留；
// - disposed：卸载后请求不得写状态或发通知。
let settingsReadSeq = 0;
let mutationEpoch = 0;
let disposed = false;

// AP04：关闭动作写入串行化。此前快速连续选择会并发发出两个写请求，只在
// 响应端忽略旧结果并不能阻止**后端**按到达顺序落盘旧值——界面显示的默认
// 动作可能与实际持久化的相反。这里从源头串行：任一时刻只有一个写请求在
// 飞；飞行期间的再次选择记为待写意图（只保留最后一次），当前写完成后立即
// 补写，最终 UI 与最后一次实际落盘的值一致。
const closeActionBusy = ref(false);
let closeActionPending: "ask" | "minimize" | "quit" | undefined;

async function setCloseAction(v: "ask" | "minimize" | "quit"): Promise<void> {
  if (!closeActionKnown.value) return; // 未知状态不得写入
  if (closeActionBusy.value) {
    closeActionPending = v; // 记最后一次意图，不并发第二条请求
    closeAction.value = v; // UI 立即反映选择（补写失败会回退）
    return;
  }
  closeActionBusy.value = true;
  let settled = closeAction.value; // 最后一次**确认落盘**的值
  let target: "ask" | "minimize" | "quit" | undefined = v;
  let failed = false;
  try {
    while (target !== undefined) {
      closeActionPending = undefined;
      mutationEpoch++; // 写入开始 → 在途读取失去提交资格
      try {
        await invoke("settings_set_close_action", { action: target === "ask" ? null : target });
      } catch (e) {
        if (disposed) return;
        failed = true;
        msg.error(String(e));
        break;
      }
      if (disposed) return; // AP04：卸载后不得写状态或弹通知
      settled = target;
      closeAction.value = target;
      closeActionKnown.value = true;
      msg.success("关闭窗口默认动作已保存");
      target = closeActionPending;
    }
  } finally {
    closeActionBusy.value = false;
    if (failed) {
      // 回退到已落盘的值：界面不保留一个并未写入的选择。
      closeActionPending = undefined;
      closeAction.value = settled;
    }
  }
}

async function openSettingsFile(): Promise<void> {
  try {
    const p = await invoke<string>("open_settings_file");
    if (disposed) return; // AP04：卸载后不弹通知
    msg.info(`已打开设置配置文件（可直接编辑，保存后对下一次读取生效）：${p}`);
  } catch (e) {
    if (disposed) return;
    msg.error(String(e));
  }
}

// UX06：各读取区块独立错误 + 局部重试——一个区块失败不影响其他区块
// 显示；错误显示原因与重试按钮，无未处理 rejection。
const sourcesError = ref<string | null>(null);
const cacheError = ref<string | null>(null);
const pricingError = ref<string | null>(null);
const settingsError = ref<string | null>(null);
const autostartError = ref<string | null>(null);
// RC03：每个区块还有自己的在途标记与读代次——重试按钮据此显示进度并
// 防重复触发（见 runBlockRead）。
const sourcesLoading = ref(false);
const cacheLoading = ref(false);
const pricingLoading = ref(false);
const settingsLoading = ref(false);
const autostartLoading = ref(false);

/**
 * RC03：写入门槛——"设置状态未知"= 最近一次读取失败**且当前没有在途重试**。
 * 重试进行中不新增禁用：用户本地的草稿仍可保存，而那次晚到的旧读取会被
 * mutationEpoch 作废（保存成功 ≠ 读取的数据更新）。错误条在重试期间保持
 * 可见，不让用户以为问题已经消失。
 */

/** RC03：单个区块的读取协调状态（读代次 + 在途轮次 + 补读意图）。 */
type BlockRead = { seq: number; inflight: Promise<void> | null; again: boolean };
function newBlock(): BlockRead {
  return { seq: 0, inflight: null, again: false };
}

/**
 * RC03：区块读取统一入口。
 * - **读代次**：只有仍属最新一轮的响应才允许提交（值与错误都不例外），
 *   晚到的旧读取不得覆盖新结果；
 * - **卸载守卫**：`disposed` 后一律不提交；
 * - **防重复但不丢刷新**：在途期间的再次触发不并发第二条请求，只记一次
 *   补读意图，本轮结束后立即重读；调用方 `await` 到的是**最终一轮**，
 *   因此"保存后刷新状态"这类串联不会读到中间态。
 */
async function runBlockRead(
  st: BlockRead,
  loadingFlag: Ref<boolean>,
  run: (isCurrent: () => boolean) => Promise<void>,
): Promise<void> {
  if (st.inflight) {
    st.again = true;
    await st.inflight;
    return;
  }
  const token = ++st.seq;
  const round = (async () => {
    loadingFlag.value = true;
    try {
      await run(() => !disposed && st.seq === token);
    } finally {
      loadingFlag.value = false;
      st.inflight = null;
      if (st.again && !disposed) {
        st.again = false;
        await runBlockRead(st, loadingFlag, run);
      }
    }
  })();
  st.inflight = round;
  await round;
}

const sourcesBlock = newBlock();
const cacheBlock = newBlock();
const pricingBlock = newBlock();
const settingsBlock = newBlock();
const autostartBlock = newBlock();

async function loadSources(): Promise<void> {
  await runBlockRead(sourcesBlock, sourcesLoading, async (isCurrent) => {
    try {
      const fresh = await readInitial<SourcePathRow[]>("source_paths_list");
      if (!isCurrent()) return;
      sourceRows.value = fresh ?? [];
      sourcesError.value = null;
    } catch (e) {
      if (!isCurrent()) return;
      sourcesError.value = e instanceof Error ? e.message : String(e);
    }
  });
}

async function loadCache(): Promise<void> {
  await runBlockRead(cacheBlock, cacheLoading, async (isCurrent) => {
    try {
      const fresh = await readInitial<CacheInfo>("cache_stats");
      if (!isCurrent()) return;
      cache.value = fresh;
      cacheError.value = null;
    } catch (e) {
      if (!isCurrent()) return;
      cacheError.value = e instanceof Error ? e.message : String(e);
    }
  });
}

/// H05：读取 CCS 导入的默认路径与存在性——**不打开来源库**，也不导入任何用量。
async function loadCcsDefaults(): Promise<void> {
  try {
    const defaults = await invoke<CcsImportDefaults>("ccs_import_defaults");
    ccsDefaults.value = defaults;
    // 仅在用户尚未编辑时填入默认值（恢复上次选择不触发任何读取）。
    if (!ccsPath.value) ccsPath.value = defaults.path;
  } catch (e) {
    ccsError.value = e instanceof Error ? e.message : String(e);
  }
}

/// H05：生成导入预览（只读来源库；预览不写入任何用量）。
async function previewCcsImport(): Promise<void> {
  if (ccsBusy.value) return; // 防重复提交：预览期间按钮禁用
  ccsBusy.value = true;
  ccsError.value = null;
  ccsReport.value = null;
  try {
    ccsPreview.value = await invoke<CcsImportPreview>("ccs_import_preview", {
      sourcePath: ccsPath.value.trim() || null,
      sourceDayTimezone: ccsSourceTz.value === "local" ? null : ccsSourceTz.value,
    });
    ccsAllowOverlap.value = false;
  } catch (e) {
    ccsPreview.value = null;
    ccsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    ccsBusy.value = false;
  }
}

/// H05：提交本次导入（只执行一个批次；计划一次性消费，重复提交会被后端拒绝）。
///
/// 存在未解决的重叠候选时后端默认拒绝：用户勾选「按新增导入」才把
/// `allowOverlap` 传下去（不猜身份、也不静默把候选当新请求）。
async function commitCcsImport(): Promise<void> {
  const plan = ccsPreview.value;
  if (!plan || ccsBusy.value) return;
  if (plan.overlap_unresolved && !ccsAllowOverlap.value) {
    ccsError.value =
      "存在与本地已有请求时间/用量相同但身份不足的记录：确认它们是不同请求后勾选「按新增导入」，或取消后先核对来源。";
    return;
  }
  ccsBusy.value = true;
  ccsError.value = null;
  try {
    const report = await invoke<CcsImportReport>("ccs_import_commit", {
      planId: plan.plan_id,
      takeSourceRollups: ccsTakeSourceRollups.value,
      allowOverlap: ccsAllowOverlap.value,
    });
    ccsReport.value = report;
    emit("data-changed");
    ccsPreview.value = null;
    ccsAllowOverlap.value = false;
    msg.success("CCS 用量已导入（与已有历史去重求并集）");
    await loadCache();
  } catch (e) {
    // 失败不自动重试。计划是一次性消费（后端在校验前即取出）：无论失败原因
    // （过期 / 代次变化 / 提交错误），预览都已失效——清掉它让「重新预览」
    // 入口直接可用，恢复动作由用户手动触发，不自动重发。
    ccsPreview.value = null;
    ccsAllowOverlap.value = false;
    ccsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    ccsBusy.value = false;
  }
}

/// H05：取消预览——丢弃计划，不写任何用量。
async function discardCcsImport(): Promise<void> {
  const plan = ccsPreview.value;
  ccsPreview.value = null;
  ccsReport.value = null;
  if (!plan) return;
  try {
    await invoke<boolean>("ccs_import_discard", { planId: plan.plan_id });
  } catch (e) {
    ccsError.value = e instanceof Error ? e.message : String(e);
  }
}

async function loadPricing(): Promise<void> {
  await runBlockRead(pricingBlock, pricingLoading, async (isCurrent) => {
    try {
      const fresh = await readInitial<PricingView>("pricing_entries");
      if (!isCurrent()) return;
      pricing.value = fresh;
      pricingError.value = null;
    } catch (e) {
      if (!isCurrent()) return;
      // RC04：读取失败是**未知**，保留旧价格列表，只记录原因（标签由错误条
      // 统一给出，是否保留旧数据在提示里显式说明）。
      pricingError.value = e instanceof Error ? e.message : String(e);
    }
  });
}

async function loadAll(): Promise<void> {
  // 各独立读取并发发起、各自处理失败——不用一个共享 loading/error
  // 覆盖全部结果。RC03：设置配置（来源草稿 + 自动同步 + 关闭动作）
  // 由**同一次** settings_get 初始化，不再分三个消费点各读一遍。
  const tasks: Promise<unknown>[] = [loadSources(), loadCache(), loadSettings(), loadCcsDefaults()];
  // U05：价格视图推迟到首次进入「价格」分区（进入前不发价格表请求、不挂载
  // 价格表）；已访问过后，后台刷新（切页 / refreshKey）照常重读。
  if (pricingVisited.value) tasks.push(loadPricing());
  await Promise.allSettled(tasks);
}

// ── PR3 §5/§9.5 第 5–7 条：来源目录管理（列表 + 追加 + 行级启停/移除）──
//
// 展示后端 source_paths_list 的行，不在 Vue 里重算路径身份/重叠；新增、
// 移除、启停请求串行（同一来源键排队）；保存成功后重读列表，失败保留
// 输入草稿；不以乐观删除掩盖后端拒绝。

const AGENT_IDS = ["claude", "codex"] as const;
type AgentId = (typeof AGENT_IDS)[number];

/** 应用总开关与自动发现开关（改完即保存，走各自命令）。 */
const agentEnabled = ref<Record<AgentId, boolean>>({ claude: true, codex: true });
const agentAuto = ref<Record<AgentId, boolean>>({ claude: true, codex: true });
const switchSaving = ref<string[]>([]);
const switchErrors = ref<Record<string, string>>({});

/** 每个来源键的追加输入草稿（失败保留、成功清空）。 */
const addDrafts = ref<Record<AgentId, string>>({ claude: "", codex: "" });
const addSaving = ref<string[]>([]);
const sourceErrors = ref<Record<string, string>>({});
/** 行级启停/移除的操作中键。 */
const rowOpSaving = ref<string[]>([]);
const rowErrors = ref<Record<string, string>>({});
/** 已保存但列表重读失败的提示（≠ 保存失败）。 */
const refreshAfterSaveError = ref<string | null>(null);

function isBusy(list: string[] | Ref<string[]>, key: string): boolean {
  const arr = isRef(list) ? list.value : list;
  return arr.includes(key);
}

/** 串行队列：同一键的操作逐个执行（排队期间新操作继续入队）。 */
const opChains = new Map<string, Promise<void>>();
function enqueueSourceOp(key: string, op: () => Promise<void>): Promise<void> {
  const prev = opChains.get(key) ?? Promise.resolve();
  const next = prev.then(op, op);
  opChains.set(
    key,
    next.catch(() => {}),
  );
  return next;
}

function rowsOf(agent: AgentId): SourcePathRow[] {
  return sourceRows.value.filter((r) => r.agent === agent);
}

/** 移除只对手动行可用：没有手动 origin 的行（纯默认/环境）不显示移除。 */
function isRemovable(row: SourcePathRow): boolean {
  return row.origins.includes("手动");
}

function stateText(row: SourcePathRow): string {
  const base =
    row.state === "ready"
      ? "可读取"
      : row.state === "missing"
        ? "缺失"
        : row.state === "no_permission"
          ? "无权限"
          : "目录无效";
  return row.row_enabled ? base : `已停用 · ${base}`;
}

function errorTextOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** 应用总开关：改完即保存（事务只写 enabled 一个字段）。 */
function setAgentEnabled(agent: AgentId, v: boolean): void {
  const key = `switch:${agent}`;
  if (isBusy(switchSaving, key)) return;
  mutationEpoch++;
  const before = agentEnabled.value[agent];
  agentEnabled.value[agent] = v;
  switchSaving.value = [...switchSaving.value, key];
  delete switchErrors.value[key];
  enqueueSourceOp(key, async () => {
    try {
      await invoke("source_config_set", { agent, enabled: v, dir: null });
      if (disposed) return;
      refreshAfterSaveError.value = null;
    } catch (e) {
      if (disposed) return;
      agentEnabled.value[agent] = before; // 失败回滚到已落盘值
      switchErrors.value[key] = errorTextOf(e);
    } finally {
      switchSaving.value = switchSaving.value.filter((x) => x !== key);
    }
  });
}

/** 自动发现开关：改完即保存。 */
function setAgentAuto(agent: AgentId, v: boolean): void {
  const key = `auto:${agent}`;
  if (isBusy(switchSaving, key)) return;
  mutationEpoch++;
  const before = agentAuto.value[agent];
  agentAuto.value[agent] = v;
  switchSaving.value = [...switchSaving.value, key];
  delete switchErrors.value[key];
  enqueueSourceOp(key, async () => {
    try {
      await invoke("source_auto_set", { agent, enabled: v });
      if (disposed) return;
    } catch (e) {
      if (disposed) return;
      agentAuto.value[agent] = before;
      switchErrors.value[key] = errorTextOf(e);
    } finally {
      switchSaving.value = switchSaving.value.filter((x) => x !== key);
    }
  });
}

/** 追加目录：明确动作（点击「添加」才提交）；成功清空草稿并重读列表。 */
function addSource(agent: AgentId): void {
  const key = `add:${agent}`;
  if (isBusy(addSaving, key)) return; // 防重复提交
  const raw = addDrafts.value[agent].trim();
  if (raw === "") return;
  mutationEpoch++;
  addSaving.value = [...addSaving.value, key];
  delete sourceErrors.value[key];
  refreshAfterSaveError.value = null;
  enqueueSourceOp(key, async () => {
    try {
      await invoke("source_path_add", { agent, path: raw });
      if (disposed) return;
      addDrafts.value[agent] = ""; // 成功清空；失败保留草稿
      await loadSources();
      if (disposed) return;
      if (sourcesError.value) {
        refreshAfterSaveError.value = `已保存，列表刷新失败（${sourcesError.value}，可重试）`;
      }
    } catch (e) {
      if (disposed) return;
      // 后端拒绝（形态/同应用嵌套/跨应用冲突）只归属该来源行；输入保留。
      sourceErrors.value[key] = errorTextOf(e);
    } finally {
      addSaving.value = addSaving.value.filter((x) => x !== key);
    }
  });
}

/** 行启停：作用于该行全部 origin；失败回滚显示值并保留错误。 */
function setRowEnabled(row: SourcePathRow, v: boolean): void {
  const key = `op:${row.agent}:${row.id}`;
  if (isBusy(rowOpSaving, key)) return;
  const before = row.row_enabled;
  row.row_enabled = v;
  rowOpSaving.value = [...rowOpSaving.value, key];
  delete rowErrors.value[key];
  enqueueSourceOp(key, async () => {
    try {
      await invoke("source_path_set_enabled", { agent: row.agent, id: row.id, enabled: v });
      if (disposed) return;
      await loadSources();
    } catch (e) {
      if (disposed) return;
      row.row_enabled = before; // 失败回滚（不以乐观更新掩盖拒绝）
      rowErrors.value[key] = errorTextOf(e);
    } finally {
      rowOpSaving.value = rowOpSaving.value.filter((x) => x !== key);
    }
  });
}

/** 移除手动项；移除只影响后续读取，历史库不删数据。 */
function removeRow(row: SourcePathRow): void {
  const key = `op:${row.agent}:${row.id}`;
  if (isBusy(rowOpSaving, key)) return;
  rowOpSaving.value = [...rowOpSaving.value, key];
  delete rowErrors.value[key];
  enqueueSourceOp(key, async () => {
    try {
      await invoke("source_path_remove", { agent: row.agent, id: row.id });
      if (disposed) return;
      await loadSources();
    } catch (e) {
      if (disposed) return;
      rowErrors.value[key] = errorTextOf(e);
    } finally {
      rowOpSaving.value = rowOpSaving.value.filter((x) => x !== key);
    }
  });
}

/** RC03：设置配置读取结果（自动同步 + 关闭动作 + 开关初值，同一次读取）。 */
type SettingsPayload = {
  close_action?: "minimize" | "quit" | null;
  price_auto_sync?: boolean;
  sources?: Record<string, { enabled?: boolean; auto_discover?: boolean }>;
  /** U06：后台自动读取 JSONL 的间隔（秒）；缺省 300。 */
  collect_interval_secs?: number;
};

/**
 * RC03：统一设置读取——**一次** settings_get 同时填充来源草稿、自动同步
 * 与关闭动作；任一相关写入开始（mutationEpoch 变化）或读取已过期
 * （settingsReadSeq 变化）、组件已卸载时都不提交。
 *
 * 失败是**未知**状态：不清空已有数据、不把 ask/false 伪装成可操作默认值；
 * 关闭动作控件禁用并说明原因，重试入口调用本函数恢复全部依赖项。
 */
async function loadSettings(): Promise<void> {
  await runBlockRead(settingsBlock, settingsLoading, async () => {
    const readSeq = ++settingsReadSeq;
    const epochAtRead = mutationEpoch;
    let s: SettingsPayload;
    try {
      s = await readInitial<SettingsPayload>("settings_get");
    } catch (e) {
      if (readSeq !== settingsReadSeq || disposed) return;
      // AP04：晚到的**失败**读取与晚到的成功读取适用同一守卫——写入已
      // 发生（mutationEpoch 变化）时，旧读取的拒绝不得把刚保存的值降级成
      // 「未知」，也不得把旧错误当成当前状态。修复前只有成功分支检查。
      if (epochAtRead !== mutationEpoch) return;
      settingsError.value = e instanceof Error ? e.message : String(e);
      closeActionKnown.value = false;
      autoSync.value = null;
      return;
    }
    // 晚到的旧读取 / 写入已发生 / 已卸载 → 不提交（旧读取不覆盖用户草稿）。
    if (readSeq !== settingsReadSeq || epochAtRead !== mutationEpoch || disposed) return;
    settingsError.value = null;
    closeAction.value = s.close_action ?? "ask";
    closeActionKnown.value = true;
    autoSync.value = s.price_auto_sync ?? null;
    // U06：自动读取间隔的权威值（缺字段、白名单外的脏值都在 adopt 内回缺省）。
    adoptCollectInterval(s.collect_interval_secs);
    const src = s.sources ?? {};
    agentEnabled.value.claude = src.claude?.enabled ?? true;
    agentEnabled.value.codex = src.codex?.enabled ?? true;
    agentAuto.value.claude = src.claude?.auto_discover ?? true;
    agentAuto.value.codex = src.codex?.auto_discover ?? true;
  });
}

async function rebuild(): Promise<void> {
  // UX08：函数级防重复——不只依赖按钮 loading 态（程序化触发/快速双击
  // 都不得并发发起第二次重扫）。重扫是解析层操作，保留既有进度反馈，
  // 不增加强制确认。
  if (rebuilding.value) return;
  rebuilding.value = true;
  try {
    const info = await invoke<CacheInfo>("refresh_cache");
    if (disposed) return; // AP04：卸载后不写状态或弹通知
    cache.value = info;
    emit("data-changed");
    // H04：重扫 = 重置采集指纹并重新解析当前文件；范围 = 当前生效的来源配置，
    // 且**不删除**已保存的历史用量（历史删除不是缓存操作）。
    msg.success("已重扫日志（按当前来源配置重新解析，已保存的历史用量不受影响）");
  } catch (e) {
    if (disposed) return;
    msg.error(String(e));
  } finally {
    rebuilding.value = false;
  }
}

async function syncPricing(): Promise<void> {
  // RC04：函数级防重复——不只依赖按钮 loading（程序化触发/快速双击都不得
  // 并发发起第二次同步）；卸载后在途请求不得再写状态或发通知。
  if (syncing.value || disposed) return;
  syncing.value = true;
  try {
    const reports = await invoke<{ source: string; count: number }[]>("sync_pricing_openrouter");
    if (disposed) return; // AP04：卸载后不弹通知
    msg.success(reports.map((r) => `${r.source} ${r.count} 条`).join("，"));
  } catch (e) {
    // 部分失败也要刷新：主源已写盘的数据立即可见（审阅 Task 3）。
    if (disposed) return;
    msg.error(`同步失败：${e}`);
  } finally {
    // 成功与部分失败两条路径都重新读取价格视图 + 通知全局横幅刷新；
    // 列表读取失败由 loadPricing 归类为"价格列表读取失败"并保留旧数据
    //（RC04：不把读取失败伪装成同步失败，也不清空已显示的价格）。
    try {
      await loadPricing();
    } finally {
      if (!disposed) {
        window.dispatchEvent(new Event("pricing-status-changed"));
        emit("data-changed");
      }
      syncing.value = false;
    }
  }
}

async function openPricing(): Promise<void> {
  try {
    const p = await invoke<string>("open_pricing_file");
    if (disposed) return; // AP04：卸载后不弹通知
    msg.info(`已在系统默认编辑器打开（保存后下次统计生效）\n${p}`);
    // RC03：价格列表只经 loadPricing 一条路径读取（带读代次/卸载守卫）。
    await loadPricing();
  } catch (e) {
    if (disposed) return;
    msg.error(String(e));
  }
}

async function loadAutostart(): Promise<void> {
  await runBlockRead(autostartBlock, autostartLoading, async (isCurrent) => {
    try {
      const fresh = await readInitial<boolean>("autostart_status");
      if (!isCurrent()) return;
      autostart.value = fresh;
      autostartError.value = null;
    } catch (e) {
      if (!isCurrent()) return;
      autostartError.value = e instanceof Error ? e.message : String(e);
    }
  });
}

async function setAutostart(enabled: boolean): Promise<void> {
  autostartBusy.value = true;
  try {
    const fresh = await invoke<boolean>("autostart_set", { enabled });
    if (disposed) return; // AP04：卸载后不写状态或弹通知
    autostart.value = fresh;
    msg.success(enabled ? "已开启开机自启" : "已关闭开机自启");
  } catch (e) {
    if (disposed) return;
    msg.error(String(e));
  } finally {
    autostartBusy.value = false;
  }
}

async function setAutoSync(enabled: boolean): Promise<void> {
  // RC03：未知状态（读取失败）不得写入，避免把未知改成猜测值。
  if (autoSync.value == null) return;
  mutationEpoch++; // 写入开始 → 在途读取失去提交资格
  autoSyncBusy.value = true;
  try {
    const committed = await invoke<boolean>("settings_set_price_auto_sync", { enabled });
    if (disposed) return; // AP04：卸载后不写状态或弹通知
    autoSync.value = committed;
    msg.success(enabled ? "已开启自动同步（每 24h）" : "已关闭自动同步");
  } catch (e) {
    if (disposed) return;
    msg.error(String(e));
  } finally {
    autoSyncBusy.value = false;
  }
}

let active = true;
let activatedOnce = false;
onActivated(() => {
  active = true;
  if (activatedOnce) {
    void loadAll();
    void loadAutostart();
  }
  activatedOnce = true;
});
onDeactivated(() => {
  active = false;
});
onMounted(() => {
  // U05：布局断点监听。非浏览器 / 测试环境可能没有 matchMedia——按宽窗口
  // 处理，不抛异常。
  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    narrowMedia = window.matchMedia(NARROW_NAV_QUERY);
    narrowNav.value = narrowMedia.matches;
    if (typeof narrowMedia.addEventListener === "function") {
      narrowMedia.addEventListener("change", onNarrowChange);
    } else if (typeof narrowMedia.addListener === "function") {
      narrowMedia.addListener(onNarrowChange);
    }
  }
  // RC03：设置配置只读一次（loadAll → loadSettings）。
  void loadAll().then(() => {
    // 久未打开的启动预读先展示，再校验新鲜度，不遮挡现有界面。
    if (!disposed && preload && preload.age() > 30_000) void loadAll();
  });
  void loadAutostart();
});
watch(
  () => props.refreshKey,
  () => {
    if (active) void loadAll();
  },
);
onUnmounted(() => {
  disposed = true; // RC03：卸载后请求不得写状态或发通知
  const mq = narrowMedia;
  narrowMedia = null;
  if (!mq) return;
  if (typeof mq.removeEventListener === "function")
    mq.removeEventListener("change", onNarrowChange);
  else if (typeof mq.removeListener === "function") mq.removeListener(onNarrowChange);
});

/// RC06：单价列由**显式四桶 key** 决定名称与取值。
///
/// 修复前 `priceCell(r, pick)` 用 `columnLabelOf(pick)` 反查列语义，靠
/// `pick === ((e) => e.input)` 比较**函数对象**——每次调用都新建箭头函数，
/// 比较永不相等，于是四列全部显示成「缓存命中单价说明」。现在 key 显式
/// 传入，显示词统一来自 `tokenDisplay` 单一来源。
function priceCell(r: object, key: TokenBucketKey): VNode {
  const e = asEntry(r);
  const or = e.openrouter;
  const orLine = or
    ? `OpenRouter：${TOKEN_BUCKETS.map((b) => `${b.label} ${fmtPriceOrUnknown(or[b.key])}`).join(" · ")}`
    : "OpenRouter：未知价格（无对应模型）";
  const spec = e[key];
  const unknown = spec == null;
  // UX03：hover/focus/click/Enter/Space 可读、Escape 关闭且焦点不离开
  // 触发器；aria-describedby 指向打开时的内容节点。说明文字用语义色。
  return h(
    HelpTooltip,
    { label: `${e.prefix} ${tokenBucketLabel(key)}单价说明` },
    {
      trigger: () =>
        h(
          "span",
          { style: unknown ? "color: var(--ts-warning)" : undefined },
          fmtPriceOrUnknown(spec),
        ),
      default: () =>
        h("div", { style: "font-size: 12px; line-height: 1.8" }, [
          h("div", priceSourceLine(e)),
          h("div", orLine),
          or?.name
            ? h("div", { style: "color: var(--ts-text-secondary)" }, `模型：${or.name}`)
            : null,
        ]),
    },
  );
}

/// Task 8：模型前缀列——有分段/峰谷规则时悬浮展开档位明细。
function prefixCell(r: object): VNode {
  const e = asEntry(r);
  const lines = formatTieredPricing(e);
  if (!lines.length) return h("span", e.prefix);
  // UX03：档位明细同样走可访问浮层（focus/click/Escape 全路径）。
  return h(
    HelpTooltip,
    { label: `${e.prefix} 档位计价明细`, contentLabel: "档位计价明细" },
    {
      trigger: () => [
        e.prefix,
        h(
          NTag,
          { size: "tiny", bordered: false, type: "info", style: "margin-left: 6px" },
          { default: () => "分段" },
        ),
      ],
      default: () =>
        h(
          "div",
          { style: "white-space: normal" },
          lines.map((t) => h("div", t)),
        ),
    },
  );
}

const priceColumns = computed<DataTableColumn[]>(() => [
  {
    title: "显示名",
    key: "name",
    render: (r) => asEntry(r).name ?? "",
  },
  {
    title: "模型前缀",
    key: "prefix",
    render: (r) => prefixCell(r),
  },
  // RC06：四桶单价列由显式 key 驱动，显示词来自 tokenDisplay 单一来源
  //（不再散落「缓存读$」）。RC07：列头明确量纲——单价是 USD / 1M token。
  {
    title: `${tokenBucketLabel("input")}（${UNIT_PRICE_SUFFIX}）`,
    key: "input",
    align: "center",
    render: (r) => priceCell(r, "input"),
  },
  {
    title: `${tokenBucketLabel("output")}（${UNIT_PRICE_SUFFIX}）`,
    key: "output",
    align: "center",
    render: (r) => priceCell(r, "output"),
  },
  {
    title: `${tokenBucketLabel("cache_write")}（${UNIT_PRICE_SUFFIX}）`,
    key: "cache_write",
    align: "center",
    render: (r) => priceCell(r, "cache_write"),
  },
  {
    title: `${tokenBucketLabel("cache_read")}（${UNIT_PRICE_SUFFIX}）`,
    key: "cache_read",
    align: "center",
    render: (r) => priceCell(r, "cache_read"),
  },
  {
    title: "来源",
    key: "source",
    render: (r) =>
      h(
        NTag,
        {
          // 设计系统 Task 7：来源身份用中性色；成功/警告/错误只表达状态
          size: "small",
          bordered: false,
        },
        { default: () => asEntry(r).source },
      ),
  },
]);

/// Naive UI 表格行类型是 Record<string, unknown>，统一经 unknown 转换。
const asEntry = (r: object): PricingEntry => r as unknown as PricingEntry;
// 同前缀可能同时存在 models.dev/openrouter/外置行，键必须含来源
const rowKey = (r: object): string => `${asEntry(r).source}|${asEntry(r).prefix}`;

// UX03：暴露价格列定义供真实浮层测试触达（NDataTable 测试环境不渲染行）。
defineExpose({ priceColumns });
</script>

<template>
  <div class="settings-view" :data-nav-mode="navMode" :class="{ 'is-top-nav': navMode === 'top' }">
    <!-- U05：分区导航——窗口 ≤ 1100px 改为顶部分段控件。导航只切换显示，
         不销毁分区（未选中的分区用 v-show 隐藏，未保存草稿留在实例里）。 -->
    <div class="settings-nav">
      <nav v-if="navMode === 'side'" class="settings-nav-rail ts-card" aria-label="设置分区">
        <button
          v-for="s in SECTIONS"
          :key="s.id"
          type="button"
          class="settings-nav-item ts-focusable"
          :class="{ 'is-active': activeSection === s.id }"
          :aria-current="activeSection === s.id ? 'true' : undefined"
          @click="selectSection(s.id)"
        >
          {{ s.label }}
        </button>
      </nav>
      <SegmentedControl
        v-else
        class="settings-nav-segmented"
        :model-value="activeSection"
        :options="sectionOptions"
        aria-label="设置分区"
        @update:model-value="selectSection"
      />
    </div>
    <!-- 首次读取尚未完成时保留稳定布局，各分区独立显示状态。 -->
    <div class="settings-panels">
      <!-- 任务 7：macOS 系统设置式分组——分区标题在卡片外，每组一张 .ts-card -->
      <section
        class="settings-group settings-panel"
        v-show="activeSection === 'general'"
        data-section="general"
      >
        <h2 class="group-title">通用</h2>
        <div v-if="autostartError" class="ts-notice ts-notice-inline block-error" role="alert">
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
          <span class="ts-notice-content">自启状态读取失败：{{ autostartError }}</span>
          <NButton size="tiny" :loading="autostartLoading" @click="loadAutostart">重试</NButton>
        </div>
        <section class="ts-card settings-card">
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">聚合/展示时区</div>
              <div class="setting-help">
                存储/计算一律 UTC，仅展示按此时区一次转换。
                <span v-if="tzSaving" class="tz-state">保存中…</span>
                <span v-else-if="tzError" class="tz-state is-error">
                  保存失败：{{ tzError }}（已回到上次保存的值）
                </span>
              </div>
            </div>
            <div class="setting-control">
              <NSelect
                v-model:value="tz"
                :options="TZ_OPTIONS"
                size="small"
                style="width: 200px"
                aria-label="聚合/展示时区"
              />
            </div>
          </div>
          <!-- U03：旧 localStorage 时区的一次性迁移提示（非阻断，可关闭）。 -->
          <div v-if="tzNotice" class="ts-notice ts-notice-inline tz-notice" role="status">
            <span class="ts-notice-content">{{ tzNotice }}</span>
            <NButton size="tiny" @click="clearTzNotice">知道了</NButton>
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">开机自启</div>
              <div class="setting-help">开机后自动启动并驻留托盘。</div>
            </div>
            <div class="setting-control">
              <NSwitch
                :value="autostart === true"
                :disabled="autostart == null || autostartBusy"
                :loading="autostartBusy"
                aria-label="开机自启"
                @update:value="setAutostart"
              />
            </div>
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">关闭窗口时</div>
              <div class="setting-help">
                <template v-if="!closeActionKnown">
                  读取失败，关闭动作未知（点击上方错误条重试后可修改）。
                </template>
                <template v-else>
                  最小化后可从托盘恢复；记忆后仍可在此修改或恢复每次询问。
                </template>
              </div>
            </div>
            <div class="setting-control">
              <NSelect
                :value="closeAction"
                :options="CLOSE_ACTION_OPTIONS"
                size="small"
                style="width: 160px"
                aria-label="关闭窗口时"
                :disabled="!closeActionKnown"
                :loading="closeActionBusy"
                @update:value="setCloseAction"
              />
            </div>
          </div>
        </section>
      </section>

      <section
        class="settings-group settings-panel"
        v-show="activeSection === 'sources'"
        data-section="sources"
      >
        <h2 class="group-title">数据源</h2>
        <!-- UX06：区块独立错误 + 局部重试 -->
        <div v-if="settingsError" class="ts-notice ts-notice-inline block-error" role="alert">
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
          <span class="ts-notice-content"
            >设置读取失败，来源配置暂不可保存：{{ settingsError }}</span
          >
          <NButton size="tiny" :loading="settingsLoading" @click="loadSettings">重试</NButton>
        </div>
        <div v-if="sourcesError" class="ts-notice ts-notice-inline block-error" role="alert">
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
          <span class="ts-notice-content">来源状态读取失败：{{ sourcesError }}</span>
          <NButton size="tiny" :loading="sourcesLoading" @click="loadSources">重试</NButton>
        </div>
        <section class="ts-card settings-card">
          <div
            v-for="agent in AGENT_IDS"
            :key="agent"
            class="source-block"
            :class="{ 'has-divider': agent !== AGENT_IDS[0] }"
          >
            <div class="source-head">
              <strong>{{ AGENT_LABEL[agent] ?? agent }}</strong>
              <NSwitch
                :value="agentEnabled[agent]"
                size="small"
                :aria-label="`${AGENT_LABEL[agent] ?? agent} 启用`"
                @update:value="(v: boolean) => setAgentEnabled(agent, v)"
              />
              <span class="dir-label">自动发现</span>
              <NSwitch
                :value="agentAuto[agent]"
                size="small"
                :aria-label="`${AGENT_LABEL[agent] ?? agent} 自动发现`"
                @update:value="(v: boolean) => setAgentAuto(agent, v)"
              />
              <span
                v-if="
                  isBusy(switchSaving, `switch:${agent}`) || isBusy(switchSaving, `auto:${agent}`)
                "
                class="source-state"
                >保存中…</span
              >
              <span
                v-else-if="switchErrors[`switch:${agent}`] || switchErrors[`auto:${agent}`]"
                class="source-state is-error"
                >保存失败</span
              >
            </div>
            <div
              v-if="switchErrors[`switch:${agent}`] || switchErrors[`auto:${agent}`]"
              class="ts-notice ts-notice-inline source-error"
            >
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
                switchErrors[`switch:${agent}`] || switchErrors[`auto:${agent}`]
              }}</span>
            </div>

            <!-- 来源行列表（后端解析的身份/标签/状态；前端不重算） -->
            <div
              v-for="row in rowsOf(agent)"
              :key="row.id"
              class="source-row"
              :class="{ 'is-disabled': !row.row_enabled }"
            >
              <div class="source-row-head">
                <NSwitch
                  :value="row.row_enabled"
                  size="small"
                  :aria-label="`${AGENT_LABEL[agent] ?? agent} ${row.path} 启用`"
                  @update:value="(v: boolean) => setRowEnabled(row, v)"
                />
                <span v-for="tag in row.origins" :key="tag" class="origin-tag">{{ tag }}</span>
                <span class="source-state">{{ stateText(row) }}</span>
                <span class="flex-fill"></span>
                <NButton
                  v-if="isRemovable(row)"
                  size="tiny"
                  text
                  :loading="isBusy(rowOpSaving, `op:${row.agent}:${row.id}`)"
                  :aria-label="`移除 ${row.path}`"
                  @click="removeRow(row)"
                >
                  移除
                </NButton>
              </div>
              <!-- 完整路径：可复制、长路径换行不省略 -->
              <div class="source-effective">
                <span class="effective-path ts-mono">{{ row.path }}</span>
              </div>
              <div v-if="row.diagnostics.length" class="row-diagnostics">
                <span v-for="d in row.diagnostics" :key="d" class="diag-line">{{ d }}</span>
              </div>
              <div v-if="rowErrors[`op:${row.agent}:${row.id}`]" class="row-error">
                {{ rowErrors[`op:${row.agent}:${row.id}`] }}
              </div>
            </div>

            <!-- 追加目录：明确动作；错误保留草稿 -->
            <label class="dir-label" :for="`source-add-${agent}`"
              >{{ AGENT_LABEL[agent] ?? agent }} 追加日志目录</label
            >
            <div class="add-row">
              <NInput
                :value="addDrafts[agent]"
                size="small"
                placeholder="绝对路径（如 D:\\history\\claude\\projects）"
                :input-props="{
                  id: `source-add-${agent}`,
                  'aria-label': `${AGENT_LABEL[agent] ?? agent} 追加日志目录`,
                }"
                :disabled="isBusy(addSaving, `add:${agent}`)"
                @update:value="(v: string) => (addDrafts[agent] = v)"
                @keydown.enter="addSource(agent)"
              />
              <NButton
                size="small"
                :loading="isBusy(addSaving, `add:${agent}`)"
                :aria-label="`${AGENT_LABEL[agent] ?? agent} 添加目录`"
                @click="addSource(agent)"
              >
                添加
              </NButton>
            </div>
            <div
              v-if="sourceErrors[`add:${agent}`]"
              class="ts-notice ts-notice-inline source-error"
              role="alert"
            >
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
              <span class="ts-notice-content">{{ sourceErrors[`add:${agent}`] }}</span>
            </div>
            <div class="setting-help">
              自动发现覆盖当前用户的默认目录与环境变量指定的目录（环境变量只在启动时读取，修改后需重启
              TokenScope）；追加目录按列表逐个读取；移除或停用只影响后续读取，已保存的用量不会删除。
            </div>
          </div>
          <div
            v-if="refreshAfterSaveError"
            class="ts-notice ts-notice-inline block-error"
            role="alert"
          >
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
            <span class="ts-notice-content">{{ refreshAfterSaveError }}</span>
          </div>
          <!-- U06/PR2：自动读取间隔——后台按此间隔读取 JSONL 日志；写入自动
               保存，修改立即生效；只有确有新用量才刷新统计（读取心跳不代表
               有新数据），汇总页读取状态由真实采集生命周期驱动。 -->
          <div class="setting-row interval-row">
            <div class="setting-main">
              <div class="setting-label">自动读取间隔</div>
              <div class="setting-help">
                后台按此间隔自动读取日志并保存（自动保存，修改后立即生效）；只有出现新用量才刷新统计，没有新用量时数字保持不变。
                <span v-if="intervalSaving" class="tz-state">保存中…</span>
                <span v-else-if="intervalError" class="tz-state is-error">
                  保存失败：{{ intervalError }}（已回到上次保存的值）
                </span>
              </div>
            </div>
            <div class="setting-control">
              <NSelect
                v-model:value="intervalSecs"
                :options="COLLECT_INTERVAL_OPTIONS"
                size="small"
                style="width: 140px"
                aria-label="自动读取间隔"
              />
            </div>
          </div>
        </section>
      </section>

      <section
        class="settings-group settings-panel"
        v-show="activeSection === 'usage'"
        data-section="usage"
      >
        <h2 class="group-title">用量数据</h2>
        <div v-if="cacheError" class="ts-notice ts-notice-inline block-error" role="alert">
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
          <span class="ts-notice-content">用量数据统计读取失败：{{ cacheError }}</span>
          <NButton size="tiny" :loading="cacheLoading" @click="loadCache">重试</NButton>
        </div>
        <section class="ts-card settings-card">
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">已采集文件</div>
            </div>
            <span class="setting-value ts-num">{{ cache ? fmtNum(cache.files) : "—" }}</span>
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">已保存请求</div>
            </div>
            <span class="setting-value ts-num">{{ cache ? fmtNum(cache.events) : "—" }}</span>
          </div>
          <div class="setting-help">
            用量保存在
            <span class="ts-mono">history.db</span
            >；来源日志删除或停用后，已保存的用量仍保留；重扫只重新解析日志，不删除历史。
          </div>
          <NCollapse class="tech-collapse">
            <NCollapseItem title="技术详情" name="tech">
              <!-- UX08：完整「当前生效目录」（含显式覆盖）——可选中复制、长路径换行 -->
              <div v-for="row in sourceRows" :key="`eff-${row.id}`" class="tech-line">
                {{ AGENT_LABEL[row.agent] ?? row.agent }} 日志根（{{ row.origins.join("、") }}）：{{
                  row.path
                }}
              </div>
              <div class="tech-line">历史库路径：{{ cache?.path ?? "—" }}</div>
              <div class="tech-line">models.dev 快照：{{ pricing?.modelsdev_path ?? "—" }}</div>
              <div class="tech-line">OpenRouter 快照：{{ pricing?.openrouter_path ?? "—" }}</div>
              <div class="tech-line">外置价格文件：{{ pricing?.path ?? "—" }}</div>
              <div class="tech-line">
                models.dev 上次同步：{{ pricing?.modelsdev_synced_at ?? "—" }}
              </div>
              <div class="tech-line">
                OpenRouter 上次同步：{{ pricing?.openrouter_synced_at ?? "—" }}
              </div>
            </NCollapseItem>
          </NCollapse>
          <div class="rebuild-row">
            <NButton size="small" :loading="rebuilding" @click="rebuild">重扫日志</NButton>
            <!-- UX08：重扫预期说明；AP03：范围由当前来源配置决定；
                 H04：重扫只重置采集指纹并重新解析当前文件，**不**删除已保存的用量 -->
            <span class="rebuild-hint"
              >重置采集指纹并按当前来源配置重新扫描日志，可能需要一段时间（已保存的历史用量不会被删除）。</span
            >
          </div>
        </section>

        <!-- U05：CCS 手动导入并入「用量数据」分区（同一分区里的第二张卡） -->
        <section class="ts-card settings-card">
          <div class="setting-help">
            手动一次性导入 cc-switch 的历史用量（请求明细 + 日汇总）：只在点击后<span
              class="ts-strong"
              >只读</span
            >打开来源库，不自动同步、不重试；导入结果与 TokenScope
            已有历史去重求并集，重复导入不会重复计费。
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">来源库</div>
              <div class="setting-help">
                {{ ccsDefaults?.exists ? "默认路径已找到" : "默认路径未找到（可改选其它副本）" }}
                <template v-if="ccsDefaults"
                  >；日汇总按来源时区 {{ ccsDefaults.timezone }} 记录</template
                >
              </div>
            </div>
            <!-- H05：来源库路径可改选其它副本；稳定可访问名称与来源目录字段一致
                 （label 关联 + 真实 input 的 aria-label） -->
            <label class="dir-label ccs-path-label" for="ccs-import-path">来源库路径</label>
            <NInput
              v-model:value="ccsPath"
              class="ccs-path"
              size="small"
              placeholder="留空使用默认位置"
              :disabled="ccsBusy || !!ccsPreview"
              :input-props="{
                id: 'ccs-import-path',
                'aria-label': '来源库路径',
              }"
            />
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">来源统计时区</div>
              <div class="setting-help">
                来源库按本机时区生成日键：日汇总只在<span class="ts-strong">这个时区</span
                >下与展示时区一致时才参与按日统计（不一致时按日视图会提示切换或只看请求明细）。
              </div>
            </div>
            <NSelect
              v-model:value="ccsSourceTz"
              class="ccs-tz"
              size="small"
              :options="CCS_TZ_OPTIONS"
              :disabled="ccsBusy || !!ccsPreview"
              aria-label="来源统计时区"
            />
          </div>
          <div class="rebuild-row">
            <NButton
              size="small"
              :loading="ccsBusy"
              :disabled="ccsBusy || !!ccsPreview"
              @click="previewCcsImport"
              >从 CCS 导入用量</NButton
            >
            <span class="rebuild-hint"
              >点击后才只读打开该库；换选同一库的备份仍更新同一逻辑来源，不重复计费。</span
            >
          </div>

          <div v-if="ccsError" class="ts-notice ts-notice-inline block-error" role="alert">
            <span class="ts-notice-content">CCS 导入未完成：{{ ccsError }}</span>
          </div>

          <div v-if="ccsPreview" class="ccs-preview">
            <div class="setting-row">
              <div class="setting-main">
                <div class="setting-label">预览（尚未写入）</div>
                <div class="setting-help">
                  明细 {{ fmtNum(ccsPreview.requests_total) }} 条：可导入
                  {{ fmtNum(ccsPreview.requests_importable) }}、其他应用
                  {{ fmtNum(ccsPreview.requests_skipped_other_app) }}、与代理日志重复
                  {{ fmtNum(ccsPreview.requests_skipped_duplicate_of_proxy) }}、字段不合法
                  {{ fmtNum(ccsPreview.requests_rejected) }}；日汇总
                  {{ fmtNum(ccsPreview.rollups_total) }} 条（新增
                  {{ fmtNum(ccsPreview.rollups_new) }}、已存在
                  {{ fmtNum(ccsPreview.rollups_unchanged) }}、冲突
                  {{ fmtNum(ccsPreview.rollups_conflicting) }}、不支持/不合法被拒
                  {{ fmtNum(ccsPreview.rollups_rejected) }}）。
                </div>
              </div>
            </div>
            <div class="setting-row">
              <div class="setting-main">
                <div class="setting-label">预计变化</div>
                <div class="setting-help">
                  新增 {{ fmtNum(ccsPreview.would_insert) }}、更新
                  {{ fmtNum(ccsPreview.would_update) }}、已存在
                  {{ fmtNum(ccsPreview.would_unchanged) }}、冲突保留现有值
                  {{ fmtNum(ccsPreview.would_conflict) }}、过期跳过
                  {{ fmtNum(ccsPreview.would_stale) }}（库内已有更晚观察的终值，已跳过）； 净新增
                  token {{ fmtNum(netNewTokens) }}。无项目归属记录
                  {{ fmtNum(ccsPreview.records_without_project) }} 条（项目视图显示为「未知」）。
                  <span v-if="ccsPreview.unsupported_apps.length > 0"
                    >不支持的应用：{{
                      ccsPreview.unsupported_apps.map(([a, n]) => `${a}×${n}`).join("、")
                    }}。</span
                  >
                  <span v-if="ccsPreview.rejected_reasons.length > 0">
                    拒绝原因：{{ ccsPreview.rejected_reasons.join("；") }}。</span
                  >
                </div>
              </div>
            </div>
            <div
              v-if="ccsPreview.would_conflict > 0 || ccsPreview.rollups_conflicting > 0"
              class="setting-row"
            >
              <div class="setting-main">
                <div class="setting-label">以来源快照为准</div>
                <div class="setting-help">
                  存在与已存值冲突的来源记录：默认保留 TokenScope
                  已保存的值（不静默覆盖较新数据）。勾选后本批次改用来源快照。
                </div>
              </div>
              <NSwitch v-model:value="ccsTakeSourceRollups" size="small" />
            </div>
            <div v-if="ccsPreview.overlap_unresolved" class="setting-row">
              <div class="setting-main">
                <div class="setting-label">重叠候选</div>
                <div class="setting-help">
                  有
                  {{ fmtNum(ccsPreview.would_overlap) }}
                  条来源记录与本地已有请求的时间与用量相同，但缺少可证实的请求身份；时间与用量相同只是线索，不能据此认定是同一次请求。默认不导入——确认它们是不同请求后再勾选。
                  <template v-if="ccsPreview.overlap_examples.length > 0">
                    <br />例：{{ ccsPreview.overlap_examples.join("；") }}
                  </template>
                </div>
              </div>
              <NSwitch v-model:value="ccsAllowOverlap" size="small" />
            </div>
            <div class="rebuild-row">
              <NButton size="small" type="primary" :loading="ccsBusy" @click="commitCcsImport"
                >导入</NButton
              >
              <NButton size="small" :disabled="ccsBusy" @click="discardCcsImport">取消</NButton>
              <span class="rebuild-hint"
                >预览基于当前历史快照；期间若有新的采集或导入，提交会被拒绝并要求重新预览。</span
              >
            </div>
          </div>

          <div v-if="ccsReport" class="setting-help">
            最近一次导入：新增 {{ fmtNum(ccsReport.requests_inserted) }}、更新
            {{ fmtNum(ccsReport.requests_updated) }}、已存在
            {{ fmtNum(ccsReport.requests_unchanged) }}、冲突
            {{ fmtNum(ccsReport.requests_conflicted) }}；日汇总落盘
            {{ fmtNum(ccsReport.rollups_snapshotted) }} 条；净新增 token
            {{ fmtNum(reportNetTokens) }}。
          </div>
        </section>
      </section>

      <section
        class="settings-group settings-panel"
        v-show="activeSection === 'pricing'"
        data-section="pricing"
      >
        <h2 class="group-title">价格</h2>
        <div v-if="pricingError" class="ts-notice ts-notice-inline block-error" role="alert">
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
          <span class="ts-notice-content"
            >价格列表读取失败{{ pricing ? "（保留上次数据）" : "" }}：{{ pricingError }}</span
          >
          <NButton size="tiny" :loading="pricingLoading" @click="loadPricing">重试</NButton>
        </div>
        <section class="ts-card settings-card">
          <div class="price-actions">
            <NButton size="small" type="primary" :loading="syncing" @click="syncPricing">
              同步在线价格
            </NButton>
            <NButton size="small" @click="openPricing">打开 / 创建外置价格文件</NButton>
          </div>
          <div class="setting-help">
            候选来源：外置（{{ pricing?.external_count ?? 0 }} 条）+ models.dev（{{
              pricing?.modelsdev_count ?? 0
            }}
            条，主源）+ OpenRouter（{{ pricing?.openrouter_count ?? 0 }} 条，补充源）；
            同一模型的候选按本次请求条件计价，未收录模型按未知价格处理（无内置兜底）。
            <HelpTooltip label="价格候选选择规则" content-label="价格候选选择规则">
              <template #trigger>候选选择规则</template>
              <div>
                有完整四桶价格的候选优先，在其中取总费用最高者；没有完整候选时才回退到部分候选中已知费用最高者（并列按来源优先级
                &gt; 完整匹配 &gt; 前缀更长打破），四桶单价取自同一候选、不跨候选拼价。
              </div>
              <div>
                本地快照是离线缓存：断网时继续按上次同步数据计价；同步时间与快照路径见「用量数据」分区的技术详情。
              </div>
            </HelpTooltip>
          </div>
          <!-- 任务 7：主源缺失/损坏与快照警告 = .ts-notice（不再用高饱和 NAlert） -->
          <div
            v-if="pricing && (pricing.modelsdev_count === 0 || !pricing.modelsdev_synced_at)"
            class="ts-notice ts-notice-inline price-notice"
            role="alert"
          >
            <svg
              class="ts-notice-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M12 3.5 21 19.5H3z" />
              <path d="M12 10v4" />
              <path d="M12 17h.01" />
            </svg>
            <span class="ts-notice-content">
              主源（models.dev）尚未就绪{{ pricing.modelsdev_synced_at ? "或数据为空" : "" }}：
              {{ pricing.modelsdev_path }}。点击上方「同步在线价格」获取定价； 外置价格表与
              OpenRouter 快照中已有的价格仍参与估算，未被任何来源覆盖的模型显示为未知。
            </span>
          </div>
          <div
            v-for="(w, i) in pricing?.warnings ?? []"
            :key="i"
            class="ts-notice ts-notice-inline price-notice"
          >
            <svg
              class="ts-notice-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M12 3.5 21 19.5H3z" />
              <path d="M12 10v4" />
              <path d="M12 17h.01" />
            </svg>
            <span class="ts-notice-content">{{ w }}</span>
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">自动同步价格（每 24h）</div>
            </div>
            <div class="setting-control">
              <NSwitch
                :value="autoSync === true"
                :disabled="autoSync == null || autoSyncBusy"
                :loading="autoSyncBusy"
                aria-label="自动同步价格"
                @update:value="setAutoSync"
              />
            </div>
          </div>
          <div class="setting-help">
            {{ pricing?.path ?? "—" }}（TOML；候选来源之一，同价并列时优先，保存后下次统计生效）
          </div>
          <!-- RC07：单价量纲显式说明（列头同样标注 $/M）。 -->
          <div class="setting-help unit-note">
            单价单位：{{ UNIT_PRICE_DENOMINATOR }}（每百万
            token）；四桶单价来自同一选中候选（不跨候选拼价）。
          </div>
          <!-- U05：价格表只在首次进入「价格」分区时挂载（进入前不发价格表请求、
               不挂载价格表组件）。 -->
          <ScrollList
            v-if="pricingVisited"
            :rows="pricing?.entries ?? []"
            label="价格列表"
            class="ts-auto-table-pricing"
          >
            <template #default="{ rows }">
              <NDataTable
                :columns="priceColumns"
                :data="rows"
                :row-key="rowKey"
                size="small"
                :bordered="false"
                table-layout="auto"
                class="ts-auto-table"
              />
            </template>
          </ScrollList>
        </section>
      </section>

      <section
        class="settings-group settings-panel token-color-settings"
        v-show="activeSection === 'appearance'"
        data-section="appearance"
      >
        <h2 class="group-title">外观</h2>
        <section class="ts-card settings-card">
          <div class="setting-help">
            用于趋势图、图例和用量比例条。修改即时生效并自动保存，自定义颜色在明暗模式下共用。
          </div>
          <div v-for="bucket in TOKEN_BUCKETS" :key="bucket.key" class="setting-row">
            <label :for="`token-color-${bucket.key}`" class="setting-label">{{
              bucket.label
            }}</label>
            <div class="setting-control token-color-control">
              <span class="ts-num">{{ tokenColors[bucket.key] }}</span>
              <input
                :id="`token-color-${bucket.key}`"
                type="color"
                :value="tokenColors[bucket.key]"
                :aria-label="`${bucket.label}显示颜色`"
                @input="setTokenColor(bucket.key, ($event.target as HTMLInputElement).value)"
              />
            </div>
          </div>
          <NButton size="small" @click="resetColors">恢复默认颜色</NButton>
          <div v-if="colorSaveError" class="setting-help" role="alert">
            颜色已应用，但本机偏好保存失败；请重新选择颜色后重试。
          </div>
        </section>
      </section>

      <!-- 关闭确认与配置文件计划 Task 4：高级 = 直接编辑设置文件 -->
      <section
        class="settings-group settings-panel"
        v-show="activeSection === 'advanced'"
        data-section="advanced"
      >
        <h2 class="group-title">高级</h2>
        <section class="ts-card settings-card">
          <div class="setting-help">
            设置配置文件：~/.tokenscope/settings.toml（TOML，带字段注释，可直接编辑）。
            手动保存后对下一次读取立即生效；GUI 内的修改会重写整个文件，
            自定义注释会丢失（字段说明以文件头为准）。
          </div>
          <NButton size="small" @click="openSettingsFile">打开设置配置文件</NButton>
        </section>
      </section>
    </div>
  </div>
</template>
<style scoped>
/* U05：分区布局——宽窗口左侧导航 + 右侧内容；≤1100px 改为顶部分段控件。 */
.settings-view {
  display: flex;
  align-items: flex-start;
  gap: var(--ts-space-5);
}
.settings-view.is-top-nav {
  flex-direction: column;
  align-items: stretch;
}

.settings-nav {
  flex: 0 0 168px;
  position: sticky;
  /* 吸顶导航栏高 52px（MainApp.vue）+ 16px 间距：滚动时导航不被顶栏盖住 */
  top: calc(52px + var(--ts-space-4));
  z-index: 1;
}
.settings-view.is-top-nav .settings-nav {
  flex: 0 0 auto;
  position: static;
}

/* 分区导航沿用 .ts-card 材质与分段控件的交互 token，不新增材质。 */
.settings-nav-rail {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--ts-space-2);
}
.settings-nav-item {
  display: block;
  width: 100%;
  padding: var(--ts-space-2) var(--ts-space-3);
  border: none;
  border-radius: var(--ts-radius-control);
  background: transparent;
  color: var(--ts-text-secondary);
  font: inherit;
  font-size: 13px;
  line-height: 1.4;
  text-align: left;
  cursor: pointer;
  transition:
    color var(--ts-motion) var(--ts-ease),
    background var(--ts-motion) var(--ts-ease);
}
.settings-nav-item:hover {
  background: var(--ts-row-hover);
  color: var(--ts-text);
}
.settings-nav-item.is-active {
  background: var(--ts-selected-bg);
  box-shadow:
    inset 0 0 0 1px var(--ts-selected-stroke),
    var(--ts-selected-glow);
  color: var(--ts-text);
  font-weight: 600;
}
.settings-nav-segmented {
  max-width: 100%;
}

.settings-panels {
  flex: 1 1 auto;
  min-width: 0;
  /* 首次读取尚未完成时保留稳定布局（各分区独立显示状态）。 */
  min-height: 380px;
}

.token-color-control {
  display: flex;
  align-items: center;
  gap: 12px;
}
.token-color-control input {
  width: 44px;
  height: 32px;
  padding: 2px;
  border: 1px solid var(--ts-separator);
  border-radius: 6px;
  background: var(--ts-fill);
  cursor: pointer;
}
.token-color-control input:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 2px;
}
/* 任务 7：macOS 系统设置式分组——组标题在卡片外（13px/600 次要色） */
.settings-group {
  margin-bottom: var(--ts-space-5);
}
.settings-group:last-child {
  margin-bottom: 0;
}
/* U05：同一分区里多张卡（「用量数据」= 历史统计 + CCS 导入）之间留出间距 */
.settings-panel .ts-card + .ts-card {
  margin-top: var(--ts-space-4);
}

.block-error {
  margin-bottom: var(--ts-space-2);
}

.group-title {
  font-size: 13px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ts-text-secondary);
  margin: 0 0 var(--ts-space-2);
}

/* 设置项行：左标签 + 右控件，行间发丝线 */
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

.setting-control {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
}

.setting-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--ts-text);
}

.setting-help {
  font-size: 12px;
  line-height: 1.5;
  color: var(--ts-text-muted);
  margin: var(--ts-space-1) 0 var(--ts-space-2);
}

/* U03：时区保存状态与一次性迁移提示 */
.tz-state {
  margin-left: var(--ts-space-1);
  color: var(--ts-text-muted);
}

.tz-state.is-error {
  color: var(--ts-error);
}

.tz-notice {
  margin: var(--ts-space-2) 0 0;
}

/* 数据源行块：块间发丝线 */
.source-block {
  padding: var(--ts-space-2) 0;
}
.source-block.has-divider {
  border-top: 1px solid var(--ts-separator);
}

/* U06：自动读取间隔行与来源块之间用发丝线分隔（沿用 .setting-row 的行样式） */
.interval-row {
  border-top: 1px solid var(--ts-separator);
}

.source-head {
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  margin-bottom: var(--ts-space-2);
}

.source-head strong {
  font-size: 14px;
  color: var(--ts-text);
}

/* UX08：来源目录字段名——稳定可访问名称，与真实 input 的 id/aria-label 关联 */
.dir-label {
  display: block;
  font-size: 12px;
  line-height: 1.4;
  color: var(--ts-text-secondary);
  margin-bottom: var(--ts-space-1);
}

/* UX08：当前生效目录——完整值可读、可选中复制、长路径换行（不省略号截断） */
.source-effective {
  display: flex;
  align-items: baseline;
  gap: var(--ts-space-1);
  margin-top: var(--ts-space-1);
  font-size: 12px;
  line-height: 1.5;
  user-select: text;
}
.effective-label {
  flex-shrink: 0;
  color: var(--ts-text-muted);
}
.effective-path {
  min-width: 0;
  color: var(--ts-text-secondary);
  white-space: normal;
  word-break: break-all;
}

/* UX08：重建缓存行——按钮 + 预期说明 */
.rebuild-row {
  display: flex;
  align-items: center;
  gap: var(--ts-space-3);
  margin-top: var(--ts-space-2);
}
.rebuild-hint {
  font-size: 12px;
  color: var(--ts-text-muted);
}

/* H05：来源库路径输入（可改选其它副本；改路径本身不读取库）。 */
.ccs-path {
  max-width: 420px;
  width: 100%;
}

.ccs-tz {
  min-width: 180px;
  max-width: 240px;
}

.source-state {
  font-size: 12px;
  color: var(--ts-text-muted);
}

.flex-fill {
  flex: 1;
}

.source-error {
  margin: var(--ts-space-2) 0;
}

.source-error .ts-notice-icon.is-error {
  color: var(--ts-error);
}

.price-actions {
  display: flex;
  gap: var(--ts-space-2);
  margin-bottom: var(--ts-space-2);
}

.price-notice {
  margin-bottom: var(--ts-space-2);
}

.price-notice .ts-notice-icon {
  color: var(--ts-warning);
}

.tech-collapse {
  margin: var(--ts-space-2) 0;
}

.tech-line {
  font-size: 12px;
  line-height: 1.8;
  color: var(--ts-text-secondary);
  font-family: var(--ts-font-mono);
  word-break: break-all;
}
</style>
