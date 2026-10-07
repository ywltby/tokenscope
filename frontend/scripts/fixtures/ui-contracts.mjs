// UX00：真实组件量测入口的合成 IPC fixture（无任何真实用户数据）。
//
// 每个 fixture 是一个纯函数 `ipc(cmd, args)`：
//   - 返回普通值 = 该命令的成功响应（脚本包成 Promise.resolve）；
//   - 返回 REJECT 标记 = 该命令以 reject 响应（用于错误恢复验收）；
//   - 返回 undefined = **未知命令**——脚本据此记为 IPC 泄漏并让验收失败。
//
// 协议对齐 SF04：query_begin 返回会话句柄，query_summary/query_events 只带
// query_id；summary/events 携带同一 query_id 与 pricing_revision。SF07 的三态
// 单价（number | "same_as_input" | null）在 fixture 中显式覆盖。
//
// 新增前端 command 时必须在此显式覆盖（否则 boot 检查会红），避免"未知
// command 静默返回 null"把协议漂移伪装成通过。

/** 命令需要 reject 时的显式标记（脚本翻译为 Promise.reject）。 */
export const REJECT = Symbol("ui-contracts.reject");

export function reject(message) {
  return { [REJECT]: true, message };
}

export function isReject(v) {
  return typeof v === "object" && v !== null && REJECT in v;
}

const TZ = "Asia/Shanghai";
const AS_OF = "2026-10-07T22:00:00+08:00";
const QUERY_ID = "q-synthetic-0001";
const PRICING_REVISION = "rev-synthetic-1";

/** 构造一个合成 Group。 */
function group(key, input, opts = {}) {
  const output = opts.output ?? Math.round(input / 2);
  const cache_write = opts.cache_write ?? 0;
  const cache_read = opts.cache_read ?? 0;
  return {
    key,
    label: opts.label ?? null,
    requests: opts.requests ?? 1,
    tokens: { input, output, cache_write, cache_read },
    cost_usd: opts.cost_usd ?? 0,
    unknown_pricing: opts.unknown_pricing ?? false,
    unknown_tokens: opts.unknown_tokens ?? { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    agents: opts.agents,
  };
}

function totals(groups) {
  const t = { input: 0, output: 0, cache_write: 0, cache_read: 0 };
  let requests = 0;
  let cost = 0;
  for (const g of groups) {
    requests += g.requests;
    t.input += g.tokens.input;
    t.output += g.tokens.output;
    t.cache_write += g.tokens.cache_write;
    t.cache_read += g.tokens.cache_read;
    cost += g.cost_usd;
  }
  return {
    key: "合计",
    requests,
    tokens: t,
    cost_usd: cost,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

function summary(realGroups, by) {
  const total = totals(realGroups);
  // 后端契约：groups 含一行 key="合计"（UsageTable 据此加粗、TrendChart 过滤）。
  const groups = realGroups.length > 0 ? [...realGroups, total] : [];
  return {
    query_id: QUERY_ID,
    pricing_revision: PRICING_REVISION,
    timezone: TZ,
    generated_at: AS_OF,
    sources: [
      { agent: "claude-code", stats: { files_scanned: 12, lines_seen: 800, events: realGroups.length } },
      { agent: "codex", stats: { files_scanned: 4, lines_seen: 200, events: 1 } },
    ],
    by,
    groups,
    totals: total,
    warnings: [],
  };
}

/** 一条事件行（含费用明细，供 UX01 费用浮层验收）。 */
function eventRow(i, opts = {}) {
  const ts = `2026-10-0${(i % 7) + 1} 1${i % 10}:0${i % 6}:00`;
  const model = opts.model ?? `claude-sonnet-4-5-20250929`;
  const project = opts.project ?? "/home/dev/projects/tokenscope";
  return {
    ts,
    record_id: `rid-${i}`,
    cursor: `${ts.replace(" ", "T")}.000Z|rid-${i}`,
    agent: opts.agent ?? "claude-code",
    model,
    session_id: `sess-${i}`,
    project,
    input: 1200 + i,
    output: 640 + i,
    cache_write: 0,
    cache_read: 0,
    cost_usd: opts.cost_usd === undefined ? 0.0126 : opts.cost_usd,
    cost_breakdown: opts.breakdown === null ? null : breakdown(),
  };
}

/** 请求级费用明细 DTO（与后端 EventCostBreakdown 对齐）。 */
function breakdown() {
  return {
    matched: {
      raw_key: "claude-sonnet-4-5-20250929",
      channel: null,
      source: "external",
      matched_key: "claude-sonnet-4-5-20250929",
      match_mode: "full",
      candidate_count: 1,
      reason: "highest_complete_cost",
      schedule_label: null,
      schedule_timezone: null,
      request_at: AS_OF,
      complete_candidate_count: 1,
      incomplete_candidate_count: 0,
      incomplete_candidates_excluded: 0,
    },
    basis: "prompt_tokens",
    basis_value: 1200,
    segment_label: null,
    lines: [
      { kind: "input", tokens: 1200, unit_price: 3, subtotal: 0.0036, priced: true, rate_kind: "fixed" },
      { kind: "output", tokens: 640, unit_price: 15, subtotal: 0.0096, priced: true, rate_kind: "fixed" },
      { kind: "cache_write", tokens: 0, unit_price: 0, subtotal: 0, priced: true, rate_kind: "fixed" },
      { kind: "cache_read", tokens: 0, unit_price: 0, subtotal: 0, priced: true, rate_kind: "fixed" },
    ],
    cost_usd: 0.0126,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    complete: true,
  };
}

function events(rows, total) {
  return { query_id: QUERY_ID, pricing_revision: PRICING_REVISION, rows, total: total ?? rows.length, warnings: [] };
}

const pricingStatusOk = {
  modelsdevAvailable: true,
  modelsdevCount: 42,
  modelsdevSyncedAt: AS_OF,
  openrouterAvailable: true,
  externalCount: 5,
  hasAnyPricing: true,
  needsSync: false,
  warnings: [],
};

const sourceStatusReady = [
  { agent: "claude-code", dir: "C:/Users/dev/.claude/projects", enabled: true, exists: true, files: 12, state: "ready" },
  { agent: "codex", dir: "C:/Users/dev/.codex/sessions", enabled: true, exists: true, files: 4, state: "ready" },
];

// ── 各 fixture 的数据 ──────────────────────────────────────────────
const NORMAL_GROUPS = [
  group("2026-10-05", 120000, { output: 60000, cache_write: 4000, cache_read: 30000, cost_usd: 0.42, requests: 12 }),
  group("2026-10-06", 90000, { output: 45000, cache_write: 2000, cache_read: 21000, cost_usd: 0.31, requests: 9 }),
  group("2026-10-07", 140000, { output: 70000, cache_write: 5000, cache_read: 36000, cost_usd: 0.51, requests: 15 }),
];

const UNKNOWN_GROUPS = [
  group("gpt-5-codex", 80000, {
    output: 40000,
    unknown_pricing: true,
    cost_usd: 0,
    unknown_tokens: { input: 80000, output: 40000, cache_write: 0, cache_read: 0 },
  }),
  group("claude-opus-4-1", 20000, { output: 10000, cost_usd: 0.2 }),
];

const PARTIAL_GROUPS = [
  group("claude-sonnet-4-5", 60000, { output: 30000, cost_usd: 0.18 }),
  group("mystery-model", 30000, {
    output: 10000,
    unknown_pricing: true,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 9000 },
  }),
];

const LONG_GROUPS = [
  group(
    "/Users/dev/very/deeply/nested/project/directory/name/that/is/extremely/long/tokenscope-monorepo",
    100000,
    { label: "tokenscope-monorepo", output: 50000, cost_usd: 0.35 },
  ),
  group("claude-sonnet-4-5-20250929-with-a-very-long-model-suffix-v2", 60000, {
    output: 30000,
    cost_usd: 0.2,
  }),
];

const MULTI_GROUPS = Array.from({ length: 30 }, (_, i) =>
  group(`model-${String(i).padStart(2, "0")}`, 40000 - i * 500, { output: 20000 - i * 200, cost_usd: 0.1 }),
);

function dimGroups(by, base) {
  // 让 fixture 在任意聚合维度下都有可渲染的数据（维度切换验收用）。
  if (by === "model") {
    return base === "multi" ? MULTI_GROUPS : NORMAL_GROUPS.map((g, i) => ({ ...g, key: `model-${i}` }));
  }
  if (by === "project") return NORMAL_GROUPS.map((g, i) => ({ ...g, key: `/proj/p${i}` }));
  if (by === "agent")
    return [
      group("claude-code", 200000, { output: 100000, cost_usd: 0.8 }),
      group("codex", 80000, { output: 40000, cost_usd: 0.3 }),
    ];
  return NORMAL_GROUPS;
}

const EVENT_ROWS = [
  eventRow(1, { cost_usd: 0.0126 }),
  eventRow(2, { cost_usd: 0.008 }),
  eventRow(3, { cost_usd: null, breakdown: null, model: "mystery-model" }),
  eventRow(4, { agent: "codex", model: "gpt-5-codex", cost_usd: 0.004 }),
];

/**
 * 构造指定名称的 fixture。
 * @param {string} name
 * @returns {{ name: string, meta: object, ipc: (cmd: string, args: object) => unknown }}
 */
export function buildFixture(name) {
  const base = {
    name,
    meta: { theme: null, viewport: null },
    ipc: () => undefined,
  };

  const common = (cmd) => {
    switch (cmd) {
      case "startup_diagnostics":
        return { state: "ok", dir: "C:/Users/dev/AppData/Roaming/tokenscope/logs", message: null };
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "pricing_status":
        return pricingStatusOk;
      case "source_status":
        return sourceStatusReady;
      case "view_cache_load":
        return null;
      case "view_cache_save":
        return null;
      default:
        return undefined;
    }
  };

  const withQuery = (groupsFor) => (cmd, args) => {
    const c = common(cmd);
    if (c !== undefined) return c;
    if (cmd === "query_begin") {
      return {
        queryId: QUERY_ID,
        generation: 1,
        pricingRevision: PRICING_REVISION,
        timezone: args?.tz ?? TZ,
        asOf: AS_OF,
      };
    }
    if (cmd === "query_summary") return summary(groupsFor(args), args?.by ?? "day");
    if (cmd === "query_events") return events(EVENT_ROWS);
    return undefined;
  };

  switch (name) {
    case "normal":
      return { ...base, ipc: withQuery(() => NORMAL_GROUPS) };
    case "empty":
      return { ...base, ipc: withQuery(() => []) };
    case "unknown-price":
      return { ...base, ipc: withQuery(() => UNKNOWN_GROUPS) };
    case "partial-price":
      return { ...base, ipc: withQuery(() => PARTIAL_GROUPS) };
    case "long-text":
      return { ...base, ipc: withQuery(() => LONG_GROUPS) };
    case "multi-category":
      return { ...base, ipc: withQuery(() => MULTI_GROUPS) };
    case "settings-read-failure": {
      return {
        ...base,
        meta: { ...base.meta, settingsFailure: true },
        ipc: (cmd) => {
          if (cmd === "settings_get") return reject("设置文件损坏（合成）");
          if (cmd === "cache_stats") return { path: "C:/cache", files: 3, events: 120 };
          if (cmd === "pricing_entries") return pricingView();
          if (cmd === "autostart_status") return false;
          const c = common(cmd);
          if (c !== undefined) return c;
          if (cmd === "query_begin")
            return {
              queryId: QUERY_ID,
              generation: 1,
              pricingRevision: PRICING_REVISION,
              timezone: TZ,
              asOf: AS_OF,
            };
          if (cmd === "query_summary") return summary(NORMAL_GROUPS, "day");
          if (cmd === "query_events") return events(EVENT_ROWS);
          return undefined;
        },
      };
    }
    default:
      throw new Error(`未知 fixture：${name}`);
  }
}

/** 设置页价格表视图（SF07 三态：number | "same_as_input" | null）。 */
function pricingView() {
  return {
    path: "C:/Users/dev/AppData/Roaming/tokenscope/pricing.toml",
    modelsdev_path: "C:/Users/dev/AppData/Roaming/tokenscope/models.dev.json",
    modelsdev_synced_at: AS_OF,
    modelsdev_count: 42,
    openrouter_path: "C:/Users/dev/AppData/Roaming/tokenscope/openrouter.json",
    openrouter_synced_at: AS_OF,
    openrouter_count: 18,
    external_count: 5,
    entries: [
      {
        prefix: "claude-sonnet-4-5",
        name: "Claude Sonnet 4.5",
        channel: null,
        input: 3,
        output: 15,
        cache_write: "same_as_input",
        cache_read: 0.3,
        source: "external",
        base_incomplete: false,
        basis: "prompt_tokens",
        has_tiered_pricing: false,
      },
      {
        prefix: "mystery-model",
        name: null,
        channel: null,
        input: null,
        output: null,
        cache_write: null,
        cache_read: null,
        source: "models.dev",
        base_incomplete: true,
        basis: null,
        has_tiered_pricing: false,
      },
    ],
    warnings: [],
  };
}

export const FIXTURE_NAMES = [
  "normal",
  "empty",
  "unknown-price",
  "partial-price",
  "long-text",
  "multi-category",
  "settings-read-failure",
];
