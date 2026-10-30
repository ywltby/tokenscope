// AC10（N12 单实例）纯逻辑：应用标识后缀、隔离根预检、第二次启动的
// 退出/窗口恢复/托盘观察判定。所有外部观察由调用方注入；本文件不 spawn
// 进程、不读写真实数据，便于在 node 环境做有区分度的合成测试。
import path from "node:path";

/** 第二次启动必须在限定时间内退出；超时即失败并由调用方终止已记录 PID。 */
export const SINGLE_INSTANCE_EXIT_BUDGET_MS = 15000;
/** 第二次启动退出后，第一个窗口恢复可见 + 获得前台的观察上限。 */
export const SINGLE_INSTANCE_RESTORE_BUDGET_MS = 10000;
/** 第一个实例就绪后等待 TokenScope 托盘图标出现的上限。 */
export const SINGLE_INSTANCE_TRAY_BUDGET_MS = 10000;
/** 与 src-tauri/src/acceptance.rs 的 identifier 后缀保持一致。 */
export const ACCEPTANCE_IDENTIFIER_SUFFIX = ".acceptance";

/** 验收构建的 identifier：普通实例与验收实例使用不同单实例互斥名。 */
export function acceptanceIdentifier(baseIdentifier) {
  const base = String(baseIdentifier ?? "").trim();
  if (!base) throw new Error("acceptanceIdentifier 需要非空应用标识");
  return `${base}${ACCEPTANCE_IDENTIFIER_SUFFIX}`;
}

function comparePath(value) {
  const resolved = path.resolve(value);
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

/** `child` 是否等于 `parent` 或位于其下（按规范化绝对路径的分量边界）。 */
function isWithin(child, parent) {
  const rel = path.relative(comparePath(parent), comparePath(child));
  return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
}

/**
 * 验收根预检：必须是非盘符根的绝对路径，且与真实 `~/.tokenscope` 互不包含。
 * 只证明路径隔离；运行时仍由 acceptance 构建强制 `TOKENSCOPE_ACCEPTANCE_ROOT`。
 */
export function isIsolatedAcceptanceRoot(root, realDataDir) {
  const rawRoot = String(root ?? "").trim();
  const rawReal = String(realDataDir ?? "").trim();
  if (!rawRoot) return { ok: false, reason: "隔离根为空" };
  if (!rawReal) return { ok: false, reason: "真实数据目录为空，无法证明隔离" };
  if (!path.isAbsolute(rawRoot)) return { ok: false, reason: `隔离根不是绝对路径：${rawRoot}` };
  const resolved = path.resolve(rawRoot);
  if (resolved === path.parse(resolved).root) {
    return { ok: false, reason: `隔离根不能是盘符根：${resolved}` };
  }
  if (isWithin(resolved, rawReal)) {
    return { ok: false, reason: `隔离根位于真实数据目录内：${resolved}` };
  }
  if (isWithin(rawReal, resolved)) {
    return { ok: false, reason: `隔离根是真实数据目录的祖先：${resolved}` };
  }
  return {
    ok: true,
    reason: `隔离根 ${resolved} 与真实数据目录 ${path.resolve(rawReal)} 互不包含`,
  };
}

/**
 * 解析 `native-window-action.ps1 -Action state` 的输出：
 * `STATE found true hwnd 123 visible true minimized false rect x,y,w,h`。
 * 输出缺失/畸形时返回 `ok:false`，由调用方记为待验证而不是猜状态。
 */
export function parseWindowState(text) {
  const line = String(text ?? "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .find((value) => value.startsWith("STATE "));
  if (!line) return { ok: false, error: "窗口状态输出缺少 STATE 行" };
  const tokens = line.split(/\s+/);
  const fields = {};
  for (let i = 1; i + 1 < tokens.length; i += 2) fields[tokens[i]] = tokens[i + 1];
  if (fields.found !== "true" && fields.found !== "false") {
    return { ok: false, error: `窗口状态 found 字段非法：${fields.found ?? "缺失"}` };
  }
  if (fields.found === "true") {
    for (const key of ["visible", "minimized"]) {
      if (fields[key] !== "true" && fields[key] !== "false") {
        return { ok: false, error: `窗口状态 ${key} 字段非法：${fields[key] ?? "缺失"}` };
      }
    }
  }
  return {
    ok: true,
    found: fields.found === "true",
    hwnd: fields.hwnd ?? "0",
    visible: fields.visible === "true",
    minimized: fields.minimized === "true",
    rect: fields.rect ?? "none",
  };
}

/**
 * 解析 `native-window-action.ps1 -Action foreground` 的输出：
 * `FOREGROUND match true pid 123 hwnd 123 fg_pid 123 fg_hwnd 456`。
 */
export function parseForeground(text) {
  const line = String(text ?? "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .find((value) => value.startsWith("FOREGROUND "));
  if (!line) return { ok: false, error: "前台窗口输出缺少 FOREGROUND 行" };
  const tokens = line.split(/\s+/);
  const fields = {};
  for (let i = 1; i + 1 < tokens.length; i += 2) fields[tokens[i]] = tokens[i + 1];
  if (fields.match !== "true" && fields.match !== "false") {
    return { ok: false, error: `前台窗口 match 字段非法：${fields.match ?? "缺失"}` };
  }
  return {
    ok: true,
    match: fields.match === "true",
    pid: fields.pid ?? null,
    hwnd: fields.hwnd ?? null,
    foreground_pid: fields.fg_pid ?? null,
    foreground_hwnd: fields.fg_hwnd ?? null,
  };
}

/** 托盘图标文本的中文/英文 tooltip 都可能带后缀；只按名称包含关系计数。 */
export function countTokenScopeMatches(matches) {
  return (Array.isArray(matches) ? matches : []).filter((entry) => {
    const text = typeof entry === "string" ? entry : entry?.text;
    return /tokenscope/i.test(String(text ?? ""));
  }).length;
}

/**
 * 解析托盘观察脚本的 JSON。PowerShell 告警可能混入 stdout，因此取最后一个
 * 可解析的 JSON 行；解析不出来时返回 `ok:false`（调用方记待验证）。
 */
export function parseTrayObservation(text) {
  const lines = String(text ?? "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter((value) => value.startsWith("{"));
  for (const line of lines.reverse()) {
    try {
      const data = JSON.parse(line);
      const matches =
        data.matches === null || data.matches === undefined
          ? []
          : Array.isArray(data.matches)
            ? data.matches
            : [data.matches];
      const hasCount = Number.isInteger(data.token_scope_count);
      const hasMatches = data.matches !== null && data.matches !== undefined;
      if (!hasCount && !hasMatches) {
        return {
          ok: false,
          error: "托盘观察缺少 token_scope_count 与 matches",
          token_scope_count: null,
          matches: [],
        };
      }
      const derived = countTokenScopeMatches(matches);
      const count = hasCount ? data.token_scope_count : derived;
      return {
        ok: data.ok === true,
        error: data.error ?? null,
        visible_count: Number.isInteger(data.visible_count) ? data.visible_count : null,
        overflow_count: Number.isInteger(data.overflow_count) ? data.overflow_count : null,
        total_count: Number.isInteger(data.total_count) ? data.total_count : null,
        token_scope_count: count,
        matches,
      };
    } catch {
      /* 换上一行 */
    }
  }
  return { ok: false, error: "托盘观察输出不是 JSON", token_scope_count: null, matches: [] };
}

/**
 * 第二个实例的环境：必须继承固定隔离通道（价格桩/代理），强制同一个
 * TOKENSCOPE_ACCEPTANCE_ROOT，并清掉远程调试参数（第二个实例不接 CDP）。
 */
export function secondaryLaunchEnvironment({
  inherited = {},
  controlled = {},
  scenario = {},
  root,
} = {}) {
  if (!String(root ?? "").trim()) {
    throw new Error("secondaryLaunchEnvironment 需要非空隔离根");
  }
  const env = { ...inherited, ...controlled, ...scenario, TOKENSCOPE_ACCEPTANCE_ROOT: root };
  delete env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS;
  return env;
}

/**
 * 第二个实例的退出等待：exit/error 事件与短轮询取先到者；超时或场景取消时
 * 只终止调用方传入的 child。返回原始观察，判定交由 classifySecondaryExit。
 * 独立成函数是为了能用真实短命子进程做合成验证，而不必启动 Native。
 */
export async function waitForSecondaryExit({
  child,
  timeoutMs = SINGLE_INSTANCE_EXIT_BUDGET_MS,
  isCanceled = () => false,
  intervalMs = 50,
  killGraceMs = 5000,
  now = () => Date.now(),
  sleepFn = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
} = {}) {
  if (!child || typeof child.once !== "function") {
    throw new Error("waitForSecondaryExit 需要 child 进程");
  }
  const startedAt = now();
  const deadline = startedAt + Number(timeoutMs);
  let terminal = null;
  const terminalPromise = new Promise((resolve) => {
    child.once("exit", (code, signal) => resolve({ kind: "exit", code, signal, at: now() }));
    child.once("error", (error) => resolve({ kind: "error", error, at: now() }));
  });
  let timedOut = false;
  let canceled = false;
  while (!terminal) {
    const next = await Promise.race([terminalPromise, sleepFn(intervalMs).then(() => null)]);
    if (next) {
      terminal = next;
      break;
    }
    if (isCanceled()) {
      canceled = true;
      break;
    }
    if (now() >= deadline) {
      timedOut = true;
      break;
    }
  }
  let killed = false;
  if (!terminal && (timedOut || canceled)) {
    try {
      killed = child.kill() !== false;
    } catch {
      killed = false;
    }
    terminal = await Promise.race([terminalPromise, sleepFn(killGraceMs).then(() => null)]);
  }
  const exitCode = terminal?.kind === "exit" ? terminal.code : null;
  const signal = terminal?.kind === "exit" ? terminal.signal : null;
  const spawnError =
    terminal?.kind === "error" ? String(terminal.error?.message ?? terminal.error) : null;
  return {
    kind: terminal?.kind ?? null,
    exitCode,
    signal,
    spawnError,
    elapsedMs: Math.round((terminal?.at ?? now()) - startedAt),
    timedOut,
    canceled,
    killed,
  };
}

/**
 * 第二个启动的退出判定：正常移交必须是限定时间内以退出码 0 结束。
 * 超时、非零退出、被信号终止都是失败；退出码不可读时是待验证，不猜成 0。
 */
export function classifySecondaryExit({
  exitCode = null,
  signal = null,
  elapsedMs = null,
  budgetMs = SINGLE_INSTANCE_EXIT_BUDGET_MS,
  timedOut = false,
} = {}) {
  const budget = Number(budgetMs);
  const elapsed = Number(elapsedMs);
  if (timedOut) {
    return {
      status: "failed",
      reason: `第二次启动未在 ${budget} ms 内退出（已由驱动终止该 PID）`,
    };
  }
  if (signal) {
    return { status: "failed", reason: `第二次启动被信号 ${signal} 终止，不是正常移交` };
  }
  if (!Number.isFinite(elapsed) || elapsed < 0) {
    return { status: "pending", reason: "第二次启动的退出耗时不可读" };
  }
  if (!Number.isInteger(exitCode)) {
    return { status: "pending", reason: "第二次启动的退出码不可读" };
  }
  if (elapsed > budget) {
    return {
      status: "failed",
      reason: `第二次启动退出耗时 ${Math.round(elapsed)} ms 超过 ${budget} ms`,
    };
  }
  if (exitCode !== 0) {
    return { status: "failed", reason: `第二次启动退出码 ${exitCode}，期望 0` };
  }
  return {
    status: "passed",
    reason: `第二次启动 ${Math.round(elapsed)} ms 内以退出码 0 结束`,
  };
}

/**
 * 第一个窗口恢复判定：可见与前台分开取证；观察机制不可用时对应项为
 * pending，不能用“第一个进程仍存活”之类的替身结论。
 */
export function classifyWindowRestore({
  state = null,
  foreground = null,
  elapsedMs = null,
  budgetMs = SINGLE_INSTANCE_RESTORE_BUDGET_MS,
} = {}) {
  let visibility;
  if (!state || state.ok !== true) {
    visibility = {
      status: "pending",
      reason: `窗口状态观察不可用：${state?.error ?? "无结果"}`,
    };
  } else if (!state.found) {
    visibility = { status: "failed", reason: "找不到第一个实例的主窗口" };
  } else if (state.visible !== true) {
    visibility = { status: "failed", reason: "第一个窗口未恢复可见" };
  } else if (state.minimized !== false) {
    visibility = { status: "failed", reason: "第一个窗口仍处于最小化" };
  } else {
    visibility = { status: "passed", reason: "第一个窗口可见且未最小化" };
  }

  let foregroundVerdict;
  if (!foreground || foreground.ok !== true) {
    foregroundVerdict = {
      status: "pending",
      reason: `前台窗口观察不可用：${foreground?.error ?? "无结果"}`,
    };
  } else if (foreground.match !== true) {
    foregroundVerdict = {
      status: "failed",
      reason: `前台窗口不属于第一个实例（fg_pid=${foreground.foreground_pid ?? "unknown"}）`,
    };
  } else {
    foregroundVerdict = { status: "passed", reason: "前台窗口属于第一个实例" };
  }

  const statuses = [visibility.status, foregroundVerdict.status];
  const status = statuses.includes("failed")
    ? "failed"
    : statuses.includes("pending")
      ? "pending"
      : "passed";
  const elapsed = Number(elapsedMs);
  const elapsedText = Number.isFinite(elapsed)
    ? `恢复观察 ${Math.round(elapsed)} ms（上限 ${Number(budgetMs)} ms）`
    : "恢复观察耗时不可读";
  return {
    status,
    visibility,
    foreground: foregroundVerdict,
    reason: [visibility.reason, foregroundVerdict.reason, elapsedText].join("；"),
  };
}

/**
 * 托盘唯一性判定：必须有实际可观察的 TokenScope 图标计数。
 * 观察不可用时是 pending；传入 processCount 也不会自动变成通过。
 */
export function classifyTrayObservation({
  observation = null,
  expected = 1,
  processCount = null,
} = {}) {
  if (!observation || observation.ok !== true) {
    return {
      status: "pending",
      count: null,
      processCount,
      reason: `托盘观察不可用：${observation?.error ?? "无结果"}`,
    };
  }
  if (!Number.isInteger(observation.token_scope_count)) {
    return {
      status: "pending",
      count: null,
      processCount,
      reason: "托盘观察缺少 TokenScope 图标计数",
    };
  }
  const count = observation.token_scope_count;
  if (count !== expected) {
    return {
      status: "failed",
      count,
      processCount,
      reason: `TokenScope 托盘图标数 ${count}，期望 ${expected}`,
    };
  }
  return {
    status: "passed",
    count,
    processCount,
    reason: `TokenScope 托盘图标数 ${count}`,
  };
}
