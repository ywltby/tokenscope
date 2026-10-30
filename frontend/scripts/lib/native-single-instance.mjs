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

/**
 * `child` 是否等于 `parent` 或位于其下（按规范化绝对路径的分量边界）。
 * `path.relative` 可能返回 `..fixture`（子目录名以 .. 开头，但不是父目录
 * 遍历），因此必须区分 `..` / `..${sep}x` 与 `..name`。
 */
function isWithin(child, parent) {
  const rel = path.relative(comparePath(parent), comparePath(child));
  if (rel === "") return true;
  if (path.isAbsolute(rel)) return false;
  return !(rel === ".." || rel.startsWith(`..${path.sep}`) || rel.startsWith("../"));
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
 * 受保护根预检（单 root + 单受保护目录列表，兼容旧接口）。
 */
export function isIsolatedAcceptanceRoot(root, realDataDir) {
  const result = validateAcceptanceRoot({ root, protectedDirs: [realDataDir] });
  return { ok: result.ok, reason: result.reason };
}

/**
 * 验收构建会读写的全部受保护用户目录（基于用户主目录派生）。
 * 用户主目录无法确定时返回 null（调用方拒绝）。
 */
export function getUserProtectedDirs(homeDir) {
  const dir = String(homeDir ?? "").trim();
  if (!dir || !path.isAbsolute(dir)) return null;
  return [
    path.join(dir, ".tokenscope"),
    path.join(dir, ".claude"),
    path.join(dir, ".codex"),
    path.join(dir, ".cc-switch"),
    path.join(dir, ".workbuddy"),
    path.join(dir, ".workbuddy-ai"),
  ];
}

/**
 * 验收实例会读写的全部路径（隔离根 + 子目录 + manifest + 输出目录）。
 */
export function getAcceptanceWritePaths(root, outDir) {
  const paths = [
    root,
    path.join(root, "tokenscope"),
    path.join(root, "sources"),
    path.join(root, "webview"),
    path.join(root, "manifest.json"),
  ];
  if (outDir) paths.push(outDir);
  return paths;
}

/**
 * 完整路径预检：对验收实例会读写的每个路径（root、root/tokenscope、
 * root/sources、root/webview、root/manifest.json、--out）逐个做真实路径
 * 解析（含 junction/symlink），并与全部受保护用户目录做双向包含检查。
 * 用户目录无法确定时拒绝。
 */
export function validateAcceptancePaths({
  root,
  outDir,
  homeDir,
  canonicalize = canonicalizeAcceptancePath,
} = {}) {
  const protectedDirs = getUserProtectedDirs(homeDir);
  if (!protectedDirs) {
    return { ok: false, reason: "无法确定用户主目录，无法证明来源目录隔离，拒绝启动" };
  }
  if (!protectedDirs.length) {
    return { ok: false, reason: "受保护目录列表为空，无法证明隔离" };
  }

  const rawRoot = String(root ?? "").trim();
  if (!rawRoot) return { ok: false, reason: "隔离根为空" };
  if (!path.isAbsolute(rawRoot)) return { ok: false, reason: `隔离根不是绝对路径：${rawRoot}` };

  const writePaths = getAcceptanceWritePaths(root, outDir);

  // 逐个解析验收写入路径的真实位置
  const canonicalWritePaths = [];
  for (const wp of writePaths) {
    const c = canonicalize(wp);
    if (!c.ok) return { ok: false, reason: `验收路径 ${wp} 无法解析：${c.reason}` };
    canonicalWritePaths.push({ source: wp, path: c.path });
  }

  // 逐个解析受保护目录的真实位置
  const canonicalProtected = [];
  for (const pd of protectedDirs) {
    const c = canonicalize(pd);
    if (!c.ok) return { ok: false, reason: `受保护目录 ${pd} 无法解析：${c.reason}` };
    canonicalProtected.push({ source: pd, path: c.path });
  }

  // 双向包含检查：每个验收路径 vs 每个受保护目录
  for (const wp of canonicalWritePaths) {
    for (const pd of canonicalProtected) {
      if (comparePath(wp.path) === comparePath(pd.path)) {
        return {
          ok: false,
          reason: `验收路径 ${wp.source} 是受保护目录 ${pd.path} 本身`,
        };
      }
      if (isWithin(wp.path, pd.path)) {
        return {
          ok: false,
          reason: `验收路径 ${wp.source} 位于受保护目录 ${pd.path} 内`,
        };
      }
      if (isWithin(pd.path, wp.path)) {
        return {
          ok: false,
          reason: `验收路径 ${wp.source} 是受保护目录 ${pd.path} 的祖先`,
        };
      }
    }
  }

  return {
    ok: true,
    reason: "全部验收路径与受保护用户目录互不包含",
    root: canonicalWritePaths[0]?.path,
    write_paths: canonicalWritePaths.map((p) => p.path),
    protected_dirs: canonicalProtected.map((p) => p.path),
    alias_resolved: canonicalWritePaths.some((p) => p.path !== path.resolve(p.source)),
  };
}

/** 旧接口：单 root + 单受保护目录列表。 */
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
 * EXE 验收构建筛查：目标必须是 PE 文件，包含 acceptance 专用标记，
 * 且不含普通构建 stub 标记。字符串标记仅用于**预筛**，不构成构建身份证明——
 * 构建身份需要与受控构建来源的产物哈希比对（CI 尚未提供），当前结论为
 * `marker-screened`（非 `verified`）。
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
      build_identity: "rejected",
    };
  }
  if (!positive.length) {
    return {
      ok: false,
      reason: "EXE 未包含 acceptance 构建标记；普通 EXE 或无法验证，拒绝启动",
      size: bytes.length,
      markers: [],
      negative_markers: negative,
      build_identity: "rejected",
    };
  }
  return {
    ok: true,
    reason: `EXE 通过 acceptance 字符串标记预筛（命中 ${positive.length} 个；构建身份未验证，仅用于预筛）`,
    size: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
    markers: positive,
    negative_markers: [],
    build_identity: "marker-screened",
  };
}

/**
 * 启动前预检：完整路径隔离（含全部子路径与受保护目录别名解析）与实际 EXE
 * 构建属性。attach 模式不启动 EXE，只校验路径；非 attach 模式两者都必须通过。
 */
export function evaluateAcceptanceLaunchPreflight({
  root,
  outDir,
  exePath,
  homeDir,
  attachExisting = false,
  canonicalize = canonicalizeAcceptancePath,
  readFile = fs.readFileSync,
} = {}) {
  const pathCheck = validateAcceptancePaths({
    root,
    outDir,
    homeDir,
    canonicalize,
  });
  const exeCheck = attachExisting
    ? { ok: true, skipped: true, reason: "attach 模式不启动 EXE", build_identity: "skipped" }
    : verifyAcceptanceExecutable(exePath, { readFile });
  return {
    ok: pathCheck.ok && exeCheck.ok,
    paths: pathCheck,
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
