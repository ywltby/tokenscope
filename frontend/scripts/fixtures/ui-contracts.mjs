// UX00/RC09：真实组件量测入口的合成 IPC fixture（无任何真实用户数据）。
//
// 每个 fixture 是一个纯函数 `ipc(cmd, args)`：
//   - 返回普通值 = 该命令的成功响应（脚本包成 Promise.resolve）；
//   - 返回 REJECT 标记 = 该命令以 reject 响应（用于错误恢复验收）；
//   - 返回 DEFER 标记 = 该命令**挂起**，直到测试调用 `settleDeferred` 才响应
//    （RC09：写后旧读取回退、分页在途过期等竞态必须在真实浏览器里重演）；
//   - 返回 undefined = **未知命令**——脚本据此记为 IPC 泄漏并让验收失败。
//
// 协议对齐 SF04：query_begin 返回会话句柄，query_summary/query_events 只带
// query_id；因此 fixture 必须维护**真正的会话表**（多个 query_id、各自冻结
// 参数与游标位置），不能用一个常量 ID + 单个 sessionBy 冒充并发会话。
// 会话过期/游标错会话按后端同一口径返回结构化 `query_expired` 拒绝。
//
// SF07 的三态单价（number | "same_as_input" | null）在 fixture 中显式覆盖；
// empty / unknown-price / partial-price 同时给出**一致的汇总与明细 DTO**，
// 不允许"只有分组变化、明细固定正常"。
//
// 新增前端 command 时必须在此显式覆盖（否则脚本记为未知命令并失败）。

/** 命令需要 reject 时的显式标记（脚本翻译为 Promise.reject）。 */
export const REJECT = Symbol("ui-contracts.reject");
/** 命令需要挂起时的显式标记（脚本翻译为直到 settle 才结束的 Promise）。 */
export const DEFER = Symbol("ui-contracts.defer");

export function reject(message) {
  return { [REJECT]: true, message };
}

/** P06：未同意基线的政策 DTO（与 src-tauri 下发的 PrivacySnapshot.policy 同形）。 */
export const PRIVACY_POLICY_DTO = {
  title: "TokenScope 隐私政策",
  date: "2026 年 10 月 10 日",
  markdown: [
    "# TokenScope 隐私政策",
    "",
    "更新日期：2026 年 10 月 10 日",
    "",
    "同意前不读取使用数据。",
    "",
    "## 一、适用范围",
    "",
    "本政策适用于 TokenScope 自身的日志读取、统计、缓存、设置和价格同步功能。",
    "",
    "## 二、读取的数据及用途",
    "",
    "读取已启用来源中的会话日志并提取统计字段。",
    "",
    "## 三、本地保存位置",
    "",
    "应用自身的数据保存在用户主目录的 .tokenscope 目录。",
    "",
    "## 四、网络请求与数据传输",
    "",
    "价格同步访问 models.dev 与 OpenRouter 的公开目录。",
    "",
    "## 五、第三方服务与运行时",
    "",
    "WebView2 受 Microsoft 的产品设置与政策约束。",
    "",
    "## 六、用户可以如何控制数据",
    "",
    "可以在设置中关闭某个日志来源或关闭自动价格同步。",
    "",
    "## 七、保留、删除与卸载",
    "",
    "删除本地数据需要清理数据目录。",
    "",
    "## 八、反馈与联系方式",
    "",
    "可通过项目 Issues 联系维护者。",
    "",
    "## 九、政策更新",
    "",
    "数据处理方式变化时更新本文件的日期与内容。",
  ].join("\n"),
};

/**
 * P06：受隐私闸门保护的业务命令（与 `src-tauri/src/privacy.rs` 的
 * PROTECTED_COMMANDS 对齐）。未同意的 fixture 对这些命令一律返回
 * `privacy_consent_required`，与真实后端同形——否则既测不出泄漏，
 * 也测不出前端是否提前发业务命令。
 */
export const PRIVACY_PROTECTED_COMMANDS = [
  "summarize",
  "list_events",
  "query_begin",
  "query_summary",
  "query_events",
  "startup_diagnostics",
  "view_cache_load",
  "view_cache_save",
  "source_status",
  "source_config_set",
  "source_paths_list",
  "source_path_add",
  "source_path_remove",
  "source_path_set_enabled",
  "source_auto_set",
  "cache_stats",
  "refresh_cache",
  "pricing_entries",
  "pricing_status",
  "open_pricing_file",
  "sync_pricing_openrouter",
  "autostart_status",
  "autostart_set",
  "settings_get",
  "settings_set_price_auto_sync",
  "settings_set_close_action",
  // U06：自动读取间隔（settings.toml 单一来源）同样受隐私闸门保护。
  "settings_set_collect_interval",
  // U03：统计时区（settings.toml 单一来源）同样受隐私闸门保护。
  "settings_set_timezone",
  "close_resolve",
  "open_settings_file",
];

/** 引导状态快照（`ready` = 已有有效同意记录）。 */
export function privacySnapshot(phase) {
  return {
    phase,
    detail: null,
    exitPromptPending: false,
    diagnostics: [],
    policy: PRIVACY_POLICY_DTO,
  };
}

export function isReject(v) {
  return typeof v === "object" && v !== null && REJECT in v;
}

/** 挂起某个命令的响应，直到测试显式 settle/fail（key 用于定位那一次调用）。 */
export function defer(key) {
  return { [DEFER]: key };
}

export function isDeferred(v) {
  return typeof v === "object" && v !== null && DEFER in v;
}

const TZ = "Asia/Shanghai";
const AS_OF = "2026-10-07T22:00:00+08:00";
/** U01：同一冻结基准的统计时区自然日与本地分钟（`range` 回传用）。 */
const AS_OF_DAY = AS_OF.slice(0, 10);
const AS_OF_LOCAL_MINUTE = `${AS_OF_DAY}T${AS_OF.slice(11, 16)}`;
/** 纯日历减法（与前端 `addDays` 同语义，不受本机时区影响）。 */
function dayMinus(day, n) {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d) - n * 86400e3).toISOString().slice(0, 10);
}
/** 滚动 24 小时的起点（本地时刻与 UTC），固定值——只用于形态断言。 */
const ROLLING_START_LOCAL = `${dayMinus(AS_OF_DAY, 1)}T${AS_OF.slice(11, 16)}`;
const ROLLING_START_UTC = `${dayMinus(AS_OF_DAY, 1)}T14:00:00Z`;
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
  let unknown = false;
  const ut = { input: 0, output: 0, cache_write: 0, cache_read: 0 };
  for (const g of groups) {
    requests += g.requests;
    t.input += g.tokens.input;
    t.output += g.tokens.output;
    t.cache_write += g.tokens.cache_write;
    t.cache_read += g.tokens.cache_read;
    cost += g.cost_usd;
    unknown = unknown || g.unknown_pricing;
    for (const k of ["input", "output", "cache_write", "cache_read"]) {
      ut[k] += g.unknown_tokens?.[k] ?? 0;
    }
  }
  return {
    key: "合计",
    requests,
    tokens: t,
    cost_usd: cost,
    unknown_pricing: unknown,
    unknown_tokens: ut,
  };
}

/** 一条事件行（含费用明细，供费用浮层与明细一致性验收）。 */
function eventRow(i, opts = {}) {
  const ts = `2026-10-0${(i % 7) + 1} 1${i % 10}:0${i % 6}:00`;
  const model = opts.model ?? "claude-sonnet-4-5-20250929";
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
    cost_breakdown: opts.breakdown === null ? null : breakdown(opts.cost_usd ?? 0.0126),
  };
}

/** 请求级费用明细 DTO（与后端 EventCostBreakdown 对齐）。 */
function breakdown(costUsd) {
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
      {
        kind: "input",
        tokens: 1200,
        unit_price: 3,
        subtotal: 0.0036,
        priced: true,
        rate_kind: "fixed",
      },
      {
        kind: "output",
        tokens: 640,
        unit_price: 15,
        subtotal: 0.0096,
        priced: true,
        rate_kind: "fixed",
      },
      {
        kind: "cache_write",
        tokens: 0,
        unit_price: 0,
        subtotal: 0,
        priced: true,
        rate_kind: "fixed",
      },
      {
        kind: "cache_read",
        tokens: 0,
        unit_price: 0,
        subtotal: 0,
        priced: true,
        rate_kind: "fixed",
      },
    ],
    cost_usd: costUsd,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    complete: true,
  };
}

/** 部分计价明细：缓存命中无单价（不得把缺价显示成 $0）。 */
function partialBreakdown(costUsd) {
  const bd = breakdown(costUsd);
  bd.complete = false;
  bd.lines[3] = {
    kind: "cache_read",
    tokens: 9000,
    unit_price: null,
    subtotal: 0,
    priced: false,
    rate_kind: "unknown",
  };
  bd.unknown = { input: 0, output: 0, cache_write: 0, cache_read: 9000 };
  return bd;
}

const pricingStatusOk = {
  modelsdevAvailable: true,
  modelsdevCount: 42,
  modelsdevValidCount: 42,
  modelsdevSyncedAt: AS_OF,
  openrouterAvailable: true,
  openrouterValidCount: 8,
  externalCount: 5,
  externalValidCount: 5,
  hasAnyPricing: true,
  needsSync: false,
  warnings: [],
};

/** 主源缺失且**完全无价**：要清空所有可用价格计数（横幅才可以宣称"仅能显示为未知"）。 */
const pricingStatusNeedsSync = {
  ...pricingStatusOk,
  modelsdevAvailable: false,
  modelsdevCount: 0,
  modelsdevValidCount: 0,
  openrouterAvailable: false,
  openrouterValidCount: 0,
  externalCount: 0,
  externalValidCount: 0,
  hasAnyPricing: false,
  needsSync: true,
};

/**
 * AP05：主源缺失但**仍有可用价格**——needsSync=true 只说明主源没有有效候选，
 * 不等于"没有任何价格"。两种子态用来区分横幅不能只按 needsSync 下结论：
 * 只有外置价格表 / 只有 OpenRouter。
 */
const pricingStatusNeedsSyncExternalOnly = {
  ...pricingStatusNeedsSync,
  externalCount: 5,
  externalValidCount: 5,
  hasAnyPricing: true,
};

const pricingStatusNeedsSyncOpenrouterOnly = {
  ...pricingStatusNeedsSync,
  openrouterAvailable: true,
  openrouterValidCount: 8,
  hasAnyPricing: true,
};

const sourceStatusReady = [
  {
    agent: "claude-code",
    dir: "C:/Users/dev/.claude/projects",
    enabled: true,
    exists: true,
    files: 12,
    state: "ready",
  },
  {
    agent: "codex",
    dir: "C:/Users/dev/.codex/sessions",
    enabled: true,
    exists: true,
    files: 4,
    state: "ready",
  },
];

/** AP06：目录尚未修好时的来源状态（触发"数据目录不存在"通知）。 */
const sourceStatusMissing = [
  {
    agent: "claude-code",
    dir: "C:/Users/dev/.claude/projects",
    enabled: true,
    exists: false,
    files: 0,
    state: "missing",
  },
  {
    agent: "codex",
    dir: "C:/Users/dev/.codex/sessions",
    enabled: true,
    exists: true,
    files: 4,
    state: "ready",
  },
];

/** UX08：超长「当前生效目录」——用于真实浏览器换行/不横向溢出验收。 */
export const LONG_SOURCE_DIR =
  "C:/Users/very.long.user.name/AppData/Roaming/deeply/nested/agent/workspaces/tokenscope-monorepo/packages/frontend/.claude/projects";
/** PR3 §5：source_paths_list 的行（默认 + 手动；后端已解析身份/标签）。 */
export const SOURCE_ROWS_DEFAULT = [
  {
    id: "c:/users/dev/.claude/projects",
    agent: "claude",
    path: "C:/Users/dev/.claude/projects",
    canonical_identity: "c:/users/dev/.claude/projects",
    identity_confirmed: true,
    origins: ["默认自动"],
    mapping_path: "C:/Users/dev/.claude.json",
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: [],
  },
  {
    id: "c:/users/dev/.codex/sessions",
    agent: "codex",
    path: "C:/Users/dev/.codex/sessions",
    canonical_identity: "c:/users/dev/.codex/sessions",
    identity_confirmed: true,
    origins: ["默认自动"],
    mapping_path: null,
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: [],
  },
];

/** PR3：合并行（默认自动 + 手动同根）与超长路径行——标签合并与换行验收。 */
export const SOURCE_ROWS_MERGED_AND_LONG = [
  {
    id: "c:/users/dev/.claude/projects",
    agent: "claude",
    path: "C:/Users/dev/.claude/projects",
    canonical_identity: "c:/users/dev/.claude/projects",
    identity_confirmed: true,
    origins: ["默认自动", "手动"],
    mapping_path: "C:/Users/dev/.claude.json",
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: ["同一物理根由多个候选指到：项目映射采用已核实的默认绑定"],
  },
  {
    id: LONG_SOURCE_DIR.toLowerCase(),
    agent: "claude",
    path: LONG_SOURCE_DIR,
    canonical_identity: LONG_SOURCE_DIR.toLowerCase(),
    identity_confirmed: true,
    origins: ["手动"],
    mapping_path: null,
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: [],
  },
  {
    id: "c:/users/dev/.codex/sessions",
    agent: "codex",
    path: "C:/Users/dev/.codex/sessions",
    canonical_identity: "c:/users/dev/.codex/sessions",
    identity_confirmed: true,
    origins: ["默认自动"],
    mapping_path: null,
    row_enabled: true,
    enabled: true,
    state: "ready",
    diagnostics: [],
  },
];

const sourceStatusLong = [
  {
    agent: "claude-code",
    dir: LONG_SOURCE_DIR,
    enabled: true,
    exists: true,
    files: 12,
    state: "ready",
  },
  {
    agent: "codex",
    dir: "C:/Users/dev/.codex/sessions",
    enabled: true,
    exists: true,
    files: 4,
    state: "ready",
  },
];

// ── 各 fixture 的数据 ──────────────────────────────────────────────
const NORMAL_GROUPS = [
  group("2026-10-05", 120000, {
    output: 60000,
    cache_write: 4000,
    cache_read: 30000,
    cost_usd: 0.42,
    requests: 12,
  }),
  group("2026-10-06", 90000, {
    output: 45000,
    cache_write: 2000,
    cache_read: 21000,
    cost_usd: 0.31,
    requests: 9,
  }),
  group("2026-10-07", 140000, {
    output: 70000,
    cache_write: 5000,
    cache_read: 36000,
    cost_usd: 0.51,
    requests: 15,
  }),
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
  group(`model-${String(i).padStart(2, "0")}`, 40000 - i * 500, {
    output: 20000 - i * 200,
    cost_usd: 0.1,
  }),
);

/** 明细行：与汇总同源（empty → 零行；unknown → 未知价行；partial → 混合）。 */
function rowsFor(kind) {
  switch (kind) {
    case "empty":
      return [];
    case "unknown":
      return [
        eventRow(1, { model: "gpt-5-codex", cost_usd: null, breakdown: null }),
        eventRow(2, { model: "claude-opus-4-1", cost_usd: 0.2 }),
        eventRow(3, { model: "gpt-5-codex", cost_usd: null, breakdown: null }),
      ];
    case "partial":
      return [
        eventRow(1, { model: "claude-sonnet-4-5", cost_usd: 0.18 }),
        eventRow(2, {
          model: "mystery-model",
          cost_usd: 0.02,
          breakdown: partialBreakdown(0.02),
        }),
        eventRow(3, { model: "mystery-model", cost_usd: 0.03, breakdown: partialBreakdown(0.03) }),
      ];
    case "long":
      return [
        eventRow(1, { project: LONG_SOURCE_DIR, model: "claude-sonnet-4-5-20250929-v2" }),
        eventRow(2, {
          project: LONG_SOURCE_DIR,
          model: "claude-sonnet-4-5-20250929-v2",
          cost_usd: 0.008,
        }),
      ];
    case "multi":
      return Array.from({ length: 8 }, (_, i) =>
        eventRow(i + 1, { model: `model-${String(i).padStart(2, "0")}` }),
      );
    default:
      return [
        eventRow(1, { cost_usd: 0.0126 }),
        eventRow(2, { cost_usd: 0.008 }),
        eventRow(3, { cost_usd: null, breakdown: null, model: "mystery-model" }),
        eventRow(4, { agent: "codex", model: "gpt-5-codex", cost_usd: 0.004 }),
        eventRow(5, { agent: "codex", model: "gpt-5-codex", cost_usd: 0.005 }),
        eventRow(6, { cost_usd: 0.006 }),
      ];
  }
}

function dimGroups(kind, by) {
  const base =
    kind === "multi"
      ? MULTI_GROUPS
      : kind === "unknown"
        ? UNKNOWN_GROUPS
        : kind === "partial"
          ? PARTIAL_GROUPS
          : kind === "long"
            ? LONG_GROUPS
            : kind === "empty"
              ? []
              : NORMAL_GROUPS;
  if (by === "model") return base.map((g, i) => ({ ...g, key: `${kind}-model-${i}`, label: null }));
  if (by === "project")
    return base.map((g, i) => ({ ...g, key: `/proj/${kind}-${i}`, label: `${kind}-project-${i}` }));
  if (by === "day") {
    // 日维度的类别必须是日期——否则"切维后不残留旧类别"会被 fixture 自己满足。
    return base.map((g, i) => ({
      ...g,
      key: `2026-09-${String(i + 1).padStart(2, "0")}`,
      label: null,
    }));
  }
  if (by === "agent")
    return [
      group("claude-code", 200000, { output: 100000, cost_usd: 0.8, requests: 30 }),
      group("codex", 80000, { output: 40000, cost_usd: 0.3, requests: 12 }),
    ];
  return base;
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

// ── 会话表（SF04：多 query_id、各自冻结参数与游标位置） ─────────────
/**
 * @param {object} cfg
 * @param {object[]} cfg.rows 明细全量行
 * @param {(by: string) => object[]} cfg.groupsOf 按维度取汇总分组
 * @param {number} cfg.pageSize 每页行数
 * @param {number} cfg.expireAfterPages 第几页之后返回 query_expired（Infinity = 不过期）
 */
function createSessionTable(cfg) {
  const pageSize = cfg.pageSize ?? 2;
  const expireAfterPages = cfg.expireAfterPages ?? Infinity;
  const sessions = new Map();
  let counter = 0;

  /**
   * U01：按请求的时间参数回传**冻结的范围**（与后端 `RangeView` 同形）——
   * 界面标签/悬停提示只读它，契约才能断言"范围与汇总同源"。
   * 四组参数（days / rollingHours / startAt+endAt / untilNow）与后端一致。
   */
  function rangeOf(args) {
    const rolling = args?.rollingHours;
    const startAt = typeof args?.startAt === "string" ? args.startAt : null;
    const endAt = typeof args?.endAt === "string" ? args.endAt : null;
    const untilNow = args?.untilNow === true;
    if (rolling != null) {
      return {
        granularity: "instant",
        start_local: ROLLING_START_LOCAL,
        end_local: AS_OF_LOCAL_MINUTE,
        start: ROLLING_START_UTC,
        end: AS_OF,
      };
    }
    if (startAt) {
      const midnight = startAt.slice(11) === "00:00";
      const lastMinute = endAt != null && endAt.slice(11) === "23:59";
      if (midnight && (lastMinute || untilNow)) {
        return {
          granularity: "day",
          from: startAt.slice(0, 10),
          to: lastMinute ? endAt.slice(0, 10) : AS_OF_DAY,
        };
      }
      return {
        granularity: "instant",
        start_local: startAt,
        end_local: untilNow ? AS_OF_LOCAL_MINUTE : endAt,
        start: AS_OF,
        end: AS_OF,
      };
    }
    if (args?.days != null) {
      const n = Math.max(1, Number(args.days) || 1);
      return { granularity: "day", from: dayMinus(AS_OF_DAY, n - 1), to: AS_OF_DAY };
    }
    return null;
  }

  function begin(args) {
    counter += 1;
    // 每个会话有独立 query_id（跨会话不复用），并**冻结**建会话时的参数。
    const id = `q-syn-${counter}-g1`;
    const range = rangeOf(args);
    sessions.set(id, {
      by: args?.by ?? "day",
      tz: args?.tz ?? TZ,
      range,
      frozenRows: cfg.rows.slice(),
      pagesServed: 0,
      cursors: new Set(),
    });
    return {
      queryId: id,
      generation: 1,
      pricingRevision: PRICING_REVISION,
      timezone: args?.tz ?? TZ,
      asOf: AS_OF,
      range,
    };
  }

  function summary(id) {
    const s = sessions.get(id);
    if (!s) return reject(`query_expired: 会话 ${id} 不存在或已失效，请刷新重试`);
    const real = cfg.groupsOf(s.by);
    const total = totals(real);
    return {
      query_id: id,
      pricing_revision: PRICING_REVISION,
      timezone: s.tz,
      generated_at: AS_OF,
      // U01：范围与汇总同源（界面显示必须取自这里）。
      range: s.range ?? null,
      sources: [
        {
          agent: "claude-code",
          stats: { files_scanned: 12, lines_seen: 800, events: s.frozenRows.length },
        },
        { agent: "codex", stats: { files_scanned: 4, lines_seen: 200, events: 1 } },
      ],
      by: s.by,
      groups: real.length > 0 ? [...real, total] : [],
      totals: total,
      warnings: [],
    };
  }

  function events(id, args) {
    const s = sessions.get(id);
    if (!s) return reject(`query_expired: 会话 ${id} 不存在或已失效，请刷新重试`);
    const rows = s.frozenRows;
    const before = args?.before ?? null;
    let offset = 0;
    if (before != null) {
      if (!s.cursors.has(before)) {
        // 游标必须属于当前会话（外会话/旧磁盘视图的游标 → 结构化拒绝）。
        return reject("query_expired: 游标不属于当前查询会话，请刷新重试");
      }
      const idx = rows.findIndex((r) => r.cursor === before);
      offset = idx + 1;
    }
    s.pagesServed += 1;
    if (s.pagesServed > expireAfterPages) {
      sessions.delete(id);
      return reject("query_expired: 查询会话已过期，请刷新重试");
    }
    // 下钻过滤与后端一致（模型/项目/日）
    let scoped = rows;
    if (args?.model) scoped = scoped.filter((r) => r.model === args.model);
    if (args?.project) scoped = scoped.filter((r) => r.project === args.project);
    if (args?.day) scoped = scoped.filter((r) => r.ts.startsWith(args.day));
    const page = scoped.slice(offset, offset + pageSize);
    for (const r of page) s.cursors.add(r.cursor);
    return {
      query_id: id,
      pricing_revision: PRICING_REVISION,
      rows: page,
      total: scoped.length,
      warnings: [],
    };
  }

  return {
    begin,
    summary,
    events,
    /** 强制让某会话失效（测试可在页面上读取到句柄后再触发过期）。 */
    expire(id) {
      sessions.delete(id);
    },
    ids() {
      return [...sessions.keys()];
    },
    size() {
      return sessions.size;
    },
  };
}

/**
 * 构造指定名称的 fixture。
 * @param {string} name
 * @returns {{ name: string, meta: object, ipc: (cmd: string, args: object) => unknown,
 *   table: ReturnType<typeof createSessionTable>, settleDeferred: (key: string, value: unknown) => void,
 *   failDeferred: (key: string, message: string) => void, deferredState: () => string[] }}
 */
export function buildFixture(name) {
  const cfg = SCENARIOS[name] ?? SCENARIOS.normal;
  const rows = rowsFor(cfg.kind);
  const table = createSessionTable({
    rows,
    groupsOf: (by) => dimGroups(cfg.kind, by),
    pageSize: cfg.pageSize ?? 2,
    expireAfterPages: cfg.expireAfterPages ?? Infinity,
  });

  // 挂起响应：key → {resolve, reject}，由测试在确定时刻 settle。
  const pending = new Map();
  function settleDeferred(key, value) {
    const p = pending.get(key);
    if (!p)
      throw new Error(
        `没有挂起的 deferred：${key}（现有：${[...pending.keys()].join(",") || "无"}）`,
      );
    pending.delete(key);
    p.resolve(value);
  }
  function failDeferred(key, message) {
    const p = pending.get(key);
    if (!p) throw new Error(`没有挂起的 deferred：${key}`);
    pending.delete(key);
    p.reject(new Error(message));
  }

  // PR3：来源行可变集合（cfg.sourceRows 指定初始集；缺省默认两行）。
  let sourceRows = (cfg.sourceRows ?? "default") === "merged"
    ? SOURCE_ROWS_MERGED_AND_LONG.map((r) => ({ ...r, origins: [...r.origins] }))
    : SOURCE_ROWS_DEFAULT.map((r) => ({ ...r, origins: [...r.origins] }));

  // 可变状态（错误恢复类 fixture 需要"第一次失败、重试成功"）
  const state = {
    settingsGets: 0,
    syncRuns: 0,
    statusReads: 0,
    sourceReads: 0,
    sourceWrites: 0,
    savedSources: {},
    closeWrites: 0,
  };

  /** 设置读取响应（按 cfg.settings 模式）。
   *
   * U03 起应用启动时会先读一次 `settings_get`（统计时区的唯一来源），
   * 因此这里的第 1 次调用属于启动，**设置页的首次读取是第 2 次**：
   * 错误恢复类场景必须把失败点放在第 2 次，否则失败会被启动读取吃掉，
   * 设置页永远看不到首读错误条。
   */
  function settingsResponse() {
    state.settingsGets += 1;
    const n = state.settingsGets;
    if (cfg.settings === "fail-then-ok") {
      if (n === 2) return reject("设置文件损坏（合成）");
      return {
        close_action: "quit",
        price_auto_sync: true,
        // U06：自动读取间隔（后端缺省 300）。
        collect_interval_secs: 300,
        sources: {
          claude: { enabled: true, dir: "C:/restored/claude" },
          codex: { enabled: false, dir: "C:/restored/codex" },
        },
      };
    }
    if (cfg.settings === "close-quit") {
      // AP04：已确认的关闭动作（quit）——用于"写入失败必须回退到已确认值"。
      return {
        close_action: "quit",
        price_auto_sync: true,
        collect_interval_secs: 300,
        sources: {
          claude: { enabled: true, dir: "C:/confirmed/claude" },
          codex: { enabled: false, dir: "C:/confirmed/codex" },
        },
      };
    }
    if (cfg.settings === "defer-second-read") {
      // RC09：首读失败（错误条出现、页面级 loading 已结束）→ 用户点重试发起
      // 第二次读取并挂起 → 期间编辑并保存 → 测试释放"旧值"读取。
      // 这一步必须是物理上可操作的：页面遮罩还在时用户根本点不到保存。
      // U03：第 1 次是应用启动的时区读取，**必须成功**（时区门控靠它放行
      // 业务页）；设置页首读是第 2 次（失败），重试的第 3 次起才挂起。
      if (n === 1)
        return { close_action: null, price_auto_sync: true, collect_interval_secs: 300 };
      if (n === 2) return reject("设置读取失败（合成）");
      return defer(`settings_get#${n}`);
    }
    return { close_action: null, price_auto_sync: true, collect_interval_secs: 300, sources: structuredClone(state.savedSources) };
  }

  /** 挂起的第二次 settings_get 在测试里被释放时使用的"旧值"载荷。 */
  const staleSettingsPayload = {
    close_action: "minimize",
    price_auto_sync: false,
    sources: { claude: { enabled: true, dir: "C:/old" }, codex: { enabled: true, dir: "D:/old" } },
  };

  function common(cmd, args) {
    switch (cmd) {
      // P06：隐私引导——普通场景等价"已有有效同意记录（ready）"，
      // 未同意场景由 cfg.privacy === "pending" 返回 needs_consent，
      // 业务命令在 ipc() 里统一被拒绝。
      case "privacy_bootstrap":
        return privacySnapshot(cfg.privacy === "pending" ? "needs_consent" : "ready");
      case "privacy_accept":
        return privacySnapshot("ready");
      case "privacy_exit_resolve":
        return null;
      case "startup_diagnostics":
        return (
          cfg.startup ?? {
            state: "ok",
            dir: "C:/Users/dev/AppData/Roaming/tokenscope/logs",
            message: null,
          }
        );
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "collection_status":
        // PR2：只读采集状态快照——浏览器契约里等价"最近一轮成功、无警告"。
        // latestGeneration 为 null：不触发"有新数据"自动刷新，保持既有
        // 契约的查询次数语义不变。
        return {
          revision: 2,
          runId: 1,
          phase: "succeeded",
          trigger: "startup",
          latestGeneration: null,
          changed: true,
          hasWarnings: false,
          lastCompletedAt: "2026-10-15T02:16:05.041Z",
          error: null,
        };
      case "pricing_status": {
        state.statusReads += 1;
        // RC09：状态读取失败形态——pricing_status 本身拒绝（不是价格列表）。
        if (cfg.statusFails) return reject("pricing_status 读取失败（合成）");
        if (cfg.sync === "partial-then-ok") {
          // 同步前：还没有可用价格；同步后：主源已可用（needsSync=false）。
          return state.syncRuns > 0 ? { ...pricingStatusOk } : { ...pricingStatusNeedsSync };
        }
        // AP05：按 DTO 区分横幅状态——needsSync 与 hasAnyPricing 是两个独立维度。
        if (cfg.pricing === "needs-sync-external-only")
          return { ...pricingStatusNeedsSyncExternalOnly };
        if (cfg.pricing === "needs-sync-openrouter-only")
          return { ...pricingStatusNeedsSyncOpenrouterOnly };
        if (cfg.pricing === "none-available") return { ...pricingStatusNeedsSync };
        return pricingStatusOk;
      }
      case "source_status": {
        state.sourceReads += 1;
        // AP06：首次检测失败 / 目录尚未修好 → 用户重试或手动刷新后必须重查。
        if (cfg.sourceStatus === "fail-then-ready") {
          if (state.sourceReads === 1) return reject("来源状态读取失败（合成）");
          return sourceStatusReady;
        }
        if (cfg.sourceStatus === "missing-then-ready") {
          return state.sourceReads === 1 ? sourceStatusMissing : sourceStatusReady;
        }
        return cfg.kind === "long" ? sourceStatusLong : sourceStatusReady;
      }
      case "view_cache_load":
        return cfg.viewCache ?? null;
      case "view_cache_save":
        return null;
      case "settings_get":
        return settingsResponse();
      case "settings_set_close_action": {
        state.closeWrites += 1;
        // AP04：写入被后端拒绝（不是读取失败）——界面必须回退到已确认值。
        if (cfg.closeWrite === "fail-then-ok" && state.closeWrites === 1)
          return reject("写入设置失败：磁盘不可写（合成）");
        return null;
      }
      case "settings_set_collect_interval": {
        // U06：自动读取间隔（白名单 5/10/30/60/300/600）——返回落盘值；
        // 界面写入走串行保存队列，白名单外的值后端会拒绝（这里同形拒绝）。
        const secs = args?.secs;
        if (![5, 10, 30, 60, 300, 600].includes(secs))
          return reject(`自动读取间隔只能是 5/10/30/60/300/600 秒（收到 ${secs}）`);
        return secs;
      }
      case "settings_set_price_auto_sync":
        return args?.enabled ?? true;
      case "cache_stats":
      case "refresh_cache":
        return { path: "C:/Users/dev/AppData/Roaming/tokenscope/cache.db", files: 3, events: 120 };
      case "pricing_entries": {
        if (cfg.statusFails) return reject("价格状态读取失败（合成）");
        const view = pricingView();
        if (cfg.largePricing) {
          const sample = view.entries[0];
          view.entries = Array.from({ length: 8466 }, (_, i) => ({
            ...sample, prefix: `synthetic-model-${i}`, name: `合成模型 ${i}`, source: "models.dev",
          }));
          view.modelsdev_count = 8466;
        }
        return view;
      }
      case "autostart_status":
        return false;
      case "autostart_set":
        return true;
      case "source_config_set": {
        state.sourceWrites += 1;
        // AP04/Task 2：后端按行拒绝（目录冲突）——错误必须只归属该来源行。
        if (cfg.sourceWrite === "fail-then-ok" && state.sourceWrites === 1)
          return reject("来源目录冲突：两个启用的来源不能指向同一目录（合成）");
        // 成功写入必须可读回；设置保存通知刷新后不能凭空退回旧目录。
        state.savedSources[args.agent] = { enabled: args.enabled, dir: args.dir };
        // PR3：总开关只写 enabled（dir 参数已废弃，不得覆盖列表）。
        const row = sourceRows.find((r) => r.agent === args.agent);
        if (row) row.enabled = args.enabled;
        return null;
      }
      // ── PR3 §5：来源目录管理（列表/追加/移除/行启停/自动开关）──
      case "source_paths_list": {
        state.sourceReads += 1;
        return sourceRows.map((r) => ({ ...r, origins: [...r.origins] }));
      }
      case "source_path_add": {
        const path = String(args?.path ?? "").trim();
        if (!/^[a-zA-Z]:[\/]/.test(path))
          return reject(`目录必须是绝对路径：${path || "（空）"}（合成）`);
        const exists = sourceRows.find(
          (r) => r.canonical_identity === path.toLowerCase(),
        );
        if (exists)
          return reject(
            `目录已在列表中：${exists.path}（合成）`,
          );
        // 合成口径：C:/Users/dev 的子目录一律按"与默认根嵌套"拒绝（父/子）。
        if (path.toLowerCase().startsWith("c:/users/dev/") && !path.toLowerCase().includes("history"))
          return reject(
            "目录与已有来源存在嵌套关系（合成）：请改用互不嵌套的目录",
          );
        const id = path.toLowerCase();
        sourceRows.push({
          id,
          agent: args.agent,
          path,
          canonical_identity: id,
          identity_confirmed: true,
          origins: ["手动"],
          mapping_path: null,
          row_enabled: true,
          enabled: true,
          state: "ready",
          diagnostics: [],
        });
        return `added:${id}`;
      }
      case "source_path_remove": {
        const before = sourceRows.length;
        sourceRows = sourceRows.filter(
          (r) => !(r.agent === args.agent && r.id === args.id && r.origins.includes("手动")),
        );
        if (sourceRows.length === before)
          return reject("该行没有可移除的手动目录（合成）");
        return null;
      }
      case "source_path_set_enabled": {
        const row = sourceRows.find(
          (r) => r.agent === args.agent && r.id === args.id,
        );
        if (!row) return reject("来源已变化，请重试（合成）");
        row.row_enabled = args.enabled;
        row.enabled = args.enabled && row.origins.includes("默认自动");
        return null;
      }
      case "source_auto_set":
        return args?.enabled ?? true;
      case "sync_pricing_openrouter": {
        state.syncRuns += 1;
        if (cfg.sync === "fail") return reject("OpenRouter: 同步失败（合成）");
        if (cfg.sync === "partial-then-ok") {
          if (state.syncRuns === 1)
            return reject(
              "补充源 OpenRouter 同步失败：网络不可达（合成）；主源 models.dev 已写入 42 条",
            );
          return [{ source: "models.dev", count: 42 }];
        }
        return [];
      }
      // U03：统计时区保存（settings.toml 单一来源）——返回归一化后的落盘值。
      case "settings_set_timezone":
        return typeof args?.timezone === "string" ? args.timezone : null;
      // H05/U05：CCS 导入面板进入「用量数据」分区后才解析默认路径（只读路径
      // 解析，不打开来源库）。本契约矩阵不驱动导入流程，因此预览/提交显式
      // 拒绝——一旦误触发立刻可见，而不是静默返回空数据。
      case "ccs_import_defaults":
        return {
          path: "C:/Users/dev/AppData/Roaming/cc-switch/cc-switch.db",
          exists: true,
          timezone: "Asia/Shanghai",
        };
      case "ccs_import_preview":
        return reject("CCS 预览未在本契约矩阵中启用（合成）");
      case "ccs_import_commit":
        return reject("CCS 提交未在本契约矩阵中启用（合成）");
      case "ccs_import_discard":
        return false;
      case "open_settings_file":
        return "C:/Users/dev/AppData/Roaming/tokenscope/settings.toml";
      case "open_pricing_file":
        return "C:/Users/dev/AppData/Roaming/tokenscope/pricing.toml";
      default:
        return undefined;
    }
  }

  function ipc(cmd, args) {
    // P06：未同意基线——业务命令一律返回 privacy_consent_required（与真实
    // 后端同形）：前端若在同意前发业务命令，契约会直接失败而不是静默通过。
    if (cfg.privacy === "pending" && PRIVACY_PROTECTED_COMMANDS.includes(cmd)) {
      return reject(`privacy_consent_required：尚未同意隐私政策（${cmd}）`);
    }
    if (cmd === "query_begin") return table.begin(args);
    if (cmd === "query_summary") return table.summary(args?.queryId);
    if (cmd === "query_events") return table.events(args?.queryId, args);
    const res = common(cmd, args);
    if (isDeferred(res)) {
      // 注册挂起响应；测试通过 settleDeferred/failDeferred 决定结束时刻。
      const key = res[DEFER];
      return new Promise((resolve, rejectFn) => {
        pending.set(key, { resolve, reject: rejectFn });
      });
    }
    return res;
  }

  return {
    name,
    meta: { theme: null, viewport: null, kind: cfg.kind, scenario: cfg },
    ipc,
    table,
    settleDeferred,
    failDeferred,
    deferredState: () => [...pending.keys()],
    /** 释放"第二次设置读取"用的旧载荷（写后回退竞态用）。 */
    staleSettings: () => staleSettingsPayload,
    state,
  };
}

/** 场景配置表：每个 fixture 明确它的失败/挂起/过期形态。 */
const SCENARIOS = {
  normal: { kind: "normal" },
  "large-pricing": { kind: "normal", largePricing: true },
  empty: { kind: "empty" },
  "unknown-price": { kind: "unknown" },
  "partial-price": { kind: "partial" },
  "long-text": { kind: "long" },
  "multi-category": { kind: "multi", pageSize: 4 },
  // RC09：设置首载失败 → 重试恢复全部依赖控件
  "settings-read-failure": { kind: "normal", settings: "fail-then-ok" },
  // RC09：真实浏览器重演"读取挂起 → 编辑 → 保存成功 → 释放旧读取"
  "settings-deferred-race": { kind: "normal", settings: "defer-second-read" },
  // RC09：补充源同步失败但主源已可用 → 错误条必须有可点击的重试
  "sync-partial-failure": { kind: "normal", sync: "partial-then-ok" },
  // RC09：分页在途/翻页后会话过期 → 错误可见且重试只建一个新批次
  "query-expired-on-page2": { kind: "normal", pageSize: 2, expireAfterPages: 1 },
  // RC09：状态读取失败（不谎称部分成功）+ 价格列表读取失败
  "status-read-failure": { kind: "normal", statusFails: true },
  // AP04：来源保存被后端拒绝（目录冲突）→ 错误只归属该行、输入保留、重试成功
  "source-save-rejected": { kind: "normal", sourceWrite: "fail-then-ok" },
  // PR3 §5：来源目录管理——合并标签/长路径初始集 + 追加/移除/行启停/开关。
  "source-paths-management": { kind: "normal", sourceRows: "merged" },
  // AP04：关闭动作写入被拒绝 → 界面回退到已确认值（quit），错误可见可重试
  "close-action-write-failure": { kind: "normal", settings: "close-quit", closeWrite: "fail-then-ok" },
  // AP04：挂起的 settings_get 以 reject 结束 → 不得把已保存值降级成"未知"
  "settings-late-failure-after-save": { kind: "normal", settings: "defer-second-read" },
  // AP06：来源检测首次拒绝 → 错误条重试成功后来源状态就绪，汇总不受影响
  "source-status-failure-then-ready": { kind: "normal", sourceStatus: "fail-then-ready" },
  // AP06：目录从 missing 变 ready → 手动刷新重查来源状态，旧通知消失
  "source-status-missing-then-ready": { kind: "normal", sourceStatus: "missing-then-ready" },
  // AP05：needsSync=true 但**有**可用价格（只有外置价格表 / 只有 OpenRouter）
  "pricing-needs-sync-external-only": { kind: "normal", pricing: "needs-sync-external-only" },
  "pricing-needs-sync-openrouter-only": { kind: "normal", pricing: "needs-sync-openrouter-only" },
  // AP05：主源缺失且完全无价 → 这才是"当前费用仅能显示为未知"
  "pricing-none-available": { kind: "normal", pricing: "none-available" },
  // P06：未同意隐私政策——只有引导命令可用，业务命令统一被拒绝，业务界面不挂载
  "privacy-consent-pending": { kind: "normal", privacy: "pending" },
};

export const FIXTURE_NAMES = Object.keys(SCENARIOS);
