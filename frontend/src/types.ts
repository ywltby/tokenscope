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
  /** H06：未知费用的原因（`rollup_only` = 该组含只有日粒度的历史）。 */
  unknown_reason?: "rollup_only" | null;
  agents?: string[];
}

export interface SourceStat {
  agent: "claude-code" | "codex";
  stats: Record<string, number>;
}

/** H06：日汇总桶覆盖诊断（明细覆盖 / 汇总口径 / 未解决覆盖 / 时区提示）。 */
export interface RollupCoverage {
  detail_buckets: number;
  rollup_buckets: number;
  unresolved_buckets: number;
  unresolved_covered_tokens: TokenCounts;
  timezone_mismatch?: string | null;
}

export interface SummaryReport {
  /** SF04：所属查询会话 ID（同批次的明细分页必须携带同一 query_id）。 */
  query_id: string;
  /** SF04：冻结的价格修订号。 */
  pricing_revision: string;
  timezone: string;
  generated_at: string;
  sources: SourceStat[];
  by: string;
  groups: Group[];
  totals: Group;
  warnings: string[];
  /** H06：日汇总桶覆盖诊断（缺省 = 本次查询没有日粒度历史）。 */
  rollup_coverage?: RollupCoverage;
}

/** H05：CCS 导入的来源库默认路径与存在性（只读路径解析，不打开库）。 */
export interface CcsImportDefaults {
  path: string;
  exists: boolean;
  /** 日汇总的来源统计时区假设（CCS 用本机日生成日键）。 */
  timezone: string;
}

/** H05：导入预览（不写入任何用量）。 */
export interface CcsImportPreview {
  plan_id: string;
  logical_source: string;
  source_path: string;
  source_schema: string;
  source_day_timezone: string;
  history_generation: number;
  generated_at: string;
  expires_in_seconds: number;
  requests_total: number;
  requests_importable: number;
  requests_skipped_other_app: number;
  requests_skipped_duplicate_of_proxy: number;
  requests_rejected: number;
  rejected_reasons: string[];
  would_insert: number;
  would_update: number;
  would_unchanged: number;
  would_conflict: number;
  would_stale: number;
  /** H05：身份不足但库内有"同时间同用量"候选的记录数（只作核对线索）。 */
  would_overlap: number;
  /** H05：存在未解决的重叠候选——提交需要用户显式确认，否则后端拒绝。 */
  overlap_unresolved: boolean;
  /** 重叠候选样例（最多 5 条）。 */
  overlap_examples: string[];
  net_new_tokens: TokenCounts;
  rollups_total: number;
  rollups_new: number;
  rollups_unchanged: number;
  rollups_conflicting: number;
  unsupported_apps: [string, number][];
  records_without_project: number;
}

/** H05：导入提交结果（可核查）。 */
export interface CcsImportReport {
  run_id: number;
  logical_source: string;
  requests_inserted: number;
  requests_updated: number;
  requests_unchanged: number;
  requests_conflicted: number;
  requests_stale: number;
  /** 本批按"新增"导入但存在重叠候选的记录数（审计用）。 */
  requests_overlap: number;
  rollups_snapshotted: number;
  rollups_conflicted: number;
  net_new_tokens: TokenCounts;
  generation_after: number;
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
  /** SF10：available = 存在可解析费率路径的有效候选（明确 0 也算有价） */
  modelsdevAvailable: boolean;
  /** 原始条目数（技术诊断，与有效候选数区分"已读取"与"有效可用"） */
  modelsdevCount: number;
  /** SF10：主源有效候选数 */
  modelsdevValidCount?: number;
  modelsdevSyncedAt?: string | null;
  openrouterAvailable: boolean;
  /** SF10：补充源有效候选数 */
  openrouterValidCount?: number;
  externalCount: number;
  /** SF10：外置源有效候选数 */
  externalValidCount?: number;
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

/** SF07：三态单价线格式（与后端 RateSpec serde 对齐）——数字 = USD/百万
 * token；"same_as_input" = 沿用输入价（随分段/时间规则解析）；null = 未知。 */
export type RateSpecView = number | "same_as_input" | null;

export interface OpenRouterPrice {
  input: RateSpecView;
  output: RateSpecView;
  cache_write: RateSpecView;
  cache_read: RateSpecView;
  name?: string | null;
}

/** Task 8：价格计划视图（与后端 PricePlan 对齐）。 */
export interface PriceRatesView {
  input?: RateSpecView;
  output?: RateSpecView;
  cache_write?: RateSpecView;
  cache_read?: RateSpecView;
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
  /** SF07：四类基础单价三态（null = 未知、0 = 免费、"same_as_input" = 同输入价） */
  input: RateSpecView;
  output: RateSpecView;
  cache_write: RateSpecView;
  cache_read: RateSpecView;
  source: string;
  /** SF07（原 incomplete）：基础费率可解析性缺失——任一分项未知，或
   * SameAsInput 的输入价未知（依赖未定）。不等同"任意请求都会完整"。 */
  base_incomplete?: boolean;
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

/** 单价来源（缓存读取定价解析计划 Task 5）：fixed / same_as_input / unknown。 */
export type RateKind = "fixed" | "same_as_input" | "unknown";

export interface CostLine {
  kind: CostLineKind;
  tokens: number;
  /** USD / 百万 token；null = 该分项缺价（未计价，≠ 0） */
  unit_price: number | null;
  subtotal: number;
  priced: boolean;
  /** 单价来源；same_as_input 时 unit_price 已是解析后的实际数值 */
  rate_kind?: RateKind;
  /** F04：单价合法但 token × 单价超出 f64 表示范围——分项不计金额，
   *  token 计入未计价；展示为"金额超出可表示范围"，不得显示为免费 */
  overflow?: boolean;
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
  /** 候选中最高费用（保守估算）：highest_complete_cost / highest_partial_cost */
  reason: string;
  schedule_label: string | null;
  schedule_timezone: string | null;
  /** 历史事件时间（RFC3339） */
  request_at: string | null;
  /** 完整候选数（两阶段选择诊断） */
  complete_candidate_count?: number;
  /** 不完整候选数 */
  incomplete_candidate_count?: number;
  /** 被排除未参与主估算的不完整候选数 */
  incomplete_candidates_excluded?: number;
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
  /** 有不完整候选被排除时的提示（非本公式 unknown） */
  excluded_candidate_warning?: string | null;
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
  /** B02：会话初始工作目录（项目根归并口径的一部分）；null/缺省 = 无可信 cwd */
  session_initial_cwd?: string | null;
  /** B02：该请求作用域内最近观察到的结构化工作目录（保留子目录细节） */
  event_cwd?: string | null;
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
  cost_usd?: number | null;
  /** Task 6：请求级费用计算明细；null/缺省 = 未收录模型或旧后端 */
  cost_breakdown?: EventCostBreakdown | null;
}

export interface EventList {
  /** SF04：所属查询会话 ID（分页/追加必须同一会话）。 */
  query_id: string;
  /** SF04：冻结的价格修订号。 */
  pricing_revision: string;
  rows: EventRow[];
  total: number;
  warnings: string[];
}

export interface EventDrill {
  type: "day" | "model" | "project";
  /** 后端筛选键（模型维度 = 等价身份键，如 `claudeopus55`）。 */
  key: string;
  /** MP04：展示名（模型维度 = 友好名，项目维度 = 路径末段）；仅用于文案。 */
  label?: string;
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
