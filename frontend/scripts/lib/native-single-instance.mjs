// AC10（N12 单实例）纯逻辑：应用标识后缀、隔离根预检、第二次启动的
// 退出/窗口恢复/托盘观察判定。所有外部观察由调用方注入；本文件不 spawn
// 进程、不读写真实数据，便于在 node 环境做有区分度的合成测试。
import fs from "node:fs";
import crypto from "node:crypto";
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
 * 解析路径的最终真实位置，包含尚不存在的尾部与符号链接/junction 别名。
 * 失败（权限、断链、无法解析）一律 fail closed，不把别名当成独立目录。
 */
export function canonicalizeAcceptancePath(
  target,
  {
    lstatSync = fs.lstatSync,
    realpathSync = fs.realpathSync.native ?? fs.realpathSync,
    statSync = fs.statSync,
  } = {},
) {
  const raw = String(target ?? "").trim();
  if (!raw) return { ok: false, reason: "路径为空" };
  const absolute = path.resolve(raw);
  let current = absolute;
  const tail = [];
  for (;;) {
    try {
      lstatSync(current);
      break;
    } catch (error) {
      if (error?.code !== "ENOENT") {
        return { ok: false, reason: `无法检查路径：${error?.message ?? error}` };
      }
      const parent = path.dirname(current);
      if (parent === current) {
        return { ok: false, reason: `找不到存在的祖先：${absolute}` };
      }
      tail.unshift(path.basename(current));
      current = parent;
    }
  }
  let real;
  try {
    real = realpathSync(current);
  } catch (error) {
    return { ok: false, reason: `无法解析真实路径（别名/链接/权限）：${error?.message ?? error}` };
  }
  const resolved = tail.length ? path.join(real, ...tail) : real;
  try {
    const stat = statSync(resolved);
    if (!stat.isDirectory()) return { ok: false, reason: `不是目录：${resolved}` };
  } catch (error) {
    if (error?.code !== "ENOENT") {
      return { ok: false, reason: `无法检查目录：${error?.message ?? error}` };
    }
  }
  return { ok: true, path: path.resolve(resolved) };
}

/**
 * 受保护根预检：拒绝受保护目录本身、其祖先、其子目录，以及通过符号链接/
 * junction 指向这些位置或位于这些位置下的别名。无法解析真实路径时拒绝。
 */
export function validateAcceptanceRoot({
  root,
  protectedDirs = [],
  canonicalize = canonicalizeAcceptancePath,
} = {}) {
  const rawRoot = String(root ?? "").trim();
  const protectedList = (Array.isArray(protectedDirs) ? protectedDirs : [protectedDirs])
    .map((value) => String(value ?? "").trim())
    .filter(Boolean);
  if (!rawRoot) return { ok: false, reason: "隔离根为空" };
  if (!protectedList.length) return { ok: false, reason: "未提供受保护目录，无法证明隔离" };
  if (!path.isAbsolute(rawRoot)) return { ok: false, reason: `隔离根不是绝对路径：${rawRoot}` };
  const canonicalRoot = canonicalize(rawRoot);
  if (!canonicalRoot.ok) {
    return { ok: false, reason: `隔离根无法解析：${canonicalRoot.reason}` };
  }
  if (canonicalRoot.path === path.parse(canonicalRoot.path).root) {
    return { ok: false, reason: `隔离根不能是盘符根：${canonicalRoot.path}` };
  }
  const protectedCanonical = [];
  for (const source of protectedList) {
    const canonical = canonicalize(source);
    if (!canonical.ok) {
      return { ok: false, reason: `受保护目录无法解析：${canonical.reason}` };
    }
    protectedCanonical.push({ source, path: canonical.path });
  }
  for (const protectedDir of protectedCanonical) {
    if (comparePath(canonicalRoot.path) === comparePath(protectedDir.path)) {
      return {
        ok: false,
        reason: `隔离根是受保护目录本身：${protectedDir.path}`,
      };
    }
    if (isWithin(canonicalRoot.path, protectedDir.path)) {
      return {
        ok: false,
        reason: `隔离根位于受保护目录内：${canonicalRoot.path}`,
      };
    }
    if (isWithin(protectedDir.path, canonicalRoot.path)) {
      return {
        ok: false,
        reason: `隔离根是受保护目录的祖先：${canonicalRoot.path}`,
      };
    }
  }
  return {
    ok: true,
    reason: `隔离根 ${canonicalRoot.path} 与受保护目录互不包含`,
    root: canonicalRoot.path,
    protected: protectedCanonical,
    alias_resolved: canonicalRoot.path !== path.resolve(rawRoot),
  };
}

/** `child` 是否等于 `parent` 或位于其下（按规范化绝对路径的分量边界）。 */

/**
 * 验收根预检：必须是非盘符根的绝对路径，且与真实 `~/.tokenscope` 互不包含。
 * 只证明路径隔离；运行时仍由 acceptance 构建强制 `TOKENSCOPE_ACCEPTANCE_ROOT`。
 */
export function isIsolatedAcceptanceRoot(root, realDataDir) {
  const result = validateAcceptanceRoot({ root, protectedDirs: [realDataDir] });
  return { ok: result.ok, reason: result.reason };
}

/** acceptance 构建中才会编译进 EXE 的字符串标记（shell 验收模块/分支）。 */
export const ACCEPTANCE_EXE_MARKERS = [
  "验收构建要求进程级",
  "验收模式：WebView 用户数据与 identifier 已指向隔离根",
  "验收模式启动失败",
];
/** 普通构建核心库 stub 中的标记；出现即判定不是验收构建。 */
export const NORMAL_ACCEPTANCE_STUB_MARKER = "当前构建未启用 acceptance feature，不接受验收根注入";

function containsBytes(bytes, text) {
  return bytes.includes(Buffer.from(text, "utf8")) || bytes.includes(Buffer.from(text, "utf16le"));
}

function isPeImage(bytes) {
  if (!bytes || bytes.length < 0x40 || bytes[0] !== 0x4d || bytes[1] !== 0x5a) return false;
  const peOffset = bytes.readUInt32LE(0x3c);
  return (
    peOffset + 4 <= bytes.length &&
    bytes.toString("latin1", peOffset, peOffset + 4) === "PE\u0000\u0000"
  );
}

/**
 * 实际 EXE 的验收构建验证：目标必须是 PE 文件，包含 acceptance 专用标记，
 * 且不含普通构建的 acceptance stub 标记。缺文件、读失败、普通 EXE 或
 * 无法验证一律拒绝启动；不依赖从 tauri.conf/identifier 推导。
 */
export function verifyAcceptanceExecutable(exePath, { readFile = fs.readFileSync } = {}) {
  const raw = String(exePath ?? "").trim();
  if (!raw) return { ok: false, reason: "EXE 路径为空" };
  let bytes;
  try {
    bytes = readFile(raw);
  } catch (error) {
    return { ok: false, reason: `无法读取 EXE：${error?.message ?? error}` };
  }
  if (!Buffer.isBuffer(bytes) || bytes.length === 0) {
    return { ok: false, reason: "EXE 为空或读取结果不是 Buffer，无法验证" };
  }
  if (!isPeImage(bytes)) {
    return { ok: false, reason: "文件不是 PE 可执行文件，拒绝启动" };
  }
  const positive = ACCEPTANCE_EXE_MARKERS.filter((marker) => containsBytes(bytes, marker));
  const negative = containsBytes(bytes, NORMAL_ACCEPTANCE_STUB_MARKER)
    ? [NORMAL_ACCEPTANCE_STUB_MARKER]
    : [];
  if (negative.length) {
    return {
      ok: false,
      reason: "EXE 含普通构建 acceptance stub 标记，判定不是 acceptance 构建",
      size: bytes.length,
      markers: positive,
      negative_markers: negative,
    };
  }
  if (!positive.length) {
    return {
      ok: false,
      reason: "EXE 未包含 acceptance 构建标记；普通 EXE 或无法验证，拒绝启动",
      size: bytes.length,
      markers: [],
      negative_markers: negative,
    };
  }
  return {
    ok: true,
    reason: `EXE 为 acceptance 构建（命中 ${positive.length} 个专用标记）`,
    size: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
    markers: positive,
    negative_markers: [],
  };
}

/**
 * 启动前预检：隔离根（含别名解析）与实际 EXE 构建属性。attach 模式不启动
 * EXE，只校验根；非 attach 模式两者都必须通过。
 */
export function evaluateAcceptanceLaunchPreflight({
  root,
  exePath,
  realDataDir,
  attachExisting = false,
  canonicalize = canonicalizeAcceptancePath,
  readFile = fs.readFileSync,
} = {}) {
  const rootCheck = validateAcceptanceRoot({
    root,
    protectedDirs: [realDataDir],
    canonicalize,
  });
  const exeCheck = attachExisting
    ? { ok: true, skipped: true, reason: "attach 模式不启动 EXE" }
    : verifyAcceptanceExecutable(exePath, { readFile });
  return {
    ok: rootCheck.ok && exeCheck.ok,
    root: rootCheck,
    exe: exeCheck,
    attachExisting,
  };
}

/**
 * 预检门：只有预检通过才执行准备与启动；返回是否真的调用过。
 * 合成反例用它证明受保护根/非 acceptance EXE 下 prepare/launch 均为零调用。
 */
export async function startIfPreflightAllows({ preflight, prepare, launch } = {}) {
  const result = await preflight();
  if (!result?.ok) {
    return { started: false, preflight: result, prepared: null, launched: null };
  }
  const prepared = await prepare(result);
  const launched = await launch(result);
  return { started: true, preflight: result, prepared, launched };
}

/** 主进程存活：Node 子进程可能以 signalCode 结束而 exitCode 仍为 null。 */
export function hostProcessAlive(proc) {
  return (proc?.exitCode ?? null) === null && (proc?.signalCode ?? null) === null;
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
 * 托盘观察一致性：计数必须是完整整数、可见+溢出等于总数、TokenScope 计数
 * 必须等于 matches 明细条数，且每条明细文本确实包含 TokenScope。
 * 任一不一致都返回不可通过，不把部分观察当成成功。
 */
export function validateTrayConsistency(observation) {
  if (!observation || observation.ok !== true) {
    return { ok: false, reason: `托盘观察不可用：${observation?.error ?? "无结果"}` };
  }
  const counts = [
    observation.visible_count,
    observation.overflow_count,
    observation.total_count,
    observation.token_scope_count,
  ];
  if (!counts.every(Number.isInteger)) {
    return { ok: false, reason: "托盘观察计数不完整（不可把缺失计数当作 0）" };
  }
  if (observation.visible_count + observation.overflow_count !== observation.total_count) {
    return {
      ok: false,
      reason: `托盘图标总数不一致：${observation.visible_count}+${observation.overflow_count} != ${observation.total_count}`,
    };
  }
  if (!Array.isArray(observation.matches)) {
    return { ok: false, reason: "托盘观察缺少 matches 明细" };
  }
  if (observation.matches.length !== observation.token_scope_count) {
    return {
      ok: false,
      reason: `TokenScope 计数 ${observation.token_scope_count} 与 matches 明细 ${observation.matches.length} 不一致`,
    };
  }
  for (const match of observation.matches) {
    if (!/tokenscope/i.test(String(match?.text ?? ""))) {
      return { ok: false, reason: "matches 明细含非 TokenScope 文本" };
    }
  }
  return { ok: true };
}

/**
 * 解析托盘观察脚本的 JSON。PowerShell 告警可能混入 stdout，因此取最后一个
 * 可解析的 JSON 行；解析不出来或计数/明细自相矛盾时返回 `ok:false`。
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
      const observation = {
        ok: data.ok === true,
        error: data.error ?? null,
        visible_count: Number.isInteger(data.visible_count) ? data.visible_count : null,
        overflow_count: Number.isInteger(data.overflow_count) ? data.overflow_count : null,
        total_count: Number.isInteger(data.total_count) ? data.total_count : null,
        token_scope_count: count,
        matches,
      };
      const consistency = validateTrayConsistency(observation);
      if (!consistency.ok) {
        return { ...observation, ok: false, error: consistency.reason };
      }
      return observation;
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
  ownedPid = null,
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
  const exitBeforeTerminate = terminal?.kind === "exit";
  let terminateRequested = false;
  let terminateError = null;
  let exitAfterTerminate = false;
  if (!terminal && (timedOut || canceled)) {
    if (ownedPid !== null && child.pid !== ownedPid) {
      terminateError = `拒绝终止非本次拥有的 PID ${child.pid}（期望 ${ownedPid}）`;
    } else {
      terminateRequested = true;
      try {
        if (child.kill() === false) terminateError = "child.kill() 返回 false";
      } catch (error) {
        terminateError = `child.kill() 失败：${String(error?.message ?? error)}`;
      }
    }
    terminal = await Promise.race([terminalPromise, sleepFn(killGraceMs).then(() => null)]);
    exitAfterTerminate = terminal?.kind === "exit";
  }
  const exitConfirmed = terminal?.kind === "exit";
  const exitCode = exitConfirmed ? terminal.code : null;
  const signal = exitConfirmed ? terminal.signal : null;
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
    exitBeforeTerminate,
    exitConfirmed,
    exitAfterTerminate,
    terminateRequested,
    terminateError,
    terminateUnconfirmed: terminateRequested && !exitConfirmed,
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
  canceled = false,
  exitConfirmed = true,
  terminateRequested = false,
  terminateError = null,
  terminateUnconfirmed = false,
} = {}) {
  const budget = Number(budgetMs);
  const elapsed = Number(elapsedMs);
  const cleanup = terminateRequested
    ? terminateError
      ? `终止请求失败：${terminateError}`
      : exitConfirmed
        ? "已发送终止请求并观察到退出"
        : "已发送终止请求但未确认退出"
    : terminateError
      ? `未终止：${terminateError}`
      : "未发送终止请求";
  if (!exitConfirmed) {
    if (timedOut) {
      return {
        status: "failed",
        reason: `第二次启动未在 ${budget} ms 内退出；${cleanup}${terminateUnconfirmed ? "，退出未确认" : ""}`,
      };
    }
    if (canceled) {
      return {
        status: "failed",
        reason: `场景取消；${cleanup}${terminateUnconfirmed ? "，退出未确认" : ""}`,
      };
    }
    return { status: "pending", reason: `未观察到第二次启动的退出事件；${cleanup}` };
  }
  if (timedOut) {
    return { status: "failed", reason: `第二次启动未在 ${budget} ms 内退出；${cleanup}` };
  }
  if (canceled) {
    return { status: "failed", reason: `场景取消；${cleanup}` };
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
  const consistency = validateTrayConsistency(observation);
  if (!consistency.ok) {
    return {
      status: "pending",
      count: null,
      processCount,
      reason: consistency.reason,
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
