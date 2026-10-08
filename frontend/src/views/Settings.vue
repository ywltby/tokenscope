<script setup lang="ts">
import { computed, h, onMounted, onUnmounted, ref, watch, type Ref, type VNode } from "vue";
import { priceSourceLine } from "../lib/statsView";
import { invoke } from "@tauri-apps/api/core";
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NDataTable,
  NInput,
  NSelect,
  NSpin,
  NSwitch,
  NTag,
  useMessage,
  type DataTableColumn,
} from "naive-ui";
import {
  AGENT_LABEL,
  fmtNum,
  sourceIdOf,
  type CacheInfo,
  type PricingEntry,
  type PricingView,
  type SourceStatus,
} from "../types";
import { TZ_OPTIONS, useTimezone } from "../composables/timezone";
import { fmtPriceOrUnknown, formatTieredPricing } from "../lib/tieredPrice";
import {
  TOKEN_BUCKETS,
  UNIT_PRICE_DENOMINATOR,
  UNIT_PRICE_SUFFIX,
  tokenBucketLabel,
  type TokenBucketKey,
} from "../lib/tokenDisplay";
import HelpTooltip from "../components/HelpTooltip.vue";

const props = defineProps<{ refreshKey: number }>();
const msg = useMessage();

const sources = ref<SourceStatus[]>([]);
const cache = ref<CacheInfo | null>(null);
const pricing = ref<PricingView | null>(null);
const syncing = ref(false);
const loading = ref(false);
const rebuilding = ref(false);
const { tz } = useTimezone();
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
let sourceEditCounter = 0;
const sourceEditRev = new Map<string, number>();
let disposed = false;

function sourceEditRevOf(id: string): number {
  return sourceEditRev.get(id) ?? 0;
}

/** 标记来源行被用户编辑：推进该行编辑版本并置 dirty。 */
function markSourceEdited(id: string): void {
  sourceEditRev.set(id, ++sourceEditCounter);
  dirtySources.add(id);
}

async function setCloseAction(v: "ask" | "minimize" | "quit"): Promise<void> {
  if (!closeActionKnown.value) return; // 未知状态不得写入
  mutationEpoch++; // 写入开始 → 在途读取失去提交资格
  try {
    await invoke("settings_set_close_action", { action: v === "ask" ? null : v });
    closeAction.value = v;
    closeActionKnown.value = true;
    msg.success("关闭窗口默认动作已保存");
  } catch (e) {
    msg.error(String(e));
  }
}

async function openSettingsFile(): Promise<void> {
  try {
    const p = await invoke<string>("open_settings_file");
    msg.info(`已打开设置配置文件（可直接编辑，保存后对下一次读取生效）：${p}`);
  } catch (e) {
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
const settingsUnknown = computed(() => settingsError.value !== null && !settingsLoading.value);

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
      const fresh = await invoke<SourceStatus[]>("source_status");
      if (!isCurrent()) return;
      sources.value = fresh;
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
      const fresh = await invoke<CacheInfo>("cache_stats");
      if (!isCurrent()) return;
      cache.value = fresh;
      cacheError.value = null;
    } catch (e) {
      if (!isCurrent()) return;
      cacheError.value = e instanceof Error ? e.message : String(e);
    }
  });
}

async function loadPricing(): Promise<void> {
  await runBlockRead(pricingBlock, pricingLoading, async (isCurrent) => {
    try {
      const fresh = await invoke<PricingView>("pricing_entries");
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
  loading.value = true;
  try {
    // 各独立读取并发发起、各自处理失败——不用一个共享 loading/error
    // 覆盖全部结果。RC03：设置配置（来源草稿 + 自动同步 + 关闭动作）
    // 由**同一次** settings_get 初始化，不再分三个消费点各读一遍。
    await Promise.allSettled([loadSources(), loadCache(), loadPricing(), loadSettings()]);
  } finally {
    loading.value = false;
  }
}

// C1：来源配置草稿（编辑后按行保存）
const drafts = ref<Record<string, { enabled: boolean; dir: string }>>({});
// RC03：按**行**记录保存中状态——修复前用单个 savingSource，跨行并发保存
// 时后一行会覆盖前一行、前一行结束即提前解锁后一行。
const savingSources = ref<string[]>([]);
const isSourceSaving = (id: string): boolean => savingSources.value.includes(id);
const sourceErrors = ref<Record<string, string>>({});

/// 读取或补建该来源的草稿（设置缺键时按默认开启兜底）。
function ensureDraft(id: string): { enabled: boolean; dir: string } {
  const d = drafts.value[id];
  if (d) return d;
  const fresh = { enabled: true, dir: "" };
  drafts.value[id] = fresh;
  return fresh;
}

function setSourceEnabled(id: string, v: boolean): void {
  ensureDraft(id).enabled = v;
  markSourceEdited(id);
}

function setSourceDir(id: string, v: string): void {
  ensureDraft(id).dir = v;
  markSourceEdited(id);
}

/// UX08：来源目录输入框的稳定 DOM id——`<label for>` 与真实 `<input>` 的
/// `aria-label` 同源，保证可访问名称稳定（不依赖 placeholder 兜底）。
function dirInputId(agent: string): string {
  return `source-dir-${sourceIdOf(agent)}`;
}

/// UX08：来源目录字段的稳定可访问名称，如「Claude Code 日志目录」。
function dirInputLabel(agent: string): string {
  return `${AGENT_LABEL[agent] ?? agent} 日志目录`;
}

// UX06：草稿 dirty 集合——用户编辑过的行在重试/刷新时**不被覆盖**
//（retry_preserves_dirty_source_drafts）；保存成功后清除该行 dirty
//（RC03：仅当编辑版本与提交时一致）。
const dirtySources = new Set<string>();

/** RC03：设置配置读取结果（来源草稿 + 自动同步 + 关闭动作，同一次读取）。 */
type SettingsPayload = {
  close_action?: "minimize" | "quit" | null;
  price_auto_sync?: boolean;
  sources?: Record<string, { enabled?: boolean; dir?: string | null }>;
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
      s = await invoke<SettingsPayload>("settings_get");
    } catch (e) {
      if (readSeq !== settingsReadSeq || disposed) return;
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
    const src = s.sources ?? {};
    const fresh: typeof drafts.value = {
      claude: { enabled: src.claude?.enabled ?? true, dir: src.claude?.dir ?? "" },
      codex: { enabled: src.codex?.enabled ?? true, dir: src.codex?.dir ?? "" },
    };
    // 只更新未编辑的草稿行——dirty 行保留用户输入。
    drafts.value = {
      claude: dirtySources.has("claude") ? drafts.value.claude : fresh.claude,
      codex: dirtySources.has("codex") ? drafts.value.codex : fresh.codex,
    };
  });
}

async function saveSource(agent: string): Promise<void> {
  const id = sourceIdOf(agent);
  if (isSourceSaving(id)) return; // 同行防重复
  // RC03：写入开始即推进 mutationEpoch——在途的旧读取随即失去提交资格
  //（即使随后保存失败，也不得让旧读取覆盖用户草稿）。
  mutationEpoch++;
  const submittedRev = sourceEditRevOf(id);
  savingSources.value = [...savingSources.value, id];
  delete sourceErrors.value[id];
  try {
    const d = ensureDraft(id);
    await invoke("source_config_set", {
      agent: id,
      enabled: d.enabled,
      dir: d.dir.trim() === "" ? null : d.dir.trim(),
    });
    msg.success(`已保存 ${AGENT_LABEL[id] ?? id} 来源配置`);
    // RC03：只清除"提交时版本一致"的 dirty——保存期间的新编辑保留。
    if (sourceEditRevOf(id) === submittedRev) dirtySources.delete(id);
    // UX06：保存成功后状态刷新失败 ≠ 保存失败——区分提示，避免用户
    // 误以为需要重复保存；RC03：原始失败原因必须一并保留（不能只说
    // "可重试"却把为什么失败丢掉）。
    await loadSources();
    if (sourcesError.value) {
      const reason = sourcesError.value;
      sourcesError.value = `已保存，但状态刷新失败（${reason}，可重试）`;
    }
  } catch (e) {
    // Task 2：后端返回的重叠等配置错误必须可见，保留用户当前输入以便
    // 修改；错误只归属对应来源行（不再在 v-for 内跨行重复渲染）。
    sourceErrors.value[id] = e instanceof Error ? e.message : String(e);
  } finally {
    savingSources.value = savingSources.value.filter((x) => x !== id);
  }
}

async function rebuild(): Promise<void> {
  // UX08：函数级防重复——不只依赖按钮 loading 态（程序化触发/快速双击
  // 都不得并发发起第二次重建）。重建是派生数据操作，保留既有进度反馈，
  // 不增加强制确认。
  if (rebuilding.value) return;
  rebuilding.value = true;
  try {
    cache.value = await invoke<CacheInfo>("refresh_cache");
    // AP03：重建范围 = 当前生效的来源配置（用户停用的来源不参与），且只
    // 重建派生数据——不能声称配置外来源也被采集。
    msg.success("缓存已重建（按当前来源配置，仅重建派生数据）");
  } catch (e) {
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
    msg.success(reports.map((r) => `${r.source} ${r.count} 条`).join("，"));
  } catch (e) {
    // 部分失败也要刷新：主源已写盘的数据立即可见（审阅 Task 3）。
    msg.error(`同步失败：${e}`);
  } finally {
    // 成功与部分失败两条路径都重新读取价格视图 + 通知全局横幅刷新；
    // 列表读取失败由 loadPricing 归类为"价格列表读取失败"并保留旧数据
    //（RC04：不把读取失败伪装成同步失败，也不清空已显示的价格）。
    try {
      await loadPricing();
    } finally {
      if (!disposed) window.dispatchEvent(new Event("pricing-status-changed"));
      syncing.value = false;
    }
  }
}

async function openPricing(): Promise<void> {
  try {
    const p = await invoke<string>("open_pricing_file");
    msg.info(`已在系统默认编辑器打开（保存后下次统计生效）\n${p}`);
    // RC03：价格列表只经 loadPricing 一条路径读取（带读代次/卸载守卫）。
    await loadPricing();
  } catch (e) {
    msg.error(String(e));
  }
}

async function loadAutostart(): Promise<void> {
  await runBlockRead(autostartBlock, autostartLoading, async (isCurrent) => {
    try {
      const fresh = await invoke<boolean>("autostart_status");
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
    autostart.value = await invoke<boolean>("autostart_set", { enabled });
    msg.success(enabled ? "已开启开机自启" : "已关闭开机自启");
  } catch (e) {
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
    autoSync.value = await invoke<boolean>("settings_set_price_auto_sync", { enabled });
    msg.success(enabled ? "已开启自动同步（每 24h）" : "已关闭自动同步");
  } catch (e) {
    msg.error(String(e));
  } finally {
    autoSyncBusy.value = false;
  }
}

onMounted(() => {
  // RC03：设置配置只读一次（loadAll → loadSettings）。
  void loadAll();
  void loadAutostart();
});
watch(
  () => props.refreshKey,
  () => {
    void loadAll();
  },
);
onUnmounted(() => {
  disposed = true; // RC03：卸载后请求不得写状态或发通知
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
    minWidth: 180,
    ellipsis: { tooltip: true },
    render: (r) => asEntry(r).name ?? "",
  },
  {
    title: "模型前缀",
    key: "prefix",
    minWidth: 220,
    render: (r) => prefixCell(r),
  },
  // RC06：四桶单价列由显式 key 驱动，显示词来自 tokenDisplay 单一来源
  //（不再散落「缓存读$」）。RC07：列头明确量纲——单价是 USD / 1M token。
  {
    title: `${tokenBucketLabel("input")}（${UNIT_PRICE_SUFFIX}）`,
    key: "input",
    align: "right",
    render: (r) => priceCell(r, "input"),
  },
  {
    title: `${tokenBucketLabel("output")}（${UNIT_PRICE_SUFFIX}）`,
    key: "output",
    align: "right",
    render: (r) => priceCell(r, "output"),
  },
  {
    title: `${tokenBucketLabel("cache_write")}（${UNIT_PRICE_SUFFIX}）`,
    key: "cache_write",
    align: "right",
    render: (r) => priceCell(r, "cache_write"),
  },
  {
    title: `${tokenBucketLabel("cache_read")}（${UNIT_PRICE_SUFFIX}）`,
    key: "cache_read",
    align: "right",
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
  <NSpin :show="loading">
    <!-- 最小高度保证加载转圈居中于可视区 -->
    <div style="min-height: 380px">
      <!-- 任务 7：macOS 系统设置式分组——组标题在卡片外，每组一张 .ts-card -->
      <section class="settings-group">
        <h2 class="group-title">应用</h2>
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
              <div class="setting-help">存储/计算一律 UTC，仅展示按此时区一次转换。</div>
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
                @update:value="setCloseAction"
              />
            </div>
          </div>
        </section>
      </section>

      <section class="settings-group">
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
            v-for="(s, i) in sources"
            :key="s.agent"
            class="source-block"
            :class="{ 'has-divider': i > 0 }"
          >
            <div class="source-head">
              <strong>{{ AGENT_LABEL[s.agent] ?? s.agent }}</strong>
              <NSwitch
                :value="drafts[sourceIdOf(s.agent)]?.enabled ?? true"
                size="small"
                :aria-label="`${AGENT_LABEL[s.agent] ?? s.agent} 启用`"
                @update:value="(v: boolean) => setSourceEnabled(sourceIdOf(s.agent), v)"
              />
              <span class="source-state">
                {{
                  s.state === "ready"
                    ? `${fmtNum(s.files)} 个会话文件`
                    : s.state === "disabled"
                      ? "已停用"
                      : s.state === "missing"
                        ? "目录不存在"
                        : "无日志"
                }}
              </span>
              <span class="flex-fill"></span>
              <NButton
                size="tiny"
                :loading="isSourceSaving(sourceIdOf(s.agent))"
                :disabled="settingsUnknown"
                @click="saveSource(s.agent)"
              >
                保存
              </NButton>
            </div>
            <!-- UX08：稳定可访问名称（label 关联 + 真实 input 的 aria-label），
                 不再依赖 placeholder 兜底 -->
            <label class="dir-label" :for="dirInputId(s.agent)">{{ dirInputLabel(s.agent) }}</label>
            <NInput
              :value="drafts[sourceIdOf(s.agent)]?.dir ?? ''"
              size="small"
              placeholder="留空使用当前生效目录"
              :input-props="{
                id: dirInputId(s.agent),
                'aria-label': dirInputLabel(s.agent),
              }"
              @update:value="(v: string) => setSourceDir(sourceIdOf(s.agent), v)"
            />
            <!-- UX08：s.dir 是**当前生效目录**（可能是显式覆盖，不必然是默认
                 目录）；完整值可读、可选中复制、长路径换行不省略 -->
            <div class="source-effective">
              <span class="effective-label">当前生效目录：</span>
              <span class="effective-path ts-mono">{{ s.dir }}</span>
            </div>
            <div
              v-if="sourceErrors[sourceIdOf(s.agent)]"
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
              <span class="ts-notice-content">{{ sourceErrors[sourceIdOf(s.agent)] }}</span>
            </div>
            <div class="setting-help">
              留空则回到该来源的默认目录（上方「当前生效目录」是实际使用的目录，
              可能来自显式覆盖）；停用后该来源完全不参与统计。两个来源不能指向同一目录。
              保存后回到汇总页生效。
            </div>
          </div>
        </section>
      </section>

      <section class="settings-group">
        <h2 class="group-title">缓存</h2>
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
          <span class="ts-notice-content">缓存统计读取失败：{{ cacheError }}</span>
          <NButton size="tiny" :loading="cacheLoading" @click="loadCache">重试</NButton>
        </div>
        <section class="ts-card settings-card">
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">缓存文件</div>
            </div>
            <span class="setting-value ts-num">{{ cache ? fmtNum(cache.files) : "—" }}</span>
          </div>
          <div class="setting-row">
            <div class="setting-main">
              <div class="setting-label">缓存事件</div>
            </div>
            <span class="setting-value ts-num">{{ cache ? fmtNum(cache.events) : "—" }}</span>
          </div>
          <div class="setting-help">
            缓存是纯优化：任何故障都会自动退回全量扫描，统计数字不受影响。
          </div>
          <NCollapse class="tech-collapse">
            <NCollapseItem title="技术详情" name="tech">
              <!-- UX08：完整「当前生效目录」（含显式覆盖）——可选中复制、长路径换行 -->
              <div v-for="s in sources" :key="`eff-${s.agent}`" class="tech-line">
                {{ AGENT_LABEL[s.agent] ?? s.agent }} 当前生效目录：{{ s.dir }}
              </div>
              <div class="tech-line">缓存路径：{{ cache?.path ?? "—" }}</div>
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
            <NButton size="small" :loading="rebuilding" @click="rebuild">重建缓存</NButton>
            <!-- UX08：重建预期说明（重建是派生数据操作，保留进度与结果反馈） -->
            <span class="rebuild-hint">重新扫描日志，可能需要一段时间。</span>
          </div>
        </section>
      </section>

      <section class="settings-group">
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
            优先级：外置（{{ pricing?.external_count ?? 0 }} 条）> models.dev（{{
              pricing?.modelsdev_count ?? 0
            }}
            条，主源）> OpenRouter（{{ pricing?.openrouter_count ?? 0 }} 条，补充源）；
            层内最长前缀匹配，未收录模型按未知价格处理（无内置兜底）。本地快照是
            离线缓存：断网时继续按上次同步数据计价；同步时间与快照路径见"缓存"组的技术详情。
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
              {{ pricing.modelsdev_path }}。点击上方「同步在线价格」获取定价；
              当前未覆盖模型的费用将显示为未知。
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
            {{ pricing?.path }}（TOML；本地价格最高优先，保存后下次统计生效）
          </div>
          <!-- RC07：单价量纲显式说明（列头同样标注 $/M）。 -->
          <div class="setting-help unit-note">
            单价单位：{{ UNIT_PRICE_DENOMINATOR }}（每百万 token）；四桶单价按下方来源优先级解析。
          </div>
          <NDataTable
            :columns="priceColumns"
            :data="pricing?.entries ?? []"
            :row-key="rowKey"
            size="small"
            :bordered="false"
            :max-height="420"
            virtual-scroll
          />
        </section>
      </section>

      <!-- 关闭确认与配置文件计划 Task 4：高级配置 = 直接编辑设置文件 -->
      <section class="settings-group">
        <h2 class="group-title">高级配置</h2>
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
  </NSpin>
</template>
<style scoped>
/* 任务 7：macOS 系统设置式分组——组标题在卡片外（13px/600 次要色） */
.settings-group {
  margin-bottom: var(--ts-space-5);
}
.settings-group:last-child {
  margin-bottom: 0;
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

/* 数据源行块：块间发丝线 */
.source-block {
  padding: var(--ts-space-2) 0;
}
.source-block.has-divider {
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
