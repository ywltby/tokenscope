// RC11：原生生产验收驱动器（release + `acceptance` 构建，跑在隔离根里）。
//
// 为什么是 Node + CDP 而不是"只看截图"：原生实例必须证明的是**真实 WebView2 +
// 真实 Rust 后端**下的行为，因此这里启动的是发布构建产物本身，然后用
// playwright-core 通过 CDP 连上它，用真实点击/键盘驱动真实 IPC。
//
// 边界（与 plan 一致，不放宽）：
//   - 只启动/终止本次自己记录的 PID（及其 WebView2 子进程）；
//   - 只读写隔离根，真实 ~/.tokenscope 前后指纹必须完全一致；
//   - CDP 仅用于观测与真实交互；**调试器求值不作为 CSP 证据**（见 §4）；
//   - 不安装、不写注册表、不改系统显示缩放。
// UP09（升级测试补测计划 §4.8）：实例生命周期改为“端口不复用 + 归属核实 + 退出确认”，
// 纯逻辑在 `lib/native-instance.mjs`（可单测），证据在 `evidence.json` 的 `launches[]`。
import { spawn } from "node:child_process";
import crypto from "node:crypto";
import net from "node:net";
import zlib from "node:zlib";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { parse as parseToml } from "smol-toml";
import { parseTOML } from "toml-eslint-parser";
import { nativeUiRefresh } from "./contracts/native-ui-refresh.mjs";
import { nativeCollectionStatus } from "./contracts/native-collection-status.mjs";
import { nativeSourcePaths } from "./contracts/native-source-paths.mjs";
import { nativeIdentity } from "./contracts/native-identity.mjs";
import { nativeCcsImport } from "./contracts/native-ccs-import.mjs";
import { nativePricing, preparePricingScenario } from "./contracts/native-pricing.mjs";
import { nativeUiRelaunch, nativeUiSystem } from "./contracts/native-ui-system.mjs";
import { decodePng } from "./lib/png.mjs";
import {
  startAcceptanceIo,
  readAcceptanceTrace,
  snapshotBusinessFiles,
  changedBusinessFiles,
  proxyEnvironment,
  beginIsolationCleanup,
  terminateAfterSnapshot,
  missingBusinessEvents,
  settingsFingerprint,
} from "./lib/acceptance-io.mjs";
import {
  CDP_READY_BUDGET_MS,
  CONNECT_RETRY_INTERVAL_MS,
  HOST_EXIT_POLL_MS,
  LAUNCH_BUDGET_MS,
  PAGE_TARGET_BUDGET_MS,
  PROBE_SHELL,
  STOP_TIMEOUT_MS,
  BudgetExhaustedError,
  awaitWithHostExit,
  buildLaunchesEvidence,
  connectWithBudget,
  clampTimeoutToBudget,
  classifyPortOwner,
  collectDescendants,
  connectDecision,
  createHostExitSignal,
  createLaunchRecord,
  createProbeFn,
  descendantsAliveByName,
  hostLogTail,
  launchPort,
  listenersOnPort,
  mergePids,
  pageTargetDecision,
  recordConnectAttempt,
  recordOwnerCheck,
  recordProbe,
  remainingBudgetMs,
  stopDecision,
} from "./lib/native-instance.mjs";
// S00（PR3 计划 §9.1）：场景看门狗、有界命令与阶段记录——纯逻辑在 lib，可单测。
import {
  FAILURE_CAPTURE_TIMEOUT_MS,
  closeBrowserBounded,
  createStageTracker,
  ensureNotCanceled,
  errorEvidenceFields,
  finalizeScenarioTimeout,
  runScenarioBounded,
  scenarioBudgetMs,
  spawnSyncBounded,
  withTimeout,
} from "./lib/native-scenario.mjs";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
// 统一 workspace 后，不带 --target 的 release 构建产物落在根 target/。
const EXE_DEFAULT = path.join(REPO, "target", "release", "tokenscope.exe");
/** 当前共同同意修订号：从受测源码提取（单一事实源，不硬编码）。 */
const CURRENT_CONSENT_REVISION = (() => {
  const source = fs.readFileSync(
    path.join(REPO, "crates", "tokenscope-core", "src", "settings.rs"),
    "utf8",
  );
  const stripped = source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/[^\n]*/g, "");
  const match = /^\s*(?:pub\s+)?const\s+PRIVACY_POLICY_REVISION\s*:\s*u8\s*=\s*(\d+)\s*;\s*$/m.exec(
    stripped,
  );
  if (!match) throw new Error("settings.rs 缺少 PRIVACY_POLICY_REVISION 常量");
  return Number(match[1]);
})();
const PREP = path.join(REPO, "scripts", "prepare-native-acceptance.ps1");
const RECORDER = path.join(REPO, "scripts", "capture-window-frames.ps1");
const WINDOW_ACTION = path.join(REPO, "scripts", "native-window-action.ps1");
const REAL_DATA = path.join(process.env.USERPROFILE || "", ".tokenscope");
/** 页面 target 轮询间隔；每次等待都受剩余预算与宿主退出约束（§4.8 第 4 条）。 */
const PAGE_TARGET_POLL_MS = 200;

const argv = process.argv.slice(2);
function arg(name, def = undefined) {
  const i = argv.indexOf(`--${name}`);
  if (i === -1) return def;
  return argv[i + 1];
}
function flag(name) {
  return argv.includes(`--${name}`);
}
const SCENARIO = arg("scenario");
const OUT = path.resolve(
  REPO,
  arg("out", path.join(REPO, "qa-artifacts", "native-recheck-2026-10-08")),
);
const PORT = Number(arg("port", "9411"));
const EXE = arg("exe", EXE_DEFAULT);
const THEME = arg("theme", "system"); // light | dark | system（--force-*-mode 通道）
// 默认使用真实系统 DPI；显式 --scale 只作布局探查，不作为系统缩放验收。
const SCALE = arg("scale") === undefined ? null : Number(arg("scale"));
const EVENTS = Number(arg("events", "4"));
const ROOT = arg("root", "");
const EXPECT_STORED = arg("expect-stored", undefined);
const EXPECT_THEME = arg("expect-theme", null);
// §4.8 第 7 条：多路重启压力场景的次数（只在本地跑，不进 CI）。
const RELAUNCH_CYCLES = Number(arg("relaunch-cycles", "20"));
const RELAUNCH_CYCLES_GIVEN = argv.includes("--relaunch-cycles");

// ── 小工具 ────────────────────────────────────────────────────────
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function nowIso() {
  return new Date().toISOString();
}
function sha256File(p) {
  return crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");
}
/** 目录指纹：相对路径 + 字节数 + mtime（毫秒）。真实数据用它做"未被触碰"证明。 */
function fingerprint(dir) {
  if (!fs.existsSync(dir)) return "<missing>";
  const rows = [];
  const walk = (d, rel) => {
    for (const e of fs
      .readdirSync(d, { withFileTypes: true })
      .sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const abs = path.join(d, e.name);
      const r = `${rel}/${e.name}`;
      if (e.isDirectory()) walk(abs, r);
      else {
        const st = fs.statSync(abs);
        rows.push(`${r}|${st.size}|${Math.round(st.mtimeMs)}`);
      }
    }
  };
  walk(dir, "");
  return rows.sort().join("\n");
}
function hashOfFingerprint(fp) {
  return crypto.createHash("sha256").update(fp).digest("hex").slice(0, 16);
}
function listFiles(dir) {
  const out = [];
  const walk = (d, rel) => {
    for (const e of fs
      .readdirSync(d, { withFileTypes: true })
      .sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const abs = path.join(d, e.name);
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) walk(abs, r);
      else out.push(r);
    }
  };
  walk(dir, "");
  return out;
}
/** 纯 JS 递归拷贝目录，避免 Windows GBK 环境下 Node fs.cpSync C++ 绑定的中文路径乱码缺陷 */
function copyDirSync(src, dst) {
  if (!fs.existsSync(src)) return;
  fs.mkdirSync(dst, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const srcPath = path.join(src, entry.name);
    const dstPath = path.join(dst, entry.name);
    if (entry.isDirectory()) {
      copyDirSync(srcPath, dstPath);
    } else {
      fs.copyFileSync(srcPath, dstPath);
    }
  }
}
function ps(scriptPath, argsArray) {
  // S00（计划 §9.1 第 2 条）：同步命令统一 30 秒上限；动作名取 -Action 参数便于定位。
  // 例外（2026-10-23 更正并登记为待确认的临时例外）：prepare-native-acceptance.ps1
  // 不执行任何构建，只做隔离根文件准备（成功运行低于 10 秒）；main 曾连续两次
  // 在其 CCS fixture 生成（python make_ccs_fixture.py，仅 unified 档）处
  // spawnSync ETIMEDOUT，原因未定位——600 秒上限只是让该偶发不再必然失败，
  // 偏离统一 30 秒缺乏原始依据，是否保留/收紧由用户决定；准备阶段有分阶段
  // 计时与超时输出尾部（见 native-scenario.mjs 与 prepare 脚本取证装配）。
  const action = argsArray.includes("-Action")
    ? `window-${argsArray[argsArray.indexOf("-Action") + 1]}`
    : path.basename(scriptPath);
  const timeoutMs = scriptPath === PREP ? 600000 : undefined;
  return spawnSyncBounded(
    "pwsh",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", scriptPath, ...argsArray],
    timeoutMs ? { action, timeoutMs } : { action },
  );
}

/** 一次 PowerShell 内联调用（只用于结束已记录 PID；其余查询走 probeProcessAndListeners）。 */
function psCommand(script) {
  return spawnSyncBounded(PROBE_SHELL, ["-NoProfile", "-NonInteractive", "-Command", script], {
    action: "powershell-command",
  });
}

/**
 * 一次 PowerShell 调用取回进程表与监听表（§4.8 第 6 条：沿用脚本现有方式，
 * 不另建适配器脚本）。等待受**本次启动剩余预算**约束：自身超时 30 秒取
 * `min(自身超时, 剩余预算)`，预算耗尽时根本不 spawn；宿主退出信号先到时立刻终止本次探测。
 * 返回 `{ ok, processes, listeners, ms, timeout_ms, error }`；
 * 探测失败不抛错，由调用方按“归属未知”处理，耗时进 `launches[]` 证据。
 */
const probeProcessAndListeners = createProbeFn({ spawnFn: spawn });

/**
 * 端口是否有监听者：连得上或超时都算“有”（保守），只有明确的 ECONNREFUSED 才算没有。
 * 这是廉价的每轮检查，进程归属仍由 PowerShell 探测给出。
 */
function portListening(port, timeoutMs = 400) {
  return new Promise((resolve) => {
    const socket = net.connect({ host: "127.0.0.1", port });
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      try {
        socket.destroy();
      } catch {}
      resolve(value);
    };
    socket.setTimeout(timeoutMs, () => finish(true));
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
  });
}

/** 进程是否仍存活（`kill(pid, 0)`；EPERM 说明存在但无权限，仍算存活）。 */
function isProcessAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e?.code === "EPERM";
  }
}

/**
 * `/json/version`：超时取 `min(3000, 剩余预算)`（不再被抬到 300 ms 之外），
 * 宿主退出立即中止；被取消时主动 `abort()`，不让底层请求悬空（§4.8 第 4 条）。
 */
async function fetchJsonVersion(record, { deadlineMs, hostExit }) {
  const timeoutMs = clampTimeoutToBudget({ timeoutMs: 3000, deadlineMs, minMs: 300 });
  if (timeoutMs <= 0) {
    throw new BudgetExhaustedError({
      detail: `本次启动剩余预算已耗尽，不再请求 /json/version（端口 ${record.port}）`,
    });
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await awaitWithHostExit({
      task: async () => {
        const res = await fetch(`http://127.0.0.1:${record.port}/json/version`, {
          signal: controller.signal,
        });
        if (!res.ok) throw new Error(`/json/version 返回 HTTP ${res.status}`);
        return await res.json();
      },
      hostExit,
      timeoutMs,
      timeoutDetail: `/json/version 超过剩余预算 ${timeoutMs} ms（端口 ${record.port}）`,
      // 取消当下就 abort：宿主退出或预算耗尽后请求不再继续挂着（不是等它自己结束）。
      onCancel: () => controller.abort(),
    });
  } finally {
    clearTimeout(timer);
  }
}

/**
 * 展开错误链：undici 的 `fetch failed` 把真实原因放在 `cause`
 * （如 `connect ECONNREFUSED 127.0.0.1:9601`、`operation was aborted due to timeout`），
 * 不展开就会把可重试瞬断误判成致命错误。
 */
function errorChain(error) {
  const parts = [];
  let current = error;
  for (let depth = 0; current && depth < 4; depth++) {
    const text = String(current?.message ?? current);
    if (text && !parts.includes(text)) parts.push(text);
    current = current?.cause;
  }
  return parts.join(" ← ") || "未知错误";
}

/**
 * 在剩余预算内建立 CDP 连接（§4.8 第 4 条）：底层 `connectOverCDP` 拿到同一份超时，
 * 竞速失败后迟到的连接交给 `close()`——既不悬空，也不会被当成新的尝试结果。
 */
function budgetedConnect(record, { deadlineMs, hostExit }) {
  const timeoutMs = clampTimeoutToBudget({
    timeoutMs: remainingBudgetMs(deadlineMs),
    deadlineMs,
    minMs: 300,
  });
  if (timeoutMs <= 0) {
    return Promise.reject(
      new BudgetExhaustedError({
        detail: `本次启动剩余预算已耗尽，不再发起 CDP 连接（端口 ${record.port}）`,
      }),
    );
  }
  return connectWithBudget({
    connect: (url, options) => chromium.connectOverCDP(url, options),
    url: `http://127.0.0.1:${record.port}`,
    timeoutMs,
    hostExit,
    timeoutDetail: `connectOverCDP 超过剩余预算 ${timeoutMs} ms（WebSocket 未建立）`,
  });
}

/** 心跳：挂起点会打印出来，避免"静默卡住"被当成通过。 */
function hb(msg) {
  process.stderr.write(`[native-acceptance ${new Date().toISOString()}] ${msg}
`);
}

/**
 * 首帧取证：连接后立即连续抓真实 WebView 画面帧。
 * 每帧记录相对进程启动的毫秒数，亮度用最小 PNG 解码（非隔行 8bit）算出，
 * 解码失败就记 null 并保留帧文件——不用假数字凑断言。
 */
function pngLuma(buf) {
  try {
    const png = decodePng(buf);
    if (!png) return null;
    const { width: w, height: h, channels, data } = png;
    const stride = w * channels;
    let sum = 0;
    let n = 0;
    for (let y = 0; y < h; y++) {
      const cur = data.subarray(y * stride, (y + 1) * stride);
      for (let x = 0; x < stride; x += channels * 53) {
        sum += 0.2126 * cur[x] + 0.7152 * cur[x + 1] + 0.0722 * cur[x + 2];
        n++;
      }
    }
    return n ? Math.round((sum / n) * 10) / 10 : null;
  } catch {
    return null;
  }
}

async function captureFrames(inst, dir, { durationMs, processStartedAt }) {
  fs.mkdirSync(dir, { recursive: true });
  const frames = [];
  const deadline = Date.now() + durationMs;
  let i = 0;
  while (Date.now() < deadline) {
    const at = Date.now();
    let buf = null;
    try {
      const shot = await Promise.race([
        inst.cdp.send("Page.captureScreenshot", { format: "png" }),
        sleep(1500).then(() => null),
      ]);
      if (shot && shot.data) buf = Buffer.from(shot.data, "base64");
    } catch {}
    if (buf) {
      const file = path.join(dir, `${String(i).padStart(3, "0")}.png`);
      fs.writeFileSync(file, buf);
      frames.push({
        index: i,
        at_ms: at - processStartedAt,
        gap_ms: i ? at - frames[i - 1].capturedAtEpoch : null,
        luma: pngLuma(buf),
        file: file,
        capturedAtEpoch: at,
      });
      i++;
    } else {
      await sleep(80);
    }
  }
  const found = frames.filter((f) => f.luma !== null);
  return {
    count: frames.length,
    decoded: found.length,
    dir,
    first_frames: frames.slice(0, 10).map(({ capturedAtEpoch, ...r }) => r),
    luma_sequence: frames.map((f) => `${f.at_ms}:${f.luma ?? "undecoded"}`),
    first_painted_frame: found[0] ?? null,
    luma_min_after_first: found.length ? Math.min(...found.map((f) => f.luma)) : null,
    luma_max_after_first: found.length ? Math.max(...found.map((f) => f.luma)) : null,
  };
}

/**
 * 一次原生实例：准备隔离根 → 启动 release/exe → CDP 连接。
 *
 * 生命周期按升级矩阵 §4.8：
 *   - 进程内第 n 次启动用 `--port + n`，端口不复用；启动前端口已有监听者即失败并
 *     记录占用进程，不自动换端口；
 *   - 连接前核实端口监听者属于宿主或本次已记录的后代；外来进程立即失败且不结束它；
 *   - 单次启动预算 65 秒（CDP 就绪 45 秒 + 页面 target 20 秒），连接类瞬断每 300 ms
 *     重试，宿主退出立即失败并报告退出码；
 *   - 每次关闭都确认宿主与全部已记录后代退出、端口不再监听（上限 20 秒），超时列出
 *     残留 PID；强制结束只用于失败后的清理，且只针对已记录的 PID；
 *   - 每次启动的证据进 `launches[]`，成败都写出。
 */
class Instance {
  constructor(opts) {
    this.opts = opts;
    this.root = opts.root;
    this.proc = null;
    this.browser = null;
    this.ctx = null;
    this.page = null;
    this.console = [];
    this.violations = [];
    this.logPath = opts.logPath;
    this.launches = [];
    this.launchCount = 0;
    this.current = null;
    this.attached = false;
    this.snapshot = { at: 0, ms: 0, ok: false, processes: [], listeners: [], error: null };
    this.lastConnectError = null;
    // 宿主退出信号（§4.8 第 4 条）：仅启动阶段武装；attach / 收尾阶段为 null
    this.hostExit = null;
    this.budgetArmed = false;
  }
  async prepareRoot() {
    fs.mkdirSync(OUT, { recursive: true });
    if (this.opts.events && this.opts.events > 0) {
      const prepareArgs = ["-Root", this.root, "-Events", String(this.opts.events)];
      if (this.opts.profile) prepareArgs.push("-Profile", this.opts.profile);
      const r = ps(PREP, prepareArgs, { encoding: "utf8" });
      if (r.status !== 0) throw new Error(`prepare failed: ${r.stdout}${r.stderr}`);
    } else if (!fs.existsSync(path.join(this.root, "manifest.json"))) {
      throw new Error(`隔离根缺少 manifest：${this.root}`);
    }
    if (this.opts.mutate) this.opts.mutate(this.root);
    this.prelaunchBusinessFiles = snapshotBusinessFiles(path.join(this.root, "tokenscope"));
    const prelaunchSettingsPath = path.join(this.root, "tokenscope", "settings.toml");
    // 十七轮复核 P2：与 snapshotDataDir 同构（text/size/mtimeMs）。
    const prelaunchStat = fs.existsSync(prelaunchSettingsPath)
      ? fs.statSync(prelaunchSettingsPath)
      : null;
    this.prelaunchSettings = {
      text: prelaunchStat ? fs.readFileSync(prelaunchSettingsPath, "utf8") : null,
      bytesHex: prelaunchStat
        ? fs.readFileSync(prelaunchSettingsPath).toString("hex")
        : null,
      size: prelaunchStat ? prelaunchStat.size : null,
      mtimeMs: prelaunchStat ? prelaunchStat.mtimeMs : null,
    };
    const manifest = JSON.parse(fs.readFileSync(path.join(this.root, "manifest.json"), "utf8"));
    return manifest;
  }
  /** WebView2 启动参数：端口由调用方按「基准端口 + 启动序号」算好后传入。 */
  browserArgs(port) {
    const parts = [`--remote-debugging-port=${port}`];
    if (this.opts.scale !== null) parts.push(`--force-device-scale-factor=${this.opts.scale}`);
    if (this.opts.scheme === "dark") parts.push("--force-dark-mode");
    if (this.opts.scheme === "light") parts.push("--force-light-mode");
    if (this.opts.extraBrowserArgs) parts.push(...this.opts.extraBrowserArgs);
    return parts.join(" ");
  }
  /**
   * 本次启动的等待截止时刻（§4.8 第 4 条）：阶段上限与 65 秒总上限取更严者。
   * 未武装（预检查阶段、场景收尾、attach 模式）返回 null，表示不受启动预算约束。
   */
  budgetDeadline(record = this.current) {
    if (!this.budgetArmed || !record) return null;
    const phase = Number(record.phaseDeadlineMs);
    const total = Number(record.launchDeadlineMs);
    if (!Number.isFinite(phase)) return Number.isFinite(total) ? total : null;
    if (!Number.isFinite(total)) return phase;
    return Math.min(phase, total);
  }
  /** 启动阶段才把宿主退出接入等待；场景收尾解除，避免正常关闭被当成启动失败。 */
  armedHostExit() {
    return this.budgetArmed ? this.hostExit : null;
  }
  /**
   * 宿主退出码（§6.2 行 4）：优先取子进程退出码；退出信号已经判定退出但退出码不可读时
   * 记为 `unknown`——0 与非 0 都必须立即失败，不能被当成"还没退出"继续等。
   */
  hostExitCodeOf(record) {
    this.markHostExit(record);
    if (record.hostExitCode !== null && record.hostExitCode !== undefined)
      return record.hostExitCode;
    if (record.hostExitDetected) return record.hostExitDetected.code ?? "unknown";
    return null;
  }
  /** 解除宿主退出监听（预检查阶段、场景收尾与 attach 模式）。 */
  disarmHostWatch() {
    this.budgetArmed = false;
    try {
      this.hostExit?.stop();
    } catch {}
    this.hostExit = null;
  }
  /**
   * 可中断的短等待（重试间隔、页面 target 轮询）：等待时长先按本次启动剩余预算截断，
   * 宿主退出时立刻抛出带退出码的错误，而不是等满这一轮（§4.8 第 4 条）。
   * 竞速计时与等待目标同刻：同刻触发只说明"睡够了"，由调用方决策点判定失败。
   */
  async waitOrExit(record, ms) {
    const remaining = remainingBudgetMs(this.budgetDeadline(record));
    const wanted = Math.max(0, Math.round(Number(ms) || 0));
    const waitMs = Number.isFinite(remaining) ? Math.max(0, Math.min(wanted, remaining)) : wanted;
    if (waitMs <= 0) return; // 预算耗尽：不再等待，由调用方决策点判定失败
    try {
      await awaitWithHostExit({
        task: () => sleep(waitMs),
        hostExit: this.armedHostExit(),
        timeoutMs: waitMs,
        timeoutDetail: `等待 ${waitMs} ms 超过本次启动剩余预算`,
      });
    } catch (error) {
      if (error?.abortedBy === "host-exit") throw error;
      // 竞速超时说明预算已到底：视作等待结束，由调用方决策点判定失败
    }
  }
  /** 等待被中止（宿主退出 / 预算耗尽）时的最终失败：宿主退出带退出码，预算耗尽按原失败处理。 */
  abortFailure(record, error, { budgetMs } = {}) {
    const decision = connectDecision({
      error: null,
      hostExitCode: this.hostExitCodeOf(record),
      ownerKind: "none",
      elapsedMs: Date.now() - record.startedAtMs,
      budgetMs,
    });
    const detail = decision.action === "fail" ? decision.detail : String(error?.message ?? error);
    record.failure = detail;
    return new Error(detail);
  }
  /** 启动失败的统一收尾：写明原因、清理本次记录到的 PID，再把原错误抛给上层。 */
  async failLaunch(record, error) {
    record.stageMs.total = Date.now() - record.startedAtMs;
    this.markHostExit(record);
    const detail = String(error?.message ?? error);
    if (!record.failure) record.failure = detail;
    hb(`启动失败：${detail}`);
    await this.cleanupFailedLaunch(record);
    return error instanceof Error ? error : new Error(detail);
  }
  /**
   * 进程表 + 监听表快照：`maxAgeMs` 内复用，避免每次重试都重启 PowerShell。
   * `deadlineMs` 显式给出时用它（例如启动前的端口预检查也要压在 65 秒总上限内）。
   */
  async probe({ maxAgeMs = 1000, force = false, deadlineMs = null } = {}) {
    const now = Date.now();
    if (!force && this.snapshot.at && now - this.snapshot.at <= maxAgeMs) return this.snapshot;
    const fresh = await probeProcessAndListeners({
      deadlineMs: deadlineMs ?? this.budgetDeadline(),
      hostExit: this.armedHostExit(),
    });
    this.snapshot = { ...fresh, at: Date.now() };
    return this.snapshot;
  }
  /**
   * 刷新并累计本次已记录的后代（按父 PID 递归），同时把探测耗时写进证据。
   * 宿主已退出后**不再**从 hostPid 走后代树：该 PID 可能已被系统进程复用，
   * 从复用者出发会把整棵系统进程树记成"后代"（PR #14 CI 实测：runner 上
   * 宿主 pid 752 干净退出后被复用，退出确认把 126 个系统进程当残留）。
   * 既有累计清单保留，由 stop() 的同名身份校验过滤。
   */
  async refreshDescendants(record) {
    const snap = await this.probe({ force: true });
    const hostGone = record.hostExitCode !== null && record.hostExitCode !== undefined;
    if (!hostGone) {
      record.descendantPids = mergePids(
        record.descendantPids,
        collectDescendants(snap.processes, record.hostPid),
      );
    }
    recordProbe(record, {
      atMs: Date.now() - record.startedAtMs,
      ms: snap.ms,
      ok: snap.ok,
      error: snap.error,
      listenerPids: listenersOnPort(snap.listeners, record.port),
    });
    return snap;
  }
  /**
   * 存活后代判定带进程名身份校验：pid 只在当前名字仍是本应用家族
   * （tokenscope / msedgewebview2）时才认——PID 复用后新名字不匹配，
   * 不再把无关进程当成"退出未确认"的残留。探测清单里查不到的 PID
   * 保守按存活处理（不因探测缺口漏掉真实后代）。
   */
  aliveDescendantPidsOf(record) {
    return descendantsAliveByName(record.descendantPids, this.snapshot.processes, isProcessAlive);
  }
  markHostExit(record) {
    const code = this.proc?.exitCode;
    if (code !== null && code !== undefined) record.hostExitCode = code;
  }
  /**
   * 端口归属核实（§4.8 第 3 条）：刷新后代记录并判定监听该端口的进程是不是自己人。
   * 探测失败按“无监听者”处理——宁可继续等，也不连一个没核实过归属的端点。
   */
  async ownerOf(record, { maxAgeMs = 1000, force = false } = {}) {
    const snap = await this.probe({ maxAgeMs, force });
    record.descendantPids = mergePids(
      record.descendantPids,
      collectDescendants(snap.processes, record.hostPid),
    );
    const owner = classifyPortOwner({
      listenerPids: listenersOnPort(snap.listeners, record.port),
      hostPid: record.hostPid,
      descendantPids: record.descendantPids,
    });
    recordProbe(record, {
      atMs: Date.now() - record.startedAtMs,
      ms: snap.ms,
      ok: snap.ok,
      error: snap.error,
      listenerPids: owner.listenerPids,
    });
    recordOwnerCheck(record, { atMs: Date.now() - record.startedAtMs, owner, probe_ms: snap.ms });
    if (!snap.ok) {
      return {
        ...owner,
        kind: "none",
        detail: `${owner.detail}（探测失败：${snap.error ?? "未知"}）`,
      };
    }
    return owner;
  }
  /**
   * 连接 CDP：每次尝试前核实宿主存活与端口归属（§4.8 第 3、4 条）。
   * 端口无监听者（ECONNREFUSED）继续等；外来监听者立即失败；连接类瞬断每 300 ms 重试；
   * 宿主退出立即失败并报告退出码；预算用尽按原失败处理。
   */
  async connectCdp(record, { deadlineMs, phase = "cdp" }) {
    // 本阶段的截止时刻写进记录：探测、等待与连接共用同一上限（§4.8 第 4 条）
    record.phaseDeadlineMs = Number.isFinite(Number(record.launchDeadlineMs))
      ? Math.min(deadlineMs, Number(record.launchDeadlineMs))
      : deadlineMs;
    // 等待期间定期重新核实端口归属（§4.8 第 4 条：重试前确认宿主存活、端口归属不变）
    const ownershipRefreshMs = 8000;
    let lastOwnerCheckAt = 0;
    while (true) {
      const elapsedMs = Date.now() - record.startedAtMs;
      const budgetMs = deadlineMs - record.startedAtMs;
      const hostExitCode = this.hostExitCodeOf(record);
      if (record.spawnError) {
        record.failure = `进程启动失败：${record.spawnError}`;
        throw new Error(record.failure);
      }
      // 决策点 1：宿主退出立即失败、预算用尽按原失败、上次错误不可重试即失败
      let decision = connectDecision({
        error: this.lastConnectError,
        hostExitCode,
        ownerKind: "none",
        elapsedMs,
        budgetMs,
      });
      if (decision.action === "fail") {
        record.failure = decision.detail;
        throw new Error(decision.detail);
      }
      if (Date.now() - lastOwnerCheckAt >= ownershipRefreshMs) {
        lastOwnerCheckAt = Date.now();
        const periodic = await this.ownerOf(record, { maxAgeMs: 500 });
        if (periodic.kind === "foreign") {
          const foreign = connectDecision({
            ownerKind: "foreign",
            ownerDetail: periodic.detail,
            elapsedMs: Date.now() - record.startedAtMs,
            budgetMs,
          });
          record.failure = foreign.detail;
          throw new Error(foreign.detail);
        }
      }
      // 端点是否已经响应：拒绝连接说明还没有监听者，继续等（不额外重启 PowerShell）
      let version = null;
      try {
        version = await fetchJsonVersion(record, {
          deadlineMs: this.budgetDeadline(record),
          hostExit: this.armedHostExit(),
        });
      } catch (error) {
        const message = errorChain(error);
        this.lastConnectError = message;
        recordConnectAttempt(record, {
          atMs: elapsedMs,
          phase: "endpoint",
          ok: false,
          error: message,
          aborted: error?.abortedBy ?? null,
        });
        // 宿主退出 / 预算耗尽：不重试，直接给出带退出码或预算的失败
        if (error?.abortedBy) throw this.abortFailure(record, error, { budgetMs });
        await this.waitOrExit(record, CONNECT_RETRY_INTERVAL_MS);
        continue;
      }
      // 决策点 2：连接前核实监听者必须是宿主或本次已记录的后代
      let owner = await this.ownerOf(record, { maxAgeMs: 1000 });
      if (owner.kind === "none") owner = await this.ownerOf(record, { force: true });
      lastOwnerCheckAt = Date.now();
      decision = connectDecision({
        error: this.lastConnectError,
        hostExitCode: this.hostExitCodeOf(record),
        ownerKind: owner.kind,
        ownerDetail: owner.detail,
        elapsedMs: Date.now() - record.startedAtMs,
        budgetMs,
      });
      if (decision.action === "fail") {
        record.failure = decision.detail;
        throw new Error(decision.detail);
      }
      if (owner.kind === "none") {
        // 端点已响应但监听者还没核实成自己人：继续等，不连未核实的端点
        await this.waitOrExit(record, CONNECT_RETRY_INTERVAL_MS);
        continue;
      }
      const attemptStartedAt = Date.now();
      try {
        const browser = await budgetedConnect(record, {
          deadlineMs: this.budgetDeadline(record),
          hostExit: this.armedHostExit(),
        });
        this.browser = browser;
        this.ctx = browser.contexts()[0];
        this.lastConnectError = null;
        recordConnectAttempt(record, {
          atMs: elapsedMs,
          phase,
          ok: true,
          ms: Date.now() - attemptStartedAt,
        });
        return version;
      } catch (error) {
        const ms = Date.now() - attemptStartedAt;
        if (error?.abortedBy) {
          // 被取消的尝试单独留证据：底层连接已由 onAbandon 关闭，不作为新的尝试结果
          recordConnectAttempt(record, {
            atMs: elapsedMs,
            phase,
            ok: false,
            error: errorChain(error),
            ms,
            aborted: error.abortedBy,
          });
          throw this.abortFailure(record, error, { budgetMs });
        }
        this.lastConnectError = errorChain(error);
        recordConnectAttempt(record, {
          atMs: elapsedMs,
          phase,
          ok: false,
          error: this.lastConnectError,
          ms,
        });
        await this.waitOrExit(record, CONNECT_RETRY_INTERVAL_MS);
      }
    }
  }
  /** 启动失败后的清理：只针对已记录的 PID（§4.8 第 2 条）。 */
  async cleanupFailedLaunch(record) {
    try {
      await this.refreshDescendants(record);
    } catch {}
    await this.terminateRecordedPids(record, "launch-failure-cleanup");
    // S00：CDP 关闭 10 秒上限，超时继续进程终止流程，不在清理里挂死
    await closeBrowserBounded(this.browser);
    this.browser = null;
    this.ctx = null;
  }
  /**
   * 结束记录中的 PID（宿主 + 已记录后代）；绝不动外来进程。宿主已退出后
   * 不再杀 hostPid（该 PID 可能已被复用），且后代按最近快照做进程名身份
   * 校验——复用 PID 的"假后代"不得进入强杀清单。
   */
  async terminateRecordedPids(record, reason) {
    let snap = this.snapshot;
    if (snap.at === 0) {
      try {
        snap = await this.probe({ force: true });
      } catch {
        /* 探测失败按"身份未知"保守处理，但宿主退出后的 hostPid 仍不杀 */
      }
    }
    const hostGone = record.hostExitCode !== null && record.hostExitCode !== undefined;
    const hostPids = hostGone ? [] : [record.hostPid];
    const oursDescendants = descendantsAliveByName(
      record.descendantPids ?? [],
      snap.processes,
      () => true,
    );
    const pids = mergePids(hostPids, oursDescendants).filter(
      (pid) => pid !== process.pid && pid > 4,
    );
    record.terminated = record.terminated ?? [];
    if (!pids.length) {
      record.terminated.push({ at: nowIso(), reason, pids: [] });
      return { reason, pids: [] };
    }
    try {
      psCommand(
        `@(${pids.join(",")}) | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }`,
      );
    } catch (e) {
      // S00：终止命令本身超时也不能阻断收尾——记录后继续退出确认（有界）
      record.terminate_error = String(e?.message ?? e);
      hb(`终止已记录 PID 失败：${record.terminate_error}`);
    }
    record.terminated.push({ at: nowIso(), reason, pids });
    return { reason, pids };
  }
  /**
   * 启动一次实例（§4.8 第 1–5 条）。端口不接受参数：由 `--port + 启动序号` 计算。
   * 返回 `{ pid, port, launchIndex, browserArgs, cdpVersion }`。
   */
  async launch() {
    if (arguments.length) {
      throw new Error(
        "launch() 不接受端口参数：端口由 --port + 启动序号 计算（升级矩阵 §4.8 第 1 条）",
      );
    }
    // S00（计划 §9.1 第 6 条）：场景超时后迟到的分支不得再次 launch。
    ensureNotCanceled(this.opts.isCanceled, "launch");
    const launchStage = this.launchCount >= 1 ? "relaunch" : "launch";
    this.opts.markStage?.(launchStage);
    if (this.current && !this.current.stop) {
      // 上一个实例尚未确认退出：先按同一套关闭流程确认（同一隔离根不能有两份实例）
      await this.stop({ terminate: false, reason: "launch-requires-previous-exit" });
    }
    const index = ++this.launchCount;
    const port = launchPort(PORT, index);
    const browserArgs = this.browserArgs(port);
    const record = createLaunchRecord({
      index,
      port,
      root: this.root,
      logPath: this.logPath,
      browserArgs,
      startedAtMs: Date.now(),
    });
    // 启动预算（§4.8 第 4 条）：总上限 65 秒，阶段上限随后收窄；一切等待都不得超过二者
    record.launchDeadlineMs = record.startedAtMs + LAUNCH_BUDGET_MS;
    record.phaseDeadlineMs = record.startedAtMs + CDP_READY_BUDGET_MS;
    this.launches.push(record);
    this.current = record;
    // 上一轮的宿主退出监听不跨启动复用
    this.disarmHostWatch();

    // 启动前端口不得已有监听者：失败并记录占用进程，不自动换端口（§4.8 第 1 条）
    if (await portListening(port)) {
      // 预检查发生在 spawn 之前，但探测同样压在 65 秒总上限内，不额外占用启动预算
      const snap = await this.probe({ force: true, deadlineMs: record.launchDeadlineMs });
      const occupiers = listenersOnPort(snap.listeners, port);
      recordProbe(record, {
        atMs: 0,
        ms: snap.ms,
        ok: snap.ok,
        error: snap.error,
        listenerPids: occupiers,
      });
      record.precheck = { port_free: false, occupier_pids: occupiers, probe_ok: snap.ok };
      const detail = occupiers.length
        ? `占用进程 PID ${occupiers.join(",")}`
        : `探测不到占用进程（探测失败：${snap.error ?? "未知"}）`;
      record.failure = `启动前端口 ${port} 已有监听者：${detail}`;
      hb(`启动失败：${record.failure}（不自动换端口）`);
      throw new Error(record.failure);
    }
    record.precheck = { port_free: true, occupier_pids: [], probe_ok: true };

    const env = {
      ...process.env,
      // 固定隔离通道每次启动都装配；场景可以替换 env，不能因此丢失计数代理。
      ...this.opts.controlledIo?.env,
      TOKENSCOPE_ACCEPTANCE_ROOT: this.root,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArgs,
      ...this.opts.env,
    };
    const out = fs.openSync(this.logPath, "a");
    this.startedAt = Date.now();
    this.proc = spawn(EXE, [], { env, stdio: ["ignore", out, out], detached: false });
    record.hostPid = this.proc.pid;
    record.stageMs.spawn = Date.now() - this.startedAt;
    record.spawnError = null;
    this.proc.on("error", (e) => {
      record.spawnError = String(e?.message ?? e);
    });
    // 宿主退出接入全部等待（§4.8 第 4 条）：子进程 `exit` 事件与 250 ms 存活轮询取先到者，
    // 探测、`/json/version`、`connectOverCDP`、重试间隔与页面 target 轮询都能在数百毫秒内中止。
    this.hostExit = createHostExitSignal({
      isAlive: () => isProcessAlive(record.hostPid),
      readExitCode: () => {
        this.markHostExit(record);
        return record.hostExitCode;
      },
      intervalMs: HOST_EXIT_POLL_MS,
      onExit: (info) => {
        record.hostExitDetected = {
          code: info.code,
          at_ms: Math.round(info.atMs - record.startedAtMs),
          source: info.source,
        };
      },
    });
    this.budgetArmed = true;
    this.proc.on("exit", (code) => {
      record.hostExitCode = code;
      this.hostExit?.notify(code);
    });
    this.hostExit.start();
    hb(
      `launch #${index} PID=${record.hostPid} port=${port}；预算 ${LAUNCH_BUDGET_MS / 1000} s（CDP ${CDP_READY_BUDGET_MS / 1000} s + 页面 ${PAGE_TARGET_BUDGET_MS / 1000} s）`,
    );

    const cdpDeadline = record.startedAtMs + CDP_READY_BUDGET_MS;
    const totalDeadline = record.startedAtMs + LAUNCH_BUDGET_MS;
    let version;
    try {
      version = await this.connectCdp(record, { deadlineMs: cdpDeadline, phase: "cdp" });
    } catch (error) {
      throw await this.failLaunch(record, error);
    }
    record.stageMs.cdp_ready = Date.now() - record.startedAtMs;
    hb("CDP 就绪，等待应用页面 target");

    // WebView2 先给出 about:blank，再挂上 tauri://localhost；直接取 pages[0]
    // 会连到已销毁的旧 target（实测：重启后拿到已销毁的旧 target）。
    const pagePhaseStartedAt = Date.now();
    const pageDeadline = Math.min(pagePhaseStartedAt + PAGE_TARGET_BUDGET_MS, totalDeadline);
    // 页面阶段的等待同样受该阶段上限与 65 秒总上限约束（§4.8 第 4 条）
    record.phaseDeadlineMs = pageDeadline;
    this.page = null;
    while (true) {
      let urls = [];
      try {
        urls = this.ctx ? this.ctx.pages().map((p) => p.url()) : [];
      } catch (error) {
        // target 被销毁：按瞬断处理，仍在总预算内时重连（§4.8 第 4 条）
        recordConnectAttempt(record, {
          atMs: Date.now() - record.startedAtMs,
          phase: "targets",
          ok: false,
          error: errorChain(error),
        });
        try {
          await this.connectCdp(record, { deadlineMs: totalDeadline, phase: "reconnect" });
          // 重连会改写阶段截止；页面阶段仍按自己的上限计时
          record.phaseDeadlineMs = pageDeadline;
          continue;
        } catch (reconnectError) {
          throw await this.failLaunch(record, reconnectError);
        }
      }
      record.pageTargets = urls;
      const decision = pageTargetDecision({
        hostExitCode: this.hostExitCodeOf(record),
        urls,
        elapsedMs: Date.now() - pagePhaseStartedAt,
        budgetMs: pageDeadline - pagePhaseStartedAt,
      });
      if (decision.action === "found") {
        this.page = this.ctx.pages().find((p) => p.url() === decision.target) ?? null;
        if (this.page) break;
      }
      if (decision.action === "fail") {
        throw await this.failLaunch(record, new Error(decision.detail));
      }
      try {
        // 宿主退出时立刻抛出带退出码的错误，不等满这一轮（§4.8 第 4 条）
        await this.waitOrExit(record, PAGE_TARGET_POLL_MS);
      } catch (error) {
        throw await this.failLaunch(record, error);
      }
    }
    record.pageTargetUrl = this.page.url();
    record.stageMs.page_target = Date.now() - pagePhaseStartedAt;
    record.stageMs.total = Date.now() - record.startedAtMs;
    this.page.setDefaultTimeout(20000);
    hb("已连接页面 " + this.page.url());
    this.cdp = await this.ctx.newCDPSession(this.page);
    this.page.on("console", (m) => {
      const t = `${m.type()}: ${m.text()}`;
      this.console.push(t);
      if (/Content Security Policy|Refused to/i.test(m.text())) this.violations.push(t);
    });
    this.page.on("pageerror", (e) => this.console.push(`pageerror: ${e.message}`));
    this.cdp.on("Log.entryAdded", (e) => {
      const t = `${e.entry.source}:${e.entry.level}: ${e.entry.text}`;
      this.console.push(t);
      if (/Content Security Policy|Refused to/i.test(e.entry.text)) this.violations.push(t);
    });
    await this.cdp.send("Log.enable").catch(() => {});
    this.opts.markStageDone?.(launchStage);
    return {
      pid: this.proc.pid,
      port,
      launchIndex: index,
      browserArgs,
      cdpVersion: version?.["Browser"] ?? version?.browser ?? "unknown",
    };
  }
  async waitForData(timeoutMs = 30000) {
    await this.page.waitForSelector(".app-nav", { timeout: timeoutMs });
    await this.page.waitForFunction(
      () =>
        document.querySelectorAll(".ts-skeleton, .n-skeleton").length === 0 &&
        !!document.querySelector(".chart-canvas svg"),
      null,
      { timeout: timeoutMs, polling: 250 },
    );
  }
  /** 读取宿主客户区物理尺寸；CDP window bounds 不是 Tauri 宿主窗口。 */
  async windowInfo() {
    try {
      const w = this.nativeGeometry();
      const dpr = await this.page.evaluate(() => devicePixelRatio);
      return {
        logical: `${w.width / dpr}x${w.height / dpr} CSS px (client)`,
        dpr,
        physical: `${w.width}x${w.height} px (client)`,
      };
    } catch (e) {
      return { logical: `error:${e.message}`, dpr: null, physical: "unknown" };
    }
  }
  nativeGeometry(action = "geometry", extra = []) {
    const result = spawnSyncBounded(
      "pwsh",
      [
        "-NoProfile",
        "-File",
        WINDOW_ACTION,
        "-ProcessId",
        String(this.proc.pid),
        "-Action",
        action,
        ...extra,
      ],
      { action: `window-action:${action}` },
    );
    if (result.status !== 0) throw new Error(`Native geometry: ${result.stderr}`);
    return JSON.parse(result.stdout);
  }
  /** 调整真实宿主客户区；CDP setWindowBounds 在 WebView2 中只会移动内嵌视图。 */
  async setWindowSize(width, height) {
    const before = this.nativeGeometry();
    const dpr = await this.page.evaluate(() => devicePixelRatio);
    const prev = { width: before.width / dpr, height: before.height / dpr };
    this.nativeGeometry("resize", [
      "-Width",
      String(Math.round(width * dpr)),
      "-Height",
      String(Math.round(height * dpr)),
    ]);
    await sleep(800);
    await this.assertHostFilled();
    return prev;
  }
  async assertHostFilled() {
    const client = this.nativeGeometry();
    const view = await this.page.evaluate(() => ({
      width: innerWidth,
      height: innerHeight,
      dpr: devicePixelRatio,
    }));
    if (
      Math.abs(client.width - view.width * view.dpr) > 2 ||
      Math.abs(client.height - view.height * view.dpr) > 2
    )
      throw new Error(`WebView does not fill native client: ${JSON.stringify({ client, view })}`);
    return { client, view };
  }
  /**
   * 挂到一个已经在跑的实例上（`expire-finish`）：只连接，不认领、不结束它。
   * §4.8：attach 模式不认领、不结束外部实例。
   */
  async attachToExisting(state) {
    this.attached = true;
    // attach 模式不认领、不结束外部实例，也不把启动预算与宿主退出监听强加到它身上
    this.disarmHostWatch();
    const record = createLaunchRecord({
      index: 0,
      port: state.port,
      root: this.root,
      logPath: this.logPath,
      startedAtMs: Date.now(),
    });
    record.attached = true;
    record.hostPid = state.pid;
    record.stageMs.total = null;
    this.launches.push(record);
    this.current = record;
    this.proc = { pid: state.pid, exitCode: null };
    const browser = await chromium.connectOverCDP(`http://127.0.0.1:${state.port}`);
    this.browser = browser;
    this.ctx = browser.contexts()[0];
    record.pageTargets = this.ctx.pages().map((p) => p.url());
    this.page = this.ctx.pages().find((p) => /tauri/i.test(p.url())) ?? this.ctx.pages()[0];
    record.pageTargetUrl = this.page?.url() ?? null;
    this.cdp = await this.ctx.newCDPSession(this.page);
    this.console = [];
    this.violations = [];
    this.opts = {
      ...this.opts,
      idleNote: `started_at=${state.at} planned=${state.idleDeadlineIso}`,
    };
    return record;
  }
  /**
   * 关闭并确认退出（§4.8 第 2 条）：宿主与全部已记录后代退出、端口不再监听，上限 20 秒；
   * 超时失败并列出残留 PID；强制结束只用于失败后的清理，且只针对已记录的 PID。
   *
   * `terminate: true`（默认，场景结束时）：先把本次自己启动的宿主与已记录后代关掉，
   * 再执行同一套退出确认——pkill 只针对本次记录的 PID，绝不动外来进程。
   * `terminate: false`（重启前）：场景已经让宿主自己退出，这里只做确认，宿主仍在
   * 运行即视为失败（不替它做优雅退出）。
   */
  async stop({ terminate = true, reason = "scenario-end" } = {}) {
    // S00（计划 §9.1 第 3、6 条）：进入停止阶段即记录；场景超时后迟到的分支不得继续关闭
    ensureNotCanceled(this.opts.isCanceled, "stop");
    this.opts.markStage?.("stop");
    const record = this.current;
    if (!record) return { action: "skipped", detail: "本次没有启动实例" };
    if (record.stop) return record.stop;
    // 收尾阶段解除启动预算与宿主退出监听：正常关闭（宿主已退出）不能被当成启动失败
    this.disarmHostWatch();
    // S00（计划 §6 S00 第 2 条）：CDP 关闭 10 秒上限，超时记入停止证据后继续既有流程
    const browserClose = await closeBrowserBounded(this.browser);
    if (browserClose.reason !== null && browserClose.reason !== "no-browser") {
      hb(`停止阶段 CDP 关闭未完成（${browserClose.reason}），继续退出确认与终止流程`);
    }
    this.browser = null;
    this.ctx = null;
    if (this.attached) {
      // attach 模式（expire-finish）：不认领、不结束外部实例，只断开 CDP
      record.stop = {
        at: nowIso(),
        reason,
        action: "attach-external-untouched",
        kind: "attach-external-untouched",
        elapsedMs: 0,
        residual_pids: [],
        port_owner_pids: [],
        host_exit_code: record.hostExitCode,
        detail: "attach 模式：不认领、不结束外部实例",
      };
      this.opts.markStageDone?.("stop");
      return record.stop;
    }
    const startedAt = Date.now();
    if (!record.hostPid) {
      // 启动前就被端口占用、或者进程根本没创建：没有宿主可等，也不该等别人的端口
      record.stop = {
        at: nowIso(),
        reason,
        action: "never-started",
        kind: "no-host-process",
        elapsedMs: 0,
        residual_pids: [],
        port_owner_pids: [...record.portOwnerPids],
        host_exit_code: record.hostExitCode,
        detail: "本次启动没有宿主进程（端口预占或进程未创建），不做退出等待、不动他人进程",
      };
      this.opts.markStageDone?.("stop");
      return record.stop;
    }
    await this.refreshDescendants(record);
    this.markHostExit(record);
    if (terminate) await this.terminateRecordedPids(record, `${reason}-terminate`);
    let decision;
    while (true) {
      const elapsedMs = Date.now() - startedAt;
      this.markHostExit(record);
      const hostExited = !(record.hostExitCode === null || record.hostExitCode === undefined);
      const aliveDescendantPids = this.aliveDescendantPidsOf(record);
      const listening = await portListening(record.port);
      decision = stopDecision({
        hostPid: record.hostPid,
        hostExited,
        aliveDescendantPids,
        portListening: listening,
        elapsedMs,
        timeoutMs: STOP_TIMEOUT_MS,
      });
      if (decision.action !== "wait") break;
      await sleep(250);
    }
    let ownerPids = [...record.portOwnerPids];
    if (decision.action === "timeout") {
      // 失败证据要说清端口是谁在占：只重新探测一次，不改动外来进程
      const finalSnap = await this.probe({ force: true });
      ownerPids = listenersOnPort(finalSnap.listeners, record.port);
      record.descendantPids = mergePids(
        record.descendantPids,
        collectDescendants(finalSnap.processes, record.hostPid),
      );
      recordProbe(record, {
        atMs: Date.now() - record.startedAtMs,
        ms: finalSnap.ms,
        ok: finalSnap.ok,
        error: finalSnap.error,
        listenerPids: ownerPids,
      });
    }
    record.stop = {
      at: nowIso(),
      reason,
      terminate,
      action: decision.action,
      kind: decision.kind,
      elapsedMs: Date.now() - startedAt,
      residual_pids: decision.residualPids,
      port_owner_pids: ownerPids,
      host_exit_code: record.hostExitCode,
      browser_close: browserClose,
      detail: decision.detail,
    };
    if (decision.action === "timeout") {
      await this.terminateRecordedPids(record, `${reason}-timeout-cleanup`);
      hb(`退出确认失败：${decision.detail}`);
      throw new Error(`${decision.detail}（reason=${reason}）`);
    }
    hb(`退出确认通过：${decision.detail}`);
    this.opts.markStageDone?.("stop");
    return record.stop;
  }
  /** 证据用：宿主日志尾部（进程日志由本脚本按次追加）。 */
  logTail() {
    try {
      return fs.existsSync(this.logPath)
        ? hostLogTail(fs.readFileSync(this.logPath, "utf8"))
        : null;
    } catch (e) {
      return `读取宿主日志失败：${String(e?.message ?? e)}`;
    }
  }
}

/**
 * 已定位的 CSP 噪声：naive-ui 2.45.3 在 tree/utils 模块顶层 new Image() 预热一张
 * 1x1 data-URI gif，该 `emptyImage` 在包内没有任何读取点（上游死代码）。生产 CSP 的
 * `img-src 'self'` 会拦下它——功能无影响，但**不为此放宽 CSP**，只单独计数。
 */
const KNOWN_CSP_NOISE = /Loading the image 'data:image\/gif;base64,R0lGODlhAQAB/i;
function splitViolations(list) {
  const known = [];
  const unexpected = [];
  for (const v of list) (KNOWN_CSP_NOISE.test(v) ? known : unexpected).push(v);
  return { known, unexpected };
}

// ── 量测助手 ──────────────────────────────────────────────────────
class Checks {
  constructor() {
    this.rows = [];
  }
  add(name, ok, detail = "") {
    this.rows.push({ name, ok: !!ok, detail: String(detail).slice(0, 300) });
    return this;
  }
  get failures() {
    return this.rows.filter((r) => !r.ok);
  }
}

async function boxOf(page, selector) {
  const el = page.locator(selector).first();
  if (!(await el.count())) return null;
  return await el.boundingBox();
}

/** 费用浮层：真实键盘遍历到触发器 → Enter 固定 → 量四边 → Escape 关闭。 */
async function measureCostPopover(page, vp) {
  const out = { steps: [] };
  const trigger = page.locator(".cost-trigger").first();
  if (!(await trigger.count())) return { ...out, available: false };
  // 真实 Tab 遍历（最多 60 次）抵达费用触发器
  await page.evaluate(() => document.body.focus());
  let reached = false;
  for (let i = 0; i < 60; i++) {
    await page.keyboard.press("Tab");
    const isTrigger = await page.evaluate(() =>
      document.activeElement?.classList?.contains("cost-trigger"),
    );
    if (isTrigger) {
      reached = true;
      out.tabPresses = i + 1;
      break;
    }
  }
  out.keyboardReachedTrigger = reached;
  if (!reached) return { ...out, available: false };
  out.accessibleName = await page.evaluate(
    () => document.activeElement?.getAttribute("aria-label") ?? "",
  );
  const focusRing = await page.evaluate(() => {
    const cs = getComputedStyle(document.activeElement);
    return {
      outlineWidth: cs.outlineWidth,
      outlineStyle: cs.outlineStyle,
      boxShadow: cs.boxShadow.slice(0, 80),
    };
  });
  out.focusRing = focusRing;
  await page.keyboard.press("Enter");
  await page.waitForTimeout(450);
  out.ariaExpanded = await page.evaluate(
    () => document.activeElement?.getAttribute("aria-expanded") ?? null,
  );
  const tip = page.locator(".n-popover:visible, .n-tooltip:visible").first();
  out.popoverPresent = (await tip.count()) > 0;
  if (out.popoverPresent) {
    const b = await tip.boundingBox();
    out.popoverBox = b;
    out.fourEdgesInside =
      b && b.x >= 0 && b.y >= 0 && b.x + b.width <= vp.width + 1 && b.y + b.height <= vp.height + 1;
    const fullText = await tip.innerText();
    out.text = fullText.replace(/\s+/g, " ").slice(0, 200);
    out.hasUnitDimension = /USD\s*\/\s*1M|每百万 token/.test(fullText);
  }
  await page.keyboard.press("Escape");
  await page.waitForTimeout(350);
  out.closedByEscape = (await page.locator(".n-popover:visible, .n-tooltip:visible").count()) === 0;
  // 移开鼠标，避免 hover 强调影响后续截图指纹
  await page.mouse.move(2, 2);
  return out;
}

/** 日期外壳：单层玻璃契约（外壳 0.85 + blur16 + 12px，内层透明）。 */
async function measureDateShell(page) {
  const out = {};
  const trigger = page.locator(".range-trigger").first();
  if (!(await trigger.count())) return { available: false };
  await trigger.click();
  await page.waitForTimeout(500);
  out.shell = await page.evaluate(() => {
    const shell = document.querySelector(".n-popover");
    if (!shell) return null;
    const cs = getComputedStyle(shell);
    const inner = shell.querySelector(".range-panel");
    const ics = inner ? getComputedStyle(inner) : null;
    return {
      backgroundColor: cs.backgroundColor,
      backdropFilter: cs.backdropFilter,
      borderRadius: cs.borderRadius,
      innerBackground: ics?.backgroundColor ?? null,
      innerBackdrop: ics?.backdropFilter ?? null,
      boxShadow: cs.boxShadow.slice(0, 60),
    };
  });
  out.opened = !!out.shell;
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  return out;
}

async function measureChart(page) {
  const out = {};
  const c = page.locator(".chart-canvas svg").first();
  out.canvasPresent = (await c.count()) > 0;
  if (out.canvasPresent) {
    const b = await c.boundingBox();
    out.box = b;
    out.sizeOk = b && b.width > 100 && b.height > 60;
    out.painted = await c.evaluate((el) => {
      return el.querySelectorAll("path").length > 0 && el.querySelectorAll("text").length > 0;
    });
  }
  return out;
}

async function measureNavScroll(page) {
  const out = await page.evaluate(() => {
    const cont = document.querySelector(".scroll-container");
    const nav = document.querySelector(".app-nav");
    if (!cont || !nav) return { available: false };
    const navTopBefore = nav.getBoundingClientRect().top;
    cont.scrollTop = 400;
    const cs = getComputedStyle(nav);
    const after = nav.getBoundingClientRect().top;
    const scrolled = cont.scrollTop > 0;
    return {
      available: true,
      scrolled,
      scrollTop: cont.scrollTop,
      stickyOk: Math.abs(after - navTopBefore) < 2,
      position: cs.position,
      backdropFilter: cs.backdropFilter,
      background: cs.backgroundColor,
    };
  });
  return out;
}

async function measureA11y(page) {
  return page.evaluate(() => {
    const sel =
      "button,[role=button],input,select,textarea,a[href],[tabindex]:not([tabindex='-1'])";
    const nodes = [...document.querySelectorAll(sel)].filter(
      (n) => n.getClientRects().length > 0 && !n.disabled,
    );
    const nameOf = (n) => {
      const aria = n.getAttribute("aria-label");
      if (aria) return aria;
      const lb = n.labels?.[0];
      if (lb) return lb.innerText;
      const t = (n.innerText || n.value || "").trim();
      if (t) return t;
      const title = n.getAttribute("title");
      return title || "";
    };
    const unnamed = nodes.filter((n) => !nameOf(n)).map((n) => n.className || n.tagName);
    return { focusable: nodes.length, unnamed: unnamed.slice(0, 6), unnamedCount: unnamed.length };
  });
}

// ── AC06 共享辅助（模块层；被下方多个隐私场景复用）─────────────

/**
 * 从仓库源码提取受保护命令清单（不写死数量），并给出每个命令的最小
 * 合法参数——让参数反序列化成功、隐私闸门拒绝成为唯一错误来源。
 */
function readProtectedCommands() {
  const text = fs.readFileSync(
    fileURLToPath(new URL("../../src-tauri/src/privacy.rs", import.meta.url)),
    "utf8",
  );
  const block = text.match(/PROTECTED_COMMANDS:\s*&\[(.*?)\];/s);
  if (!block) throw new Error("privacy.rs 中找不到 PROTECTED_COMMANDS 清单");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/** 受保护命令的最小合法参数（camelCase；仅用于让闸门成为唯一拒绝来源）。 */
const MINIMAL_INVOKER_ARGS = {
  summarize: { by: "day" },
  list_events: { limit: 1 },
  query_begin: { by: "day" },
  query_summary: { queryId: "x" },
  query_events: { queryId: "x" },
  startup_diagnostics: {},
  view_cache_load: {},
  view_cache_save: { value: {} },
  source_status: {},
  source_config_set: { agent: "claude", enabled: false },
  source_paths_list: {},
  source_path_add: { agent: "claude", path: "C:/synthetic/denied-ipc" },
  source_path_remove: { agent: "claude", id: "x" },
  source_path_set_enabled: { agent: "claude", id: "x", enabled: false },
  source_auto_set: { agent: "claude", enabled: false },
  cache_stats: {},
  refresh_cache: {},
  ccs_import_defaults: {},
  ccs_import_preview: {},
  ccs_import_commit: { planId: "x" },
  ccs_import_discard: { planId: "x" },
  pricing_entries: {},
  pricing_status: {},
  open_pricing_file: {},
  sync_pricing_openrouter: {},
  autostart_status: {},
  autostart_set: { enabled: false },
  settings_get: {},
  settings_set_price_auto_sync: { enabled: false },
  settings_set_close_action: { action: "minimize" },
  settings_set_timezone: { timezone: "UTC" },
  settings_set_collect_interval: { secs: 300 },
  collection_status: {},
  close_resolve: { minimize: false, remember: false },
  open_settings_file: {},
};

/** AC06：全蜂巢只读注册表快照（辅助证据；只读、不写、不删）。 */
function registrySnapshot() {
  const keys = [
    "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
    "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
    "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
    "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
  ];
  const snapshot = {};
  for (const key of keys) {
    const result = spawnSyncBounded("reg", ["query", key], {
      action: "registry-snapshot",
      stage: "registry-snapshot",
    });
    if (result.status !== 0) {
      // 键不存在属于合法形态：记为空列表。
      snapshot[key] = [];
      continue;
    }
    snapshot[key] = result.stdout
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.includes("REG_"))
      .sort();
  }
  return snapshot;
}

/** AC06：预置业务文件（预置 + 新增一律纳入快照比对）。十七轮复核 P2：
 * 夹具达到计划 §4b.1 组合——有效 SQLite 历史库（WAL 模式，进程退出残留
 * WAL/SHM）、两份价格快照；旧修订同意由 mutate 写入 settings。 */
function preseedBusinessFiles(dataDir) {
  const writeFile = (name, text) => {
    const target = path.join(dataDir, name);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, text, "utf8");
  };
  // 有效 SQLite 历史库（十八轮复核 P2：必须是**产品 schema**——旧夹具
  // 缺 ts_seconds 等列，HistoryDb::open 的 v1 迁移直接报错）。建表 SQL
  // 从 crates/tokenscope-core/src/history.rs 的 SCHEMA_V1 常量提取
  // （单一事实源，不双份维护），user_version=1 + 三行完整列事件。
  // 脚本末尾不 close 连接——解释器退出时 WAL/SHM 以残留形态留在盘上
  // （与"业务数据躺在盘上"的真实形态一致）。
  const historySource = fs.readFileSync(
    path.join(REPO, "crates", "tokenscope-core", "src", "history.rs"),
    "utf8",
  );
  const schemaStart = historySource.indexOf('const SCHEMA_V1: &str = r"');
  if (schemaStart < 0) throw new Error("history.rs 缺少 SCHEMA_V1 常量");
  const sqlStart = schemaStart + 'const SCHEMA_V1: &str = r"'.length;
  const sqlEnd = historySource.indexOf('"', sqlStart);
  const schemaV1 = historySource.slice(sqlStart, sqlEnd);
  const seedScript = [
    "import sqlite3, sys",
    "con = sqlite3.connect(sys.argv[1])",
    "con.execute('PRAGMA journal_mode=WAL')",
    "con.executescript(sys.argv[2])",
    "con.execute('PRAGMA user_version=1')",
    "con.executemany('INSERT INTO usage_events(event_key,app,ts_seconds,ts_nanos,model_raw,model_identity,identity_revision,session_id,record_id,project_key,session_initial_cwd,event_cwd,input_tokens,output_tokens,cache_write_tokens,cache_read_tokens,origin_rank,observed_at_utc,observed_at_seconds,observed_at_nanos,parser_revision) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)', [('ac06-session:rec-'+str(i), 'claude-code', 1761580000+i, 0, 'claude-sonnet-4-5', 'claude-sonnet-4-5', 1, 'ac06-session', 'rec-'+str(i), '/ac06/project', '/ac06/project', '/ac06/project', '10', '20', '0', '5', 1, '2026-10-27T00:00:0'+str(i)+'Z', 1761580000+i, 0, 2) for i in range(3)])",
    "con.commit()",
    "import os",
    "os._exit(0)",
  ].join("; ");
  const result = spawnSyncBounded("python", ["-c", seedScript, path.join(dataDir, "history.db"), schemaV1], {
    action: "preseed-sqlite",
    stage: "preseed-sqlite",
  });
  if (result.status !== 0) {
    throw new Error(`预置 SQLite 历史库失败：${result.stdout}${result.stderr}`);
  }
  writeFile("logs/ac06-preseeded.log", "ac06-preseeded-log\n");
  writeFile("view-cache.json", '{"ac06":"preseeded"}\n');
  // 十八轮复核：两份价格快照按产品加载器格式生成（synced_at 必填；
  // modelsdev 的 input/output/cache_* 与 openrouter 的 prompt/completion/
  // cache_* 各按自身结构）。
  const syncedAt = "2026-10-27T00:00:00+08:00";
  writeFile(
    "pricing-modelsdev.json",
    JSON.stringify({
      v: 2,
      synced_at: syncedAt,
      entries: [
        {
          id: "claude-code/claude-sonnet-4-5",
          name: null,
          input: 3.0,
          output: 15.0,
          cache_read: 0.3,
          cache_write: 3.75,
        },
      ],
    }) + "\n",
  );
  writeFile(
    "pricing-openrouter.json",
    JSON.stringify({
      v: 2,
      synced_at: syncedAt,
      entries: [
        {
          id: "claude-sonnet-4-5",
          name: null,
          prompt: 3.0,
          completion: 15.0,
          cache_read: 0.3,
          cache_write: 3.75,
        },
      ],
    }) + "\n",
  );
}

/** AC06：业务文件与设置的双重快照。十七轮复核 P2：settings 附带
 * 大小/mtime 元数据——同字节重写（内容不变、mtime 变化）也要可检出，
 * 不能只比内容。 */
function snapshotDataDir(dataDir) {
  const settingsPath = path.join(dataDir, "settings.toml");
  const stat = fs.existsSync(settingsPath) ? fs.statSync(settingsPath) : null;
  return {
    business: snapshotBusinessFiles(dataDir),
    settings: {
      text: stat ? fs.readFileSync(settingsPath, "utf8") : null,
      // 十八轮复核 P2：字节快照（hex）——宽松解码会把非法字节变 U+FFFD，
      // 同 size/mtime 的非法字节改写在 text/size/mtime 上不可辨。
      bytesHex: stat
        ? fs.readFileSync(settingsPath).toString("hex")
        : null,
      size: stat ? stat.size : null,
      mtimeMs: stat ? stat.mtimeMs : null,
    },
  };
}

/** AC06：双向快照比对（预置缺失/新增/内容变化/设置元数据变化均算变化）。 */
function dataDirChanges(before, after) {
  return [
    ...changedBusinessFiles(before.business, after.business),
    ...(JSON.stringify(before.settings) === JSON.stringify(after.settings)
      ? []
      : ["settings.toml"]),
  ];
}

/** AC06：拒绝路径退出（不同意 → 退出程序 → 进程干净退出）。 */
async function rejectAndExit(page, inst, ck, label) {
  await page.locator(".privacy-actions button", { hasText: "不同意" }).first().click();
  await page.waitForSelector(".exit-dialog", { timeout: 5000 });
  await page.locator(".exit-actions button", { hasText: "退出程序" }).first().click();
  const deadline = Date.now() + 20000;
  while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
  ck.add(`${label}_exit_clean`, inst.proc.exitCode === 0, `exitCode=${inst.proc.exitCode}`);
}

/** AC06：业务追踪事件抽取（沿用 policy-update 的判法）。十七轮复核 P2：
 * settings_write 计入——未同意阶段的任何设置写入（含同字节重写）都是
 * 违规，不能被过滤掉。 */
function businessTraceEvents(inst) {
  const trace = readAcceptanceTrace(inst.root, { partial: true });
  if (trace.violations.length) throw new Error(trace.violations.join(" | "));
  return trace.records
    .filter((r) =>
      [
        "business_ready",
        "collect_round_start",
        "source_root_read",
        "history_open",
        "ccs_open",
        "pricing_fetch",
        "autostart_read",
        "autostart_write",
        "settings_write",
      ].includes(r.event),
    )
    .map((r) => r.event);
}

// ── 场景 ──────────────────────────────────────────────────────────
const scenarios = {
  "ui-identity": nativeIdentity,
  "ui-ccs-import": nativeCcsImport,
  "ui-pricing": nativePricing,
  "ui-refresh": nativeUiRefresh,
  "ui-system": nativeUiSystem,
  // PR2：真实采集生命周期 + 读取显示期 + 失败恢复 + 翻页保护（验收注入驱动慢轮次与失败）。
  "ui-collection-status": nativeCollectionStatus,
  // PR3（§9.6 第 2 条）：来源目录主线——UI 追加手动目录、真实下一轮统计、重启后保留。
  "ui-source-paths": nativeSourcePaths,
  // §4.8 第 7 条：多路重启压力验证，只在本地跑（CI 由各场景原有的重启覆盖）。
  "ui-relaunch": nativeUiRelaunch,
  // 在同一个驱动进程内完成等待；部分终端会在父进程结束时回收其原生子进程。
  async "expire-cycle"(context) {
    await scenarios["expire-start"](context);
    const started = Date.now();
    while (Date.now() < Date.parse(context.ev.sessionIdleDeadlineIso)) {
      await sleep(1000);
      if (context.inst.proc.exitCode !== null)
        throw new Error(`空闲期间进程退出：${context.inst.proc.exitCode}`);
      if (Math.floor((Date.now() - started) / 1000) % 30 === 0)
        hb("原生查询会话保持空闲，等待真实 TTL");
    }
    context.ev.actualIdleMs = Date.now() - started;
    context.ck.add(
      "real_idle_exceeds_ttl",
      context.ev.actualIdleMs > 600000,
      `${context.ev.actualIdleMs} ms`,
    );
    await scenarios["expire-finish"](context);
  },
  /**
   * §2 首帧：录制连续帧 + 页面实测（偏好来自隔离 profile 的真实持久化）。
   *
   * P04 语义（2026-10-10）：首帧只跟随**系统**明暗——同意之前不读持久化偏好，
   * 因此 `--theme`（存储偏好）不影响首帧亮度断言，系统明暗由
   * `--force-dark-mode` / `--force-light-mode` 通道决定（`--expect-theme`）。
   * 存储偏好在点击「同意并继续」、后端 Ready 后才恢复（见 privacy-accept-restart）。
   */
  async "first-frame"({ inst, ck, ev }) {
    if (!inst.framesPromise)
      throw new Error("first-frame 必须带 --frames（首帧要用连续帧取证，加载完成截图不算证据）");
    await inst.waitForData();
    const win = await inst.page.evaluate(() => {
      const root = document.documentElement;
      const shell = document.querySelector(".app-shell") || root;
      const cs = getComputedStyle(shell);
      return {
        url: location.href,
        schemeDark: matchMedia("(prefers-color-scheme: dark)").matches,
        storedTheme: (() => {
          try {
            return localStorage.getItem("tokenscope-theme");
          } catch {
            return "<storage-blocked>";
          }
        })(),
        shellBackground: cs.backgroundColor,
        htmlBackground: getComputedStyle(document.documentElement).backgroundColor,
        bodyBackground: getComputedStyle(document.body).backgroundColor,
        dataTheme: root.getAttribute("data-theme") ?? shell.getAttribute("data-theme") ?? null,
        paint: performance
          .getEntriesByType("paint")
          .map((p) => `${p.name}=${Math.round(p.startTime)}ms`),
        navigation: (() => {
          const n = performance.getEntriesByType("navigation")[0];
          return n
            ? `domContentLoaded=${Math.round(n.domContentLoadedEventEnd)}ms load=${Math.round(n.loadEventEnd)}ms`
            : "n/a";
        })(),
      };
    });
    ev.webview = win;
    const frames = await inst.framesPromise;
    ev.frames = frames;
    await inst.page.screenshot({ path: path.join(inst.opts.shots, "page-loaded.png") });
    const darkExpected = EXPECT_THEME === "dark";
    const lightExpected = EXPECT_THEME === "light";
    const firstLum = frames.first_painted_frame ? frames.first_painted_frame.luma : null;
    ck.add(
      "first_frame_captured_after_process_start",
      !!frames.first_painted_frame,
      frames.first_painted_frame
        ? `首帧于进程启动后 ${frames.first_painted_frame.at_ms} ms 被观测，亮度 ${frames.first_painted_frame.luma}，共 ${frames.count} 帧（解码 ${frames.decoded}）`
        : `无可用帧（count=${frames.count} decoded=${frames.decoded}）`,
    );
    if (firstLum !== null && firstLum !== undefined && !Number.isNaN(firstLum)) {
      if (darkExpected)
        ck.add("first_frame_dark_canvas", firstLum < 110, `亮度=${firstLum}（深色期望 <110）`);
      if (lightExpected)
        ck.add("first_frame_light_canvas", firstLum > 150, `亮度=${firstLum}（浅色期望 >150）`);
      // 首帧之后不得出现"先深后浅"的闪烁：窗口帧序列里最小亮度必须就是首帧
      const lumas = frames.luma_sequence
        .map((t) => Number(t.split(":")[1]))
        .filter((v) => Number.isFinite(v) && v >= 0);
      const note = `min=${Math.min(...lumas)} max=${Math.max(...lumas)} n=${lumas.length}`;
      if (darkExpected) {
        ck.add(
          "no_light_before_dark_flash",
          Math.max(...lumas) < 110,
          `深色帧序列最大亮度 ${Math.max(...lumas)}；${note}`,
        );
      }
      if (lightExpected) {
        ck.add(
          "no_dark_before_light_flash",
          Math.min(...lumas) > 150,
          `浅色帧序列最小亮度 ${Math.min(...lumas)}；${note}`,
        );
      }
    }
    ck.add(
      "stored_theme_matches_launch",
      EXPECT_STORED === undefined || win.storedTheme === EXPECT_STORED,
      `localStorage tokenscope-theme=${win.storedTheme}，期望=${EXPECT_STORED ?? "（未指定）"}`,
    );
    ck.add(
      "boot_script_applies_stored_preference_before_paint",
      EXPECT_THEME === null || win.dataTheme === EXPECT_THEME,
      `data-theme=${win.dataTheme}，期望=${EXPECT_THEME ?? "（未指定）"}，html底色=${win.htmlBackground}，body底色=${win.bodyBackground}`,
    );
    ck.add(
      "paint_entries_recorded",
      win.paint.length > 0 || win.navigation !== "n/a",
      `${win.paint.join(" ")} | ${win.navigation}`,
    );
    ev.windowRect = await inst.windowInfo();
    ck.add("window_dpr_recorded", true, `dpr=${await inst.page.evaluate(() => devicePixelRatio)}`);
  },

  /** §3 主题 × DPI 组合下的真实交互量测。 */
  async combo({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    // 主题用**应用自己的偏好开关**切换（真实点击）：`--force-dark-mode` 不改
    // prefers-color-scheme，实测会静默跑成浅色，所以这里必须显式核对模式。
    const wantTheme = EXPECT_THEME ?? "system";
    if (wantTheme !== "system") {
      const label = { light: "浅色模式", dark: "深色模式", system: "跟随系统" }[wantTheme];
      await page.getByRole("radio", { name: label }).click();
      await page.waitForTimeout(800);
    }
    ev.dataTheme = await page.evaluate(() => document.documentElement.getAttribute("data-theme"));
    ev.forcedSchemeFlag = inst.opts.scheme ?? null;
    ck.add(
      "combo_runs_in_expected_theme_mode",
      wantTheme === "system" || ev.dataTheme === wantTheme,
      `data-theme=${ev.dataTheme}，期望=${wantTheme}（--force-*-mode=${inst.opts.scheme ?? "none"}）`,
    );
    const vp =
      page.viewportSize() ??
      (await page.evaluate(() => ({ width: innerWidth, height: innerHeight })));
    ev.viewport = vp;
    ev.dpr = await page.evaluate(() => devicePixelRatio);
    ev.nativeDpi = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "dpi"]).stdout || ""
    ).trim();
    if (SCALE === null) {
      const dpi = /DPI (\d+)/.exec(ev.nativeDpi);
      ck.add(
        "unforced_webview_matches_native_window_dpi",
        !!dpi && Math.abs(Number(dpi[1]) / 96 - ev.dpr) < 0.01,
        `${ev.nativeDpi}; WebView dpr=${ev.dpr}; 未使用缩放强制参数`,
      );
    }
    ev.windowRect = await inst.windowInfo();
    ev.themeMode = await page.evaluate(() => ({
      schemeDark: matchMedia("(prefers-color-scheme: dark)").matches,
      bodyBackground: getComputedStyle(document.body).backgroundColor,
      stored: (() => {
        try {
          return localStorage.getItem("tokenscope-theme");
        } catch {
          return "<blocked>";
        }
      })(),
    }));
    // 1) 键盘 + 可访问名称 + 费用浮层四边
    ev.cost = await measureCostPopover(page, vp);
    ck.add(
      "keyboard_reaches_cost_trigger",
      ev.cost.keyboardReachedTrigger,
      `Tab=${ev.cost.tabPresses ?? 0} 次`,
    );
    ck.add(
      "cost_trigger_has_accessible_name",
      (ev.cost.accessibleName || "").length > 3,
      ev.cost.accessibleName,
    );
    ck.add(
      "cost_trigger_focus_ring_visible",
      !!ev.cost.focusRing &&
        (ev.cost.focusRing.outlineStyle !== "none" ||
          /0px 0px 0px [1-9]/.test(ev.cost.focusRing.boxShadow || "")),
      JSON.stringify(ev.cost.focusRing),
    );
    ck.add(
      "cost_popover_opens_by_keyboard",
      ev.cost.ariaExpanded === "true",
      `aria-expanded=${ev.cost.ariaExpanded}`,
    );
    ck.add(
      "cost_popover_four_edges_inside_viewport",
      ev.cost.fourEdgesInside === true,
      JSON.stringify(ev.cost.popoverBox),
    );
    ck.add("cost_popover_states_unit_dimension", ev.cost.hasUnitDimension === true, ev.cost.text);
    ck.add("cost_popover_closes_on_escape", ev.cost.closedByEscape === true, "");
    await page.screenshot({ path: path.join(inst.opts.shots, "01-dashboard.png") });
    // 2) 图表
    ev.chart = await measureChart(page);
    ck.add(
      "chart_canvas_painted",
      ev.chart.sizeOk && ev.chart.painted,
      JSON.stringify(ev.chart.box),
    );
    // 3) 维度切换（按模型）后仍可渲染
    await page.getByRole("radio", { name: "按模型" }).click();
    await page.waitForTimeout(900);
    ev.chartByModel = await measureChart(page);
    ck.add(
      "chart_renders_after_dimension_switch",
      ev.chartByModel.sizeOk === true,
      JSON.stringify(ev.chartByModel.box),
    );
    await page.getByRole("radio", { name: "按时间" }).click();
    await page.waitForTimeout(700);
    // 4) 日期外壳
    ev.date = await measureDateShell(page);
    const shell = ev.date.shell || {};
    const alpha = /rgba?\([^)]*,\s*([\d.]+)\)/.exec(shell.backgroundColor || "");
    ck.add(
      "date_shell_single_elevated_layer",
      ev.date.opened &&
        alpha &&
        Math.abs(Number(alpha[1]) - 0.85) <= 0.06 &&
        /blur\(16px\)/.test(shell.backdropFilter || "") &&
        /12px/.test(shell.borderRadius || "") &&
        (!shell.innerBackground || shell.innerBackground === "rgba(0, 0, 0, 0)") &&
        (!shell.innerBackdrop || shell.innerBackdrop === "none"),
      JSON.stringify(shell),
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "02-date-open.png") });
    // 5) 长路径：设置页"当前生效目录"换行且不产生横向溢出。
    //    窗口真实收窄到 620 DIP（CDP setWindowBounds）——不模拟、不缩 CSS。
    await page.getByRole("radio", { name: "设置" }).click();
    await page.waitForTimeout(1200);
    ev.windowBeforeNarrow = await inst.windowInfo();
    ev.originalBounds = await inst.setWindowSize(620, 820); // 返回改前 bounds
    ev.viewportNarrow = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
    ev.settingsOverflow = await page.evaluate(() => {
      const de = document.documentElement;
      const path = document.querySelector(".effective-path");
      const cs = path ? getComputedStyle(path) : null;
      // flex 子项会被块化，getClientRects 只给一个盒子；行箱数要用 Range 取。
      let lineBoxes = 0;
      if (path) {
        const range = document.createRange();
        range.selectNodeContents(path);
        lineBoxes = range.getClientRects().length;
      }
      const lineHeight = cs ? Number.parseFloat(cs.lineHeight) || 18 : 18;
      const heightLines = path ? Math.round(path.getBoundingClientRect().height / lineHeight) : 0;
      const lines = Math.max(lineBoxes, heightLines);
      return {
        scrollWidth: de.scrollWidth,
        clientWidth: de.clientWidth,
        pathText: path ? path.textContent : null,
        // 内联元素的 getClientRects() 每行一个矩形 → 真实换行数
        lines,
        lineBoxes,
        heightLines,
        wordBreak: cs?.wordBreak ?? null,
        overflowWrap: cs?.overflowWrap ?? null,
        whiteSpace: cs?.whiteSpace ?? null,
        textTruncated: path
          ? path.scrollWidth > Math.ceil(path.getBoundingClientRect().width)
          : null,
      };
    });
    ck.add(
      "long_source_path_wraps_without_horizontal_overflow",
      ev.settingsOverflow.scrollWidth <= ev.settingsOverflow.clientWidth + 1 &&
        ev.settingsOverflow.lines >= 2 &&
        ev.settingsOverflow.textTruncated === false,
      `窗口=${JSON.stringify(ev.viewportNarrow)} scroll=${ev.settingsOverflow.scrollWidth}/${ev.settingsOverflow.clientWidth} 行数=${ev.settingsOverflow.lines} 截断=${ev.settingsOverflow.textTruncated} 断词=${ev.settingsOverflow.wordBreak}`,
    );
    ev.a11ySettings = await measureA11y(page);
    ck.add(
      "settings_focusables_all_named",
      ev.a11ySettings.unnamedCount === 0,
      JSON.stringify(ev.a11ySettings),
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "03-settings-narrow.png") });
    await inst.setWindowSize(ev.originalBounds.width ?? 1280, ev.originalBounds.height ?? 800);
    ev.viewportRestored = page.viewportSize();
    // 6) 导航滚动（玻璃吸顶有内容可透）
    await page.getByRole("radio", { name: "汇总" }).click();
    await page.waitForTimeout(900);
    ev.nav = await measureNavScroll(page);
    ck.add(
      "nav_sticky_with_blur_in_same_scroll_context",
      ev.nav.available &&
        ev.nav.stickyOk &&
        /blur\(/.test(ev.nav.backdropFilter || "") &&
        ev.nav.scrolled,
      JSON.stringify(ev.nav),
    );
    ev.a11yDashboard = await measureA11y(page);
    ck.add(
      "dashboard_focusables_all_named",
      ev.a11yDashboard.unnamedCount === 0,
      JSON.stringify(ev.a11yDashboard),
    );
    // 正常态通知清点（故障列由 §5 同组合故障轮实测）
    ev.noticesInNormalState = await page.locator(".ts-notice").count();
    ev.noticeTexts = (await page.locator(".ts-notice").allInnerTexts()).map((t) =>
      t.replace(/\s+/g, " ").slice(0, 140),
    );
  },

  /** §4 生产 CSP：真实文档响应头/HTML + 全功能交互期间的违规观测。 */
  async csp({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    // 重新加载并抓真实文档响应（协议级观测，不是调试器求值）
    await inst.cdp.send("Network.enable");
    await inst.cdp.send("Log.enable").catch(() => {});
    const captured = [];
    inst.cdp.on("Network.responseReceived", (e) => {
      if (e.type === "Document")
        captured.push({
          requestId: e.requestId,
          url: e.response.url,
          headers: e.response.headers,
          status: e.response.status,
        });
    });
    await inst.cdp.send("Page.reload");
    await inst.waitForData();
    ev.documentResponses = captured;
    const doc = captured.at(-1) ?? null;
    let htmlSnippet = null;
    if (doc) {
      try {
        const body = await inst.cdp.send("Network.getResponseBody", { requestId: doc.requestId });
        const text = body.base64Encoded
          ? Buffer.from(body.body, "base64").toString("utf8")
          : body.body;
        htmlSnippet = text.slice(0, 1200);
        ev.indexHtmlHasInlineScript = /<script(?![^>]*src=)[^>]*>[^<]/i.test(htmlSnippet);
        ev.metaCsp = /Content-Security-Policy/i.test(htmlSnippet)
          ? "present-in-html"
          : "not-in-html";
      } catch (e) {
        ev.responseBodyError = String(e.message ?? e);
      }
    }
    ev.cspHeader = doc
      ? (doc.headers["Content-Security-Policy"] ?? doc.headers["content-security-policy"] ?? null)
      : null;
    ck.add(
      "production_document_served_from_tauri_origin",
      !!doc && /tauri/.test(doc.url),
      doc?.url ?? "no-document",
    );
    ck.add(
      "production_csp_text_recorded",
      !!ev.cspHeader || ev.metaCsp === "present-in-html",
      `header=${String(ev.cspHeader).slice(0, 160)} html=${ev.metaCsp}`,
    );
    // 真实功能在 CSP 下可用（IPC / Naive / ECharts / 主题 / 日期）
    await page.getByRole("radio", { name: "按模型" }).click();
    await page.waitForTimeout(800);
    const chart = await measureChart(page);
    await page.getByRole("radio", { name: "按时间" }).click();
    await page.waitForTimeout(600);
    const date = await measureDateShell(page);
    // 主题生效量测取**有颜色的卡片**：.app-shell/html/body 本身透明（实测），
    // 用透明元素判断主题会永远得到同一个 rgba(0,0,0,0)。
    const themeProbe = () =>
      page.evaluate(() => ({
        dataTheme: document.documentElement.getAttribute("data-theme"),
        card: getComputedStyle(document.querySelector(".metric-card") ?? document.body)
          .backgroundColor,
      }));
    await page.getByRole("radio", { name: "深色模式" }).click();
    await page.waitForTimeout(600);
    const darkBg = await themeProbe();
    await page.getByRole("radio", { name: "浅色模式" }).click();
    await page.waitForTimeout(600);
    const lightBg = await themeProbe();
    ck.add(
      "ipc_summary_data_rendered",
      (await page.locator(".summary-card, .ts-card").count()) > 0,
      `cards=${await page.locator(".summary-card, .ts-card").count()}`,
    );
    ck.add("echarts_works_under_csp", chart.sizeOk === true, JSON.stringify(chart.box));
    ck.add("naive_popover_works_under_csp", date.opened === true, JSON.stringify(date.shell));
    ck.add(
      "theme_switch_works_under_csp",
      darkBg.dataTheme === "dark" && lightBg.dataTheme === "light" && darkBg.card !== lightBg.card,
      `dark=${JSON.stringify(darkBg)} light=${JSON.stringify(lightBg)}`,
    );
    ev.consoleMessages = inst.console.slice(0, 60);
    const vs = splitViolations(inst.violations);
    ev.cspViolations = inst.violations.slice(0, 20);
    ev.knownNoiseCount = vs.known.length;
    ck.add(
      "no_unexpected_csp_violation_while_app_runs",
      vs.unexpected.length === 0,
      vs.unexpected.slice(0, 3).join(" | ") || `仅已知噪声 ${vs.known.length} 条`,
    );
    const beforeProbe = inst.violations.length;
    // 入口脚本由同源 HTTP 资源加载，真实点击才插入 script；evaluate 只读结果。
    await page.locator("#ts-acceptance-csp").click();
    await page.waitForFunction(
      () => window.__tsCspProbe?.positiveRan && window.__tsCspProbe.violations.length >= 2,
    );
    ev.scriptProbe = await page.evaluate(() => window.__tsCspProbe);
    ck.add(
      "same_origin_positive_script_executes",
      ev.scriptProbe.positiveRan === true,
      "允许的同源脚本确实执行，排除全部脚本无法加载的假阳性",
    );
    ck.add(
      "unauthorized_inline_script_is_blocked",
      ev.scriptProbe.inlineRan === false &&
        ev.scriptProbe.violations.some((v) => v.blockedURI === "inline"),
      JSON.stringify(ev.scriptProbe),
    );
    ck.add(
      "external_script_is_blocked_by_policy",
      ev.scriptProbe.violations.some(
        (v) => v.blockedURI === "https://tokenscope.invalid/acceptance-blocked.js",
      ),
      JSON.stringify(ev.scriptProbe.violations),
    );
    ck.add(
      "only_two_expected_script_policy_events",
      ev.scriptProbe.violations.length === 2,
      JSON.stringify(ev.scriptProbe.violations),
    );
    // console 与 CDP Log 可能重复上报同一事件；与 DOM 事件分开记录。
    ev.expectedScriptConsole = inst.violations
      .slice(beforeProbe)
      .filter((v) =>
        /Executing inline script violates|Loading the script 'https:\/\/tokenscope\.invalid\/acceptance-blocked\.js'/i.test(
          v,
        ),
      );
    inst.expectedScriptConsole = new Set(ev.expectedScriptConsole);
    ck.add(
      "production_policy_not_relaxed_for_probe",
      !!ev.cspHeader && !/script-src[^;]*'unsafe-inline'/.test(ev.cspHeader),
      ev.cspHeader,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "csp-state.png") });
  },

  /** 播种：真实点击主题偏好 → 隔离 WebView profile 持久化 → 供首帧冷启动复用。 */
  async "seed-theme"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    const label =
      { light: "浅色模式", dark: "深色模式", system: "跟随系统" }[inst.opts.scheme ?? "system"] ??
      "跟随系统";
    await page.getByRole("radio", { name: label }).click();
    await page.waitForTimeout(1500);
    ev.stored = await page.evaluate(() => {
      try {
        return localStorage.getItem("tokenscope-theme");
      } catch {
        return "<blocked>";
      }
    });
    ev.modeAfterSeed = await page.evaluate(
      () => getComputedStyle(document.querySelector(".app-shell") ?? document.body).backgroundColor,
    );
    ck.add(
      "theme_preference_persisted_in_isolated_profile",
      ev.stored === labelToValue(label),
      `stored=${ev.stored}，期望=${labelToValue(label)}`,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, `seeded-${ev.stored}.png`) });
    // 必须走应用自己的"直接退出"：强杀进程可能让 WebView2 来不及把
    // localStorage 落盘，下一轮"偏好已持久化"的前提就不成立（实测过）。
    inst.opts.gracefulExit = true;
    const closeRes = ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]);
    ev.closeRequest = (closeRes.stdout || "").trim();
    await page.waitForTimeout(1500);
    ev.dialogForExit = (await page.locator(".close-dialog").count()) > 0;
    ck.add(
      "exit_path_reachable_from_close_dialog",
      ev.dialogForExit === true,
      String(ev.closeRequest),
    );
    if (ev.dialogForExit) {
      await page.getByRole("button", { name: "直接退出" }).click();
      const deadline = Date.now() + 20000;
      while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
      ev.cleanExitCode = inst.proc.exitCode;
      ck.add(
        "process_exited_without_force_kill",
        inst.proc.exitCode !== null,
        `exitCode=${inst.proc.exitCode}`,
      );
    }
  },

  /** §5-1 logs 路径是普通文件 → 窗口仍出现 + 启动降级通知（SF06）。 */
  async "fault-logs-file"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    await page.waitForTimeout(2500);
    const notice = page.locator(".banner-slot .ts-notice[role='status']").first();
    ev.noticeCount = await page.locator(".ts-notice").count();
    ev.noticeText = (await notice.count()) ? await notice.innerText() : null;
    ev.logFileStillFile = (() => {
      const p = path.join(inst.root, "tokenscope", "logs");
      return fs.existsSync(p) ? (fs.statSync(p).isFile() ? "file" : "dir") : "absent";
    })();
    ck.add(
      "window_appears_with_unusable_logs",
      (await page.locator(".app-nav").count()) > 0,
      "导航已渲染",
    );
    ck.add(
      "startup_degradation_notice_visible",
      !!ev.noticeText && /文件日志不可用/.test(ev.noticeText),
      String(ev.noticeText).replace(/\s+/g, " ").slice(0, 160),
    );
    // 数据仍可用（降级不阻断）
    await inst.waitForData().catch(() => {});
    ev.chart = await measureChart(page);
    ck.add(
      "data_still_available_after_log_degradation",
      ev.chart.sizeOk === true,
      JSON.stringify(ev.chart.box),
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "fault-logs-file.png") });
    ev.windowRect = await inst.windowInfo();
  },

  /** §5-2 pricing.toml 被共享锁占用 → 降级提示；释放后重试恢复。 */
  async "fault-pricing-locked"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    await page.getByRole("radio", { name: "设置" }).click();
    await page.waitForTimeout(2500);
    ev.lockHolderPid = inst.lockHolderPid ?? null;
    ev.lockWasHeldFrom = inst.lockHeldFrom;
    ev.lockConfirmedBeforeLaunch = inst.lockConfirmed === true;
    ck.add(
      "pricing_file_locked_before_process_start",
      inst.lockConfirmed === true,
      `holder=${inst.lockHolderPid}`,
    );
    // 后端把"读取失败"当作**告警**返回（不是 IPC 错误），前端在价格组内联展示，
    // 所以这里量的是 .price-notice 而不是 .ts-notice.is-error。
    ev.priceWarnings = (await page.locator(".price-notice").allInnerTexts()).map((t) =>
      t.replace(/\s+/g, " "),
    );
    ev.readFailureWarningVisible = ev.priceWarnings.some((t) => /外置价格文件读取失败/.test(t));
    ck.add(
      "locked_pricing_surfaces_read_failure_warning",
      ev.readFailureWarningVisible === true,
      ev.priceWarnings.map((t) => t.slice(0, 120)).join(" || ") || "无价格告警",
    );
    // 降级不阻断：汇总数据仍在（费用退化为未知也要有可见结论）
    ck.add(
      "app_still_usable_while_pricing_locked",
      (await page.locator(".settings-group").count()) >= 3,
      `分组数=${await page.locator(".settings-group").count()}`,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "pricing-locked.png") });
    let released = false;
    const deadline = Date.now() + 70000;
    while (Date.now() < deadline) {
      try {
        const fd = fs.openSync(path.join(inst.root, "tokenscope", "pricing.toml"), "r+");
        fs.closeSync(fd);
        released = true;
        break;
      } catch {
        await sleep(500);
      }
    }
    ev.lockReleased = released;
    ck.add(
      "lock_released_before_recovery_probe",
      released,
      `等待 ${70 - Math.round((deadline - Date.now()) / 1000)} s`,
    );
    // 恢复通道：真实重新读取（切走再切回设置页 → 价格组重新 loadPricing）
    await page.getByRole("radio", { name: "汇总" }).click();
    await page.waitForTimeout(1500);
    await page.getByRole("radio", { name: "设置" }).click();
    await page.waitForTimeout(3000);
    ev.warningsAfterRelease = (await page.locator(".price-notice").allInnerTexts()).map((t) =>
      t.replace(/\s+/g, " "),
    );
    ck.add(
      "price_read_recovers_after_lock_release",
      ev.lockReleased && !ev.warningsAfterRelease.some((t) => /读取失败/.test(t)),
      ev.warningsAfterRelease.map((t) => t.slice(0, 110)).join(" || ") || "价格告警已消失",
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "pricing-recovered.png") });
    ev.windowRect = await inst.windowInfo();
  },

  /** §6-2 最小化到托盘：窗口隐藏但进程与统计继续（隔离实例）。 */
  async "close-remember-failure"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    ev.settingsShaBefore = sha256File(settingsPath);
    ev.closeRequest = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || ""
    ).trim();
    await page.locator(".close-error").waitFor();
    ev.errorText = await page.locator(".close-error").innerText();
    ck.add(
      "remembered_failure_visible_with_reason",
      /acceptance-hide-once/.test(ev.errorText),
      ev.errorText,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "remembered-failure.png") });
    await page.getByRole("button", { name: "最小化到托盘", exact: true }).click();
    await page.waitForTimeout(1500);
    ev.rectAfterRetry = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "mainrect"]).stdout || ""
    ).trim();
    const rect = /MAINRECT\s+(-?\d+),(-?\d+),(\d+),(\d+)/.exec(ev.rectAfterRetry);
    ck.add(
      "retry_hides_real_window",
      /MAINRECT none/.test(ev.rectAfterRetry) ||
        (rect && Number(rect[3]) < 100 && Number(rect[4]) < 100),
      ev.rectAfterRetry,
    );
    ck.add(
      "retry_preserves_process",
      inst.proc.exitCode === null,
      `exitCode=${inst.proc.exitCode}`,
    );
    ck.add(
      "retry_keeps_remembered_settings",
      sha256File(settingsPath) === ev.settingsShaBefore,
      ev.settingsShaBefore,
    );
  },

  async "close-tray"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    ev.settingsShaBefore = sha256File(settingsPath);
    ev.closeRequest = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || ""
    ).trim();
    await page.waitForTimeout(1500);
    ev.dialogVisible = (await page.locator(".close-dialog").count()) > 0;
    ck.add("close_dialog_appears", ev.dialogVisible === true, String(ev.closeRequest));
    await page.getByRole("button", { name: "最小化到托盘" }).click();
    await page.waitForTimeout(2000);
    // 隐藏主窗口后进程仍有可见顶层窗口（22x22 的宿主/托盘窗口，实测），
    // 因此按"最大可见顶层窗口"判定主内容窗口是否真的不在屏上。
    ev.rectAfterMinimize = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "mainrect"]).stdout || ""
    ).trim();
    const m = /MAINRECT\s+(?:none|\d+,-?\d+,(\d+),(-?\d+))/.exec(ev.rectAfterMinimize);
    ev.largestWindowArea = m ? Number(m[1]) * Number(m[2]) : null;
    ev.windowHidden =
      /MAINRECT none/.test(ev.rectAfterMinimize) ||
      (m !== null && Number(m[1]) < 100 && Number(m[2]) < 100);
    ev.processAlive = inst.proc.exitCode === null;
    ev.settingsShaAfter = sha256File(settingsPath);
    ck.add("tray_minimize_hides_main_window", ev.windowHidden === true, ev.rectAfterMinimize);
    ck.add(
      "process_alive_after_tray_minimize",
      ev.processAlive === true,
      `exitCode=${inst.proc.exitCode}`,
    );
    ck.add(
      "tray_minimize_does_not_write_close_action",
      ev.settingsShaBefore === ev.settingsShaAfter,
      `before=${ev.settingsShaBefore.slice(0, 12)} after=${ev.settingsShaAfter.slice(0, 12)}`,
    );
    ev.dialogStillMounted = await page.locator(".close-dialog").count();
  },

  /** §6-3 记忆默认动作：勾选后退出 → 重启不再询问，且写入的是隔离 settings。 */
  async "close-remember"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    ev.settingsBefore = fs.readFileSync(settingsPath, "utf8");
    ev.settingsShaBefore = sha256File(settingsPath);
    ev.close1 = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || ""
    ).trim();
    await page.waitForTimeout(1500);
    ev.dialog1 = (await page.locator(".close-dialog").count()) > 0;
    ck.add("first_close_asks", ev.dialog1 === true, ev.close1);
    await page.locator(".close-dialog .n-checkbox").first().click();
    await page.waitForTimeout(400);
    ev.checkboxChecked = await page
      .locator(".close-dialog .n-checkbox")
      .first()
      .evaluate((el) => el.classList.contains("n-checkbox--checked"));
    ck.add(
      "remember_checkbox_checkable",
      ev.checkboxChecked === true,
      `checked=${ev.checkboxChecked}`,
    );
    await page.getByRole("button", { name: "直接退出" }).click();
    const deadline = Date.now() + 20000;
    while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
    ev.exitCode = inst.proc.exitCode;
    ck.add(
      "remembered_exit_closes_process",
      inst.proc.exitCode !== null,
      `exitCode=${inst.proc.exitCode}`,
    );
    ev.settingsAfter = fs.readFileSync(settingsPath, "utf8");
    ev.settingsShaAfter = sha256File(settingsPath);
    ev.closeActionWritten = /close_action/.test(ev.settingsAfter);
    ck.add(
      "remembered_choice_persisted_to_isolated_settings",
      ev.closeActionWritten === true && ev.settingsShaAfter !== ev.settingsShaBefore,
      `sha ${ev.settingsShaBefore.slice(0, 12)}→${ev.settingsShaAfter.slice(0, 12)}；文件含 close_action=${ev.closeActionWritten}`,
    );
    // 重启同一隔离根：不应再弹询问（页面句柄必须重新取，旧 target 已销毁）
    const info = await inst.launch();
    ev.relaunchPid = info.pid;
    ev.relaunchPort = info.port;
    ev.relaunchLaunchIndex = info.launchIndex;
    const page2 = inst.page;
    await page2.waitForSelector(".app-nav", { timeout: 30000 });
    await page2.waitForTimeout(2500);
    ev.relaunchStoredTheme = await page2.evaluate(() => {
      try {
        return localStorage.getItem("tokenscope-theme");
      } catch {
        return "<blocked>";
      }
    });
    // 记住动作后重启：静息态不应挂着询问弹窗
    ev.dialogBeforeSecondClose = await page2.locator(".close-dialog").count();
    ck.add(
      "relaunch_shows_no_prompt_at_rest",
      ev.dialogBeforeSecondClose === 0,
      `弹窗数=${ev.dialogBeforeSecondClose}`,
    );
    ev.close2 = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || ""
    ).trim();
    // 记住的是"直接退出"：页面会随进程结束而销毁，这里只等进程自己退出
    const deadline2 = Date.now() + 15000;
    while (inst.proc.exitCode === null && Date.now() < deadline2) await sleep(250);
    ev.processAfterRememberedClose = inst.proc.exitCode;
    ck.add(
      "remembered_action_executes_without_prompt",
      inst.proc.exitCode !== null,
      `exitCode=${inst.proc.exitCode}（${ev.close2}）`,
    );
  },

  /** §5-3 合成日志变化：旧 query 稳定，新 query 反映变化（SF04）。 */
  async "query-stability"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    const readTotals = async () =>
      (await page.locator(".metric-card").allInnerTexts())
        .join(" | ")
        .replace(/\s+/g, " ")
        .slice(0, 400);
    ev.before = await readTotals();
    await page.waitForTimeout(3000);
    ev.stillSame = await readTotals();
    ck.add(
      "loaded_view_stable_without_new_query",
      ev.before === ev.stillSame,
      "同一视图在采集变化期间不原地变形",
    );
    // 真实追加一条合成事件到隔离根
    const projectDir = path.join(
      inst.root,
      "sources",
      "claude",
      "projects",
      "-acceptance-workspace-tokenscope",
    );
    const logFile = fs
      .readdirSync(projectDir)
      .filter((f) => f.endsWith(".jsonl") && !f.startsWith("broken-"))
      .map((f) => path.join(projectDir, f))[0];
    const line = JSON.stringify({
      type: "assistant",
      isSidechain: false,
      sessionId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      timestamp: "2026-10-08T00:30:00.000Z",
      message: {
        id: "msg-acceptance-appended-1",
        role: "assistant",
        model: "claude-sonnet-4-5",
        content: [{ type: "text", text: "acceptance appended reply" }],
        usage: {
          input_tokens: 1000,
          output_tokens: 500,
          cache_read_input_tokens: 0,
          cache_creation_input_tokens: 0,
        },
      },
    });
    fs.appendFileSync(logFile, line + "\n");
    ev.appendedTo = path.basename(logFile);
    await page.waitForTimeout(1500);
    ev.afterAppendWithoutRefresh = await readTotals();
    ck.add(
      "appended_event_not_applied_to_current_query",
      ev.afterAppendWithoutRefresh === ev.before,
      "未刷新时仍显示旧会话结果",
    );
    await page.getByRole("button", { name: "刷新", exact: true }).click();
    await page.waitForTimeout(4000);
    ev.afterRefresh = await readTotals();
    ck.add(
      "new_query_reflects_appended_event",
      ev.afterRefresh !== ev.before,
      "刷新后新会话包含追加事件",
    );
    ev.requestsBeforeAfter = await page
      .evaluate(() => {
        const t = document.body.innerText;
        const m = t.match(/请求\s*([0-9,]+)/);
        return m ? m[1] : null;
      })
      .catch(() => null);
    await page.screenshot({ path: path.join(inst.opts.shots, "query-stability.png") });
    ev.windowRect = await inst.windowInfo();
  },

  /** §5-4 补充源同步失败 → 错误条带可点击"重试同步"（RC04）。 */
  async "sync-failure"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    await page.getByRole("radio", { name: "设置" }).click();
    await page.waitForTimeout(1500);
    await page
      .getByRole("button", { name: /同步价格|立即同步|同步/ })
      .first()
      .click();
    await page.waitForTimeout(15000);
    const err = page.locator(".ts-notice.is-error");
    ev.errorTexts = await err.allInnerTexts();
    ev.syncErrorVisible = ev.errorTexts.some((t) => /同步/.test(t));
    ck.add(
      "sync_failure_shows_error_notice",
      ev.syncErrorVisible,
      ev.errorTexts.map((t) => t.replace(/\s+/g, " ").slice(0, 140)).join(" || ") || "无错误条",
    );
    const retry = page.locator(".ts-notice.is-error button", { hasText: "重试同步" });
    const retryBtn = page.getByRole("button", { name: "重试同步" });
    ev.retryPresent = await retryBtn.count();
    ck.add(
      "error_notice_has_clickable_retry_sync",
      ev.retryPresent > 0,
      `按钮数=${ev.retryPresent}（旧 locator=${await retry.count()}）`,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "sync-failure.png") });
    if (ev.retryPresent) {
      await retryBtn.first().click();
      await page.waitForTimeout(12000);
      ev.errorAfterRetry = await page.locator(".ts-notice.is-error").allInnerTexts();
      ck.add(
        "retry_sync_actually_retries_and_keeps_reason",
        ev.errorAfterRetry.some((t) => /同步/.test(t)),
        ev.errorAfterRetry.map((t) => t.replace(/\s+/g, " ").slice(0, 140)).join(" || ") ||
          "重试后错误消失（网络恢复？）",
      );
    }
    ev.windowRect = await inst.windowInfo();
  },

  /** §5-5 会话过期（真实 600 s 空闲 TTL）：错误条可见 + 点其按钮恢复。 */
  async "expire-start"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    // 明细表是虚拟滚动：DOM 行数不随分页变化，必须用"还剩 N 条"文案判断分页进度
    ev.rowsBefore = await page.locator(".events-table tbody tr, .n-data-table-tbody tr").count();
    const more = page.locator(".load-more-row button").first();
    ev.morePresent = (await more.count()) > 0;
    ev.moreTextBefore = ev.morePresent
      ? await more.evaluate((el) => el.parentElement.innerText.replace(/\s+/g, " "))
      : null;
    ck.add(
      "load_more_available_with_large_event_set",
      ev.morePresent,
      `首页 DOM 行数=${ev.rowsBefore}（虚拟滚动）；${ev.moreTextBefore}`,
    );
    // 空闲前**不点**加载更多：点完最后一页后按钮会消失，就没有可过期的游标了
    await page.waitForTimeout(1200);
    ev.sessionIdleDeadlineIso = new Date(Date.now() + 630_000).toISOString();
    ck.add(
      "state_recorded_for_idle_wait",
      true,
      `保持进程存活，≥630 s 后跑 --scenario expire-finish`,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "expire-page1.png") });
  },

  async "expire-finish"({ inst, ck, ev }) {
    const page = inst.page;
    ev.idleWaitNote = inst.opts.idleNote;
    const more = page.locator(".load-more-row button").first();
    ck.add("load_more_still_present_after_idle", (await more.count()) > 0, "");
    await more.click();
    await page.waitForTimeout(4000);
    const err = page.locator(".ts-notice").filter({ hasText: "明细加载失败" });
    ev.noticeText = (await err.count()) ? await err.first().innerText() : null;
    ck.add(
      "expired_session_shows_error_notice",
      !!ev.noticeText && /query_expired|不存在或已失效/.test(ev.noticeText),
      String(ev.noticeText).replace(/\s+/g, " ").slice(0, 160),
    );
    if (await err.count()) await err.first().scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(inst.opts.shots, "expire-error.png") });
    const retry = page.locator(".ts-notice").filter({ hasText: "明细加载失败" }).locator("button");
    ev.retryCount = await retry.count();
    ck.add("error_notice_carries_recovery_button", ev.retryCount > 0, `按钮=${ev.retryCount}`);
    if (ev.retryCount) {
      await retry.first().click();
      await page.waitForTimeout(6000);
      ev.noticeAfterRetry = await page
        .locator(".ts-notice")
        .filter({ hasText: "明细加载失败" })
        .count();
      ck.add(
        "recovery_button_restarts_batch_and_clears_error",
        ev.noticeAfterRetry === 0,
        `剩余错误条=${ev.noticeAfterRetry}`,
      );
      ev.moreTextAfterRecovery = await page
        .locator(".load-more-row")
        .first()
        .innerText()
        .catch(() => null);
      const m = /还剩\s*([0-9,]+)\s*条/.exec(String(ev.moreTextAfterRecovery ?? ""));
      ck.add(
        "pagination_available_again_after_recovery",
        !!m,
        `恢复后分页文案=${String(ev.moreTextAfterRecovery).replace(/\s+/g, " ").slice(0, 80)}`,
      );
      if (m) {
        const hintBefore = await page.locator(".events-card .table-hint").innerText();
        const loadedBefore = /已加载\s*([0-9,]+)/.exec(hintBefore);
        await page.locator(".load-more-row button").first().click();
        await page.waitForTimeout(4000);
        ev.paginationAfterNextPage = await page
          .locator(".load-more-row")
          .first()
          .innerText()
          .catch(() => "");
        ev.detailHintAfterNextPage = await page.locator(".events-card .table-hint").innerText();
        const loadedAfter = /已加载\s*([0-9,]+)/.exec(ev.detailHintAfterNextPage);
        const remaining = /还剩\s*([0-9,]+)\s*条/.exec(ev.paginationAfterNextPage);
        ck.add(
          "recovered_session_loads_next_page",
          (await err.count()) === 0 &&
            !!loadedBefore &&
            !!loadedAfter &&
            Number(loadedAfter[1].replace(/,/g, "")) > Number(loadedBefore[1].replace(/,/g, "")) &&
            (!remaining || Number(remaining[1].replace(/,/g, "")) < Number(m[1].replace(/,/g, ""))),
          ev.detailHintAfterNextPage,
        );
      }
      const logDir = path.join(inst.root, "tokenscope", "logs");
      const logs = fs
        .readdirSync(logDir)
        .sort()
        .map((f) => fs.readFileSync(path.join(logDir, f), "utf8"))
        .join("\n");
      ev.detailSessions = [...logs.matchAll(/明细完成（会话 ([^）]+)）/g)].map((m) => m[1]);
      ck.add(
        "recovery_uses_new_backend_session",
        new Set(ev.detailSessions).size >= 2,
        ev.detailSessions.join(" → "),
      );
    }
    await page.screenshot({ path: path.join(inst.opts.shots, "expire-recovered.png") });
    ev.windowRect = await inst.windowInfo();
  },

  /** §6 关窗取消：WM_CLOSE → 真实弹窗 → Escape → 进程存活 + settings 字节不变。 */
  async "close-final-save"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    ev.closeRequest = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || ""
    ).trim();
    await page.locator(".close-dialog").waitFor();
    // 等定时保存完成且 dirty 已清，再移除隔离根中的派生文件。
    // 此后没有窗口事件，只有退出前最终保存可以重新生成它。
    await page.waitForTimeout(2200);
    const stateFile = path.join(inst.root, "tokenscope", "window-state.json");
    ev.windowStateBefore = JSON.parse(fs.readFileSync(stateFile, "utf8"));
    fs.unlinkSync(stateFile);
    await page.getByRole("button", { name: "直接退出", exact: true }).click();
    const deadline = Date.now() + 15000;
    while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(100);
    ck.add(
      "dialog_quit_exits_successfully",
      inst.proc.exitCode === 0,
      `exitCode=${inst.proc.exitCode}`,
    );
    ev.windowStateAfter = fs.existsSync(stateFile)
      ? JSON.parse(fs.readFileSync(stateFile, "utf8"))
      : null;
    ck.add(
      "dialog_quit_completes_final_save",
      JSON.stringify(ev.windowStateAfter) === JSON.stringify(ev.windowStateBefore),
      JSON.stringify(ev.windowStateAfter),
    );
  },

  async "close-cancel"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    ev.settingsShaBefore = sha256File(settingsPath);
    await page.screenshot({ path: path.join(inst.opts.shots, "before-close.png") });
    const r = ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]);
    ev.closeRequest = (r.stdout || "").trim();
    ck.add("wm_close_posted", /CLOSED true/i.test(ev.closeRequest), ev.closeRequest);
    await page.waitForTimeout(1200);
    const dialog = page.locator(".close-dialog");
    ev.dialogVisible = (await dialog.count()) > 0;
    ev.dialogText = ev.dialogVisible ? await dialog.first().innerText() : null;
    ck.add(
      "close_dialog_appears",
      ev.dialogVisible === true,
      String(ev.dialogText).replace(/\s+/g, " ").slice(0, 120),
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "dialog-open.png") });
    await page.keyboard.press("Escape");
    await page.waitForTimeout(1200);
    ev.dialogAfterEscape = await dialog.count();
    ck.add("escape_dismisses_dialog", ev.dialogAfterEscape === 0, `剩余=${ev.dialogAfterEscape}`);
    ev.processAlive = inst.proc.exitCode === null;
    ck.add("process_still_alive_after_cancel", ev.processAlive, `exitCode=${inst.proc.exitCode}`);
    ev.settingsShaAfter = sha256File(settingsPath);
    ck.add(
      "settings_bytes_unchanged_by_cancel",
      ev.settingsShaBefore === ev.settingsShaAfter,
      `before=${ev.settingsShaBefore.slice(0, 12)} after=${ev.settingsShaAfter.slice(0, 12)}`,
    );
    await page.screenshot({ path: path.join(inst.opts.shots, "after-cancel.png") });
    ev.windowRect = await inst.windowInfo();
  },

  /**
   * P06 隐私闸门（未同意）：隔离根里没有同意记录（prepare 脚本写的
   * settings.toml 不含 privacy_policy_accepted）→ 只显示政策；业务界面不挂载、
   * 不产生业务文件；拒绝退出后进程结束且不留托盘/同意记录。
   */
  async "privacy-consent-gate"({ inst, ck, ev }) {
    const page = inst.page;
    const dataDir = path.join(inst.root, "tokenscope");
    const settingsPath = path.join(dataDir, "settings.toml");
    const businessFiles = () =>
      listFiles(dataDir).filter((f) =>
        /^(logs\/|cache\.db|history\.db|window-state\.json|view-cache\.json|pricing-)/.test(f),
      );

    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });
    await page.waitForTimeout(1500);
    ev.settingsBefore = fs.readFileSync(settingsPath, "utf8");
    ck.add(
      "fixture_has_no_consent_record",
      !/privacy_policy_accepted\s*=\s*true/.test(ev.settingsBefore),
      "隔离根必须未写同意记录（否则测的不是引导态）",
    );

    ev.businessDomNodes = await page.locator(".app-nav, .page-head, .chart-canvas").count();
    ck.add(
      "business_ui_not_mounted_before_consent",
      ev.businessDomNodes === 0,
      `业务节点数=${ev.businessDomNodes}`,
    );
    const text = (await page.locator(".privacy-dialog").innerText()).replace(/\s+/g, " ");
    ck.add(
      "policy_visible_before_consent",
      text.includes("TokenScope 隐私政策") &&
        text.includes("同意并继续") &&
        text.includes("不同意"),
      text.slice(0, 120),
    );

    ev.businessFilesBeforeAccept = businessFiles();
    ev.allBusinessChangesBeforeAccept = changedBusinessFiles(
      inst.prelaunchBusinessFiles,
      snapshotBusinessFiles(dataDir),
    );
    ck.add(
      "all_non_settings_files_unchanged_before_consent",
      ev.allBusinessChangesBeforeAccept.length === 0,
      JSON.stringify(ev.allBusinessChangesBeforeAccept),
    );
    ck.add(
      "no_business_files_before_consent",
      ev.businessFilesBeforeAccept.length === 0,
      ev.businessFilesBeforeAccept.join(",") || "无业务文件",
    );

    // 拒绝 → 专属退出确认（无托盘/记忆选项）
    await page.locator(".privacy-actions button", { hasText: "不同意" }).first().click();
    await page.waitForSelector(".exit-dialog", { timeout: 5000 });
    const exitText = (await page.locator(".exit-dialog").innerText()).replace(/\s+/g, " ");
    ck.add(
      "reject_opens_exit_confirmation",
      exitText.includes("退出 TokenScope？") && exitText.includes("返回隐私政策"),
      exitText.slice(0, 120),
    );
    ck.add(
      "no_tray_or_remember_option",
      !exitText.includes("最小化") && !exitText.includes("记住"),
      exitText.slice(0, 120),
    );

    // 取消 → 回到政策，仍被阻断
    await page.locator(".exit-actions button", { hasText: "返回隐私政策" }).first().click();
    await page.waitForTimeout(500);
    ev.navAfterCancel = await page.locator(".app-nav").count();
    ck.add("cancel_exit_stays_blocked", ev.navAfterCancel === 0, `nav=${ev.navAfterCancel}`);

    // 确认退出 → 进程结束、无托盘残留、无同意记录、无窗口状态落盘
    await page.locator(".privacy-actions button", { hasText: "不同意" }).first().click();
    await page.waitForSelector(".exit-dialog", { timeout: 5000 });
    await page.locator(".exit-actions button", { hasText: "退出程序" }).first().click();
    const deadline = Date.now() + 20000;
    while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
    ev.exitCode = inst.proc.exitCode;
    ck.add(
      "rejected_exit_closes_process",
      inst.proc.exitCode !== null,
      `exitCode=${inst.proc.exitCode}`,
    );

    ev.businessFilesAfterExit = businessFiles();
    ev.allBusinessChangesAfterExit = changedBusinessFiles(
      inst.prelaunchBusinessFiles,
      snapshotBusinessFiles(dataDir),
    );
    ck.add(
      "all_non_settings_files_unchanged_after_rejection",
      ev.allBusinessChangesAfterExit.length === 0,
      JSON.stringify(ev.allBusinessChangesAfterExit),
    );
    ck.add(
      "rejection_leaves_no_business_files",
      ev.businessFilesAfterExit.length === 0,
      ev.businessFilesAfterExit.join(",") || "无业务文件",
    );
    ev.settingsAfter = fs.readFileSync(settingsPath, "utf8");
    ck.add(
      "rejection_writes_no_consent_record",
      !/privacy_policy_accepted\s*=\s*true/.test(ev.settingsAfter),
      "拒绝不得留下同意记录",
    );
    ev.rectAfterExit = (
      ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "mainrect"]).stdout || ""
    ).trim();
    ck.add(
      "no_tray_process_after_reject",
      inst.proc.exitCode !== null && /MAINRECT none/i.test(ev.rectAfterExit),
      ev.rectAfterExit || "无窗口",
    );
  },

  /**
   * P06 同意记录跨重启：点击「同意并继续」→ 原子落盘 + 业务初始化；
   * 重启同一隔离根不再询问（第 9 条验收：有有效 true 时不询问）。
   */
  async "privacy-accept-restart"({ inst, ck, ev }) {
    const page = inst.page;
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });
    await page.locator(".privacy-actions button", { hasText: "同意并继续" }).first().click();
    await page.waitForSelector(".app-nav", { timeout: 60000 });
    ev.settingsAfterAccept = fs.readFileSync(settingsPath, "utf8");
    ck.add(
      "accept_writes_consent_record",
      /privacy_policy_accepted\s*=\s*true/.test(ev.settingsAfterAccept),
      ev.settingsAfterAccept.split("\n")[0],
    );
    // 弹窗关闭以「不可见」为准：NModal 关闭过渡期间 DOM 可能仍在
    // （count 不判可见性），业务界面已挂载即证明闸门放行。
    ev.consentDialogAfterAccept = await page.locator(".privacy-dialog").count();
    ev.consentDialogVisibleAfterAccept = ev.consentDialogAfterAccept
      ? await page.locator(".privacy-dialog").first().isVisible()
      : false;
    ck.add(
      "consent_dialog_closed_after_accept",
      ev.consentDialogAfterAccept === 0 || ev.consentDialogVisibleAfterAccept === false,
      `DOM 残留=${ev.consentDialogAfterAccept} 可见=${ev.consentDialogVisibleAfterAccept}`,
    );

    // 重启同一隔离根：显式结束上一实例（业务运行中不会自行退出），
    // 再以同一隔离根重新启动——验证重启不再询问。
    await inst.stop({ terminate: true, reason: "restart-explicit" });
    const info = await inst.launch();
    ev.relaunchPid = info.pid;
    ev.relaunchPort = info.port;
    ev.relaunchLaunchIndex = info.launchIndex;
    const page2 = inst.page;
    await page2.waitForSelector(".app-nav", { timeout: 60000 });
    await page2.waitForTimeout(1500);
    ev.relaunchConsentDialog = await page2.locator(".privacy-dialog").count();
    ck.add(
      "restart_does_not_ask_again",
      ev.relaunchConsentDialog === 0,
      `重启后政策弹窗数=${ev.relaunchConsentDialog}`,
    );
    await page2.screenshot({ path: path.join(inst.opts.shots, "relaunch-ready.png") });
  },

  /**
   * AC06 场景 1：预置业务文件 + 未同意观察——已存在的业务数据必须
   * 零读取零写入；追踪零业务事件；价格桩与外连零命中；实际观察 15 秒
   * （单调时钟记录起止），启动前预置快照 × 观察后快照双向比对。
   */
  async "privacy-preseeded"({ inst, ck, ev }) {
    const page = inst.page;
    const dataDir = path.join(inst.root, "tokenscope");
    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });

    // §4b.1：未同意状态实际观察 15 秒。before 取启动前预置快照
    // （prepareRoot 在 mutate 后、launch 前采集），启动期间的写入也会
    // 被观察后的快照捕获，不以「场景开始时」的快照漏掉启动期变化。
    const before = { business: inst.prelaunchBusinessFiles, settings: inst.prelaunchSettings };
    const observedFrom = performance.now();
    await page.waitForTimeout(15000);
    const observedTo = performance.now();
    const observedMs = observedTo - observedFrom;

    const businessEvents = businessTraceEvents(inst);
    ck.add(
      "preseeded_no_business_trace",
      businessEvents.length === 0,
      businessEvents.join(",") || "无业务事件",
    );

    const hits = inst.opts.controlledIo.snapshot().hits;
    const hitCount = Object.values(hits).reduce((n, rows) => n + rows.length, 0);
    ck.add("preseeded_price_stub_untouched", hitCount === 0, `桩命中=${hitCount}`);
    ck.add(
      "preseeded_proxy_attempts_zero",
      inst.opts.controlledIo.snapshot().proxy_attempts.length === 0,
      "计数代理未记录外连尝试",
    );

    ev.businessDomNodes = await page.locator(".app-nav, .page-head, .chart-canvas").count();
    ck.add(
      "preseeded_no_business_dom",
      ev.businessDomNodes === 0,
      `业务节点=${ev.businessDomNodes}`,
    );

    // 双快照不变：业务文件（字节/大小/mtime；预置、新增、删除、修改，
    // 含 history.db/-wal/-shm）+ settings.toml 逐字节。
    const changes = dataDirChanges(before, snapshotDataDir(dataDir));
    ck.add(
      "preseeded_business_files_unchanged",
      changes.length === 0,
      changes.join(",") || "双快照一致",
    );
    ck.add(
      "preseeded_observed_full_window",
      observedMs >= 15000 - 50,
      `实际观察=${observedMs.toFixed(0)}ms（要求 ≥15000ms）`,
    );
    ev.preseeded = { observedMs: Math.round(observedMs), changes };
    await page.screenshot({ path: path.join(inst.opts.shots, "privacy-preseeded-observed.png") });

    // 未同意退出：不留业务残留、进程干净退出。
    await rejectAndExit(page, inst, ck, "preseeded");
  },

  /**
   * AC06 场景 2：未同意状态下全量受保护命令拒绝——每条命令都以最小
   * 合法参数经真实 IPC 调用，全部返回隐私拒绝；追踪与文件零变化；
   * 全蜂巢注册表只读快照前后一致（辅助证据）。
   */
  async "privacy-denied-ipc"({ inst, ck, ev }) {
    const page = inst.page;
    const dataDir = path.join(inst.root, "tokenscope");
    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });

    const commands = readProtectedCommands();
    ev.deniedIpc = { commands: commands.length, results: [] };
    ck.add(
      "protected_commands_extracted_from_source",
      commands.length >= 35,
      `命令数=${commands.length}`,
    );

    const before = snapshotDataDir(dataDir);
    const registryBefore = registrySnapshot();

    for (const command of commands) {
      const args = MINIMAL_INVOKER_ARGS[command];
      if (args === undefined) throw new Error(`缺少 ${command} 的最小合法参数`);
      const outcome = await page.evaluate(
        ([cmd, invokeArgs]) =>
          window.__TAURI_INTERNALS__
            .invoke(cmd, invokeArgs)
            .then(() => ({ status: "fulfilled", error: "" }))
            .catch((error) => ({ status: "rejected", error: String(error?.message ?? error) })),
        [command, args],
      );
      ev.deniedIpc.results.push({ command, ...outcome });
      if (outcome.status !== "rejected" || !outcome.error.includes("privacy_consent_required")) {
        throw new Error(`${command} 未被隐私闸门拒绝：${JSON.stringify(outcome)}`);
      }
    }
    ck.add(
      "all_protected_commands_rejected_pre_consent",
      ev.deniedIpc.results.length === commands.length &&
        ev.deniedIpc.results.every(
          (r) => r.status === "rejected" && r.error.includes("privacy_consent_required"),
        ),
      `全部 ${commands.length} 条受保护命令返回 privacy_consent_required`,
    );

    const businessEvents = businessTraceEvents(inst);
    ck.add(
      "denied_ipc_trace_has_no_business_events",
      businessEvents.length === 0,
      businessEvents.join(",") || "无业务追踪",
    );

    const after = snapshotDataDir(dataDir);
    const changes = dataDirChanges(before, after);
    ck.add("denied_ipc_files_unchanged", changes.length === 0, changes.join(",") || "文件零变化");

    const registryAfter = registrySnapshot();
    const registryChanged = Object.keys(registryBefore).filter(
      (key) => JSON.stringify(registryBefore[key]) !== JSON.stringify(registryAfter[key]),
    );
    ck.add(
      "registry_snapshot_unchanged",
      registryChanged.length === 0,
      registryChanged.join(",") || "四蜂巢快照一致",
    );

    // 未同意退出：不留业务残留、进程干净退出。
    await rejectAndExit(page, inst, ck, "denied_ipc");
  },

  /**
   * AC06 场景 3：同意原子落盘顺序 + 双击单次初始化——consent_saved
   * 严格早于 business_ready 与首个业务读取；双击只落盘一次。
   */
  async "privacy-accept-order"({ inst, ck, ev }) {
    const page = inst.page;
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });
    // 双击（§4b.1：CDP 连续两次 click，间隔 <100ms；第二次落在保存中/
    // 已保存状态）：两次都用坐标级 page.mouse 下发真实输入——不依赖按钮
    // 在第二次点击时仍存活；先挂页面级捕获计数器证明第二次输入真实到达
    // 页面，区分「产品正确抑制」与「测试架根本没发出」；下发失败显式
    // 失败，绝不吞掉。
    const consentButton = page
      .locator(".privacy-actions button", { hasText: "同意并继续" })
      .first();
    // 按钮稳定性等待（八轮复核：坐标先取后点，布局动画会让坐标失效——
    // 失败记录的点击坐标与超时截图中按钮位置不一致即此嫌疑）。九轮复核
    // 修正：**跨帧**采样（首帧内两次相同不证明动画结束）、采样次数耗尽
    // 或按钮消失即失败，不得继续点击、不得虚报稳定。
    const sampleBox = () => consentButton.boundingBox();
    let previous = await sampleBox();
    let stable = false;
    let box = previous;
    for (let samples = 0; samples < 20; samples += 1) {
      await page.waitForTimeout(100);
      box = await sampleBox();
      if (!box) throw new Error("同意按钮在稳定性等待期间消失，拒绝点击");
      if (
        previous &&
        box.x === previous.x &&
        box.y === previous.y &&
        box.width === previous.width &&
        box.height === previous.height
      ) {
        stable = true;
        break;
      }
      previous = box;
    }
    if (!stable) {
      throw new Error(
        `同意按钮位置 20 次跨帧采样后仍不稳定（最后位置 ${JSON.stringify(box)}），拒绝点击`,
      );
    }
    ev.acceptClicks = ev.acceptClicks ?? {};
    ev.acceptClicks.buttonBoxAtClick = box;
    // 计数器挂载同样 8 秒有界：CDP 挂起时不能无限阻塞场景（七轮复核）。
    await withTimeout(
      () =>
        page.evaluate(() => {
          window.__ac06ConsentClicks = [];
          for (const type of ["pointerdown", "click"]) {
            document.addEventListener(
              type,
              (event) => {
                window.__ac06ConsentClicks.push({
                  type,
                  ts: event.timeStamp,
                  x: event.clientX,
                  y: event.clientY,
                  // 目标元素：document 级捕获只记坐标无法证明点在同意按钮上
                  // （八轮复核——点别的元素同样得到两组事件）。
                  target: {
                    tag: event.target.tagName,
                    cls: String(event.target.className || "").slice(0, 80),
                    text: String(event.target.textContent || "").trim().slice(0, 40),
                  },
                });
              },
              true,
            );
          }
          // 十五轮复核：给同意按钮打**不随文案变化**的结构标记（.privacy-
          // actions 内最后一个按钮=同意；保存中会变「正在保存…」而按钮
          // 仍在）——正常采集与失败取证共用这个标识，按文案过滤会失去
          // 保存中的按钮。
          const consentButtons = Array.from(
            document.querySelectorAll(".privacy-actions button"),
          );
          const consent = consentButtons[consentButtons.length - 1];
          if (consent) consent.dataset.ac06Consent = "1";
        }),
      8000,
      "挂载同意输入计数器",
    );
    const inputX = box.x + box.width / 2;
    const inputY = box.y + box.height / 2;
    ck.add(
      "button_position_stable_before_click",
      stable === true,
      `点击前跨帧采样稳定=${stable}（x=${box.x} y=${box.y} w=${box.width} h=${box.height}）`,
    );
    // 顺序下发（并发排队会在 CDP 输入管线里丢掉第二次完整点击——首跑实测
    // 同意键坐标只到达 1 次）；间隔仍以页面事件时间戳度量。
    await page.mouse.click(inputX, inputY);
    await page.mouse.click(inputX, inputY);
    // 业务解锁等待：超时即失败，但先抓失败现场（六轮复核缺口——此前超时
    // 直接抛错，点击计数、隐私框状态、截图全部丢失，CI 两轮 .app-nav
    // 超时无法归因）。取证失败不掩盖原错误。
    try {
      await page.waitForSelector(".app-nav", { timeout: 60000 });
      // 点击后按钮位置证据（八轮复核：判断点击坐标是否因布局变化失效）。
      // 十轮复核 P2：区分合法 null（按钮不存在）与采集异常——吞成 null
      // 会让「查询失败」伪装成「按钮不存在」，证据无法归层。
      ev.acceptClicks = ev.acceptClicks ?? {};
      try {
        // 十三轮复核：存在性判断与位置读取合并为**一次**页面内采集——
        // count() 与 boundingBox() 分开执行时，正常退出动画恰好在两次查询
        // 之间完成会导致 count=1 而 boundingBox 等待超时，合法移除被误判
        // 为查询失败。单次读取：元素缺失 → null（合法移除），通信异常 →
        // buttonBoxAfterClickError。
        const captured = await page.evaluate(() => {
          // 十五轮复核：按**不随文案变化**的标记定位（保存中按钮会变
          // 「正在保存…」而仍在；按文案过滤会误判为已移除）。
          const button = document.querySelector("[data-ac06-consent]");
          if (!button) return { exists: false };
          const rect = button.getBoundingClientRect();
          return {
            exists: true,
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
          };
        });
        ev.acceptClicks.buttonBoxAfterClick = captured.exists
          ? { x: captured.x, y: captured.y, width: captured.width, height: captured.height }
          : null;
      } catch (boxError) {
        ev.acceptClicks.buttonBoxAfterClickError = String(boxError).slice(0, 300);
      }
      // 十一轮复核：位置记录断言在**采集后**依据实际结果生成，区分三种
      // 形态——查询失败（错误字段）、合法 null（按钮不存在）、成功（两
      // 位置俱全）——不得在查询前固定为绿并声称双位置已入 evidence。
      const recordedAfter =
        "buttonBoxAfterClick" in ev.acceptClicks
          ? `事后位置=${JSON.stringify(ev.acceptClicks.buttonBoxAfterClick)}`
          : `事后位置查询失败=${ev.acceptClicks.buttonBoxAfterClickError}`;
      ck.add(
        "button_position_recorded",
        ev.acceptClicks.buttonBoxAtClick !== undefined &&
          "buttonBoxAfterClick" in ev.acceptClicks,
        `点击时位置=${JSON.stringify(ev.acceptClicks.buttonBoxAtClick)}；${recordedAfter}`,
      );
    } catch (error) {
      // 七轮复核修正：取证**逐项**采集、每项 8 秒上限（复用 withTimeout）、
      // 单项失败只追加该项错误——任何一步失败（截图异常、CDP 拒绝、CDP
      // 挂起）都不得清空已取得的现场，也不得拖到场景看门狗。
      const diagnostics = { errorText: String(error).slice(0, 300) };
      const bounded = (label, task) =>
        withTimeout(task, 8000, `取证 ${label}`)
          .then((value) => {
            diagnostics[label] = value;
          })
          .catch((diagError) => {
            diagnostics[`${label}Error`] = String(diagError).slice(0, 300);
          });
      await bounded("capturedClicks", () =>
        page.evaluate(() => window.__ac06ConsentClicks ?? []),
      );
      await bounded("privacyDialogCount", () => page.locator(".privacy-dialog").count());
      // 八轮复核 P2：内层 .catch(() => []) 会把查询失败伪装成「没有文案」
      // ——吞错移除，失败交给外层 bounded 记 privacyBlockedDetailError。
      await bounded("privacyBlockedDetail", () =>
        page.locator(".privacy-detail, .privacy-error, [class*='error']").allTextContents(),
      );
      await bounded("buttonBoxAtClick", async () => box ?? null);
      // 九轮复核：失败时按钮的**事后**位置——与点击时位置对比即可判断
      // 布局是否在点击后变化（按钮消失时为 null，同样入证据）。十五轮
      // 复核：改用与正常采集共用的**不随文案变化**的标记定位（点击前
      // 按旧文案建立的 locator 在「保存中·未解锁」时会等待文本失配超时，
      // 按钮仍在却被记为取证超时）；单次页面内采集 + 8 秒 bounded 超时
      // 保留。
      await bounded("buttonBoxAfterFailure", () =>
        page.evaluate(() => {
          const button = document.querySelector("[data-ac06-consent]");
          if (!button) return null;
          const rect = button.getBoundingClientRect();
          return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
        }),
      );
      await bounded("settingsSnapshot", async () =>
        fs.existsSync(settingsPath)
          ? fs.readFileSync(settingsPath, "utf8").slice(0, 800)
          : null,
      );
      await bounded("traceSoFar", async () =>
        readAcceptanceTrace(inst.root, { partial: true }).records.map((r) => ({
          seq: r.seq,
          event: r.event,
        })),
      );
      await bounded("screenshot", () =>
        page
          .screenshot({
            path: path.join(inst.opts.shots, "accept-order-unlock-timeout.png"),
          })
          .then(() => "accept-order-unlock-timeout.png"),
      );
      ev.acceptFailureDiagnostics = diagnostics;
      throw error;
    }
    await page.waitForTimeout(1500);

    const capturedClicks = await page.evaluate(() => window.__ac06ConsentClicks ?? []);
    const inBox = (entry) =>
      entry.x >= box.x &&
      entry.x <= box.x + box.width &&
      entry.y >= box.y &&
      entry.y <= box.y + box.height;
    // 送达判定按 pointerdown：同意过渡会替换光标下的 DOM，浏览器因此可能
    // 不合成第二次 click 事件（实跑实测：pointerdown#2 在 21.6ms 后到达正确
    // 坐标、click 未合成）——这正是「输入已发出、产品正确抑制」的形态；
    // click 合成与否属产品行为，不作为输入送达证据。
    const pressesAtButton = capturedClicks.filter((c) => c.type === "pointerdown" && inBox(c));
    const synthesizedClicks = capturedClicks.filter((c) => c.type === "click" && inBox(c)).length;
    const inputIntervalMs =
      pressesAtButton.length >= 2
        ? pressesAtButton[pressesAtButton.length - 1].ts - pressesAtButton[0].ts
        : null;
    // 九轮复核：**展开**保留稳定性等待写入的 buttonBoxAtClick 等
    // 字段——整体重写会把位置证据清掉。
    ev.acceptClicks = {
      ...ev.acceptClicks,
      delivered: pressesAtButton.length,
      intervalMs: inputIntervalMs,
      synthesizedClicks,
      captured: capturedClicks,
    };
    ck.add(
      "double_click_delivered",
      pressesAtButton.length >= 2,
      `同意键坐标实际按下=${pressesAtButton.length} 次（其中合成 click ${synthesizedClicks} 次）`,
    );
    ck.add(
      "double_click_interval_under_100ms",
      pressesAtButton.length >= 2 && inputIntervalMs !== null && inputIntervalMs < 100,
      `输入间隔=${inputIntervalMs === null ? "-" : inputIntervalMs.toFixed(1)}ms（要求 <100ms）`,
    );

    const trace = readAcceptanceTrace(inst.root, { partial: true });
    if (trace.violations.length) throw new Error(trace.violations.join(" | "));
    const seqOf = (name) => trace.records.filter((r) => r.event === name).map((r) => r.seq);
    const consentSeqs = seqOf("consent_saved");
    const readySeqs = seqOf("business_ready");
    const readSeqs = seqOf("history_open").concat(seqOf("source_root_read"));
    ev.acceptOrder = { consentSeqs, readySeqs, readSeqs: readSeqs.slice(0, 3) };
    ck.add(
      "consent_saved_exactly_once",
      consentSeqs.length === 1,
      `consent_saved=${consentSeqs.length} 次`,
    );
    ck.add(
      "business_ready_exactly_once",
      readySeqs.length === 1,
      `business_ready=${readySeqs.length} 次`,
    );
    // 严格成序：consent_saved < business_ready < 首个业务读取。旧的合并式
    // 断言只查 read > max(consent, ready)，会放过 ready 先于 consent 与
    // consent 缺失两种颠倒（PR52 复盘反例），保留为兜底并新增两条具名。
    ck.add(
      "consent_saved_precedes_business_ready",
      consentSeqs.length === 1 && readySeqs.length === 1 && consentSeqs[0] < readySeqs[0],
      `consent=${consentSeqs.join(",")} ready=${readySeqs.join(",")}`,
    );
    ck.add(
      "business_ready_precedes_first_business_read",
      readySeqs.length === 1 && readSeqs.length >= 1 && readySeqs[0] < Math.min(...readSeqs),
      `ready=${readySeqs.join(",")} firstRead=${Math.min(...(readSeqs.length ? readSeqs : [Infinity]))}`,
    );
    ck.add(
      "consent_saved_precedes_business_unlock_and_reads",
      consentSeqs.length === 1 &&
        readySeqs.length === 1 &&
        readSeqs.length >= 1 &&
        Math.min(...readSeqs) > Math.max(...consentSeqs, ...readySeqs),
      JSON.stringify(ev.acceptOrder),
    );
    // 单次原子落盘（口径对齐十七轮复核：计划判定的是**同意保存**恰一次
    // 原子替换；业务初始化后的规范化重写是合法的第二次 settings_write，
    // 不计入本判定）：consent_saved 事件自带落盘时刻的**不可变字节证据**
    // （settings_bytes_hex，Rust 侧保存时快照，JSONL 只追加不可变）与指纹
    // （fnv1a64，与 tokenscope-core settings_hash 同算法）。判定方重算
    // 证据哈希与事件指纹比对，并解析证据内容确认同意值与当前修订号——
    // 只查格式不核对内容会放行假哈希（二轮复核缺口，复核方独立复现）。
    // 实跑证明业务初始化会在同意之后合法地再次规范化重写 settings（终态
    // 字节 ≠ 保存时字节），因此不与终态文件硬比；终态仍含同意字段由
    // accept_writes_consent_with_revision 保证。
    const consentRows = trace.records.filter((r) => r.event === "consent_saved");
    const settingsWriteSeqs = trace.records
      .filter((r) => r.event === "settings_write")
      .map((r) => r.seq);
    const writesBeforeConsent = settingsWriteSeqs.filter((s) => s < consentSeqs[0]);
    const adjacentWrite =
      writesBeforeConsent.length === 1 &&
      !trace.records.some(
        (r) => r.seq > writesBeforeConsent[0] && r.seq < consentSeqs[0],
      );
    const captureBytes = (() => {
      const hex = consentRows[0]?.settings_bytes_hex;
      if (typeof hex !== "string" || hex.length === 0 || hex.length % 2 !== 0) return null;
      if (/[^0-9a-f]/.test(hex)) return null;
      return Buffer.from(hex, "hex");
    })();
    const recomputed =
      captureBytes === null ? null : settingsFingerprint(captureBytes).value;
    const hashMatchesCapture =
      recomputed !== null &&
      consentRows[0]?.settings_hash?.algorithm === "fnv1a64" &&
      /^[0-9a-f]{16}$/.test(consentRows[0]?.settings_hash?.value ?? "") &&
      recomputed === consentRows[0].settings_hash.value;
    ck.add(
      "consent_saved_single_atomic_replace",
      consentSeqs.length === 1 &&
        writesBeforeConsent.length === 1 &&
        adjacentWrite &&
        captureBytes !== null &&
        hashMatchesCapture,
      `指纹=${JSON.stringify(consentRows[0]?.settings_hash)} 重算=${recomputed ?? "无字节证据"} 同意前写入=${JSON.stringify(writesBeforeConsent)}`,
    );
    // 十九轮复核 P2：同意保存与业务解锁之间**不得再有设置写入**——旧判定
    // 只看写入发生在 consent_saved 之前，把「consent_saved → 额外写入 →
    // business_ready」的追踪也放行（受控反例 review19-independent/
    // extra-write-probe.json 16 项全过）。按写入来源分层判定：同意前恰一次
    // （同意落盘），同意后到业务解锁前零次；业务初始化后的重写另计。
    const writesBetweenConsentAndReady = settingsWriteSeqs.filter(
      (seq) => seq > consentSeqs[0] && seq < readySeqs[0],
    );
    const writesAfterReady = settingsWriteSeqs.filter((seq) => seq > readySeqs[0]);
    // 二十轮复核 P2：写入归属必须按**来源**（追踪事件自带的 origin），
    // 不能只按时间把解锁后的全部写入都算作「业务初始化」——否则注入两次
    // 解锁后写入仍会被归入同一类而全过。
    const settingsWrites = trace.records.filter((r) => r.event === "settings_write");
    const originOf = (seq) => settingsWrites.find((r) => r.seq === seq)?.origin ?? null;
    const afterReadyOrigins = writesAfterReady.map(originOf);
    // 允许出现在解锁后的业务初始化来源（每个至多一次）。
    const ALLOWED_RUNTIME_WRITE_ORIGINS = ["source_path_migration", "business_init"];
    const unknownRuntimeOrigins = afterReadyOrigins.filter(
      (origin) => !ALLOWED_RUNTIME_WRITE_ORIGINS.includes(origin),
    );
    const duplicatedRuntimeOrigins = ALLOWED_RUNTIME_WRITE_ORIGINS.filter(
      (origin) => afterReadyOrigins.filter((value) => value === origin).length > 1,
    );
    ev.consentWriteLayers = {
      beforeConsent: writesBeforeConsent.map((seq) => ({ seq, origin: originOf(seq) })),
      betweenConsentAndReady: writesBetweenConsentAndReady.map((seq) => ({
        seq,
        origin: originOf(seq),
      })),
      afterReady: writesAfterReady.map((seq) => ({ seq, origin: originOf(seq) })),
    };
    ck.add(
      "consent_save_is_the_only_write_before_business_unlock",
      writesBetweenConsentAndReady.length === 0,
      `同意后、解锁前额外写入=${JSON.stringify(ev.consentWriteLayers.betweenConsentAndReady)}`,
    );
    // 同意保存必须是来源为 privacy_consent 的那一次写入。
    ck.add(
      "consent_save_write_origin_is_privacy_consent",
      writesBeforeConsent.length === 1 && originOf(writesBeforeConsent[0]) === "privacy_consent",
      `同意前写入来源=${JSON.stringify(ev.consentWriteLayers.beforeConsent)}`,
    );
    // 解锁后的写入必须来自已知业务初始化来源，且每个来源至多一次。
    ck.add(
      "runtime_writes_have_known_single_origin",
      unknownRuntimeOrigins.length === 0 && duplicatedRuntimeOrigins.length === 0,
      `解锁后写入=${JSON.stringify(ev.consentWriteLayers.afterReady)}；未知来源=${JSON.stringify(unknownRuntimeOrigins)}；重复来源=${JSON.stringify(duplicatedRuntimeOrigins)}`,
    );
    // 严格 UTF-8 解码（五轮复核 P2：toString("utf8") 默认把坏字节替换成
    // U+FFFD，替换后的文本骗过两个解析器——注释中 FF、截断编码、过长编码、
    // 代理项编码全被误放行，而产品 String::from_utf8 报 InvalidData）。
    // fatal 模式解码失败即拒绝；合法编码的 U+FFFD 字符本身不受影响。
    const captureText = (() => {
      if (captureBytes === null) return "";
      try {
        return new TextDecoder("utf-8", { fatal: true }).decode(captureBytes);
      } catch {
        return null;
      }
    })();
    // 内容判定经**两个**真正的 TOML 解析器交叉（三轮/四轮复核：正则会被
    // 注释、子表与前缀骗过；smol-toml 单独使用会把 2.0/2e0/超精度浮点当成
    // 整数 2、并接受 2026-02-30 这类语义非法日期）：
    // - smol-toml：整篇合法性（值语义、抛错即拒）；
    // - toml-eslint-parser：**AST 保留字面量类型**——顶层
    //   privacy_policy_accepted 必须是 boolean 字面量且值为 true、顶层
    //   privacy_policy_revision 必须是 integer 字面量且值为 2；子表键、
    //   内联表、多段键路径一律不算顶层；非法日期在解析时即被拒绝。
    const captureContent = (() => {
      if (captureBytes === null) return { ok: false, detail: "无字节证据" };
      if (captureText === null) {
        return { ok: false, detail: "字节证据不是合法 UTF-8（严格解码失败，产品同口径 InvalidData）" };
      }
      try {
        parseToml(captureText);
      } catch (error) {
        return { ok: false, detail: `字节证据不是合法 TOML：${error}` };
      }
      let program;
      try {
        program = parseTOML(captureText);
      } catch (error) {
        return { ok: false, detail: `字节证据未通过严格 TOML 解析：${error}` };
      }
      const top = program.body.find((table) => table.type === "TOMLTopLevelTable");
      const topLevelValue = (name) => {
        for (const kv of top?.body ?? []) {
          const keys = kv.key.keys;
          // TOMLBare 键名在 name；TOMLQuoted（单引号/双引号/转义形式）在
          // value——五轮复核 P3：只读 content 会把合法带引号键误判缺失。
          if (
            keys.length === 1 &&
            (keys[0].name ?? keys[0].content ?? keys[0].value) === name
          ) {
            return kv.value;
          }
        }
        return null;
      };
      const accepted = topLevelValue("privacy_policy_accepted");
      const revision = topLevelValue("privacy_policy_revision");
      // 2026-10-28：用户许可协议同意与隐私政策同意同一次保存写入；
      // 终态必须四项俱全且修订为当前值（旧用户不被放行的证据链终点）。
      const agreementAccepted = topLevelValue("user_agreement_accepted");
      const agreementRevision = topLevelValue("user_agreement_revision");
      const ok =
        accepted?.kind === "boolean" &&
        accepted.value === true &&
        revision?.kind === "integer" &&
        revision.value === CURRENT_CONSENT_REVISION &&
        agreementAccepted?.kind === "boolean" &&
        agreementAccepted.value === true &&
        agreementRevision?.kind === "integer" &&
        agreementRevision.value === CURRENT_CONSENT_REVISION;
      return {
        ok,
        detail:
          `accepted kind=${accepted?.kind} value=${JSON.stringify(accepted?.value)}；` +
          `revision kind=${revision?.kind} value=${JSON.stringify(revision?.value)}；` +
          `agreement kind=${agreementAccepted?.kind} value=${JSON.stringify(agreementAccepted?.value)}；` +
          `agreementRevision kind=${agreementRevision?.kind} value=${JSON.stringify(agreementRevision?.value)}` +
          `（顶层须布尔 true / 整数 ${CURRENT_CONSENT_REVISION} 各两项）`,
      };
    })();
    ck.add("consent_saved_capture_content", captureContent.ok, captureContent.detail);
    // 十七轮复核 P2：终态 settings 与保存字节证据**同口径**严格判定——
    // 正则搜索会被注释假字段骗过（复核方复现：保存证据正确、终态实际
    // accepted=false/revision=1，注释含期望字段即全过）。严格 UTF-8 +
    // TOML 解析 + 顶层类型/值检查；业务初始化后合法重写按终态解析
    // （不与保存字节比对，重写正例由 consent_saved_capture_content 层保证）。
    const terminalCheck = (() => {
      let terminalText;
      try {
        terminalText = new TextDecoder("utf-8", { fatal: true }).decode(
          fs.readFileSync(settingsPath),
        );
      } catch (decodeError) {
        return { ok: false, detail: `终态 settings 不是合法 UTF-8：${decodeError}` };
      }
      let program;
      try {
        parseToml(terminalText);
        program = parseTOML(terminalText);
      } catch (parseError) {
        return { ok: false, detail: `终态 settings 未通过严格 TOML 解析：${parseError}` };
      }
      const top = program.body.find((table) => table.type === "TOMLTopLevelTable");
      const topLevelValue = (name) => {
        for (const kv of top?.body ?? []) {
          const keys = kv.key.keys;
          if (keys.length === 1 && (keys[0].name ?? keys[0].content ?? keys[0].value) === name) {
            return kv.value;
          }
        }
        return null;
      };
      const accepted = topLevelValue("privacy_policy_accepted");
      const revision = topLevelValue("privacy_policy_revision");
      // 2026-10-28：终态必须同时证明接受了当前用户许可协议——
      // 只有隐私政策同意（哪怕修订号更高）不构成新版协议同意。
      const agreementAccepted = topLevelValue("user_agreement_accepted");
      const agreementRevision = topLevelValue("user_agreement_revision");
      const ok =
        accepted?.kind === "boolean" &&
        accepted.value === true &&
        revision?.kind === "integer" &&
        revision.value === CURRENT_CONSENT_REVISION &&
        agreementAccepted?.kind === "boolean" &&
        agreementAccepted.value === true &&
        agreementRevision?.kind === "integer" &&
        agreementRevision.value === CURRENT_CONSENT_REVISION;
      return {
        ok,
        detail:
          `accepted kind=${accepted?.kind} value=${JSON.stringify(accepted?.value)}；` +
          `revision kind=${revision?.kind} value=${JSON.stringify(revision?.value)}；` +
          `agreement kind=${agreementAccepted?.kind} value=${JSON.stringify(agreementAccepted?.value)}；` +
          `agreementRevision kind=${agreementRevision?.kind} value=${JSON.stringify(agreementRevision?.value)}` +
          `（顶层须布尔 true / 整数 ${CURRENT_CONSENT_REVISION} 各两项）`,
      };
    })();
    ck.add("accept_writes_consent_with_revision", terminalCheck.ok, terminalCheck.detail);

    // 自启命令层路由（方案 A / PR52 复盘补证）：helper 单测不能替代命令层
    // 证据——同意后经真实 IPC 开关自启，acceptance 构建必须路由到隔离替身：
    // 返回值与状态一致、状态文件落在隔离根、追踪路径在根内、四蜂巢注册表
    // 只读快照不变（真实后端不可达；本场景绝不写真实注册表）。
    const autostartRegistryBefore = registrySnapshot();
    const autostartSet = await page.evaluate(() =>
      window.__TAURI_INTERNALS__
        .invoke("autostart_set", { enabled: true })
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, error: String(error) })),
    );
    const autostartStatus = await page.evaluate(() =>
      window.__TAURI_INTERNALS__
        .invoke("autostart_status", {})
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, error: String(error) })),
    );
    ck.add(
      "autostart_routes_to_isolated_shim",
      autostartSet.ok && autostartStatus.ok && autostartStatus.value === true,
      `set=${JSON.stringify(autostartSet)} status=${JSON.stringify(autostartStatus)}`,
    );
    const autostartStatePath = path.join(inst.root, "autostart-state.json");
    let autostartStateFile = null;
    try {
      autostartStateFile = JSON.parse(fs.readFileSync(autostartStatePath, "utf8"));
    } catch {
      autostartStateFile = null;
    }
    ck.add(
      "autostart_state_file_in_root",
      autostartStateFile?.enabled === true,
      `状态文件=${JSON.stringify(autostartStateFile)}`,
    );
    const autostartTraceRows = (() => {
      // 自启 invoke 发生在场景早前的 trace 读取之后——必须重读，否则只能
      // 看到启动期的 boot 读取（首跑实测恰因此少一条 write）。
      const traceAfter = readAcceptanceTrace(inst.root, { partial: true });
      if (traceAfter.violations.length) throw new Error(traceAfter.violations.join(" | "));
      return traceAfter.records.filter(
        (r) => r.event === "autostart_write" || r.event === "autostart_read",
      );
    })();
    // 前缀比较双侧规范化：CI Runner 的 TEMP 可能是 8.3 短名（RUNNER~1）而
    // 追踪路径经 Rust canonicalize 是长名（runneradmin），且 Windows 路径
    // 大小写不敏感。**必须用 realpathSync.native**——JS 版 realpath 不解析
    // 8.3 短名（CI 40258699869 的 rootNorm 仍为 runner~1 即此因），native
    // 走 libuv GetFinalPathNameByHandle 才会展开；统一斜杠与大小写后再比较。
    const normPath = (value) => String(value).replaceAll("\\", "/").toLowerCase();
    const rootNorm = normPath(fs.realpathSync.native(inst.root));
    ck.add(
      "autostart_trace_paths_inside_root",
      autostartTraceRows.some((r) => r.event === "autostart_write") &&
        autostartTraceRows.some((r) => r.event === "autostart_read") &&
        autostartTraceRows.every(
          (r) => typeof r.path === "string" && normPath(r.path).startsWith(rootNorm),
        ),
      `root=${rootNorm}；` +
        (autostartTraceRows.map((r) => `${r.event}:${normPath(r.path)}`).join(",") || "无自启追踪"),
    );
    const autostartRegistryAfter = registrySnapshot();
    const autostartRegistryChanged = Object.keys(autostartRegistryBefore).filter(
      (key) =>
        JSON.stringify(autostartRegistryBefore[key]) !==
        JSON.stringify(autostartRegistryAfter[key]),
    );
    ck.add(
      "autostart_registry_untouched",
      autostartRegistryChanged.length === 0,
      autostartRegistryChanged.join(",") || "四蜂巢快照一致",
    );
    ev.autostartRouting = { set: autostartSet, status: autostartStatus };
    await page.screenshot({ path: path.join(inst.opts.shots, "privacy-accept-order.png") });
  },

  /**
   * AC06 场景 4a：损坏 TOML 启动进入阻断弹窗且原文件逐字节不变。
   */
  async "privacy-variants-corrupt"({ inst, ck, ev }) {
    const page = inst.page;
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    const before = fs.readFileSync(settingsPath, "utf8");
    ev.corruptSettings = { bytes: before.length };
    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });
    const text = (await page.locator(".privacy-dialog").innerText()).replace(/\s+/g, " ");
    ck.add(
      "corrupt_toml_shows_blocked_dialog",
      text.includes("重新检查") || text.includes("隐私设置无法确认"),
      text.slice(0, 140),
    );
    const after = fs.readFileSync(settingsPath, "utf8");
    ck.add("corrupt_toml_file_unchanged", after === before, "损坏设置文件逐字节不变");
    // 阻断态退出：不同意 → 退出程序（进程干净退出，无业务残留）。
    await rejectAndExit(page, inst, ck, "corrupt_toml");
  },

  /**
   * AC06 场景 4b：close_action 记忆预设不得绕过隐私退出确认——
   * 未同意时点关闭弹隐私退出确认，而不是按记忆最小化到托盘。
   */
  async "privacy-variants-close"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });
    const action = fileURLToPath(
      new URL("../../scripts/native-window-action.ps1", import.meta.url),
    );
    const result = spawnSyncBounded(
      "pwsh",
      ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
      { action: "window-close", stage: "variants-close" },
    );
    if (result.status !== 0 || !/CLOSED true/i.test(result.stdout))
      throw new Error(`WM_CLOSE failed: ${result.stderr} ${result.stdout}`);
    await page.waitForSelector(".exit-dialog", { timeout: 10000 });
    const exitText = (await page.locator(".exit-dialog").innerText()).replace(/\s+/g, " ");
    ck.add(
      "close_action_preset_still_asks_privacy_exit",
      exitText.includes("退出 TokenScope") && !exitText.includes("记住"),
      exitText.slice(0, 140),
    );
    await page.locator(".exit-actions button", { hasText: "退出程序" }).first().click();
    const deadline = Date.now() + 15000;
    while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
    ck.add("variants_close_exit_clean", inst.proc.exitCode === 0, `exitCode=${inst.proc.exitCode}`);
  },

  /**
   * WB05/WB07：旧修订同意触发重新询问——fixture 预置 accepted=true 但无
   * 修订号（v2 程序写入的形态）；新程序按未同意处理，弹窗顶部显示
   * 「隐私政策已更新」，同意前零业务读取；同意写入当前修订号；重启不再询问。
   */
  async "privacy-policy-update"({ inst, ck, ev }) {
    const page = inst.page;
    const dataDir = path.join(inst.root, "tokenscope");
    const settingsPath = path.join(dataDir, "settings.toml");
    const businessFiles = () =>
      listFiles(dataDir).filter((f) =>
        /^(logs\/|cache\.db|history\.db|window-state\.json|view-cache\.json|pricing-)/.test(f),
      );

    await page.waitForSelector(".privacy-dialog", { timeout: 30000 });
    ev.settingsBefore = fs.readFileSync(settingsPath, "utf8");
    ck.add(
      "fixture_keeps_old_consent_without_revision",
      /privacy_policy_accepted\s*=\s*true/.test(ev.settingsBefore) &&
        !/privacy_policy_revision/.test(ev.settingsBefore),
      ev.settingsBefore.split("\n").slice(0, 3).join(" | "),
    );
    const dialogText = (await page.locator(".privacy-dialog").innerText()).replace(/\s+/g, " ");
    ck.add(
      "policy_updated_notice_visible",
      dialogText.includes("隐私政策或用户许可协议已更新"),
      dialogText.slice(0, 160),
    );
    ev.businessDomNodes = await page.locator(".app-nav, .page-head, .chart-canvas").count();
    ck.add(
      "business_ui_not_mounted_before_reconsent",
      ev.businessDomNodes === 0,
      `业务节点数=${ev.businessDomNodes}`,
    );
    ev.businessFilesBeforeAccept = businessFiles();
    ck.add(
      "no_business_files_before_reconsent",
      ev.businessFilesBeforeAccept.length === 0,
      ev.businessFilesBeforeAccept.join(",") || "无业务文件",
    );
    // 同意前的验收追踪不得出现任何业务事件（评审 2026-10-25：只看文件清单
    // 会漏掉不落文件的业务读取）。
    const traceBeforeConsent = readAcceptanceTrace(inst.root, { partial: true });
    if (traceBeforeConsent.violations.length)
      throw new Error(traceBeforeConsent.violations.join(" | "));
    const businessEventNames = [
      "business_ready",
      "collect_round_start",
      "source_root_read",
      "history_open",
    ];
    ev.businessTraceBeforeConsent = traceBeforeConsent.records
      .filter((r) => businessEventNames.includes(r.event))
      .map((r) => r.event);
    ck.add(
      "no_business_trace_before_reconsent",
      ev.businessTraceBeforeConsent.length === 0,
      ev.businessTraceBeforeConsent.join(",") || "无业务追踪事件",
    );

    // 同意 → 同一次保存写入 true 与当前修订号，进入业务。
    await page.locator(".privacy-actions button", { hasText: "同意并继续" }).first().click();
    await page.waitForSelector(".app-nav", { timeout: 60000 });
    ev.settingsAfterAccept = fs.readFileSync(settingsPath, "utf8");
    ck.add(
      "reconsent_writes_current_revision",
      /privacy_policy_accepted\s*=\s*true/.test(ev.settingsAfterAccept) &&
        new RegExp(`privacy_policy_revision\\s*=\\s*${CURRENT_CONSENT_REVISION}`).test(
          ev.settingsAfterAccept,
        ) &&
        /user_agreement_accepted\s*=\s*true/.test(ev.settingsAfterAccept) &&
        new RegExp(`user_agreement_revision\\s*=\\s*${CURRENT_CONSENT_REVISION}`).test(
          ev.settingsAfterAccept,
        ),
      ev.settingsAfterAccept.split("\n").slice(0, 6).join(" | "),
    );
    // 同意后的业务初始化必须发生在同意落盘之后：business_ready 的 seq
    // 严格大于 consent_saved（顺序颠倒 = 同意前就有业务读取）。
    const traceAfterConsent = readAcceptanceTrace(inst.root, { partial: true });
    const seqOf = (name) =>
      traceAfterConsent.records.filter((r) => r.event === name).map((r) => r.seq);
    const consentSeqs = seqOf("consent_saved");
    const readySeqs = seqOf("business_ready");
    ev.consentTraceOrder = { consentSeqs, readySeqs };
    ck.add(
      "business_trace_starts_after_reconsent",
      consentSeqs.length >= 1 &&
        readySeqs.length >= 1 &&
        Math.min(...readySeqs) > Math.max(...consentSeqs),
      JSON.stringify(ev.consentTraceOrder),
    );

    // 重启同一隔离根：不再询问（新修订同意有效）。
    // 业务态窗口不会自行退出：先走真实的优雅关闭（WM_CLOSE → 「直接退出」），
    // 确认宿主退出后再 relaunch（与 ui-source-paths 的 closeForRestart 同型）。
    {
      const action = fileURLToPath(
        new URL("../../scripts/native-window-action.ps1", import.meta.url),
      );
      const result = spawnSyncBounded(
        "pwsh",
        ["-NoProfile", "-File", action, "-ProcessId", String(inst.proc.pid), "-Action", "close"],
        { action: "window-close", stage: "policy-update-close" },
      );
      if (result.status !== 0 || !/CLOSED true/i.test(result.stdout))
        throw new Error(`WM_CLOSE failed: ${result.stderr} ${result.stdout}`);
      await page.waitForSelector(".close-dialog", { timeout: 10000 });
      await page
        .locator(".close-dialog")
        .getByRole("button", { name: "直接退出", exact: true })
        .click();
      const deadline = Date.now() + 15000;
      while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
      if (inst.proc.exitCode !== 0)
        throw new Error(`重新询问场景的优雅关闭要求退出码 0，实际 ${inst.proc.exitCode}`);
    }
    const info = await inst.launch();
    ev.relaunchPid = info.pid;
    const page2 = inst.page;
    await page2.waitForSelector(".app-nav", { timeout: 60000 });
    await page2.waitForTimeout(1500);
    ev.relaunchConsentDialog = await page2.locator(".privacy-dialog").count();
    ck.add(
      "restart_does_not_ask_after_reconsent",
      ev.relaunchConsentDialog === 0,
      `重启后政策弹窗数=${ev.relaunchConsentDialog}`,
    );
    await page2.screenshot({ path: path.join(inst.opts.shots, "policy-update-reconsented.png") });
  },
};

const labelToValue = (label) =>
  ({ 浅色模式: "light", 深色模式: "dark", 跟随系统: "system" })[label] ?? null;

// ── 主流程 ────────────────────────────────────────────────────────
function freshRoot(tag) {
  // 名称刻意偏长：验收必须覆盖"当前生效目录"这类长路径的换行/不溢出（RC09/RC11）
  return path.join(
    process.env.TEMP || "/tmp",
    `tokenscope-native-acceptance-workspace-monorepo-${tag}-${crypto.randomUUID().replace(/-/g, "").slice(0, 8)}`,
  );
}

const STATE_FILE = path.join(OUT, "expire-state.json");

async function main() {
  if (!SCENARIO) throw new Error("必须给 --scenario");
  // §4.8 第 7 条：`--relaunch-cycles` 只属于独立的 ui-relaunch 场景
  if (RELAUNCH_CYCLES_GIVEN && SCENARIO !== "ui-relaunch") {
    throw new Error("--relaunch-cycles 只用于 ui-relaunch 场景（升级矩阵 §4.8 第 7 条）");
  }
  if (!Number.isInteger(RELAUNCH_CYCLES) || RELAUNCH_CYCLES < 1) {
    throw new Error(`--relaunch-cycles 必须是正整数：${arg("relaunch-cycles")}`);
  }
  fs.mkdirSync(OUT, { recursive: true });
  const stampDir = path.join(OUT, SCENARIO);
  fs.mkdirSync(stampDir, { recursive: true });

  const attachExisting = SCENARIO === "expire-finish";
  // 状态文件损坏（例如带 BOM）也要走证据流程，不能直接退出而没有 evidence.json（§4.8 第 5 条）
  let state = null;
  let stateError = null;
  if (attachExisting) {
    try {
      state = JSON.parse(fs.readFileSync(STATE_FILE, "utf8"));
    } catch (error) {
      stateError = `读取 ${STATE_FILE} 失败：${String(error?.message ?? error)}`;
    }
  }
  const root = ROOT || state?.root || freshRoot(SCENARIO);
  const scheme = THEME === "system" ? null : THEME;
  const shots = stampDir;
  const logPath = path.join(stampDir, "process.log");

  const realBefore = fingerprint(REAL_DATA);
  const ck = new Checks();
  // S00（计划 §9.1 第 2 条）：证据收集里的同步命令也有界
  let commit = "unknown";
  try {
    commit = spawnSyncBounded("git", ["rev-parse", "--short", "HEAD"], {
      action: "evidence-commit",
      options: { cwd: REPO },
    }).stdout.trim();
  } catch (e) {
    hb(`读取 commit 失败：${String(e?.message ?? e)}`);
  }
  let windowsVersion = "";
  try {
    windowsVersion = (
      spawnSyncBounded("cmd", ["/c", "ver"], { action: "evidence-windows-version" }).stdout || ""
    ).trim();
  } catch (e) {
    hb(`读取 Windows 版本失败：${String(e?.message ?? e)}`);
  }
  const ev = {
    scenario: SCENARIO,
    at: nowIso(),
    commit,
    feature: "acceptance",
    exe: EXE,
    exe_sha256: fs.existsSync(EXE) ? sha256File(EXE) : "missing",
    acceptance_root: root,
    theme_channel: THEME,
    forced_device_scale: SCALE,
    port: PORT,
    windows_version: windowsVersion,
    userprofile: process.env.USERPROFILE,
  };

  const cancelFlag = { requested: false };
  let inst;
  let isolationCleanup = null;
  const stopForIsolation = (detail) => {
    if (ev.isolation_violation) return;
    ev.isolation_violation = detail;
    ck.add("isolation_runtime_guard", false, detail);
    hb(`隔离检查失败，立即停止本次原生实例：${detail}`);
    isolationCleanup = beginIsolationCleanup({
      terminate: () =>
        inst?.current && !inst.attached
          ? terminateAfterSnapshot({
              refresh: () => {
                inst.disarmHostWatch();
                return inst.refreshDescendants(inst.current);
              },
              terminate: () => inst.terminateRecordedPids(inst.current, "isolation-violation"),
            })
          : { pids: [] },
      stop: () =>
        inst?.current
          ? inst.stop({ reason: "isolation-violation" })
          : { action: "no-host-process" },
      cancel: () => {
        cancelFlag.requested = true;
      },
    });
    isolationCleanup.catch((error) => {
      ev.isolation_stop_error = String(error?.message ?? error);
    });
  };
  const controlledIo = await startAcceptanceIo({ onViolation: stopForIsolation });
  inst = new Instance({
    root,
    events: attachExisting ? 0 : EVENTS,
    scheme,
    scale: SCALE,
    shots,
    logPath,
    relaunchCycles: RELAUNCH_CYCLES,
    controlledIo,
    // WB07：ui-source-paths 也用统一 fixture（两版 WorkBuddy 合成转写）。
    profile: ["ui-identity", "ui-ccs-import", "ui-pricing", "ui-source-paths"].includes(SCENARIO)
      ? "unified"
      : undefined,
    env: {
      ...controlledIo.env,
      ...(SCENARIO === "sync-failure"
        ? proxyEnvironment("http://127.0.0.1:9", "")
        : SCENARIO === "csp"
          ? { TOKENSCOPE_ACCEPTANCE_CSP: "1" }
          : SCENARIO === "close-remember-failure"
            ? { TOKENSCOPE_ACCEPTANCE_HIDE_FAILURE_ONCE: "1" }
            : {}),
    },
    mutate:
      SCENARIO === "ui-pricing"
        ? preparePricingScenario
        : SCENARIO === "fault-logs-file"
          ? (r) => {
              const p = path.join(r, "tokenscope", "logs");
              fs.mkdirSync(path.dirname(p), { recursive: true });
              if (fs.existsSync(p) && fs.statSync(p).isDirectory())
                throw new Error("logs 已是目录，无法注入文件故障");
              fs.writeFileSync(p, "not-a-directory\n");
            }
          : SCENARIO === "close-remember-failure"
            ? (r) => {
                const p = path.join(r, "tokenscope", "settings.toml");
                fs.writeFileSync(
                  p,
                  'close_action = "minimize"\n' + fs.readFileSync(p, "utf8"),
                  "utf8",
                );
              }
            : SCENARIO === "privacy-consent-gate" ||
                SCENARIO === "privacy-accept-restart" ||
                SCENARIO === "privacy-policy-update" ||
                SCENARIO === "privacy-preseeded" ||
                SCENARIO === "privacy-denied-ipc" ||
                SCENARIO === "privacy-accept-order" ||
                SCENARIO === "privacy-variants-close"
              ? (r) => {
                  // P06/WB05：privacy-consent-gate 与 privacy-accept-restart 要
                  // "未同意"状态——删掉隔离根预置的同意记录与政策修订号（其余
                  // 场景保留已同意基线，继续覆盖业务路径）。
                  // privacy-policy-update 要"旧修订同意"——保留 true，只剥离
                  // 修订号，让新程序触发「政策已更新」重新询问。
                  const p = path.join(r, "tokenscope", "settings.toml");
                  const text = fs.readFileSync(p, "utf8");
                  if (SCENARIO === "privacy-policy-update") {
                    const downgraded = text.replace(
                      /^[ \t]*privacy_policy_revision[ \t]*=[ \t]*\d+[ \t]*\r?\n?/m,
                      "",
                    );
                    if (downgraded === text)
                      throw new Error("隔离根 settings.toml 未含修订号行，无法构造旧同意场景");
                    const legacyOnly = downgraded
                      .replace(/^[ \t]*user_agreement_accepted[ \t]*=.*\r?\n?/m, "")
                      .replace(/^[ \t]*user_agreement_revision[ \t]*=.*\r?\n?/m, "");
                    fs.writeFileSync(p, legacyOnly, "utf8");
                    return;
                  }
                  if (SCENARIO === "privacy-variants-close") {
                    // AC06：记忆预设变体——剥离同意后追加 close_action，
                    // 验证未同意时点关闭仍弹隐私退出确认而非最小化。
                    // 2026-10-28：协议同意两项一并剥离（构造真实未同意状态）。
                    const stripped = text
                      .replace(/^[ \t]*privacy_policy_accepted[ \t]*=[ \t]*true[ \t]*\r?\n?/m, "")
                      .replace(/^[ \t]*user_agreement_accepted[ \t]*=.*\r?\n?/m, "")
                      .replace(/^[ \t]*user_agreement_revision[ \t]*=.*\r?\n?/m, "");
                    fs.writeFileSync(p, stripped + 'close_action = "minimize"\n', "utf8");
                    return;
                  }
                  const stripped = text
                    .replace(/^[ \t]*privacy_policy_accepted[ \t]*=[ \t]*true[ \t]*\r?\n?/m, "")
                    .replace(/^[ \t]*privacy_policy_revision[ \t]*=[ \t]*\d+[ \t]*\r?\n?/m, "")
                    .replace(/^[ \t]*user_agreement_accepted[ \t]*=.*\r?\n?/m, "")
                    .replace(/^[ \t]*user_agreement_revision[ \t]*=.*\r?\n?/m, "");
                  if (stripped === text)
                    throw new Error("隔离根 settings.toml 未含同意记录，无法构造未同意场景");
                  fs.writeFileSync(p, stripped, "utf8");
                  if (SCENARIO === "privacy-preseeded") {
                    // 十七轮复核 P2：计划 §4b.1 要求预置「旧修订布尔同意」
                    // （accepted=true + revision=1 → 新版 StaleRevision 重新
                    // 询问），不是剥离同意——观察窗口证明旧修订同意状态下
                    // 对预置业务数据零读取零写入。
                    // 2026-10-28：真实旧用户只有隐私政策同意、没有协议同意
                    // 记录——协议两项一并剥离，验证新版本不会把旧记录当作
                    // 已同意协议而放行。
                    const downgraded = text
                      .replace(
                        /^[ \t]*privacy_policy_revision[ \t]*=[ \t]*\d+[ \t]*\r?\n?/m,
                        "privacy_policy_revision = 1\n",
                      )
                      .replace(/^[ \t]*user_agreement_accepted[ \t]*=.*\r?\n?/m, "")
                      .replace(/^[ \t]*user_agreement_revision[ \t]*=.*\r?\n?/m, "");
                    if (downgraded === text)
                      throw new Error("隔离根 settings.toml 未含修订号行，无法构造旧修订同意场景");
                    fs.writeFileSync(p, downgraded, "utf8");
                    preseedBusinessFiles(path.join(r, "tokenscope"));
                    return;
                  }
                }
              : SCENARIO === "privacy-variants-corrupt"
                ? (r) => {
                    // AC06：损坏 TOML 变体——启动必须进入阻断弹窗且原文件
                    // 逐字节不变（prepare 产物整体替换为非法内容）。
                    const p = path.join(r, "tokenscope", "settings.toml");
                    fs.writeFileSync(p, "not [valid toml", "utf8");
                  }
                : undefined,
  });

  // S00（计划 §9.1）：场景取消标志 + 阶段记录 + 最小证据写盘点。
  // 场景与 Instance 在每次关闭/重启前自查取消标志（Promise.race 不会取消另一分支）。
  inst.opts.isCanceled = () => cancelFlag.requested;
  const isolationMonitor = setInterval(() => {
    try {
      const trace = readAcceptanceTrace(root, { partial: true });
      if (trace.violations.length) stopForIsolation(trace.violations.join(" | "));
    } catch (error) {
      stopForIsolation(`无法核对追踪：${String(error.message ?? error)}`);
    }
  }, 100);
  isolationMonitor.unref();
  const stage = createStageTracker({ log: hb, launchIndexOf: () => inst.current?.index ?? null });
  inst.opts.markStage = stage.mark;
  inst.opts.markStageDone = stage.markDone;
  const evidencePath = path.join(stampDir, "evidence.json");
  /** 最小证据（计划 §9.1 第 4 条）：同步写、不依赖 CDP；后续增强失败不能阻止它落盘。 */
  const writeEvidenceSnapshot = () => {
    fs.writeFileSync(
      evidencePath,
      JSON.stringify(
        {
          ...ev,
          last_stage: stage.last(),
          stages: stage.list(),
          host_pid: inst.current?.hostPid ?? null,
          exit_code: inst.proc?.exitCode ?? null,
          recorded_pids: mergePids([inst.current?.hostPid], inst.current?.descendantPids ?? []),
          attached: inst.attached === true,
          launches: buildLaunchesEvidence(inst.launches),
          checks: ck.rows,
          failed: ck.failures.map((f) => f.name),
          minimal_evidence: true,
          minimal_evidence_at: nowIso(),
        },
        null,
        2,
      ),
      "utf8",
    );
  };

  // 启动与场景执行整段可失败：失败也要写出 evidence.json（§4.8 第 5 条），
  // 因此这里不用 try/finally 包住中间步骤，而是把收尾与证据放在一个无条件执行的阶段。
  let setupError = null;
  let scenarioError = null;
  if (attachExisting && !stateError) {
    // attach 模式（expire-finish）：只连接既有实例，不认领、不结束它（§4.8）
    try {
      const attachRecord = await inst.attachToExisting(state);
      ev.attach_launch_index = attachRecord.index;
      ev.pid = state.pid;
      ev.reused_instance = true;
      ev.idle_from = state.at;
      ev.idle_deadline = state.idleDeadlineIso;
      ev.idle_actual_seconds = Math.round((Date.now() - Date.parse(state.at)) / 1000);
    } catch (error) {
      setupError = error;
      Object.assign(ev, errorEvidenceFields(error));
      hb(`attach 既有实例失败：${error?.message ?? error}`);
    }
  } else if (attachExisting) {
    // attach 模式但状态文件不可用：本次运行无法继续，证据照写（§4.8 第 5 条）
    setupError = new Error(stateError);
    ev.attach_state_error = stateError;
    ev.error = stateError;
    hb(`attach 模式状态文件不可用：${stateError}`);
  } else {
    try {
      const manifest = await inst.prepareRoot();
      ev.manifest_files = Object.keys(manifest.files ?? {}).length;
      ev.manifest_sha_first = manifest.files
        ? Object.values(manifest.files).map((f) => f.sha256.slice(0, 12))
        : [];
      if (SCENARIO === "first-frame") ev.expectStored = EXPECT_STORED;
      hb("启动 release/acceptance 实例并连接 CDP");
      const info = await inst.launch();
      hb(`PID=${info.pid} port=${info.port}`);
      if (SCENARIO === "fault-pricing-locked") {
        // 在进程启动前独占打开 pricing.toml（FileShare=None），让真实后端读取失败
        const target = path.join(root, "tokenscope", "pricing.toml");
        const log = fs.openSync(path.join(stampDir, "lock-holder.log"), "a");
        // 用 -File 跑脚本，并且**不能 detached**：实测 detached:true 下
        // PowerShell 立即以 0 退出、脚本体根本不执行（锁没持有、日志为空）。
        const holdScript = path.join(stampDir, "hold-pricing.ps1");
        fs.writeFileSync(
          holdScript,
          [
            "$ErrorActionPreference = 'Stop'",
            `$f = [System.IO.File]::Open('${target}', 'Open', 'Read', 'None')`,
            "Start-Sleep -Seconds 45",
            "$f.Close()",
            "",
          ].join(String.fromCharCode(13, 10)),
        );
        const holder = spawn(
          "powershell.exe",
          ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", holdScript],
          { stdio: ["ignore", log, log] },
        );
        holder.unref();
        inst.lockHolderPid = holder.pid;
        inst.lockTarget = target;
        inst.lockHeldFrom = nowIso();
        // PowerShell 冷启动要 1~2 s，而应用启动后 1~2 s 就读价格文件——不等到
        // 真正持有锁就启动，注入会整轮落空（实测过一次"等待 0 s"）。
        let locked = false;
        const lockDeadline = Date.now() + 15000;
        while (Date.now() < lockDeadline) {
          try {
            const fd = fs.openSync(target, "r+");
            fs.closeSync(fd);
            await sleep(150);
          } catch {
            locked = true;
            break;
          }
        }
        inst.lockConfirmed = locked;
      }
      if (flag("frames")) {
        // 连上第一件事就是抓帧：晚一帧就少一帧首帧证据
        inst.framesPromise = captureFrames(inst, path.join(stampDir, "frames"), {
          durationMs: Number(arg("frame-ms", "6000")),
          processStartedAt: inst.startedAt,
        });
      }
      ev.pid = info.pid;
      ev.port = info.port;
      ev.launch_index = info.launchIndex;
      ev.browser_args = info.browserArgs;
      ev.webview2_version = info.cdpVersion;
      await sleep(200);
    } catch (error) {
      // 启动失败：本次运行没有可用实例（保留原退出码 2 语义），
      // 但 launches[] 与宿主日志尾部仍要写出（§4.8 第 5 条）
      setupError = error;
      Object.assign(ev, errorEvidenceFields(error));
      hb(`启动阶段失败：${error?.message ?? error}`);
    }
  }

  if (!setupError) {
    // S00：进入场景前的 DOM 取证也受短上限约束，不在看门狗之外挂死
    try {
      ev.dpr = await withTimeout(
        () => inst.page.evaluate(() => devicePixelRatio),
        FAILURE_CAPTURE_TIMEOUT_MS,
        "场景前 dpr 读取",
      );
    } catch (e) {
      ev.dpr = null;
      ev.failure_capture_error = String(e?.message ?? e);
    }
    // S00（计划 §6 S00 第 3 条 / §9.1 第 4 条）：场景级看门狗——普通场景 5 分钟，
    // expire-cycle/ui-relaunch/ui-collection-status 保留显式独立预算；到点置取消标志、
    // 同步写最小证据、有界收尾（复制应用日志、终止已记录 PID）后非零退出。
    const scenarioTimeoutMs = scenarioBudgetMs(SCENARIO);
    ev.scenario_timeout_budget_ms = scenarioTimeoutMs;
    hb(`场景 ${SCENARIO} 开始（看门狗预算 ${scenarioTimeoutMs / 1000} s）`);
    let outcome = null;
    try {
      outcome = await runScenarioBounded({
        scenarioFn: (ctx) =>
          scenarios[SCENARIO]({
            inst,
            ck,
            ev,
            isCanceled: ctx.isCanceled,
            markStage: stage.mark,
            markStageDone: stage.markDone,
          }),
        timeoutMs: scenarioTimeoutMs,
        cancelFlag,
        onTimeout: () => {
          ck.add(
            "scenario_timeout",
            false,
            `场景超过 ${scenarioTimeoutMs / 1000} s 未完成（最后阶段=${stage.last()?.stage ?? "无"}）`,
          );
          ev.scenario_timeout = true;
          ev.failure_stage = "scenario";
          writeEvidenceSnapshot();
        },
        hb,
      });
    } catch (e) {
      scenarioError = e;
      ck.add("scenario_executed", false, String(e.message ?? e));
      Object.assign(ev, errorEvidenceFields(e));
      if (
        SCENARIO === "ui-refresh" ||
        SCENARIO === "ui-system" ||
        SCENARIO === "ui-relaunch" ||
        SCENARIO === "ui-source-paths" ||
        SCENARIO === "ui-identity" ||
        SCENARIO === "ui-ccs-import" ||
        SCENARIO === "ui-pricing"
      ) {
        // S00：失败截图/DOM 取证有短上限，无法完成就跳过并记原因（计划 §9.1 第 5 条）
        try {
          await withTimeout(
            async () => {
              ev.failureViewport = await inst.page.evaluate(() => ({
                width: innerWidth,
                height: innerHeight,
                dpr: devicePixelRatio,
                navigation: [...document.querySelectorAll('[aria-label="设置分区"] button')].map(
                  (el) => ({ text: el.textContent, role: el.getAttribute("role") }),
                ),
              }));
              await inst.page.screenshot({ path: path.join(shots, "failure.png") });
            },
            FAILURE_CAPTURE_TIMEOUT_MS,
            "失败取证（viewport/截图）",
          );
        } catch (captureError) {
          /* 已退出的进程仍保留原失败原因；取证未完成的原因单独留痕。 */
          ev.failure_capture_error = String(captureError?.message ?? captureError);
        }
      }
    }
    if (outcome?.kind === "timeout") {
      const finalize = isolationCleanup
        ? await isolationCleanup.then(
            (stop) => ({ isolation: true, stop }),
            (error) => ({ isolation: true, error: String(error?.message ?? error) }),
          )
        : await finalizeScenarioTimeout({
            attached: inst.attached,
            record: inst.current,
            terminateRecordedPids: (record, reason) => inst.terminateRecordedPids(record, reason),
            closeBrowser: () => closeBrowserBounded(inst.browser),
            copyAppLogs: () => {
              // UI 刷新验收保留合成来源的应用日志（与正常收尾同一份复制逻辑）
              if (
                SCENARIO === "ui-refresh" ||
                SCENARIO === "ui-system" ||
                SCENARIO === "ui-source-paths" ||
                SCENARIO === "ui-identity" ||
                SCENARIO === "ui-ccs-import" ||
                SCENARIO === "ui-pricing"
              ) {
                const logs = path.join(root, "tokenscope", "logs");
                if (fs.existsSync(logs)) copyDirSync(logs, path.join(stampDir, "app-logs"));
              }
            },
            hb,
          });
      ev.finalize_after_timeout = finalize;
      clearInterval(isolationMonitor);
      ev.controlled_io = controlledIo.snapshot();
      ev.acceptance_trace = readAcceptanceTrace(root);
      await controlledIo.close();
      writeEvidenceSnapshot();
      process.exit(1);
    }
    if (!scenarioError) {
      hb(`场景 ${SCENARIO} 结束，检查 ${ck.rows.length - ck.failures.length}/${ck.rows.length}`);
    }
  }

  // 收尾（§4.8 第 2 条）：每次关闭都确认宿主与全部已记录后代退出、端口不再监听，
  // 上限 20 秒；超时失败并列出残留 PID。expire-start 故意保留实例给 expire-finish。
  if (SCENARIO === "expire-start" && !ev.isolation_violation) {
    fs.writeFileSync(
      STATE_FILE,
      JSON.stringify(
        {
          pid: inst.current?.hostPid ?? null,
          port: inst.current?.port ?? PORT,
          root,
          at: nowIso(),
          idleDeadlineIso: ev.sessionIdleDeadlineIso,
        },
        null,
        2,
      ),
    );
    ev.instance_left_alive = true;
    // S00：断开 CDP 复用 10 秒有界关闭
    await closeBrowserBounded(inst.browser);
  } else {
    try {
      const stop = isolationCleanup ? await isolationCleanup : await inst.stop();
      ck.add("instance_stop_confirmed", true, JSON.stringify(stop));
    } catch (e) {
      ck.add("instance_stop_confirmed", false, String(e?.message ?? e));
      ev.stop_error = String(e?.stack ?? e).slice(0, 400);
    }
  }

  // UI 刷新/来源目录验收保留合成来源的应用日志，失败也能核对采集/查询实际发生过。
  // 使用 copyDirSync 代替 fs.cpSync 避免 Windows GBK 环境下 C++ 绑定因中文路径编码产生乱码目录。
  if (
    SCENARIO === "ui-refresh" ||
    SCENARIO === "ui-system" ||
    SCENARIO === "ui-source-paths" ||
    SCENARIO === "ui-identity" ||
    SCENARIO === "ui-ccs-import" ||
    SCENARIO === "ui-pricing"
  ) {
    const logs = path.join(root, "tokenscope", "logs");
    if (fs.existsSync(logs)) copyDirSync(logs, path.join(stampDir, "app-logs"));
  }

  const realAfter = fingerprint(REAL_DATA);
  clearInterval(isolationMonitor);
  ev.controlled_io = controlledIo.snapshot();
  ev.acceptance_trace = readAcceptanceTrace(root);
  ck.add(
    "all_trace_paths_inside_root",
    ev.acceptance_trace.violations.length === 0,
    ev.acceptance_trace.violations.join(" | ") ||
      `${ev.acceptance_trace.records.length} 条追踪，路径均在隔离根内`,
  );
  ck.add(
    "external_connection_attempts_zero",
    ev.controlled_io.proxy_attempts.length === 0,
    JSON.stringify(ev.controlled_io.proxy_attempts),
  );
  if (
    SCENARIO.startsWith("ui-") ||
    SCENARIO === "privacy-accept-restart" ||
    SCENARIO === "privacy-policy-update" ||
    SCENARIO === "privacy-accept-order"
  ) {
    const missing = missingBusinessEvents(ev.acceptance_trace.records);
    ck.add("required_business_trace_present", missing.length === 0, JSON.stringify(missing));
  }
  await controlledIo.close();
  ev.real_tokenscope_fingerprint_before = hashOfFingerprint(realBefore);
  ev.real_tokenscope_fingerprint_after = hashOfFingerprint(realAfter);
  ev.real_tokenscope_entry_count_before =
    realBefore === "<missing>" ? 0 : realBefore.split("\n").length;
  ev.real_untouched = realBefore === realAfter;
  ev.root_files_after = fs.existsSync(root) ? listFiles(root).slice(0, 60) : [];
  ck.add(
    "real_data_untouched",
    ev.real_untouched === true,
    `${hashOfFingerprint(realBefore)} → ${hashOfFingerprint(realAfter)}`,
  );
  if (inst.violations) {
    const expected = inst.violations.filter((v) => inst.expectedScriptConsole?.has(v));
    const vs = splitViolations(inst.violations.filter((v) => !inst.expectedScriptConsole?.has(v)));
    ev.csp_violation_counts = {
      total: inst.violations.length,
      expected_script_probe: expected.length,
      known_noise: vs.known.length,
      unexpected: vs.unexpected.length,
    };
    ck.add(
      "no_unexpected_csp_violations_during_run",
      vs.unexpected.length === 0,
      vs.unexpected.slice(0, 2).join(" | ") || `仅已知噪声 ${vs.known.length} 条`,
    );
  }

  // §4.8 第 5 条：launches[] 无论成败都写出；失败时附宿主日志尾部。
  ev.failure_stage = setupError ? "setup" : scenarioError ? "scenario" : null;
  // S00（计划 §9.1 第 3 条）：最后阶段与完整阶段序列进证据，定位卡点所在段
  ev.last_stage = stage.last();
  ev.stages = stage.list();
  ev.launch_count = inst.launches.length;
  ev.launches = buildLaunchesEvidence(inst.launches);
  ev.checks = ck.rows;
  ev.failed = ck.failures.map((f) => f.name);
  if (ck.failures.length) ev.host_log_tail = inst.logTail();
  if (ck.failures.length) {
    // 应用自己的文件日志（隔离根 tokenscope/logs）尾部：进程 stdout 为空时这是唯一线索
    const appLogDir = path.join(root, "tokenscope", "logs");
    try {
      const text = fs.existsSync(appLogDir)
        ? fs
            .readdirSync(appLogDir)
            .sort()
            .map((f) => fs.readFileSync(path.join(appLogDir, f), "utf8"))
            .join("\n")
        : "";
      ev.app_log_tail = text ? hostLogTail(text.slice(-20000)) : null;
    } catch (e) {
      ev.app_log_tail = `读取应用日志失败：${String(e?.message ?? e)}`;
    }
  }

  const out = path.join(stampDir, "evidence.json");
  fs.writeFileSync(out, JSON.stringify(ev, null, 2), "utf8");
  const summary = {
    scenario: SCENARIO,
    passed: ck.rows.length - ck.failures.length,
    total: ck.rows.length,
    failures: ck.failures.map((f) => `${f.name}: ${f.detail}`),
    launches: ev.launches.map((l) => ({
      index: l.index,
      port: l.port,
      attached: l.attached,
      host_pid: l.host_pid,
      host_exit_code: l.host_exit_code,
    })),
    evidence: out,
    real_untouched: ev.real_untouched,
    pid: ev.pid,
    root,
  };
  console.log(JSON.stringify(summary, null, 2));
  if (ck.failures.length) process.exitCode = 1;
  // 启动/attach 失败：证据已写出，保留原来的退出码 2 语义
  if (setupError) throw setupError;
  if (SCENARIO === "expire-start") {
    // 子进程仍存活会让 Node 事件循环永不退出；这里必须显式结束，
    // 空闲计时从这一刻起继续走。
    process.exit(process.exitCode ?? 0);
  }
}

main().catch((e) => {
  console.error(`FAIL ${SCENARIO}: ${e.message ?? e}`);
  process.exitCode = 2;
});
