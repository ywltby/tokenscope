/// 与 Rust 侧 SummaryReport / Group / CollectStats 逐字段对应的类型。
/// 注意：Codex 专属计数器为 0 时 Rust 端会省略字段，读取用 `?? 0`。

export interface TokenCounts {
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
}

export interface Group {
  key: string;
  /** C2：展示名（项目维度 = 路径末段）；缺省展示 key */
  label?: string | null;
  requests: number;
  tokens: TokenCounts;
  cost_usd: number;
  unknown_pricing: boolean;
  unknown_tokens: TokenCounts;
  agents?: string[];
}

export interface SourceStat {
  agent: "claude-code" | "codex";
  stats: Record<string, number>;
}

export interface SummaryReport {
  timezone: string;
  generated_at: string;
  sources: SourceStat[];
  by: string;
  groups: Group[];
  totals: Group;
  warnings: string[];
}

export interface SourceStatus {
  agent: string;
  dir: string;
  enabled: boolean;
  exists: boolean;
  files: number;
  /** C1 四态：disabled / missing / empty / ready */
  state: string;
}

/** Task 2：定价可用性状态（结构化 DTO，横幅据 needs_sync 渲染） */
export interface PricingStatus {
  modelsdevAvailable: boolean;
  modelsdevCount: number;
  modelsdevSyncedAt?: string | null;
  openrouterAvailable: boolean;
  externalCount: number;
  hasAnyPricing: boolean;
  needsSync: boolean;
  warnings: string[];
}

/** C1：单一来源配置（enabled + 显式目录覆盖） */
export interface SourceConfig {
  enabled: boolean;
  dir?: string | null;
}

export interface CacheInfo {
  path: string;
  files: number;
  events: number;
}

export interface OpenRouterPrice {
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
  name?: string | null;
}

/** Task 8：价格计划视图（与后端 PricePlan 对齐）。 */
export interface PriceRatesView {
  input?: number | null;
  output?: number | null;
  cache_write?: number | null;
  cache_read?: number | null;
}

export interface PriceSegmentView {
  label?: string | null;
  min_tokens: number;
  /** null = 无上限 */
  max_tokens?: number | null;
  prices: PriceRatesView;
}

export interface SchedulePeriodView {
  start_time: string;
  end_time: string;
  /** 规范化三字母（mon/tue/...）；null = 每天 */
  weekdays?: string[] | null;
  prices: PriceRatesView;
}

export interface PriceScheduleView {
  label?: string | null;
  /** IANA 时区；null = UTC */
  timezone?: string | null;
  periods: SchedulePeriodView[];
  /** 规则级价格（无 period 命中时的基线覆盖） */
  prices?: PriceRatesView;
}

export interface PricingEntry {
  prefix: string;
  name?: string | null;
  /** 渠道（原始键第一个 / 之前）；null = 无 vendor 前缀 */
  channel?: string | null;
  /** 四类基础单价：null = 未知（显示"未知"），0 = 免费 */
  input: number | null;
  output: number | null;
  cache_write: number | null;
  cache_read: number | null;
  source: string;
  /** B3：任一分项价格未知（设置页显示"未知"并标记不完整） */
  incomplete?: boolean;
  /** 计价依据（"prompt_tokens" 等）；null = 未声明 */
  basis?: string | null;
  segments?: PriceSegmentView[];
  schedules?: PriceScheduleView[];
  /** 有分段或峰谷规则（设置页据此渲染档位展开） */
  has_tiered_pricing?: boolean;
  /** 同前缀 OpenRouter 条目价格；null = OpenRouter 无对应模型 */
  openrouter?: OpenRouterPrice | null;
}

export interface PricingView {
  path: string;
  modelsdev_path: string;
  modelsdev_synced_at?: string | null;
  modelsdev_count: number;
  openrouter_path: string;
  openrouter_synced_at?: string | null;
  openrouter_count: number;
  external_count: number;
  entries: PricingEntry[];
  warnings: string[];
}

/** Task 5/6：请求级费用明细 DTO（与后端 EventCostBreakdown 对齐）。 */
export type CostLineKind = "input" | "output" | "cache_write" | "cache_read";

export interface CostLine {
  kind: CostLineKind;
  tokens: number;
  /** USD / 百万 token；null = 该分项缺价（未计价，≠ 0） */
  unit_price: number | null;
  subtotal: number;
  priced: boolean;
}

export type MatchMode = "full" | "full_variant_fallback" | "prefix" | "prefix_variant_fallback";

export interface MatchedCandidate {
  /** 原始完整模型键（来源侧写法） */
  raw_key: string;
  channel: string | null;
  /** external / models.dev / openrouter */
  source: string;
  matched_key: string;
  match_mode: MatchMode;
  candidate_count: number;
  /** 候选中最高费用（保守估算） */
  reason: string;
  schedule_label: string | null;
  schedule_timezone: string | null;
  /** 历史事件时间（RFC3339） */
  request_at: string | null;
}

export interface EventCostBreakdown {
  matched: MatchedCandidate;
  /** "prompt_tokens" 等 */
  basis: string | null;
  basis_value: number;
  /** 命中分段标签；null = 基础价档 */
  segment_label: string | null;
  lines: CostLine[];
  cost_usd: number;
  unknown: {
    input: number;
    output: number;
    cache_write: number;
    cache_read: number;
  };
  complete: boolean;
}

export interface EventRow {
  ts: string;
  /** D1 游标第二分量（Claude = message.id；Codex 为空） */
  record_id: string;
  /** Task 2：不透明游标（完整精度 UTC 时间戳 + record_id），翻页原样回传 */
  cursor: string;
  agent: string;
  model: string;
  session_id: string;
  project: string;
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
  cost_usd?: number | null;
  /** Task 6：请求级费用计算明细；null/缺省 = 未收录模型或旧后端 */
  cost_breakdown?: EventCostBreakdown | null;
}

export interface EventList {
  rows: EventRow[];
  total: number;
  warnings: string[];
}

export interface EventDrill {
  type: "day" | "model" | "project";
  key: string;
}

export type Dim = "day" | "model" | "project" | "agent";
export type AgentFilter = "all" | "claude" | "codex";

/// 关闭窗口默认动作（settings.toml close_action；缺省 null = 每次询问）。
export type CloseAction = "minimize" | "quit";

/**
 * 来源 ID 归一化（Task 8 修复）：source_status 序列化的 agent 值
 * （"claude-code"）→ 来源 ID（"claude"，与 settings_get /
 * source_config_set 的后端契约一致）。不归一化则 Claude 行草稿键错配。
 */
export function sourceIdOf(agent: string): string {
  return agent === "claude-code" ? "claude" : agent;
}

export const AGENT_LABEL: Record<string, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  claude: "Claude Code",
  all: "全部",
};

export const STAT_KEYS: { key: string; label: string }[] = [
  { key: "files_scanned", label: "文件" },
  { key: "lines_seen", label: "行" },
  { key: "events", label: "事件" },
  { key: "duplicates_dropped", label: "去重丢弃" },
  { key: "bad_lines", label: "坏行" },
  { key: "skipped_sidechain", label: "跳过 sidechain" },
  { key: "skipped_synthetic", label: "跳过 synthetic" },
  { key: "skipped_zero_usage", label: "跳过零分量" },
  { key: "skipped_no_model", label: "跳过无模型" },
  { key: "ignored_token_usage_record", label: "忽略 usage_record" },
  { key: "io_errors", label: "读取失败" },
];

export function fmtNum(n: number): string {
  return n.toLocaleString("en-US");
}

export function fmtCost(v: number): string {
  if (v === 0) return "0.00";
  if (v < 0.01) return v.toFixed(6);
  return v.toFixed(2);
}

/// C2：明细/表格的项目展示名 = 路径末段（完整身份见原值）。
/// 正反斜杠都容忍；无分隔符（slug、"(根目录)"）原样返回。
export function projectLabel(path: string): string {
  const segs = path.split(/[\\/]/).filter((s) => s.length > 0);
  return segs.length > 0 ? segs[segs.length - 1] : path;
}
