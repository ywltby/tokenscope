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
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import zlib from "node:zlib";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
const EXE_DEFAULT = path.join(REPO, "src-tauri", "target", "release", "tokenscope.exe");
const PREP = path.join(REPO, "scripts", "prepare-native-acceptance.ps1");
const RECORDER = path.join(REPO, "scripts", "capture-window-frames.ps1");
const WINDOW_ACTION = path.join(REPO, "scripts", "native-window-action.ps1");
const REAL_DATA = path.join(process.env.USERPROFILE || "", ".tokenscope");

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
const OUT = arg("out", path.join(REPO, "qa-artifacts", "native-recheck-2026-10-08"));
const PORT = Number(arg("port", "9411"));
const EXE = arg("exe", EXE_DEFAULT);
const THEME = arg("theme", "system"); // light | dark | system（--force-*-mode 通道）
// 默认使用真实系统 DPI；显式 --scale 只作布局探查，不作为系统缩放验收。
const SCALE = arg("scale") === undefined ? null : Number(arg("scale"));
const EVENTS = Number(arg("events", "4"));
const ROOT = arg("root", "");
const EXPECT_STORED = arg("expect-stored", undefined);
const EXPECT_THEME = arg("expect-theme", null);

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
    for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
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
    for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const abs = path.join(d, e.name);
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) walk(abs, r);
      else out.push(r);
    }
  };
  walk(dir, "");
  return out;
}
function ps(scriptPath, argsArray) {
  return spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", scriptPath, ...argsArray], {
    encoding: "utf8",
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
    if (!(buf[0] === 0x89 && buf.toString("ascii", 1, 4) === "PNG")) return null;
    let pos = 8;
    let w = 0;
    let h = 0;
    let colorType = 0;
    let bitDepth = 0;
    const idat = [];
    while (pos < buf.length) {
      const len = buf.readUInt32BE(pos);
      const type = buf.toString("ascii", pos + 4, pos + 8);
      const data = buf.subarray(pos + 8, pos + 8 + len);
      if (type === "IHDR") {
        w = data.readUInt32BE(0);
        h = data.readUInt32BE(4);
        bitDepth = data[8];
        colorType = data[9];
        if (data[12] !== 0 || bitDepth !== 8 || (colorType !== 6 && colorType !== 2)) return null;
      } else if (type === "IDAT") idat.push(data);
      else if (type === "IEND") break;
      pos += 12 + len;
    }
    const raw = zlib.inflateSync(Buffer.concat(idat));
    const bpp = colorType === 6 ? 4 : 3;
    const stride = w * bpp;
    let prev = Buffer.alloc(stride);
    let sum = 0;
    let n = 0;
    const paeth = (a, b, c) => {
      const pa = Math.abs(b - c);
      const pb = Math.abs(a - c);
      const pc = Math.abs(a + b - 2 * c);
      return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
    };
    for (let y = 0; y < h; y++) {
      const line = Buffer.from(raw.subarray(y * (stride + 1), y * (stride + 1) + stride + 1));
      const filter = line[0];
      const cur = line.subarray(1);
      for (let x = 0; x < stride; x++) {
        const a = x >= bpp ? cur[x - bpp] : 0;
        const b = prev[x];
        const c = x >= bpp ? prev[x - bpp] : 0;
        let v = cur[x];
        if (filter === 1) v = (v + a) & 0xff;
        else if (filter === 2) v = (v + b) & 0xff;
        else if (filter === 3) v = (v + ((a + b) >> 1)) & 0xff;
        else if (filter === 4) v = (v + paeth(a, b, c)) & 0xff;
        cur[x] = v;
      }
      for (let x = 0; x < stride; x += bpp * 53) {
        sum += 0.2126 * cur[x] + 0.7152 * cur[x + 1] + 0.0722 * cur[x + 2];
        n++;
      }
      prev = cur;
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

/** 一次原生实例：准备隔离根 → 启动 release/exe → CDP 连接。 */
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
  }
  async prepareRoot() {
    fs.mkdirSync(OUT, { recursive: true });
    if (this.opts.events && this.opts.events > 0) {
      const r = ps(PREP, ["-Root", this.root, "-Events", String(this.opts.events)], { encoding: "utf8" });
      if (r.status !== 0) throw new Error(`prepare failed: ${r.stdout}${r.stderr}`);
    } else if (!fs.existsSync(path.join(this.root, "manifest.json"))) {
      throw new Error(`隔离根缺少 manifest：${this.root}`);
    }
    if (this.opts.mutate) this.opts.mutate(this.root);
    const manifest = JSON.parse(fs.readFileSync(path.join(this.root, "manifest.json"), "utf8"));
    return manifest;
  }
  browserArgs(port = PORT) {
    const parts = [`--remote-debugging-port=${port}`];
    if (this.opts.scale !== null) parts.push(`--force-device-scale-factor=${this.opts.scale}`);
    if (this.opts.scheme === "dark") parts.push("--force-dark-mode");
    if (this.opts.scheme === "light") parts.push("--force-light-mode");
    if (this.opts.extraBrowserArgs) parts.push(...this.opts.extraBrowserArgs);
    return parts.join(" ");
  }
  async launch(portOverride) {
    const port = portOverride ?? PORT;
    const browserArgs = this.browserArgs(port);
    const env = {
      ...process.env,
      TOKENSCOPE_ACCEPTANCE_ROOT: this.root,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArgs,
      ...this.opts.env,
    };
    const out = fs.openSync(this.logPath, "a");
    this.proc = spawn(EXE, [], { env, stdio: ["ignore", out, out], detached: false });
    this.startedAt = Date.now();
    // CDP 就绪（进程可能因验收根校验失败而提前退出——必须显式发现）
    const deadline = Date.now() + 45000;
    let info = null;
    while (Date.now() < deadline) {
      if (this.proc.exitCode !== null) {
        throw new Error(`进程提前退出 code=${this.proc.exitCode}；日志见 ${this.logPath}`);
      }
      try {
        const res = await fetch(`http://127.0.0.1:${port}/json/version`);
        info = await res.json();
        break;
      } catch {
        await sleep(150);
      }
    }
    if (!info) throw new Error("CDP 端点未就绪（WebView2 远程调试不可用）");
    this.browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    this.ctx = this.browser.contexts()[0];
    hb("CDP 就绪，等待应用页面 target");
    // WebView2 先给出 about:blank，再挂上 tauri://localhost；直接取 pages[0]
    // 会连到空 target（实测：重启后拿到已销毁的旧 target）。
    const pageDeadline = Date.now() + 20000;
    this.page = null;
    while (Date.now() < pageDeadline) {
      const pages = this.ctx.pages();
      this.page = pages.find((p) => /tauri/i.test(p.url())) ?? null;
      if (this.page) break;
      await sleep(200);
    }
    if (!this.page) throw new Error(`没有 tauri 页面 target（现有：${this.ctx.pages().map((p) => p.url()).join(",")}）`);
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
    return { pid: this.proc.pid, browserArgs, cdpVersion: info["Browser"] ?? info.browser ?? "unknown" };
  }
  async waitForData(timeoutMs = 30000) {
    await this.page.waitForSelector(".app-nav", { timeout: timeoutMs });
    await this.page.waitForFunction(
      () => document.querySelectorAll(".ts-skeleton, .n-skeleton").length === 0 && !!document.querySelector("canvas"),
      null,
      { timeout: timeoutMs, polling: 250 },
    );
  }
  /** 窗口尺寸：CDP 给逻辑（DIP）尺寸，物理尺寸按实测 dpr 换算，不再为此启 PowerShell。 */
  async windowInfo() {
    try {
      const w = await this.cdp.send("Browser.getWindowForTarget");
      const dpr = await this.page.evaluate(() => devicePixelRatio);
      return {
        logical: `${w.bounds.width}x${w.bounds.height}+${w.bounds.left}+${w.bounds.top} DIP`,
        dpr,
        physical: `${Math.round(w.bounds.width * dpr)}x${Math.round(w.bounds.height * dpr)} px`,
      };
    } catch (e) {
      return { logical: `error:${e.message}`, dpr: null, physical: "unknown" };
    }
  }
  /** 真实改变原生窗口尺寸（CDP Browser.setWindowBounds），返回改前 bounds。 */
  async setWindowSize(width, height) {
    const w = await this.cdp.send("Browser.getWindowForTarget");
    const prev = { ...w.bounds };
    await this.cdp.send("Browser.setWindowBounds", {
      windowId: w.windowId,
      bounds: { width, height, windowState: "normal" },
    });
    await sleep(800);
    return prev;
  }
  async stop({ kill = true } = {}) {
    if (!kill || !this.proc) return;
    const pid = this.proc.pid;
    try {
      await this.browser?.close();
    } catch {}
    // 只清理本次 PID 及其子进程（WebView2 渲染/工具进程）
    spawnSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-Command",
        `$ps=[System.Management.Automation.PSObject]; @(Get-CimInstance Win32_Process -Filter "ParentProcessId=${pid}" -ErrorAction SilentlyContinue) | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; Stop-Process -Id ${pid} -Force -ErrorAction SilentlyContinue`,
      ],
      { encoding: "utf8" },
    );
    await sleep(600);
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
    const isTrigger = await page.evaluate(() => document.activeElement?.classList?.contains("cost-trigger"));
    if (isTrigger) {
      reached = true;
      out.tabPresses = i + 1;
      break;
    }
  }
  out.keyboardReachedTrigger = reached;
  if (!reached) return { ...out, available: false };
  out.accessibleName = await page.evaluate(() => document.activeElement?.getAttribute("aria-label") ?? "");
  const focusRing = await page.evaluate(() => {
    const cs = getComputedStyle(document.activeElement);
    return { outlineWidth: cs.outlineWidth, outlineStyle: cs.outlineStyle, boxShadow: cs.boxShadow.slice(0, 80) };
  });
  out.focusRing = focusRing;
  await page.keyboard.press("Enter");
  await page.waitForTimeout(450);
  out.ariaExpanded = await page.evaluate(() =>
    document.activeElement?.getAttribute("aria-expanded") ?? null,
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
  const c = page.locator("canvas").first();
  out.canvasPresent = (await c.count()) > 0;
  if (out.canvasPresent) {
    const b = await c.boundingBox();
    out.box = b;
    out.sizeOk = b && b.width > 100 && b.height > 60;
    out.painted = await c.evaluate((el) => {
      const d = el.toDataURL().length;
      return d > 2000;
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
    const sel = "button,[role=button],input,select,textarea,a[href],[tabindex]:not([tabindex='-1'])";
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

// ── 场景 ──────────────────────────────────────────────────────────
const scenarios = {
  // 在同一个驱动进程内完成等待；部分终端会在父进程结束时回收其原生子进程。
  async "expire-cycle"(context) {
    await scenarios["expire-start"](context);
    const started = Date.now();
    while (Date.now() < Date.parse(context.ev.sessionIdleDeadlineIso)) {
      await sleep(1000);
      if (context.inst.proc.exitCode !== null) throw new Error(`空闲期间进程退出：${context.inst.proc.exitCode}`);
      if (Math.floor((Date.now() - started) / 1000) % 30 === 0) hb("原生查询会话保持空闲，等待真实 TTL");
    }
    context.ev.actualIdleMs = Date.now() - started;
    context.ck.add("real_idle_exceeds_ttl", context.ev.actualIdleMs > 600000, `${context.ev.actualIdleMs} ms`);
    await scenarios["expire-finish"](context);
  },
  /** §2 首帧：录制连续帧 + 页面实测（偏好来自隔离 profile 的真实持久化）。 */
  async "first-frame"({ inst, ck, ev }) {
    if (!inst.framesPromise) throw new Error("first-frame 必须带 --frames（首帧要用连续帧取证，加载完成截图不算证据）");
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
        paint: performance.getEntriesByType("paint").map((p) => `${p.name}=${Math.round(p.startTime)}ms`),
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
      if (darkExpected) ck.add("first_frame_dark_canvas", firstLum < 110, `亮度=${firstLum}（深色期望 <110）`);
      if (lightExpected) ck.add("first_frame_light_canvas", firstLum > 150, `亮度=${firstLum}（浅色期望 >150）`);
      // 首帧之后不得出现"先深后浅"的闪烁：窗口帧序列里最小亮度必须就是首帧
      const lumas = frames.luma_sequence
        .map((t) => Number(t.split(":")[1]))
        .filter((v) => Number.isFinite(v) && v >= 0);
      const note = `min=${Math.min(...lumas)} max=${Math.max(...lumas)} n=${lumas.length}`;
      if (darkExpected) {
        ck.add("no_light_before_dark_flash", Math.max(...lumas) < 110, `深色帧序列最大亮度 ${Math.max(...lumas)}；${note}`);
      }
      if (lightExpected) {
        ck.add("no_dark_before_light_flash", Math.min(...lumas) > 150, `浅色帧序列最小亮度 ${Math.min(...lumas)}；${note}`);
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
    ck.add("paint_entries_recorded", win.paint.length > 0 || win.navigation !== "n/a", `${win.paint.join(" ")} | ${win.navigation}`);
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
    const vp = page.viewportSize() ?? (await page.evaluate(() => ({ width: innerWidth, height: innerHeight })));
    ev.viewport = vp;
    ev.dpr = await page.evaluate(() => devicePixelRatio);
    ev.nativeDpi = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "dpi"]).stdout || "").trim();
    if (SCALE === null) {
      const dpi = /DPI (\d+)/.exec(ev.nativeDpi);
      ck.add("unforced_webview_matches_native_window_dpi", !!dpi && Math.abs(Number(dpi[1]) / 96 - ev.dpr) < 0.01,
        `${ev.nativeDpi}; WebView dpr=${ev.dpr}; 未使用缩放强制参数`);
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
    ck.add("keyboard_reaches_cost_trigger", ev.cost.keyboardReachedTrigger, `Tab=${ev.cost.tabPresses ?? 0} 次`);
    ck.add("cost_trigger_has_accessible_name", (ev.cost.accessibleName || "").length > 3, ev.cost.accessibleName);
    ck.add(
    "cost_trigger_focus_ring_visible",
    !!ev.cost.focusRing &&
      (ev.cost.focusRing.outlineStyle !== "none" || /0px 0px 0px [1-9]/.test(ev.cost.focusRing.boxShadow || "")),
    JSON.stringify(ev.cost.focusRing),
  );
    ck.add("cost_popover_opens_by_keyboard", ev.cost.ariaExpanded === "true", `aria-expanded=${ev.cost.ariaExpanded}`);
    ck.add("cost_popover_four_edges_inside_viewport", ev.cost.fourEdgesInside === true, JSON.stringify(ev.cost.popoverBox));
    ck.add("cost_popover_states_unit_dimension", ev.cost.hasUnitDimension === true, ev.cost.text);
    ck.add("cost_popover_closes_on_escape", ev.cost.closedByEscape === true, "");
    await page.screenshot({ path: path.join(inst.opts.shots, "01-dashboard.png") });
    // 2) 图表
    ev.chart = await measureChart(page);
    ck.add("chart_canvas_painted", ev.chart.sizeOk && ev.chart.painted, JSON.stringify(ev.chart.box));
    // 3) 维度切换（按模型）后仍可渲染
    await page.getByRole("radio", { name: "按模型" }).click();
    await page.waitForTimeout(900);
    ev.chartByModel = await measureChart(page);
    ck.add("chart_renders_after_dimension_switch", ev.chartByModel.sizeOk === true, JSON.stringify(ev.chartByModel.box));
    await page.getByRole("radio", { name: "按日" }).click();
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
        textTruncated: path ? path.scrollWidth > Math.ceil(path.getBoundingClientRect().width) : null,
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
    ck.add("settings_focusables_all_named", ev.a11ySettings.unnamedCount === 0, JSON.stringify(ev.a11ySettings));
    await page.screenshot({ path: path.join(inst.opts.shots, "03-settings-narrow.png") });
    await inst.setWindowSize(ev.originalBounds.width ?? 1280, ev.originalBounds.height ?? 800);
    ev.viewportRestored = page.viewportSize();
    // 6) 导航滚动（玻璃吸顶有内容可透）
    await page.getByRole("radio", { name: "汇总" }).click();
    await page.waitForTimeout(900);
    ev.nav = await measureNavScroll(page);
    ck.add(
      "nav_sticky_with_blur_in_same_scroll_context",
      ev.nav.available && ev.nav.stickyOk && /blur\(/.test(ev.nav.backdropFilter || "") && ev.nav.scrolled,
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
    ev.noticeTexts = (await page.locator(".ts-notice").allInnerTexts()).map((t) => t.replace(/\s+/g, " ").slice(0, 140));
  },

  /** §4 生产 CSP：真实文档响应头/HTML + 全功能交互期间的违规观测。 */
  async "csp"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    // 重新加载并抓真实文档响应（协议级观测，不是调试器求值）
    await inst.cdp.send("Network.enable");
    await inst.cdp.send("Log.enable").catch(() => {});
    const captured = [];
    inst.cdp.on("Network.responseReceived", (e) => {
      if (e.type === "Document") captured.push({ requestId: e.requestId, url: e.response.url, headers: e.response.headers, status: e.response.status });
    });
    await inst.cdp.send("Page.reload");
    await inst.waitForData();
    ev.documentResponses = captured;
    const doc = captured.at(-1) ?? null;
    let htmlSnippet = null;
    if (doc) {
      try {
        const body = await inst.cdp.send("Network.getResponseBody", { requestId: doc.requestId });
        const text = body.base64Encoded ? Buffer.from(body.body, "base64").toString("utf8") : body.body;
        htmlSnippet = text.slice(0, 1200);
        ev.indexHtmlHasInlineScript = /<script(?![^>]*src=)[^>]*>[^<]/i.test(htmlSnippet);
        ev.metaCsp = /Content-Security-Policy/i.test(htmlSnippet) ? "present-in-html" : "not-in-html";
      } catch (e) {
        ev.responseBodyError = String(e.message ?? e);
      }
    }
    ev.cspHeader = doc ? (doc.headers["Content-Security-Policy"] ?? doc.headers["content-security-policy"] ?? null) : null;
    ck.add("production_document_served_from_tauri_origin", !!doc && /tauri/.test(doc.url), doc?.url ?? "no-document");
    ck.add(
      "production_csp_text_recorded",
      !!ev.cspHeader || ev.metaCsp === "present-in-html",
      `header=${String(ev.cspHeader).slice(0, 160)} html=${ev.metaCsp}`,
    );
    // 真实功能在 CSP 下可用（IPC / Naive / ECharts / 主题 / 日期）
    await page.getByRole("radio", { name: "按模型" }).click();
    await page.waitForTimeout(800);
    const chart = await measureChart(page);
    await page.getByRole("radio", { name: "按日" }).click();
    await page.waitForTimeout(600);
    const date = await measureDateShell(page);
    // 主题生效量测取**有颜色的卡片**：.app-shell/html/body 本身透明（实测），
    // 用透明元素判断主题会永远得到同一个 rgba(0,0,0,0)。
    const themeProbe = () =>
      page.evaluate(() => ({
        dataTheme: document.documentElement.getAttribute("data-theme"),
        card: getComputedStyle(document.querySelector(".metric-card") ?? document.body).backgroundColor,
      }));
    await page.getByRole("radio", { name: "深色模式" }).click();
    await page.waitForTimeout(600);
    const darkBg = await themeProbe();
    await page.getByRole("radio", { name: "浅色模式" }).click();
    await page.waitForTimeout(600);
    const lightBg = await themeProbe();
    ck.add("ipc_summary_data_rendered", (await page.locator(".summary-card, .ts-card").count()) > 0, `cards=${await page.locator(".summary-card, .ts-card").count()}`);
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
    ck.add("no_unexpected_csp_violation_while_app_runs", vs.unexpected.length === 0, vs.unexpected.slice(0, 3).join(" | ") || `仅已知噪声 ${vs.known.length} 条`);
    const beforeProbe = inst.violations.length;
    // 入口脚本由同源 HTTP 资源加载，真实点击才插入 script；evaluate 只读结果。
    await page.locator("#ts-acceptance-csp").click();
    await page.waitForFunction(() => window.__tsCspProbe?.positiveRan && window.__tsCspProbe.violations.length >= 2);
    ev.scriptProbe = await page.evaluate(() => window.__tsCspProbe);
    ck.add("same_origin_positive_script_executes", ev.scriptProbe.positiveRan === true, "允许的同源脚本确实执行，排除全部脚本无法加载的假阳性");
    ck.add("unauthorized_inline_script_is_blocked", ev.scriptProbe.inlineRan === false && ev.scriptProbe.violations.some((v) => v.blockedURI === "inline"), JSON.stringify(ev.scriptProbe));
    ck.add("external_script_is_blocked_by_policy", ev.scriptProbe.violations.some((v) => v.blockedURI === "https://tokenscope.invalid/acceptance-blocked.js"), JSON.stringify(ev.scriptProbe.violations));
    ck.add("only_two_expected_script_policy_events", ev.scriptProbe.violations.length === 2, JSON.stringify(ev.scriptProbe.violations));
    // console 与 CDP Log 可能重复上报同一事件；与 DOM 事件分开记录。
    ev.expectedScriptConsole = inst.violations.slice(beforeProbe).filter((v) =>
      /Executing inline script violates|Loading the script 'https:\/\/tokenscope\.invalid\/acceptance-blocked\.js'/i.test(v));
    inst.expectedScriptConsole = new Set(ev.expectedScriptConsole);
    ck.add("production_policy_not_relaxed_for_probe", !!ev.cspHeader && !/script-src[^;]*'unsafe-inline'/.test(ev.cspHeader), ev.cspHeader);
    await page.screenshot({ path: path.join(inst.opts.shots, "csp-state.png") });
  },

  /** 播种：真实点击主题偏好 → 隔离 WebView profile 持久化 → 供首帧冷启动复用。 */
  async "seed-theme"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    const label = { light: "浅色模式", dark: "深色模式", system: "跟随系统" }[inst.opts.scheme ?? "system"] ?? "跟随系统";
    await page.getByRole("radio", { name: label }).click();
    await page.waitForTimeout(1500);
    ev.stored = await page.evaluate(() => {
      try {
        return localStorage.getItem("tokenscope-theme");
      } catch {
        return "<blocked>";
      }
    });
    ev.modeAfterSeed = await page.evaluate(() => getComputedStyle(document.querySelector(".app-shell") ?? document.body).backgroundColor);
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
    ck.add("exit_path_reachable_from_close_dialog", ev.dialogForExit === true, String(ev.closeRequest));
    if (ev.dialogForExit) {
      await page.getByRole("button", { name: "直接退出" }).click();
      const deadline = Date.now() + 20000;
      while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
      ev.cleanExitCode = inst.proc.exitCode;
      ck.add("process_exited_without_force_kill", inst.proc.exitCode !== null, `exitCode=${inst.proc.exitCode}`);
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
    ck.add("window_appears_with_unusable_logs", (await page.locator(".app-nav").count()) > 0, "导航已渲染");
    ck.add(
      "startup_degradation_notice_visible",
      !!ev.noticeText && /文件日志不可用/.test(ev.noticeText),
      String(ev.noticeText).replace(/\s+/g, " ").slice(0, 160),
    );
    // 数据仍可用（降级不阻断）
    await inst.waitForData().catch(() => {});
    ev.chart = await measureChart(page);
    ck.add("data_still_available_after_log_degradation", ev.chart.sizeOk === true, JSON.stringify(ev.chart.box));
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
    ck.add("pricing_file_locked_before_process_start", inst.lockConfirmed === true, `holder=${inst.lockHolderPid}`);
    // 后端把"读取失败"当作**告警**返回（不是 IPC 错误），前端在价格组内联展示，
    // 所以这里量的是 .price-notice 而不是 .ts-notice.is-error。
    ev.priceWarnings = (await page.locator(".price-notice").allInnerTexts()).map((t) => t.replace(/\s+/g, " "));
    ev.readFailureWarningVisible = ev.priceWarnings.some((t) => /外置价格文件读取失败/.test(t));
    ck.add(
      "locked_pricing_surfaces_read_failure_warning",
      ev.readFailureWarningVisible === true,
      ev.priceWarnings.map((t) => t.slice(0, 120)).join(" || ") || "无价格告警",
    );
    // 降级不阻断：汇总数据仍在（费用退化为未知也要有可见结论）
    ck.add("app_still_usable_while_pricing_locked", (await page.locator(".settings-group").count()) >= 3, `分组数=${await page.locator(".settings-group").count()}`);
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
    ck.add("lock_released_before_recovery_probe", released, `等待 ${70 - Math.round((deadline - Date.now()) / 1000)} s`);
    // 恢复通道：真实重新读取（切走再切回设置页 → 价格组重新 loadPricing）
    await page.getByRole("radio", { name: "汇总" }).click();
    await page.waitForTimeout(1500);
    await page.getByRole("radio", { name: "设置" }).click();
    await page.waitForTimeout(3000);
    ev.warningsAfterRelease = (await page.locator(".price-notice").allInnerTexts()).map((t) => t.replace(/\s+/g, " "));
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
    ev.closeRequest = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || "").trim();
    await page.locator(".close-error").waitFor();
    ev.errorText = await page.locator(".close-error").innerText();
    ck.add("remembered_failure_visible_with_reason", /acceptance-hide-once/.test(ev.errorText), ev.errorText);
    await page.screenshot({ path: path.join(inst.opts.shots, "remembered-failure.png") });
    await page.getByRole("button", { name: "最小化到托盘", exact: true }).click();
    await page.waitForTimeout(1500);
    ev.rectAfterRetry = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "mainrect"]).stdout || "").trim();
    const rect = /MAINRECT\s+(-?\d+),(-?\d+),(\d+),(\d+)/.exec(ev.rectAfterRetry);
    ck.add("retry_hides_real_window", /MAINRECT none/.test(ev.rectAfterRetry) || (rect && Number(rect[3]) < 100 && Number(rect[4]) < 100), ev.rectAfterRetry);
    ck.add("retry_preserves_process", inst.proc.exitCode === null, `exitCode=${inst.proc.exitCode}`);
    ck.add("retry_keeps_remembered_settings", sha256File(settingsPath) === ev.settingsShaBefore, ev.settingsShaBefore);
  },

  async "close-tray"({ inst, ck, ev }) {
    const page = inst.page;
    await page.waitForSelector(".app-nav", { timeout: 30000 });
    const settingsPath = path.join(inst.root, "tokenscope", "settings.toml");
    ev.settingsShaBefore = sha256File(settingsPath);
    ev.closeRequest = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || "").trim();
    await page.waitForTimeout(1500);
    ev.dialogVisible = (await page.locator(".close-dialog").count()) > 0;
    ck.add("close_dialog_appears", ev.dialogVisible === true, String(ev.closeRequest));
    await page.getByRole("button", { name: "最小化到托盘" }).click();
    await page.waitForTimeout(2000);
    // 隐藏主窗口后进程仍有可见顶层窗口（22x22 的宿主/托盘窗口，实测），
    // 因此按"最大可见顶层窗口"判定主内容窗口是否真的不在屏上。
    ev.rectAfterMinimize = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "mainrect"]).stdout || "").trim();
    const m = /MAINRECT\s+(?:none|\d+,-?\d+,(\d+),(-?\d+))/.exec(ev.rectAfterMinimize);
    ev.largestWindowArea = m ? Number(m[1]) * Number(m[2]) : null;
    ev.windowHidden = /MAINRECT none/.test(ev.rectAfterMinimize) || (m !== null && Number(m[1]) < 100 && Number(m[2]) < 100);
    ev.processAlive = inst.proc.exitCode === null;
    ev.settingsShaAfter = sha256File(settingsPath);
    ck.add("tray_minimize_hides_main_window", ev.windowHidden === true, ev.rectAfterMinimize);
    ck.add("process_alive_after_tray_minimize", ev.processAlive === true, `exitCode=${inst.proc.exitCode}`);
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
    ev.close1 = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || "").trim();
    await page.waitForTimeout(1500);
    ev.dialog1 = (await page.locator(".close-dialog").count()) > 0;
    ck.add("first_close_asks", ev.dialog1 === true, ev.close1);
    await page.locator(".close-dialog .n-checkbox").first().click();
    await page.waitForTimeout(400);
    ev.checkboxChecked = await page
      .locator(".close-dialog .n-checkbox")
      .first()
      .evaluate((el) => el.classList.contains("n-checkbox--checked"));
    ck.add("remember_checkbox_checkable", ev.checkboxChecked === true, `checked=${ev.checkboxChecked}`);
    await page.getByRole("button", { name: "直接退出" }).click();
    const deadline = Date.now() + 20000;
    while (inst.proc.exitCode === null && Date.now() < deadline) await sleep(200);
    ev.exitCode = inst.proc.exitCode;
    ck.add("remembered_exit_closes_process", inst.proc.exitCode !== null, `exitCode=${inst.proc.exitCode}`);
    ev.settingsAfter = fs.readFileSync(settingsPath, "utf8");
    ev.settingsShaAfter = sha256File(settingsPath);
    ev.closeActionWritten = /close_action/.test(ev.settingsAfter);
    ck.add(
      "remembered_choice_persisted_to_isolated_settings",
      ev.closeActionWritten === true && ev.settingsShaAfter !== ev.settingsShaBefore,
      `sha ${ev.settingsShaBefore.slice(0, 12)}→${ev.settingsShaAfter.slice(0, 12)}；文件含 close_action=${ev.closeActionWritten}`,
    );
    // 重启同一隔离根：不应再弹询问（页面句柄必须重新取，旧 target 已销毁）
    const info = await inst.launch(PORT + 1);
    ev.relaunchPid = info.pid;
    ev.relaunchPort = PORT + 1;
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
    ck.add("relaunch_shows_no_prompt_at_rest", ev.dialogBeforeSecondClose === 0, `弹窗数=${ev.dialogBeforeSecondClose}`);
    ev.close2 = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || "").trim();
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
      (await page.locator(".metric-card").allInnerTexts()).join(" | ").replace(/\s+/g, " ").slice(0, 400);
    ev.before = await readTotals();
    await page.waitForTimeout(3000);
    ev.stillSame = await readTotals();
    ck.add("loaded_view_stable_without_new_query", ev.before === ev.stillSame, "同一视图在采集变化期间不原地变形");
    // 真实追加一条合成事件到隔离根
    const projectDir = path.join(inst.root, "sources", "claude", "projects", "-acceptance-workspace-tokenscope");
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
        usage: { input_tokens: 1000, output_tokens: 500, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
      },
    });
    fs.appendFileSync(logFile, line + "\n");
    ev.appendedTo = path.basename(logFile);
    await page.waitForTimeout(1500);
    ev.afterAppendWithoutRefresh = await readTotals();
    ck.add("appended_event_not_applied_to_current_query", ev.afterAppendWithoutRefresh === ev.before, "未刷新时仍显示旧会话结果");
    await page.getByRole("button", { name: "刷新", exact: true }).click();
    await page.waitForTimeout(4000);
    ev.afterRefresh = await readTotals();
    ck.add("new_query_reflects_appended_event", ev.afterRefresh !== ev.before, "刷新后新会话包含追加事件");
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
    await page.getByRole("button", { name: /同步价格|立即同步|同步/ }).first().click();
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
    ck.add("error_notice_has_clickable_retry_sync", ev.retryPresent > 0, `按钮数=${ev.retryPresent}（旧 locator=${await retry.count()}）`);
    await page.screenshot({ path: path.join(inst.opts.shots, "sync-failure.png") });
    if (ev.retryPresent) {
      await retryBtn.first().click();
      await page.waitForTimeout(12000);
      ev.errorAfterRetry = await page.locator(".ts-notice.is-error").allInnerTexts();
      ck.add(
        "retry_sync_actually_retries_and_keeps_reason",
        ev.errorAfterRetry.some((t) => /同步/.test(t)),
        ev.errorAfterRetry.map((t) => t.replace(/\s+/g, " ").slice(0, 140)).join(" || ") || "重试后错误消失（网络恢复？）",
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
    ev.moreTextBefore = ev.morePresent ? await more.evaluate((el) => el.parentElement.innerText.replace(/\s+/g, " ")) : null;
    ck.add("load_more_available_with_large_event_set", ev.morePresent, `首页 DOM 行数=${ev.rowsBefore}（虚拟滚动）；${ev.moreTextBefore}`);
    // 空闲前**不点**加载更多：点完最后一页后按钮会消失，就没有可过期的游标了
    await page.waitForTimeout(1200);
    ev.sessionIdleDeadlineIso = new Date(Date.now() + 630_000).toISOString();
    ck.add("state_recorded_for_idle_wait", true, `保持进程存活，≥630 s 后跑 --scenario expire-finish`);
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
      ck.add("recovery_button_restarts_batch_and_clears_error", ev.noticeAfterRetry === 0, `剩余错误条=${ev.noticeAfterRetry}`);
      ev.moreTextAfterRecovery = await page.locator(".load-more-row").first().innerText().catch(() => null);
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
        ev.paginationAfterNextPage = await page.locator(".load-more-row").first().innerText().catch(() => "");
        ev.detailHintAfterNextPage = await page.locator(".events-card .table-hint").innerText();
        const loadedAfter = /已加载\s*([0-9,]+)/.exec(ev.detailHintAfterNextPage);
        const remaining = /还剩\s*([0-9,]+)\s*条/.exec(ev.paginationAfterNextPage);
        ck.add("recovered_session_loads_next_page", (await err.count()) === 0 &&
          !!loadedBefore && !!loadedAfter && Number(loadedAfter[1].replace(/,/g, "")) > Number(loadedBefore[1].replace(/,/g, "")) &&
          (!remaining || Number(remaining[1].replace(/,/g, "")) < Number(m[1].replace(/,/g, ""))),
          ev.detailHintAfterNextPage);
      }
      const logDir = path.join(inst.root, "tokenscope", "logs");
      const logs = fs.readdirSync(logDir).sort().map((f) => fs.readFileSync(path.join(logDir, f), "utf8")).join("\n");
      ev.detailSessions = [...logs.matchAll(/明细完成（会话 ([^）]+)）/g)].map((m) => m[1]);
      ck.add("recovery_uses_new_backend_session", new Set(ev.detailSessions).size >= 2,
        ev.detailSessions.join(" → "));
    }
    await page.screenshot({ path: path.join(inst.opts.shots, "expire-recovered.png") });
    ev.windowRect = await inst.windowInfo();
  },

  /** §6 关窗取消：WM_CLOSE → 真实弹窗 → Escape → 进程存活 + settings 字节不变。 */
  async "close-final-save"({ inst, ck, ev }) {
    const page = inst.page;
    await inst.waitForData();
    ev.closeRequest = (ps(WINDOW_ACTION, ["-ProcessId", String(inst.proc.pid), "-Action", "close"]).stdout || "").trim();
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
    ck.add("dialog_quit_exits_successfully", inst.proc.exitCode === 0, `exitCode=${inst.proc.exitCode}`);
    ev.windowStateAfter = fs.existsSync(stateFile) ? JSON.parse(fs.readFileSync(stateFile, "utf8")) : null;
    ck.add("dialog_quit_completes_final_save", JSON.stringify(ev.windowStateAfter) === JSON.stringify(ev.windowStateBefore),
      JSON.stringify(ev.windowStateAfter));
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
    ck.add("close_dialog_appears", ev.dialogVisible === true, String(ev.dialogText).replace(/\s+/g, " ").slice(0, 120));
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
  fs.mkdirSync(OUT, { recursive: true });
  const stampDir = path.join(OUT, SCENARIO);
  fs.mkdirSync(stampDir, { recursive: true });

  const attachExisting = SCENARIO === "expire-finish";
  const state = attachExisting ? JSON.parse(fs.readFileSync(STATE_FILE, "utf8")) : null;
  const root = ROOT || state?.root || freshRoot(SCENARIO);
  const scheme = THEME === "system" ? null : THEME;
  const shots = stampDir;
  const logPath = path.join(stampDir, "process.log");

  const realBefore = fingerprint(REAL_DATA);
  const ck = new Checks();
  const ev = {
    scenario: SCENARIO,
    at: nowIso(),
    commit: spawnSync("git", ["rev-parse", "--short", "HEAD"], { cwd: REPO, encoding: "utf8" }).stdout.trim(),
    feature: "acceptance",
    exe: EXE,
    exe_sha256: fs.existsSync(EXE) ? sha256File(EXE) : "missing",
    acceptance_root: root,
    theme_channel: THEME,
    forced_device_scale: SCALE,
    port: PORT,
    windows_version: (spawnSync("cmd", ["/c", "ver"], { encoding: "utf8" }).stdout || "").trim(),
    userprofile: process.env.USERPROFILE,
  };

  const inst = new Instance({
    root,
    events: attachExisting ? 0 : EVENTS,
    scheme,
    scale: SCALE,
    shots,
    logPath,
    env:
      SCENARIO === "sync-failure"
        ? { HTTPS_PROXY: "http://127.0.0.1:9", HTTP_PROXY: "http://127.0.0.1:9", NO_PROXY: "" }
        : SCENARIO === "csp" ? { TOKENSCOPE_ACCEPTANCE_CSP: "1" }
        : SCENARIO === "close-remember-failure" ? { TOKENSCOPE_ACCEPTANCE_HIDE_FAILURE_ONCE: "1" } : undefined,
    mutate: SCENARIO === "fault-logs-file" ? (r) => {
      const p = path.join(r, "tokenscope", "logs");
      fs.mkdirSync(path.dirname(p), { recursive: true });
      if (fs.existsSync(p) && fs.statSync(p).isDirectory()) throw new Error("logs 已是目录，无法注入文件故障");
      fs.writeFileSync(p, "not-a-directory\n");
    } : SCENARIO === "close-remember-failure" ? (r) => {
      const p = path.join(r, "tokenscope", "settings.toml");
      fs.writeFileSync(p, 'close_action = "minimize"\n' + fs.readFileSync(p, "utf8"), "utf8");
    } : undefined,
  });

  if (attachExisting) {
    // 直接连上仍在空闲等待中的实例（不重启，保证"同一会话经历 TTL"）
    inst.root = root;
    inst.logPath = logPath;
    const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${state.port}` };
    void env;
    const browser = await chromium.connectOverCDP(`http://127.0.0.1:${state.port}`);
    inst.browser = browser;
    inst.ctx = browser.contexts()[0];
    inst.page = inst.ctx.pages().find((p) => /tauri/i.test(p.url())) ?? inst.ctx.pages()[0];
    inst.cdp = await inst.ctx.newCDPSession(inst.page);
    inst.console = [];
    inst.violations = [];
    inst.proc = { pid: state.pid, exitCode: null };
    inst.opts = { ...inst.opts, shots, idleNote: `started_at=${state.at} planned=${state.idleDeadlineIso}` };
    ev.pid = state.pid;
    ev.reused_instance = true;
    ev.idle_from = state.at;
    ev.idle_deadline = state.idleDeadlineIso;
    ev.idle_actual_seconds = Math.round((Date.now() - Date.parse(state.at)) / 1000);
  } else {
    const manifest = await inst.prepareRoot();
    ev.manifest_files = Object.keys(manifest.files ?? {}).length;
    ev.manifest_sha_first = manifest.files ? Object.values(manifest.files).map((f) => f.sha256.slice(0, 12)) : [];
    if (SCENARIO === "first-frame") ev.expectStored = EXPECT_STORED;
    hb("启动 release/acceptance 实例并连接 CDP");
    const info = await inst.launch();
    hb(`PID=${info.pid}`);
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
    inst.lockHeldFrom = nowIso();
    }
    if (flag("frames")) {
      // 连上第一件事就是抓帧：晚一帧就少一帧首帧证据
      inst.framesPromise = captureFrames(inst, path.join(stampDir, "frames"), {
        durationMs: Number(arg("frame-ms", "6000")),
        processStartedAt: inst.startedAt,
      });
    }
    ev.pid = info.pid;
    ev.browser_args = info.browserArgs;
    ev.webview2_version = info.cdpVersion;
    await sleep(200);
  }

  ev.dpr = await inst.page.evaluate(() => devicePixelRatio).catch(() => null);
  const scenarioTimeoutMs = SCENARIO === "expire-cycle" ? 750000 : 240000;
  let scenarioTimer;
  try {
    hb(`场景 ${SCENARIO} 开始`);
    await Promise.race([
      scenarios[SCENARIO]({ inst, ck, ev }),
      new Promise((_, reject) => {
        scenarioTimer = setTimeout(() => reject(new Error(`场景超过 ${scenarioTimeoutMs / 1000} s 未完成`)), scenarioTimeoutMs);
      }),
    ]);
    hb(`场景 ${SCENARIO} 结束，检查 ${ck.rows.length - ck.failures.length}/${ck.rows.length}`);
  } catch (e) {
    ck.add("scenario_executed", false, String(e.message ?? e));
    ev.error = String(e.stack ?? e).slice(0, 800);
  } finally {
    clearTimeout(scenarioTimer);
  }

  ev.checks = ck.rows;

  // 收尾：保留实例（expire-start）或只杀本次 PID
  if (SCENARIO === "expire-start") {
    fs.writeFileSync(
      STATE_FILE,
      JSON.stringify({ pid: inst.proc.pid, port: PORT, root, at: nowIso(), idleDeadlineIso: ev.sessionIdleDeadlineIso }, null, 2),
    );
    ev.instance_left_alive = true;
    try {
      await inst.browser?.close();
    } catch {}
  } else {
    await inst.stop();
  }

  const realAfter = fingerprint(REAL_DATA);
  ev.real_tokenscope_fingerprint_before = hashOfFingerprint(realBefore);
  ev.real_tokenscope_fingerprint_after = hashOfFingerprint(realAfter);
  ev.real_tokenscope_entry_count_before = realBefore === "<missing>" ? 0 : realBefore.split("\n").length;
  ev.real_untouched = realBefore === realAfter;
  ev.root_files_after = listFiles(root).slice(0, 60);
  ck.add("real_data_untouched", ev.real_untouched === true, `${hashOfFingerprint(realBefore)} → ${hashOfFingerprint(realAfter)}`);
  if (inst.violations) {
    const expected = inst.violations.filter((v) => inst.expectedScriptConsole?.has(v));
    const vs = splitViolations(inst.violations.filter((v) => !inst.expectedScriptConsole?.has(v)));
    ev.csp_violation_counts = { total: inst.violations.length, expected_script_probe: expected.length, known_noise: vs.known.length, unexpected: vs.unexpected.length };
    ck.add(
      "no_unexpected_csp_violations_during_run",
      vs.unexpected.length === 0,
      vs.unexpected.slice(0, 2).join(" | ") || `仅已知噪声 ${vs.known.length} 条`,
    );
  }

  const out = path.join(stampDir, "evidence.json");
  ev.failed = ck.failures.map((f) => f.name);
  fs.writeFileSync(out, JSON.stringify(ev, null, 2), "utf8");
  const summary = {
    scenario: SCENARIO,
    passed: ck.rows.length - ck.failures.length,
    total: ck.rows.length,
    failures: ck.failures.map((f) => `${f.name}: ${f.detail}`),
    evidence: out,
    real_untouched: ev.real_untouched,
    pid: ev.pid,
    root,
  };
  console.log(JSON.stringify(summary, null, 2));
  if (ck.failures.length) process.exitCode = 1;
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
