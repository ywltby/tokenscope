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

export interface PricingEntry {
  prefix: string;
  name?: string | null;
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
  source: string;
  /** B3：任一分项价格未知（设置页按 0 展示但标记不完整；完整解释 UI 属 C5） */
  incomplete?: boolean;
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
