// @vitest-environment node
// AC06 场景契约（PR52 复盘后的维护回归）：直接执行 native-acceptance.mjs
// 的场景体（new Function 抽取，保持单一事实源），用内存桩揭示断言缺陷。
// 复盘反例（review-pr52/native-ac06-review-probes.mjs）证明旧实现放过：
// 无观察窗口、无双快照比对。这里先红后绿，防止再次回退。
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "vitest";
import { changedBusinessFiles, settingsFingerprint, snapshotBusinessFiles } from "./acceptance-io.mjs";
import { fileURLToPath } from "node:url";
import { parse as parseToml } from "smol-toml";
import { parseTOML } from "toml-eslint-parser";
import { withTimeout as realWithTimeout, spawnSyncBounded as realSpawnSyncBounded } from "./native-scenario.mjs";

const source = fs.readFileSync(new URL("../native-acceptance.mjs", import.meta.url), "utf8");

/** 从受测源码抽取顶层辅助函数并绑定依赖——回归测试执行的是**受测实现**
 * 本身（九/十七轮教训：桩/复刻替代实现会让回退不可见）。 */
const HELPER_DEP_NAMES = ["fs", "path", "REPO", "readAcceptanceTrace", "snapshotBusinessFiles", "changedBusinessFiles", "spawnSyncBounded"];
function extractHelper(name, deps) {
  const start = source.indexOf(`function ${name}`);
  assert.ok(start >= 0, `辅助函数未找到：${name}`);
  // 大括号深度扫描定位函数尾（这三个辅助函数体内无字符串大括号）。
  let depth = 0;
  let end = -1;
  for (let i = source.indexOf("{", start); i < source.length; i += 1) {
    const ch = source[i];
    if (ch === "{") depth += 1;
    else if (ch === "}") {
      depth -= 1;
      if (depth === 0) {
        end = i + 1;
        break;
      }
    }
  }
  assert.ok(end > start, `辅助函数边界未找到：${name}`);
  return new Function(...HELPER_DEP_NAMES, `return ${source.slice(start, end)};`)(
    deps.fs,
    deps.path,
    deps.REPO,
    deps.readAcceptanceTrace,
    deps.snapshotBusinessFiles,
    deps.changedBusinessFiles,
    deps.spawnSyncBounded,
  );
}

function extractScenario(name, endMarker, bindings) {
  const start = source.indexOf(`  async "${name}"`);
  const end = source.indexOf(endMarker, start);
  assert.ok(start >= 0 && end > start, `场景未找到：${name}`);
  return new Function(
    ...Object.keys(bindings),
    `return ({${source.slice(start, end)}})[${JSON.stringify(name)}]`,
  )(...Object.values(bindings));
}

/** 与驱动同语义的真实采集→比较链（复用 acceptance-io 能力）。 */
function realSnapshotDataDir(dataDir) {
  const settingsPath = path.join(dataDir, "settings.toml");
  const stat = fs.existsSync(settingsPath) ? fs.statSync(settingsPath) : null;
  return {
    business: snapshotBusinessFiles(dataDir),
    settings: {
      text: stat ? fs.readFileSync(settingsPath, "utf8") : null,
      bytesHex: stat ? fs.readFileSync(settingsPath).toString("hex") : null,
      size: stat ? stat.size : null,
      mtimeMs: stat ? stat.mtimeMs : null,
    },
  };
}

function realDataDirChanges(before, after) {
  console.log("ZZZ-CHANGES-IN:", JSON.stringify(before.settings), "->", JSON.stringify(after.settings));
  return [
    ...changedBusinessFiles(before.business, after.business),
    ...(JSON.stringify(before.settings) === JSON.stringify(after.settings)
      ? []
      : ["settings.toml"]),
  ];
}

function makePreseededRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-ac06-preseeded-"));
  const dataDir = path.join(root, "tokenscope");
  fs.mkdirSync(dataDir, { recursive: true });
  fs.writeFileSync(path.join(dataDir, "settings.toml"), 'close_action = "quit"\n');
  fs.writeFileSync(path.join(dataDir, "history.db"), "seed");
  fs.writeFileSync(path.join(dataDir, "history.db-wal"), "wal");
  fs.writeFileSync(path.join(dataDir, "history.db-shm"), "shm");
  return { root, dataDir };
}

function extractPreseeded(extraBindings = {}) {
  return extractScenario(
    "privacy-preseeded",
    "\n  /**\n   * AC06 场景 2",
    {
      path,
      snapshotDataDir: extractHelper("snapshotDataDir", { fs, path, snapshotBusinessFiles }),
      dataDirChanges: extractHelper("dataDirChanges", { changedBusinessFiles }),
      businessTraceEvents() {
        return [];
      },
      async rejectAndExit(page, inst, ck, label) {
        ck.add(`${label}_exit_clean`, true, "stub");
      },
      ...extraBindings,
    },
  );
}

test("preseeded: 实际观察满 15 秒且文件未变才通过（单调起止 + 双快照）", async () => {
  const { root, dataDir } = makePreseededRoot();
  try {
    const checks = [];
    // 单调时钟桩：第一次 now=0，之后 16000——观察真实经过 16 秒。
    const times = [0, 16000];
    const clock = { now: () => (times.length > 1 ? times.shift() : times[0]) };
    const run = extractPreseeded({ performance: clock });
    await run({
      inst: {
        page: {
          async waitForSelector() {},
          async waitForTimeout() {},
          locator() {
            return { async count() { return 0; } };
          },
          async screenshot() {},
        },
        root,
        prelaunchBusinessFiles: snapshotBusinessFiles(dataDir),
        prelaunchSettings: {
          text: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
          bytesHex: fs.readFileSync(path.join(dataDir, "settings.toml")).toString("hex"),
          size: fs.statSync(path.join(dataDir, "settings.toml")).size,
          mtimeMs: fs.statSync(path.join(dataDir, "settings.toml")).mtimeMs,
        },
        opts: {
          shots: root,
          controlledIo: { snapshot: () => ({ hits: { modelsdev: [], openrouter: [] }, proxy_attempts: [] }) },
        },
      },
      ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
      ev: {},
    });
    const failed = checks.filter((check) => !check.pass);
    assert.deepEqual(failed, [], "满足全部条件时必须全过");
    assert.ok(checks.some((check) => check.name === "preseeded_business_files_unchanged"));
    assert.ok(checks.some((check) => check.name === "preseeded_observed_full_window"));
  } finally {
    fs.rmSync(root, { recursive: true });
  }
});

test("preseeded: 没有实际观察窗口不得通过（约 1 秒结束的老缺陷）", async () => {
  const { root, dataDir } = makePreseededRoot();
  try {
    const checks = [];
    const times = [0, 900];
    const clock = { now: () => (times.length > 1 ? times.shift() : times[0]) };
    const run = extractPreseeded({ performance: clock });
    await run({
      inst: {
        page: {
          async waitForSelector() {},
          async waitForTimeout() {},
          locator() {
            return { async count() { return 0; } };
          },
          async screenshot() {},
        },
        root,
        prelaunchBusinessFiles: snapshotBusinessFiles(dataDir),
        prelaunchSettings: {
          text: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
          bytesHex: fs.readFileSync(path.join(dataDir, "settings.toml")).toString("hex"),
          size: fs.statSync(path.join(dataDir, "settings.toml")).size,
          mtimeMs: fs.statSync(path.join(dataDir, "settings.toml")).mtimeMs,
        },
        opts: {
          shots: root,
          controlledIo: { snapshot: () => ({ hits: { modelsdev: [], openrouter: [] }, proxy_attempts: [] }) },
        },
      },
      ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
      ev: {},
    });
    assert.ok(
      checks.some((check) => check.name === "preseeded_observed_full_window" && !check.pass),
      "观察不足 15 秒必须失败",
    );
  } finally {
    fs.rmSync(root, { recursive: true });
  }
});

test("preseeded: 观察窗口内修改/新增/删除业务文件，采集→比较→判定链必须失败", async () => {
  const { root, dataDir } = makePreseededRoot();
  try {
    const checks = [];
    const times = [0, 16000];
    const clock = { now: () => (times.length > 1 ? times.shift() : times[0]) };
    const run = extractPreseeded({ performance: clock });
    await run({
      inst: {
        page: {
          async waitForSelector() {},
          async waitForTimeout() {
            // 观察窗口内的外部变化：修改、新增、删除（含 WAL/SHM 之一）。
            fs.appendFileSync(path.join(dataDir, "history.db"), "mutated");
            fs.writeFileSync(path.join(dataDir, "unplanned.bin"), "new");
            fs.rmSync(path.join(dataDir, "history.db-wal"));
          },
          locator() {
            return { async count() { return 0; } };
          },
          async screenshot() {},
        },
        root,
        prelaunchBusinessFiles: snapshotBusinessFiles(dataDir),
        prelaunchSettings: {
          text: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
          bytesHex: fs.readFileSync(path.join(dataDir, "settings.toml")).toString("hex"),
          size: fs.statSync(path.join(dataDir, "settings.toml")).size,
          mtimeMs: fs.statSync(path.join(dataDir, "settings.toml")).mtimeMs,
        },
        opts: {
          shots: root,
          controlledIo: { snapshot: () => ({ hits: { modelsdev: [], openrouter: [] }, proxy_attempts: [] }) },
        },
      },
      ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
      ev: {},
    });
    const unchanged = checks.find((check) => check.name === "preseeded_business_files_unchanged");
    assert.ok(unchanged, "必须有双快照不变断言");
    assert.equal(unchanged.pass, false);
    assert.ok(unchanged.detail.includes("history.db"));
    assert.ok(unchanged.detail.includes("unplanned.bin"));
    assert.ok(unchanged.detail.includes("history.db-wal"));
  } finally {
    fs.rmSync(root, { recursive: true });
  }
});

test("preseeded: 观察窗口内改写 settings.toml 必须失败", async () => {
  const { root, dataDir } = makePreseededRoot();
  try {
    const checks = [];
    const times = [0, 16000];
    const clock = { now: () => (times.length > 1 ? times.shift() : times[0]) };
    const run = extractPreseeded({ performance: clock });
    await run({
      inst: {
        page: {
          async waitForSelector() {},
          async waitForTimeout() {
            fs.writeFileSync(path.join(dataDir, "settings.toml"), "privacy_policy_accepted = true\n");
          },
          locator() {
            return { async count() { return 0; } };
          },
          async screenshot() {},
        },
        root,
        prelaunchBusinessFiles: snapshotBusinessFiles(dataDir),
        prelaunchSettings: {
          text: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
          bytesHex: fs.readFileSync(path.join(dataDir, "settings.toml")).toString("hex"),
          size: fs.statSync(path.join(dataDir, "settings.toml")).size,
          mtimeMs: fs.statSync(path.join(dataDir, "settings.toml")).mtimeMs,
        },
        opts: {
          shots: root,
          controlledIo: { snapshot: () => ({ hits: { modelsdev: [], openrouter: [] }, proxy_attempts: [] }) },
        },
      },
      ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
      ev: {},
    });
    const unchanged = checks.find((check) => check.name === "preseeded_business_files_unchanged");
    assert.ok(unchanged && !unchanged.pass && unchanged.detail.includes("settings.toml"));
  } finally {
    fs.rmSync(root, { recursive: true });
  }
});

// ── privacy-accept-order：同意顺序 + 单次落盘（PR52 复盘缺陷 3）─────────
// 旧断言只查 read > max(consent, ready)：business_ready 先于 consent_saved、
// 或 consent_saved 整条缺失都会放行（反例 native-ac06-review-probes.mjs）。

/** privacy-accept-order 统一执行器：顺序/指纹、双击输入、自启路由三类
 * 断言共享同一场景面（点击计数器、坐标双击、同意后自启 IPC、注册表
 * 快照），旧 helper 只是对它的薄封装。 */
function runAcceptOrderScenario(options = {}) {
  const {
    settingsText = ORDER_SETTINGS,
    records = null,
    deliveredClicks = 2,
    gapMs = 30,
    failSecondDispatch = false,
    statusValue = true,
    stateFileText = '{"enabled":true}',
    autostartTrace = null,
    registryChanged = false,
    // 八轮复核：按钮位置前 N 次 boundingBox 采样返回偏移位（模拟布局动画）
    unstableBoxSamples = 0,
    // 九轮复核：稳定性等待期间按钮消失（boundingBox 返回 null）
    buttonVanishes = false,
    // 九轮复核：位置始终移动（每次采样都不同，永不稳定）
    alwaysMoving = false,
    // 十轮复核：位置由**虚拟时钟**驱动（waitForTimeout 累计时钟；时钟
    // 未达稳定阈值前每次采样位置都不同——移除等待即永不稳定）。
    stableAfterMs = 0,
    // 十轮复核 P2：解锁后（.app-nav 出现）的按钮位置查询注入失败
    unlockBoxQueryFails = false,
    // 十三轮：count() 返回该值（默认解锁后 0=已移除）
    unlockCountReturns = 0,
    // 十三轮：单次页面内采集执行时元素已被移除（exists=false）——即
    // 「count 与采集之间退出动画完成」的时序形态
    unlockCollectRemoves = false,
    // 十四轮：假 DOM 含两个按钮（不同意 x=100、同意并继续 x=700），
    // 采集回调真实执行——覆盖选择器行为（旧实现取第一个按钮 → 红）。
    multiButtonDom = false,
    // 十五轮：同意按钮处于「正在保存…」文案（按钮仍在 x=700）——按文案
    // 过滤的旧实现会失去它（记 null/超时）。
    consentSavingText = false,
    // 八轮复核 P2：阻断文案查询（allTextContents）注入失败
    blockLocatorFails = false,
  } = options;
  const box = { x: 100, y: 200, width: 120, height: 40 };
  const centerX = box.x + box.width / 2;
  const centerY = box.y + box.height / 2;
  const clicksLog = [];
  const checks = [];
  const ev = {};
  const listeners = [];
  const fakeWindow = {};
  const evaluateCalls = [];
  let dispatched = 0;
  let registryCalls = 0;
  let boxSamples = 0;
  let virtualClock = 0;
  let unlockStage = false;
  const clickCoordinates = [];
  // 十五轮：假 DOM（双按钮 + dataset 标记）——挂计数器回调真实执行后
  // 最后一个按钮获得 ac06Consent 标记；采集回调按标记定位。
  const fakeButtons = [
    {
      textContent: "不同意",
      dataset: {},
      getBoundingClientRect: () => ({ x: 100, y: 200, width: 90, height: 28 }),
    },
    {
      textContent: consentSavingText ? "正在保存…" : "同意并继续",
      dataset: {},
      getBoundingClientRect: () => ({ x: 700, y: 200, width: 90, height: 28 }),
    },
  ];
  const markedConsent = () =>
    fakeButtons.find((button) => button.dataset.ac06Consent === "1") ?? null;
  const fakeDocument = {
    addEventListener: () => {},
    querySelectorAll: (selector) =>
      String(selector).includes("privacy-actions") ? fakeButtons : [],
    querySelector: (selector) => {
      const text = String(selector);
      if (text.includes("ac06-consent")) return markedConsent();
      if (text.includes("privacy-actions")) return fakeButtons[0];
      return null;
    },
  };
  const registry = { hive: ["unchanged"] };
  const autostartRows =
    autostartTrace ?? [
      { seq: 890, event: "autostart_read", path: "C:/synthetic-root/autostart-state.json" },
      { seq: 901, event: "autostart_write", path: "C:/synthetic-root/autostart-state.json" },
      { seq: 902, event: "autostart_read", path: "C:/synthetic-root/autostart-state.json" },
    ];
  const settingsWriteRow = {
    seq: 0,
    event: "settings_write",
    path: "C:/synthetic-root/tokenscope/settings.toml",
  };
  const traceRecords = records
    ? [...records, ...autostartRows]
    : [
        settingsWriteRow,
        ...consentRecords(settingsText),
        { seq: 2, event: "business_ready" },
        { seq: 3, event: "history_open" },
        ...autostartRows,
      ];
  const run = extractScenario(
    "privacy-accept-order",
    "\n  /**\n   * AC06 场景 4a",
    {
      path,
      settingsFingerprint,
      parseToml,
      parseTOML,
      withTimeout: realWithTimeout,
      registrySnapshot() {
        registryCalls += 1;
        return registryChanged && registryCalls > 1 ? { hive: ["changed"] } : registry;
      },
      fs: {
        realpathSync: Object.assign((p) => String(p), { native: (p) => String(p) }),
        readFileSync(file, encoding) {
          const name = String(file);
          if (name.endsWith("autostart-state.json")) {
            if (stateFileText === null) throw new Error("ENOENT");
            return stateFileText;
          }
          return encoding === "utf8" ? settingsText : Buffer.from(settingsText);
        },
      },
      readAcceptanceTrace() {
        return { violations: [], records: traceRecords };
      },
    },
  );
  return run({
    inst: {
      page: {
        async waitForSelector(selector) {
          if (String(selector) === ".app-nav") unlockStage = true;
        },
        async waitForTimeout(ms) {
          virtualClock += ms ?? 0;
        },
        locator(selector) {
          const name = String(selector);
          return {
            first() {
              return {
                // 解锁后按钮已随隐私框移除（count=0 → 合法 null 形态）；
                // 查询失败形态下按钮仍在（count=1、boundingBox 抛错）。
                async count() {
                  if (unlockStage) {
                    if (unlockCountReturns !== 0) return unlockCountReturns;
                    return unlockBoxQueryFails ? 1 : 0;
                  }
                  return 1;
                },
                async boundingBox() {
                  if (unlockStage && unlockBoxQueryFails) {
                    throw new Error("CDP 查询失败（解锁后）");
                  }
                  if (unlockStage && unlockCollectRemoves) {
                    // 时序形态：count 之后按钮已被移除——Playwright
                    // boundingBox 自动等待至超时抛错（复核方真实 Edge 形态）。
                    throw new Error(
                      "TimeoutError: locator.boundingBox: Timeout 20000ms exceeded.",
                    );
                  }
                  boxSamples += 1;
                  if (buttonVanishes && boxSamples >= 2) return null;
                  if (stableAfterMs > 0) {
                    // 十轮：位置由虚拟时钟驱动——时钟未达阈值前每次采样
                    // 位置都不同（移动中）；移除等待即永不稳定。
                    if (virtualClock < stableAfterMs) {
                      return {
                        x: box.x + 50 + boxSamples,
                        y: box.y + 60,
                        width: box.width,
                        height: box.height,
                      };
                    }
                    return box;
                  }
                  if (unstableBoxSamples > 0 && boxSamples <= unstableBoxSamples) {
                    // 动画中：每次采样位置都不同（移动中），稳定后回原位。
                    return {
                      x: box.x + 50 + boxSamples,
                      y: box.y + 60,
                      width: box.width,
                      height: box.height,
                    };
                  }
                  if (alwaysMoving) {
                    return {
                      x: box.x + 50 + boxSamples,
                      y: box.y + 60,
                      width: box.width,
                      height: box.height,
                    };
                  }
                  return box;
                },
              };
            },
            count() {
              // 十一轮：查询失败形态下按钮仍在（count=1、boundingBox 抛错）
              // ——与「按钮已移除（合法 null）」区分。
              if (unlockBoxQueryFails && name.includes("privacy-actions")) return Promise.resolve(1);
              return Promise.resolve(name.includes("privacy-dialog") ? 1 : 0);
            },
            allTextContents() {
              if (blockLocatorFails && name.includes("error")) {
                return Promise.reject(new Error("CDP session closed"));
              }
              return Promise.resolve([]);
            },
          };
        },
        async evaluate(fn) {
          const text = fn.toString();
          if (text.includes("addEventListener")) {
            // 真实执行受测监听器注册与按钮标记（场景体挂计数器）。
            listeners.length = 0;
            new Function("window", "document", `return (${text});`)(
              fakeWindow,
              {
                ...fakeDocument,
                addEventListener(type, handler) {
                  listeners.push({ type, handler });
                },
              },
            )();
            return undefined;
          }
          if (text.includes("__ac06ConsentClicks")) {
            evaluateCalls.push(text.slice(0, 40));
            return fakeWindow.__ac06ConsentClicks ?? [];
          }
          if (text.includes("[data-ac06-consent]")) {
            // 十五轮：单次页面内采集按不随文案变化的标记定位。通信异常
            // 形态（unlockBoxQueryFails）整次采集抛错；移除形态 exists=
            // false；其余**真实执行**回调（标记按钮在假 DOM 内）。
            if (unlockBoxQueryFails) {
              throw new Error("CDP 查询失败（解锁后）");
            }
            if (unlockCollectRemoves) return { exists: false };
            return new Function("document", `return (${text});`)(fakeDocument)();
          }
          if (text.includes("autostart_set")) return { ok: true, value: true };
          if (text.includes("autostart_status")) return { ok: true, value: statusValue };
          return [];
        },
        mouse: {
          async click(x, y) {
            dispatched += 1;
            clickCoordinates.push({ x, y });
            if (failSecondDispatch && dispatched === 2) throw new Error("CDP 输入通道错误");
            if (dispatched <= deliveredClicks) {
              const ts = 1000 + (dispatched - 1) * gapMs;
              const fakeTarget = {
                tagName: "BUTTON",
                className: "privacy-actions",
                textContent: "  同意并继续 ",
              };
              // 真实语义：每个监听器只收自己注册的类型（pointerdown 监听器
              // 不会收到 click），否则一次点击会计成两次按下。
              for (const { type: listenType, handler } of listeners) {
                handler({
                  type: listenType,
                  timeStamp: ts + (listenType === "click" ? 0.5 : 0),
                  clientX: x,
                  clientY: y,
                  target: fakeTarget,
                });
              }
              clicksLog.push({ type: "pointerdown", ts, x, y });
              clicksLog.push({ type: "click", ts, x, y });
            }
          },
        },
        async screenshot() {},
      },
      root: "C:/synthetic-root",
      opts: { shots: "C:/synthetic-shots" },
    },
    ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
    ev,
  }).then(
    () => ({ threw: false, error: null, ev, checks, clickCoordinates }),
    (error) => ({ threw: true, error: String(error), ev, checks, clickCoordinates }),
  );
}

function runAcceptOrder({ records, settingsText }) {
  return runAcceptOrderScenario({ records, settingsText });
}

function consentRecords(settingsText, { hash } = {}) {
  const fingerprint = settingsFingerprint(Buffer.from(settingsText));
  return [
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: { algorithm: "fnv1a64", value: hash ?? fingerprint.value },
      settings_bytes_hex: Buffer.from(settingsText, "utf8").toString("hex"),
    },
  ];
}

const ORDER_SETTINGS = "privacy_policy_accepted = true\nprivacy_policy_revision = 2\n";

test("accept-order: 同意 → 解锁 → 首读严格成序且指纹一致才通过", async () => {
  const records = [
    { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    ...consentRecords(ORDER_SETTINGS),
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  assert.deepEqual(checks.filter((check) => !check.pass), [], "正例必须全过");
  for (const name of [
    "consent_saved_precedes_business_ready",
    "business_ready_precedes_first_business_read",
    "consent_saved_single_atomic_replace",
  ]) {
    assert.ok(checks.some((check) => check.name === name), `缺少具名断言：${name}`);
  }
});

test("accept-order: business_ready 早于 consent_saved 必须失败", async () => {
  const records = [
    { seq: 1, event: "business_ready" },
    { seq: 2, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    ...consentRecords(ORDER_SETTINGS).map((row) => ({ ...row, seq: 3 })),
    { seq: 4, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const failed = checks.filter((check) => !check.pass);
  assert.ok(failed.length > 0, "颠倒顺序不得通过");
  assert.ok(failed.some((check) => check.name === "consent_saved_precedes_business_ready"));
});

test("accept-order: consent_saved 整条缺失必须失败", async () => {
  const records = [
    { seq: 1, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const failed = checks.filter((check) => !check.pass);
  assert.ok(failed.length > 0, "缺事件不得通过（旧断言 read>max(空,ready) 会放行）");
  assert.ok(failed.some((check) => check.name === "consent_saved_precedes_business_ready"));
});

test("accept-order: 同意保存指纹畸形（算法/位数不符）必须失败", async () => {
  const records = [
    { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: { algorithm: "md5", value: "0000000000000000" },
    },
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const malformed = checks.find((check) => check.name === "consent_saved_single_atomic_replace");
  assert.ok(malformed, "缺少单次原子落盘指纹断言");
  assert.equal(malformed.pass, false);
});

test("accept-order: 业务期规范化重写（终态字节≠保存时）不算二次同意保存", async () => {
  // 实跑事实：业务初始化会在同意之后合法重写 settings（终态含说明头与
  // 完整来源段）。判定只看保存时的不可变字节证据（重算哈希 + 内容解析），
  // 不与终态字节比对——终态文本已变为重写后形态，仍必须通过。
  const records = [
    { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    ...consentRecords(ORDER_SETTINGS),
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
    { seq: 4, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
  ];
  const rewritten = "# TokenScope 设置文件（业务期重写后形态）\n" + ORDER_SETTINGS;
  const { checks } = await runAcceptOrder({ records, settingsText: rewritten });
  const single = checks.find((check) => check.name === "consent_saved_single_atomic_replace");
  assert.ok(single && single.pass, JSON.stringify(checks.filter((c) => !c.pass)));
  const content = checks.find((check) => check.name === "consent_saved_capture_content");
  assert.ok(content && content.pass);
});

// ── privacy-accept-order：双击输入真实性（PR52 复盘缺陷 4）───────────────
// 旧实现第二次 locator.click 失败被 catch 吞掉——双击可能只发了一次输入
// 仍全绿（反例 secondClickCounterexample：successfulClicks=1 通过）。

function runAcceptOrderClicks({ deliveredClicks, gapMs, failSecondDispatch, records }) {
  return runAcceptOrderScenario({ deliveredClicks, gapMs, failSecondDispatch, records });
}

function orderedRecords() {
  return [
    { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    ...consentRecords(ORDER_SETTINGS),
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
  ];
}

test("accept-order: 两次真实输入到达页面且间隔 <100ms 才通过（抑制 ≠ 未发出）", async () => {
  const { checks } = await runAcceptOrderClicks({
    deliveredClicks: 2,
    gapMs: 30,
    records: orderedRecords(),
  });
  assert.deepEqual(checks.filter((check) => !check.pass), [], "双击真实送达 + 单次落盘必须全过");
  for (const name of ["double_click_delivered", "double_click_interval_under_100ms"]) {
    assert.ok(checks.some((check) => check.name === name), `缺少具名断言：${name}`);
  }
});

test("accept-order: 只有第二次输入被产品抑制（页面只收到一次）必须失败", async () => {
  const { checks } = await runAcceptOrderClicks({
    deliveredClicks: 1,
    gapMs: 30,
    records: orderedRecords(),
  });
  const delivered = checks.find((check) => check.name === "double_click_delivered");
  assert.ok(delivered, "缺少双击送达断言");
  assert.equal(delivered.pass, false);
});

test("accept-order: 两次输入间隔 ≥100ms 必须失败", async () => {
  const { checks } = await runAcceptOrderClicks({
    deliveredClicks: 2,
    gapMs: 400,
    records: orderedRecords(),
  });
  const interval = checks.find((check) => check.name === "double_click_interval_under_100ms");
  assert.ok(interval && !interval.pass);
});

test("accept-order: 第二次输入下发失败必须显式失败（不得吞掉后宣称双击已测）", async () => {
  const result = await runAcceptOrderClicks({
    deliveredClicks: 2,
    gapMs: 30,
    failSecondDispatch: true,
    records: orderedRecords(),
  });
  assert.equal(result.threw, true, "第二次下发失败必须显式失败");
  assert.match(result.error, /CDP 输入通道错误/);
});

// ── privacy-accept-order：同意后自启命令层路由（方案 A / PR52 补证）──────
// helper 单测不能替代 command 层证据：同意后经真实 IPC 开关自启，acceptance
// 构建必须路由到隔离替身——返回值、状态文件、追踪路径、注册表四蜂巢快照。

function runAcceptOrderRouting(options) {
  return runAcceptOrderScenario(options);
}

test("autostart 路由：替身返回、状态文件、根内追踪（含启动期读）、注册表不变齐备才通过", async () => {
  const { checks } = await runAcceptOrderRouting({});
  assert.deepEqual(checks.filter((check) => !check.pass), [], "正例必须全过");
  for (const name of [
    "autostart_routes_to_isolated_shim",
    "autostart_state_file_in_root",
    "autostart_trace_paths_inside_root",
    "autostart_registry_untouched",
  ]) {
    assert.ok(checks.some((check) => check.name === name), `缺少具名断言：${name}`);
  }
});

test("autostart 路由：状态与落盘不一致（疑似真实插件）必须失败", async () => {
  const { checks } = await runAcceptOrderRouting({ statusValue: false });
  assert.ok(
    checks.some((check) => check.name === "autostart_routes_to_isolated_shim" && !check.pass),
  );
});

test("autostart 路由：隔离根没有状态文件必须失败", async () => {
  const { checks } = await runAcceptOrderRouting({ stateFileText: null });
  assert.ok(checks.some((check) => check.name === "autostart_state_file_in_root" && !check.pass));
});

test("autostart 路由：追踪路径越出隔离根必须失败", async () => {
  const { checks } = await runAcceptOrderRouting({
    autostartTrace: [
      { seq: 4, event: "autostart_write", path: "C:/real/registry/leak.json" },
      { seq: 5, event: "autostart_read", path: "C:/real/registry/leak.json" },
    ],
  });
  assert.ok(
    checks.some((check) => check.name === "autostart_trace_paths_inside_root" && !check.pass),
  );
});

test("autostart 路由：注册表快照变化必须失败", async () => {
  const { checks } = await runAcceptOrderRouting({ registryChanged: true });
  assert.ok(
    checks.some((check) => check.name === "autostart_registry_untouched" && !check.pass),
  );
});

test("autostart 路由：只有读没有写（set 未真正生效）必须失败", async () => {
  const { checks } = await runAcceptOrderRouting({
    autostartTrace: [
      { seq: 890, event: "autostart_read", path: "C:/synthetic-root/autostart-state.json" },
      { seq: 902, event: "autostart_read", path: "C:/synthetic-root/autostart-state.json" },
    ],
  });
  assert.ok(
    checks.some((check) => check.name === "autostart_trace_paths_inside_root" && !check.pass),
  );
});

// ── 同意保存字节证据（PR52 二轮复核缺口）──────────────────────────────
// 保存事件必须携带不可变字节证据（settings_bytes_hex）；判定方重算哈希并
// 解析内容确认同意值与修订号。只查哈希格式不核对内容的旧实现放行
// 「0000000000000000 vs 0acec52d49477739」（复核方独立复现 13/13 全过）。

const BASE_ROWS = () => [
  { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
  { seq: 2, event: "business_ready" },
  { seq: 3, event: "history_open" },
];

test("字节证据：哈希与字节重算一致且内容含同意值与修订号才通过", async () => {
  const records = [...BASE_ROWS().slice(0, 1), ...consentRecords(ORDER_SETTINGS), ...BASE_ROWS().slice(1)];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  assert.deepEqual(checks.filter((check) => !check.pass), [], "正例必须全过");
  for (const name of ["consent_saved_single_atomic_replace", "consent_saved_capture_content"]) {
    assert.ok(checks.some((check) => check.name === name), `缺少具名断言：${name}`);
  }
});

test("字节证据：格式合法但哈希与字节不符必须失败", async () => {
  const records = [
    ...BASE_ROWS().slice(0, 1),
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: { algorithm: "fnv1a64", value: "0000000000000000" },
      settings_bytes_hex: Buffer.from(ORDER_SETTINGS, "utf8").toString("hex"),
    },
    ...BASE_ROWS().slice(1),
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const mismatch = checks.find((check) => check.name === "consent_saved_single_atomic_replace");
  assert.ok(mismatch, "缺少重算比对断言");
  assert.equal(mismatch.pass, false, "复核方独立复现的缺陷：格式合法假哈希不得放行");
});

test("字节证据：哈希正确但内容缺修订号必须失败", async () => {
  const wrong = "privacy_policy_accepted = true\n";
  const records = [
    ...BASE_ROWS().slice(0, 1),
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: settingsFingerprint(Buffer.from(wrong)),
      settings_bytes_hex: Buffer.from(wrong, "utf8").toString("hex"),
    },
    ...BASE_ROWS().slice(1),
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const content = checks.find((check) => check.name === "consent_saved_capture_content");
  assert.ok(content, "缺少内容解析断言");
  assert.equal(content.pass, false);
});

test("字节证据：哈希正确但同意值非 true 必须失败", async () => {
  const wrong = "privacy_policy_accepted = false\nprivacy_policy_revision = 2\n";
  const records = [
    ...BASE_ROWS().slice(0, 1),
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: settingsFingerprint(Buffer.from(wrong)),
      settings_bytes_hex: Buffer.from(wrong, "utf8").toString("hex"),
    },
    ...BASE_ROWS().slice(1),
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const content = checks.find((check) => check.name === "consent_saved_capture_content");
  assert.ok(content && !content.pass);
});

test("字节证据：事件缺字节证据字段必须失败（不得退回只查格式）", async () => {
  const records = [
    ...BASE_ROWS().slice(0, 1),
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: settingsFingerprint(Buffer.from(ORDER_SETTINGS)),
    },
    ...BASE_ROWS().slice(1),
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const single = checks.find((check) => check.name === "consent_saved_single_atomic_replace");
  assert.ok(single && !single.pass, "缺字节证据不得通过");
});

test("字节证据：业务期合法重写（终态≠保存时字节）仍通过，不用终态硬比", async () => {
  const records = [
    ...BASE_ROWS().slice(0, 1),
    ...consentRecords(ORDER_SETTINGS),
    ...BASE_ROWS().slice(1),
    { seq: 4, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  assert.deepEqual(
    checks.filter((check) => !check.pass),
    [],
    "保存时字节证据与终态无关，业务期重写不影响判定",
  );
});

// ── 同意字节证据内容解析（三轮复核缺口）─────────────────────────────────
// capture_content 此前用正则搜索字符串——注释假字段、[notes] 表内字段、
// revision = 20（前缀匹配成 2）三种形态全部误放行（复核方复现
// review-pr52-round3/native-ac06-content-probes-same-bytes.json，哈希与
// 字节均自洽仍 14/14 全过）。判定必须用真正的 TOML 解析器：解析失败拒绝，
// 顶层 privacy_policy_accepted 严格为布尔 true、顶层修订号严格为整数 2。

const CONTENT_BAD_CASES = [
  {
    label: "注释假字段（实际 accepted = false）",
    text: "# privacy_policy_accepted = true\nprivacy_policy_accepted = false\nprivacy_policy_revision = 2\n",
  },
  {
    label: "字段只在 [notes] 表，顶层缺失",
    text: "\n[notes]\nprivacy_policy_accepted = true\nprivacy_policy_revision = 2\n",
  },
  {
    label: "修订号 20（正则会匹配成 2）",
    text: "privacy_policy_accepted = true\nprivacy_policy_revision = 20\n",
  },
  {
    label: "非法 TOML（解析失败必须拒绝）",
    text: "privacy_policy_accepted = true\nprivacy_policy_revision = [2\n",
  },
];

for (const bad of CONTENT_BAD_CASES) {
  test(`字节证据内容：${bad.label} 必须失败`, async () => {
    const records = [
      { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
      {
        seq: 1,
        event: "consent_saved",
        settings_hash: settingsFingerprint(Buffer.from(bad.text, "utf8")),
        settings_bytes_hex: Buffer.from(bad.text, "utf8").toString("hex"),
      },
      { seq: 2, event: "business_ready" },
      { seq: 3, event: "history_open" },
    ];
    const { checks } = await runAcceptOrder({ records, settingsText: bad.text });
    const content = checks.find((check) => check.name === "consent_saved_capture_content");
    assert.ok(content, "缺少内容解析断言");
    assert.equal(content.pass, false, `误放行：${bad.label}`);
    assert.equal(
      checks.find((check) => check.name === "consent_saved_single_atomic_replace").pass,
      true,
      "哈希自洽时重算断言应通过（失败必须来自内容解析层）",
    );
  });
}

// ── 字节证据类型与日期语义（四轮复核缺口）───────────────────────────────
// smol-toml 把整数/浮点都解析成 JS number：revision = 2.0 / 2e0 /
// 2.0000000000000001 的 === 2 全为 true（产品 Rust read_consent 对浮点
// 返回 Unreadable）；宽松日期 2026-02-30 也被接受而 Rust 明确拒绝。
// 判定必须保留 TOML 字面量类型（AST kind）并拒绝语义非法日期。

const TYPE_BAD_CASES = [
  { label: "浮点 2.0", extra: "privacy_policy_revision = 2.0\n", replace: true },
  { label: "指数 2e0", extra: "privacy_policy_revision = 2e0\n", replace: true },
  { label: "超精度浮点 2.0000000000000001", extra: "privacy_policy_revision = 2.0000000000000001\n", replace: true },
  { label: "语义非法日期 2026-02-30", extra: "notes = 2026-02-30\n", replace: false },
];

for (const bad of TYPE_BAD_CASES) {
  test(`字节证据类型/日期：${bad.label} 必须失败`, async () => {
    const text = bad.replace
      ? "privacy_policy_accepted = true\n" + bad.extra
      : "privacy_policy_accepted = true\nprivacy_policy_revision = 2\n" + bad.extra;
    const records = [
      { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
      {
        seq: 1,
        event: "consent_saved",
        settings_hash: settingsFingerprint(Buffer.from(text, "utf8")),
        settings_bytes_hex: Buffer.from(text, "utf8").toString("hex"),
      },
      { seq: 2, event: "business_ready" },
      { seq: 3, event: "history_open" },
    ];
    const { checks } = await runAcceptOrder({ records, settingsText: text });
    const content = checks.find((check) => check.name === "consent_saved_capture_content");
    assert.ok(content, "缺少内容解析断言");
    assert.equal(content.pass, false, `误放行：${bad.label}`);
  });
}

// ── 严格 UTF-8 与带引号顶层键（五轮复核缺口）────────────────────────────
// P2：toString("utf8") 默认把坏字节替换成 U+FFFD，两个解析器检查的是替换
// 后文本——注释中 FF、截断编码、过长编码、代理项编码全被误放行（Rust
// 严格读取报 InvalidData）。P3：TOMLQuoted 键的文本属性是 value 而非
// content——单引号/双引号/转义形式的合法顶层键被误判缺失（Rust 返回
// Granted）。

const BYTE_BAD_CASES = [
  { label: "注释中的原始 FF 字节", head: [0x23, 0x20, 0xff, 0x0a] },
  { label: "注释中的截断多字节编码", head: [0x23, 0x20, 0xe4, 0xb8, 0x0a] },
  { label: "注释中的过长编码 C0 80", head: [0x23, 0x20, 0xc0, 0x80, 0x0a] },
  { label: "注释中的代理项编码 ED A0 80", head: [0x23, 0x20, 0xed, 0xa0, 0x80, 0x0a] },
];

for (const bad of BYTE_BAD_CASES) {
  test(`字节证据：${bad.label}（非法 UTF-8）必须失败`, async () => {
    // 坏字节必须在注释行内部（复核六轮装配更正：放在注释结束后会落在
    // 裸键前，旧版 TOML 解析本就拒绝——红不真实；注释内部替换成 U+FFFD
    // 后注释仍合法，才是 toString("utf8") 误放行的原始形态）。
    const head = Buffer.from(bad.head);
    const bytes = Buffer.concat([head, Buffer.from("privacy_policy_accepted = true\nprivacy_policy_revision = 2\n", "utf8")]);
    const records = [
      { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
      {
        seq: 1,
        event: "consent_saved",
        settings_hash: settingsFingerprint(bytes),
        settings_bytes_hex: bytes.toString("hex"),
      },
      { seq: 2, event: "business_ready" },
      { seq: 3, event: "history_open" },
    ];
    const { checks } = await runAcceptOrder({
      records,
      settingsText: bytes.toString("utf8"),
    });
    const content = checks.find((check) => check.name === "consent_saved_capture_content");
    assert.ok(content, "缺少内容解析断言");
    assert.equal(content.pass, false, `误放行：${bad.label}`);
    assert.match(content.detail, /严格解码失败/);
  });
}

const QUOTED_KEY_GOOD_CASES = [
  { label: "双引号键", acceptedLine: '"privacy_policy_accepted" = true\n' },
  { label: "单引号键", acceptedLine: "'privacy_policy_accepted' = true\n" },
  // String.raw：必须测 TOML 的 \u0079 转义（复核六轮：普通字符串里 JS 会
  // 先把它转成 y，TOML 转义路径未被测试）。
  { label: String.raw`转义形式键（TOML \u0079 = y）`, acceptedLine: String.raw`"privacy_polic\u0079_accepted" = true` + "\n" },
];

for (const good of QUOTED_KEY_GOOD_CASES) {
  test(`字节证据：${good.label} 必须通过（P3 误拒修复）`, async () => {
    const text = good.acceptedLine + "privacy_policy_revision = 2\n";
    const records = [
      { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
      ...consentRecords(text),
      { seq: 2, event: "business_ready" },
      { seq: 3, event: "history_open" },
    ];
    const { checks } = await runAcceptOrder({ records, settingsText: text });
    const content = checks.find((check) => check.name === "consent_saved_capture_content");
    assert.ok(content && content.pass, JSON.stringify(checks.filter((c) => !c.pass)));
  });
}

test("字节证据：合法编码的 U+FFFD 字符必须通过（替换仅限非法字节）", async () => {
  const head = Buffer.from("# replacement char \xef\xbf\xbd is fine\n", "binary");
  const bytes = Buffer.concat([head, Buffer.from("privacy_policy_accepted = true\nprivacy_policy_revision = 2\n", "utf8")]);
  const records = [
    { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: settingsFingerprint(bytes),
      settings_bytes_hex: bytes.toString("hex"),
    },
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: bytes.toString("utf8") });
  const content = checks.find((check) => check.name === "consent_saved_capture_content");
  assert.ok(content && content.pass, JSON.stringify(checks.filter((c) => !c.pass)));
});

// ── 解锁超时取证：逐项有界、失败只追加（七轮复核缺口）────────────────────
// 旧实现整体 try/catch：任何单项取证失败（截图异常、CDP 拒绝）都会用
// {errorText, diagError} 覆盖已取得的现场；CDP 挂起时无超时保护，可能拖
// 到场景看门狗。逐项验证三反例 + 挂起有界。

function runAcceptOrderDiagnostics({ pageBehavior }) {
  const checks = [];
  const ev = {};
  const listeners = [];
  const fakeWindow = {};
  const box = { x: 100, y: 200, width: 120, height: 40 };
  // 十五轮：诊断路径共用同一假 DOM（双按钮 + dataset 标记）。
  const diagButtons = [
    {
      textContent: "不同意",
      dataset: {},
      getBoundingClientRect: () => ({ x: 100, y: 200, width: 90, height: 28 }),
    },
    {
      textContent: pageBehavior.consentSavingText ? "正在保存…" : "同意并继续",
      dataset: {},
      getBoundingClientRect: () => ({ x: 700, y: 200, width: 90, height: 28 }),
    },
  ];
  const diagMarked = () =>
    diagButtons.find((button) => button.dataset.ac06Consent === "1") ?? null;
  const diagDocument = {
    querySelectorAll: (selector) =>
      String(selector).includes("privacy-actions") ? diagButtons : [],
    querySelector: (selector) => {
      const text = String(selector);
      if (text.includes("ac06-consent")) return diagMarked();
      if (text.includes("privacy-actions")) return diagButtons[0];
      return null;
    },
  };
  const run = extractScenario(
    "privacy-accept-order",
    "\n  /**\n   * AC06 场景 4a",
    {
      path,
      settingsFingerprint,
      parseToml,
      parseTOML,
      // 真实 withTimeout 语义（race 超时跳过记原因），时间缩短便于测试。
      withTimeout: (task, _ms, detail) => realWithTimeout(task, 50, detail),
      registrySnapshot() {
        return { hive: [] };
      },
      fs: {
        realpathSync: Object.assign((p) => String(p), { native: (p) => String(p) }),
        existsSync: () => true,
        readFileSync(file, encoding) {
          const name = String(file);
          if (name.endsWith("autostart-state.json")) return '{"enabled":true}';
          return encoding === "utf8" ? ORDER_SETTINGS : Buffer.from(ORDER_SETTINGS);
        },
      },
      readAcceptanceTrace() {
        return {
          violations: [],
          records: [
            ...consentRecords(ORDER_SETTINGS),
            { seq: 2, event: "business_ready" },
            { seq: 3, event: "history_open" },
          ],
        };
      },
    },
  );
  const evaluateCalls = [];
  const invocation = run({
    inst: {
      page: {
        async waitForSelector(selector, options) {
          if (String(selector) === ".app-nav") {
            throw new Error("page.waitForSelector: Timeout 60000ms exceeded.");
          }
        },
        async waitForTimeout() {},
        locator(selector) {
          const name = String(selector);
          return {
            first() {
              return {
                async boundingBox() {
                  return box;
                },
              };
            },
            count() {
              return Promise.resolve(name.includes("privacy-dialog") ? 1 : 0);
            },
            allTextContents() {
              return pageBehavior.blockLocatorText
                ? Promise.reject(new Error("CDP session closed"))
                : Promise.resolve([]);
            },
          };
        },
        async evaluate(fn) {
          const text = fn.toString();
          // 区分调用点：挂计数器（addEventListener）**真实执行注册**——
          // 回调存入 listeners，点击时按受测源码的监听实现产生事件
          // （九轮复核 P3：跳过执行再自行塞 target 的测试不验证实现）。
          if (text.includes("addEventListener")) {
            listeners.length = 0;
            const factory = new Function(
              "window",
              "document",
              `return (${text});`,
            );
            factory(fakeWindow, {
              ...diagDocument,
              addEventListener(type, handler) {
                listeners.push({ type, handler });
              },
            })();
            return undefined;
          }
          if (text.includes("__ac06ConsentClicks")) {
            evaluateCalls.push(text.slice(0, 40));
            if (pageBehavior.evaluate) return pageBehavior.evaluate(fn);
            return fakeWindow.__ac06ConsentClicks ?? [];
          }
          if (text.includes("[data-ac06-consent]")) {
            // 十五轮：取证路径按标记采集事后位置；保存中形态按钮仍在
            // （标记有效 → 位置对象）；旧 locator 文本失配等待超时由
            // pageBehavior.blockBoxQuery 模拟。
            if (pageBehavior.blockBoxQuery) {
              throw new Error("TimeoutError: locator.boundingBox: Timeout 20000ms exceeded.");
            }
            return new Function("document", `return (${text});`)(diagDocument)();
          }
          return [];
        },
        mouse: {
          async click() {},
        },
        screenshot: pageBehavior.screenshot,
        async $() {
          return null;
        },
      },
      root: "C:/synthetic-root",
      opts: { shots: "C:/synthetic-shots" },
    },
    ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
    ev,
  });
  return invocation.then(
    () => ({ threw: false, ev, checks }),
    (error) => ({ threw: true, error: String(error), ev, checks }),
  );
}

test("取证：截图失败不清空已取得的现场（逐项保存）", async () => {
  const result = await runAcceptOrderDiagnostics({
    pageBehavior: {
      screenshot: () => Promise.reject(new Error("screenshot: target crashed")),
    },
  });
  assert.equal(result.threw, true, "原错误必须照常抛出");
  assert.match(result.error, /Timeout 60000ms/);
  const diag = result.ev.acceptFailureDiagnostics;
  assert.ok(diag.capturedClicks !== undefined, "点击捕获必须保留");
  assert.ok(diag.privacyDialogCount !== undefined, "隐私框状态必须保留");
  assert.match(diag.screenshotError ?? "", /target crashed/);
});

test("取证：CDP 拒绝（首个 evaluate 失败）只记该项错误，其余项继续", async () => {
  const result = await runAcceptOrderDiagnostics({
    pageBehavior: {
      evaluate: () => Promise.reject(new Error("CDP session closed")),
    },
  });
  assert.equal(result.threw, true);
  const diag = result.ev.acceptFailureDiagnostics;
  assert.match(diag.capturedClicksError ?? "", /CDP session closed/);
  assert.ok(diag.privacyDialogCount !== undefined, "后续项必须继续采集");
  assert.ok(diag.settingsSnapshot !== undefined, "设置快照必须继续采集");
});

test("取证：CDP 挂起在取证上限内跳过并记原因，不拖垮场景", async () => {
  const result = await runAcceptOrderDiagnostics({
    pageBehavior: {
      evaluate: () => new Promise(() => {}),
    },
  });
  assert.equal(result.threw, true);
  const diag = result.ev.acceptFailureDiagnostics;
  assert.match(diag.capturedClicksError ?? "", /上限/);
  assert.ok(diag.privacyDialogCount !== undefined, "超时项之后其余项继续");
});

// ── 点击目标证据与按钮稳定性（八轮复核缺口）────────────────────────────
// document 级计数器只记坐标不能证明目标元素是同意按钮（点击别的元素同样
// 得到两组事件）；坐标先取后点，布局动画会让坐标失效——点击前必须等按钮
// 位置稳定，并记录点击前后按钮位置与事件实际目标。

test("按钮稳定性：布局动画期不下发输入，位置连续两次采样一致后才点击", async () => {
  const { checks, clickCoordinates } = await runAcceptOrderScenario({
    unstableBoxSamples: 3,
  });
  const failed = checks.filter((check) => !check.pass);
  assert.deepEqual(failed, [], "稳定后点击必须全过");
  // 实际点击坐标必须是稳定位置的中心（偏移位 x+50/y+60 的中心不是它）。
  for (const coordinate of clickCoordinates) {
    assert.equal(coordinate.x, 100 + 120 / 2, `点击 x 未用稳定位置：${JSON.stringify(coordinate)}`);
    assert.equal(coordinate.y, 200 + 40 / 2, `点击 y 未用稳定位置：${JSON.stringify(coordinate)}`);
  }
  for (const name of ["button_position_stable_before_click", "button_position_recorded"]) {
    assert.ok(checks.some((check) => check.name === name), `缺少具名断言：${name}`);
  }
});

test("取证：解锁超时时阻断文案查询失败记为该项错误", async () => {
  // 场景 waitForSelector 正常返回 .app-nav（不进取证路径）→ 用失败注入
  // 直接验证取证段对阻断文案查询的处理。
  const result = await runAcceptOrderDiagnostics({
    pageBehavior: {
      blockLocatorText: true,
      blockLocatorReject: true,
      evaluate: () => [],
    },
  });
  assert.equal(result.threw, true);
  const diag = result.ev.acceptFailureDiagnostics;
  assert.match(diag.privacyBlockedDetailError ?? "", /CDP session closed/);
  assert.ok(!("privacyBlockedDetail" in diag) || diag.privacyBlockedDetail === undefined,
    "查询失败时不得伪装成空列表");
});

test("取证：点击证据携带事件实际目标元素", async () => {
  const { ev, checks } = await runAcceptOrderScenario({});
  assert.ok(checks.every((check) => check.pass), JSON.stringify(checks.filter((c) => !c.pass)));
  for (const entry of ev.acceptClicks.captured) {
    assert.ok(entry.target, "计数事件必须带目标元素");
    assert.equal(entry.target.tag, "BUTTON");
  }
});

// ── 稳定性判定与位置证据保留（九轮复核缺口）────────────────────────────
// 旧实现：稳定性循环耗尽直接继续点击且断言固定 true；首次两采样间无等待；
// 3075 行整体重写 ev.acceptClicks 清掉位置字段；失败路径无事后位置。

test("稳定性：按钮始终移动时不得点击，直接失败", async () => {
  const result = await runAcceptOrderScenario({ alwaysMoving: true });
  assert.equal(result.threw, true, "永不稳定必须失败而不是继续点击");
  assert.match(result.error, /不稳定|消失/);
  assert.equal(result.clickCoordinates.length, 0, "不稳定时不得下发任何点击");
  const stable = result.checks.find((check) => check.name === "button_position_stable_before_click");
  assert.ok(!stable || stable.pass === false, "不得虚报稳定");
});

test("稳定性：按钮在等待期间消失必须失败且不点击", async () => {
  const result = await runAcceptOrderScenario({ buttonVanishes: true });
  assert.equal(result.threw, true, "按钮消失必须失败");
  assert.match(result.error, /消失/);
  assert.equal(result.clickCoordinates.length, 0, "按钮消失后不得点击");
});

test("稳定性：两采样间必须跨帧等待（首次同帧相同不算稳定）", async () => {
  // 旧实现首次比较发生在取完初始位置后立即进行（同帧相同即误判稳定）；
  // 新实现先等一帧再采样。用 unstableBoxSamples=1（首采样偏移、第二帧起
  // 稳定）验证等待后用稳定位点击。
  const { checks, clickCoordinates } = await runAcceptOrderScenario({
    unstableBoxSamples: 1,
  });
  assert.deepEqual(checks.filter((check) => !check.pass), []);
  assert.equal(clickCoordinates.length, 2);
  assert.equal(clickCoordinates[0].x, 100 + 120 / 2, "必须用稳定位置中心");
});

test("取证：解锁超时的失败路径补按钮事后位置，且不覆盖已有字段", async () => {
  const result = await runAcceptOrderDiagnostics({ pageBehavior: { evaluate: () => [] } });
  assert.equal(result.threw, true);
  const diag = result.ev.acceptFailureDiagnostics;
  assert.ok(diag, "必须有诊断对象");
  assert.ok("buttonBoxAtClick" in diag, "失败路径必须有按钮当时位置");
  assert.ok("buttonBoxAfterFailure" in diag, "失败路径必须有按钮事后位置");
});

test("取证：正常路径的位置字段不被后续 acceptClicks 重写清掉", async () => {
  const { ev, checks } = await runAcceptOrderScenario({});
  assert.ok(checks.every((check) => check.pass), JSON.stringify(checks.filter((c) => !c.pass)));
  const clickRecord = ev.acceptClicks;
  assert.ok("buttonBoxAtClick" in clickRecord, "buttonBoxAtClick 必须保留");
  assert.ok("buttonBoxAfterClick" in clickRecord, "buttonBoxAfterClick 必须保留");
  // 解锁后按钮已移除时该值为 null（合法形态），字段存在即不被重写清掉。
  assert.ok(clickRecord.buttonBoxAfterClick === null || typeof clickRecord.buttonBoxAfterClick === "object",
    `事后位置应为 null 或对象：${JSON.stringify(clickRecord.buttonBoxAfterClick)}`);
});

test("目标元素：受测监听实现真实执行产生 target（移除实现即失败）", async () => {
  const { ev, checks } = await runAcceptOrderScenario({});
  assert.ok(checks.every((check) => check.pass), JSON.stringify(checks.filter((c) => !c.pass)));
  const captured = ev.acceptClicks.captured;
  assert.ok(captured.length >= 4, `捕获事件不足：${captured.length}`);
  for (const entry of captured) {
    assert.ok(entry.target, "受测监听器必须产出目标元素（没有则测试红）");
    assert.equal(entry.target.tag, "BUTTON");
    assert.match(entry.target.text, /同意并继续/);
  }
});

// ── 十轮复核缺口 ─────────────────────────────────────────────────────────

test("P2：解锁后位置查询失败必须记 buttonBoxAfterClickError（不得吞成 null）", async () => {
  const { ev, checks } = await runAcceptOrderScenario({ unlockBoxQueryFails: true });
  const clickRecord = ev.acceptClicks;
  assert.ok(
    "buttonBoxAfterClickError" in clickRecord,
    `查询失败必须记错误字段，实际：${JSON.stringify(clickRecord)}`,
  );
  assert.match(clickRecord.buttonBoxAfterClickError, /CDP 查询失败/);
  assert.ok(!("buttonBoxAfterClick" in clickRecord), "查询失败时不得伪装成合法 null");
  // 十一轮：位置记录断言在此形态下必须如实失败（不虚报双位置已入
  // evidence）——它不算进通过集。
  const recorded = checks.find((check) => check.name === "button_position_recorded");
  assert.ok(recorded && !recorded.pass && /查询失败/.test(recorded.detail),
    JSON.stringify(recorded));
});

test("P3：跨帧等待必须真实驱动位置（移除等待即永不稳定、场景失败）", async () => {
  // 位置由虚拟时钟驱动：waitForTimeout 不推进时钟就永远在动画态。旧实现
  // 移除 await page.waitForTimeout(100) 后时钟不走 → 20 次采样耗尽 → 场景
  // 失败（本用例断言通过 → 移除等待即红）。按采样次数变位置的旧桩无此
  // 牙齿（移除等待仍绿）。
  const { checks, clickCoordinates } = await runAcceptOrderScenario({ stableAfterMs: 400 });
  assert.deepEqual(checks.filter((check) => !check.pass), []);
  assert.equal(clickCoordinates.length, 2);
  assert.equal(clickCoordinates[0].x, 100 + 120 / 2, "时钟稳定后必须用原位中心");
});

// ── 十一轮复核：位置记录断言必须在采集后依据实际结果生成 ────────────────
// 旧断言在事后位置查询前执行且固定为 true——注入查询失败后错误字段已
// 正确保留，断言仍绿并声称「点击时与点击后按钮位置均已入 evidence」。

test("P3：位置记录断言在查询失败时不得虚报双位置已入 evidence", async () => {
  const { checks } = await runAcceptOrderScenario({ unlockBoxQueryFails: true });
  const recorded = checks.find((check) => check.name === "button_position_recorded");
  assert.ok(recorded, "缺少位置记录断言");
  assert.equal(recorded.pass, false, "查询失败时不得声称双位置已入 evidence");
  assert.match(recorded.detail, /查询失败/);
});

test("P3：位置记录断言在查询成功时如实报告两位置", async () => {
  const { checks } = await runAcceptOrderScenario({});
  const recorded = checks.find((check) => check.name === "button_position_recorded");
  assert.ok(recorded && recorded.pass, JSON.stringify(checks.filter((c) => !c.pass)));
  assert.match(recorded.detail, /事后位置=/);
});

// ── 十三轮复核：存在性判断与位置读取必须单次页面内采集 ──────────────────
// count() 与 boundingBox() 分开执行——正常退出动画恰好在两次查询之间完成
// 时，count=1 而 boundingBox 等待超时，合法移除被误判为查询失败（复核方
// 真实 Edge 复现）。合并为单次 evaluate：元素缺失返回 null，通信异常才
// 记错误。

test("十三轮：count 与位置采集之间按钮被移除，必须判合法 null 而非查询失败", async () => {
  // 桩语义：count() 返回 1（查询时按钮仍在）；单次页面内采集执行时元素
  // 已被移除（exists=false）——即「两次查询之间退出动画完成」的时序。
  const { ev, checks } = await runAcceptOrderScenario({
    unlockCountReturns: 1,
    unlockCollectRemoves: true,
  });
  assert.ok(checks.every((check) => check.pass), JSON.stringify(checks.filter((c) => !c.pass)));
  const clickRecord = ev.acceptClicks;
  assert.equal(clickRecord.buttonBoxAfterClick, null, "两次查询之间移除 = 合法 null");
  assert.ok(
    !("buttonBoxAfterClickError" in clickRecord),
    `不得误判为查询失败：${JSON.stringify(clickRecord.buttonBoxAfterClickError)}`,
  );
});

// ── 十四轮复核：事后采集必须定位同一同意按钮（多按钮真实 DOM）───────────
// querySelector(".privacy-actions button") 取第一个按钮——组件顺序中第一
// 个是「不同意」。点击前记录的是同意按钮位置，事后采集的是不同意按钮
// 位置，两份位置不可比较且断言仍绿。

test("十四轮：事后采集定位同意按钮而非第一个按钮（多按钮真实回调）", async () => {
  const result = await runAcceptOrderScenario({ multiButtonDom: true });
  assert.ok(result.checks.every((check) => check.pass), JSON.stringify(result.checks.filter((c) => !c.pass)));
  const clickRecord = result.ev.acceptClicks;
  assert.ok(clickRecord.buttonBoxAfterClick, "事后位置必须存在");
  // 假 DOM：不同意按钮在 x=100、同意按钮在 x=700——事后采集必须取同意
  // 按钮（旧实现 querySelector 取第一个 → x=100 → 红）。
  assert.equal(
    clickRecord.buttonBoxAfterClick.x,
    700,
    `事后采集取到的是不同意按钮：${JSON.stringify(clickRecord.buttonBoxAfterClick)}`,
  );
});

// ── 十五轮复核：按钮身份不随文案变化（保存中/已移除/取证路径）───────────
// 同意按钮保存中会变「正在保存…」而按钮仍在——按文案过滤会失去它：正常
// 采集记 null（与已移除无法区分）、失败取证用旧文案 locator 等待超时。

test("十五轮：保存中（文案已变、按钮仍在）正常采集必须取到位置而非 null", async () => {
  const { ev, checks } = await runAcceptOrderScenario({ consentSavingText: true });
  assert.ok(checks.every((check) => check.pass), JSON.stringify(checks.filter((c) => !c.pass)));
  const clickRecord = ev.acceptClicks;
  assert.ok(clickRecord.buttonBoxAfterClick, "保存中按钮仍在，不得记 null");
  assert.equal(clickRecord.buttonBoxAfterClick.x, 700, "必须取到同意按钮位置");
  assert.ok(!("buttonBoxAfterClickError" in clickRecord), "不得误记查询失败");
});

test("十五轮：已移除（标记按钮不在）采集记合法 null", async () => {
  const { ev, checks } = await runAcceptOrderScenario({ unlockCollectRemoves: true });
  assert.ok(checks.every((check) => check.pass), JSON.stringify(checks.filter((c) => !c.pass)));
  assert.equal(ev.acceptClicks.buttonBoxAfterClick, null);
  assert.ok(!("buttonBoxAfterClickError" in ev.acceptClicks));
});

test("十五轮：卡在保存中未解锁时，失败取证按标记取到事后位置（不等待文本失配）", async () => {
  const result = await runAcceptOrderDiagnostics({
    pageBehavior: { consentSavingText: true, evaluate: () => [] },
  });
  assert.equal(result.threw, true);
  const diag = result.ev.acceptFailureDiagnostics;
  assert.ok(diag, "必须有诊断对象");
  assert.ok(!("buttonBoxAfterFailureError" in diag), `不得记取证超时：${JSON.stringify(diag.buttonBoxAfterFailureError)}`);
  assert.equal(diag.buttonBoxAfterFailure?.x, 700, "事后位置必须取到保存中的同意按钮");
});

// ── 十七轮复核 P2：终态 settings 与保存证据同口径严格判定 ────────────────
// accept_writes_consent_with_revision 旧实现用正则搜索终态文本——注释里
// 出现期望字段而实际 accepted=false/revision=1 时场景全过（复核方复现
// review-merge-independent/terminal-consent-probe.json）。终态文件与保存
// 字节证据同口径：严格 UTF-8 + TOML 解析 + 顶层类型/值检查；业务初始化
// 后合法重写仍按终态解析（不与保存字节比对）。

const TERMINAL_BAD_CASES = [
  {
    label: "注释含期望字段、实际 accepted=false、revision=1",
    text: "# privacy_policy_accepted = true\n# privacy_policy_revision = 2\nprivacy_policy_accepted = false\nprivacy_policy_revision = 1\n",
  },
  {
    label: "终态非法 TOML",
    text: "privacy_policy_accepted = true\nprivacy_policy_revision = [2\n",
  },
];

for (const bad of TERMINAL_BAD_CASES) {
  test(`终态判定：${bad.label} 必须失败`, async () => {
    const records = [
      { seq: 0, event: "settings_write", path: "C:/synthetic-root/tokenscope/settings.toml" },
      ...consentRecords(ORDER_SETTINGS),
      { seq: 2, event: "business_ready" },
      { seq: 3, event: "history_open" },
    ];
    // 终态文本与保存字节证据无关（业务期重写后的形态由 settingsText 承载）。
    const { checks } = await runAcceptOrder({ records, settingsText: bad.text });
    const terminal = checks.find((check) => check.name === "accept_writes_consent_with_revision");
    assert.ok(terminal, "缺少终态判定断言");
    assert.equal(terminal.pass, false, `误放行：${bad.label}`);
    // 保存证据层不受终态影响（证据自洽）。
    const saved = checks.find((check) => check.name === "consent_saved_capture_content");
    assert.ok(saved && saved.pass, "保存字节证据应保持通过");
  });
}

// ── 十七轮复核 P2：denied-ipc 不得漏检设置写入 ──────────────────────────
// 追踪筛选旧实现排除 settings_write、settings 快照只比内容——同字节重写
// （mtime 变化）+ 合法 settings_write 追踪在场时场景仍报告无追踪、文件
// 未变化（复核方复现 review-merge-independent/denied-ipc-trace-probe.json）。

function runDeniedIpc({ records, settingsMutator }) {
  const checks = [];
  let mutatorDone = false;
  // dataDir 对准场景体真实路径 <root>/tokenscope（场景体固定拼接）。
  const dataDir = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-denied-ipc-")), "tokenscope");
  fs.mkdirSync(dataDir, { recursive: true });
  fs.writeFileSync(path.join(dataDir, "settings.toml"), 'price_auto_sync = false\n');
  console.log("ZZZ-BEFORE-COLLECT:", dataDir, fs.statSync(path.join(dataDir, "settings.toml")).mtimeMs);
  const before = realSnapshotDataDir(dataDir);
  if (settingsMutator) settingsMutator(dataDir);
  console.log("ZZZ-AFTER-MUTATOR:", dataDir, fs.statSync(path.join(dataDir, "settings.toml")).mtimeMs);
  const run = extractScenario(
    "privacy-denied-ipc",
    "\n  /**\n   * AC06 场景 3",
    {
      path,
      snapshotDataDir: extractHelper("snapshotDataDir", { fs, path, snapshotBusinessFiles }),
      dataDirChanges: extractHelper("dataDirChanges", { changedBusinessFiles }),
      readProtectedCommands: () => ["set_source_config"],
      MINIMAL_INVOKER_ARGS: { set_source_config: {} },
      businessTraceEvents: extractHelper("businessTraceEvents", {
        readAcceptanceTrace: (root, options) =>
          options?.partial || root === undefined
            ? { violations: [], records: records ?? [] }
            : { violations: [], records: records ?? [] },
      }),
      registrySnapshot: () => ({}),
      async rejectAndExit(page, inst, ck, label) {
        ck.add(`${label}_exit_clean`, true, "stub");
      },
    },
  );
  const invocation = Promise.resolve().then(() =>
    run({
      inst: {
        page: {
          async waitForSelector() {},
          locator() {
            return { count: async () => 0 };
          },
          async evaluate() {
            if (settingsMutator && !mutatorDone) {
              mutatorDone = true;
              settingsMutator(dataDir);
            }
            return { status: "rejected", error: "privacy_consent_required" };
          },
          async screenshot() {},
        },
        root: path.dirname(dataDir),
        __trace: records,
        opts: {
          shots: dataDir,
          controlledIo: { snapshot: () => ({ hits: { modelsdev: [], openrouter: [] }, proxy_attempts: [] }) },
        },
      },
      ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
      ev: {},
    }),
  );
  return invocation.then(
    (value) => {
      const settled = { threw: false, error: null, checks, before, dataDir };
      console.log("ZZZ-SETTLED:", JSON.stringify(settled).slice(0, 300));
      return settled;
    },
    (error) => {
      const settled = { threw: true, error: String(error), checks, before, dataDir };
      console.log("ZZZ-SETTLED:", JSON.stringify(settled).slice(0, 300));
      return settled;
    },
  );
}

test("denied-ipc：settings_write 追踪在场必须判违规（不得被筛选排除）", async () => {
  const result = await runDeniedIpc({
    records: [
      {
        seq: 1,
        event: "settings_write",
        path: "C:/synthetic-root/tokenscope/settings.toml",
      },
    ],
    settingsMutator: (dataDir) => {
      // 同字节重写：内容不变、mtime 显式前移（避免同毫秒写入 mtime 相同）。
      const target = path.join(dataDir, "settings.toml");
      const text = fs.readFileSync(target, "utf8");
      fs.writeFileSync(target, text, "utf8");
      const future = new Date(Date.now() + 60_000);
      fs.utimesSync(target, future, future);
    },
  });
  const trace = (result.checks ?? []).find((check) => check.name === "denied_ipc_trace_has_no_business_events");
  assert.ok(trace, `缺少追踪断言 syncError=${result.syncError}`);
  assert.equal(trace.pass, false, "settings_write 在场不得报「无业务追踪」");
  console.log("ZZZ-TRACE-DETAIL:", JSON.stringify(trace.detail));
  console.log("ZZZ-BEFORE:", JSON.stringify(result.before.settings), "AFTER:", JSON.stringify(realSnapshotDataDir(result.dataDir).settings));
  assert.match(trace.detail, /settings_write/);
  const files = result.checks.find((check) => check.name === "denied_ipc_files_unchanged");
  assert.equal(files.pass, false, "同字节重写（mtime 变化）不得报「文件未变化」");
});

test("denied-ipc：零追踪 + 文件未变保持通过（守护）", async () => {
  const result = await runDeniedIpc({ records: [], settingsMutator: null });
  assert.equal(result.threw, false, JSON.stringify(result.checks.filter((c) => !c.pass)));
  const trace = result.checks.find((check) => check.name === "denied_ipc_trace_has_no_business_events");
  assert.ok(trace && trace.pass);
});

// ── 十七轮复核 P2：preseeded 夹具达到计划 §4b.1 组合 ────────────────────
// 计划要求：有效 SQLite 历史库及 WAL/SHM、两份价格快照、settings 含旧
// 修订布尔同意。旧夹具写普通文本（非 SQLite）、单份快照、无同意字段。

test("preseeded 夹具：产品 schema SQLite 历史库 + WAL/SHM + 两份含 synced_at 的价格快照", async () => {
  // 十八轮复核：夹具必须**产品可用**——HistoryDb::open 的 v1 迁移可执行
  // （usage_events 含 ts_seconds 等产品列）、价格快照含 synced_at。建表
  // SQL 由场景实现从 history.rs 的 SCHEMA_V1 提取（单一事实源）。
  const preseedBusinessFiles = extractHelper("preseedBusinessFiles", {
    fs,
    path,
    REPO: path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../.."),
    spawnSyncBounded: realSpawnSyncBounded,
  });
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-preseed-fixture-"));
  try {
    preseedBusinessFiles(dir);
    const db = fs.readFileSync(path.join(dir, "history.db"));
    assert.ok(db.length > 1000, `SQLite 库过小：${db.length} 字节`);
    assert.equal(db.subarray(0, 16).toString("utf8"), "SQLite format 3\0", "history.db 必须是有效 SQLite 文件");
    for (const name of ["history.db-wal", "history.db-shm"]) {
      assert.ok(fs.existsSync(path.join(dir, name)), `缺少 ${name}`);
    }
    // 产品列存在性（Python sqlite3 校验——十八轮探针同口径）。
    const checkScript = [
      "import sqlite3, sys, json",
      "con = sqlite3.connect(sys.argv[1])",
      "cols = [r[1] for r in con.execute('PRAGMA table_info(usage_events)')]",
      "print(json.dumps({'cols': cols, 'rows': con.execute('SELECT COUNT(*) FROM usage_events').fetchone()[0], 'user_version': con.execute('PRAGMA user_version').fetchone()[0]}))",
    ].join("; ");
    const { execFileSync } = await import("node:child_process");
    const check = JSON.parse(
      execFileSync("python", ["-c", checkScript, path.join(dir, "history.db")], { encoding: "utf8" }),
    );
    for (const column of ["ts_seconds", "ts_nanos", "model_identity", "event_key", "observed_at_utc", "parser_revision"]) {
      assert.ok(check.cols.includes(column), `usage_events 缺产品列 ${column}`);
    }
    assert.equal(check.rows, 3);
    assert.equal(check.user_version, 1);
    for (const name of ["pricing-modelsdev.json", "pricing-openrouter.json"]) {
      const snapshot = JSON.parse(fs.readFileSync(path.join(dir, name), "utf8"));
      assert.ok(snapshot.synced_at, `${name} 缺 synced_at`);
      assert.ok(Array.isArray(snapshot.entries) && snapshot.entries.length > 0, `${name} 缺条目`);
      assert.ok(snapshot.entries[0].input != null || snapshot.entries[0].prompt != null, `${name} 条目缺价格`);
    }
  } finally {
    fs.rmSync(dir, { recursive: true });
  }
});

// ── 十八轮复核 P2：settings 快照必须逐字节（宽松解码会掩盖非法字节改写）──

test("十八轮：settings 非法字节改写（同 size/mtime、无追踪）必须被字节快照检出", async () => {
  // 构造：before 为合法 UTF-8；mutator 写入不同非法字节但 size/mtime 保持
  // ——text 宽松解码后两者可能同串，只有字节快照能区分。
  const dataDir = path.join(
    fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-byte-snap-")),
    "tokenscope",
  );
  fs.mkdirSync(dataDir, { recursive: true });
  const target = path.join(dataDir, "settings.toml");
  fs.writeFileSync(target, Buffer.from("price_auto_sync = false\n", "utf8"));
  const before = realSnapshotDataDir(dataDir);
  const stat = fs.statSync(target);
  // 写入不同非法字节（与原文同 size：把一个 ASCII 换成多字节非法序列再
  // 补齐大小），mtime 保持原值。
  const evil = Buffer.concat([
    Buffer.from("price_auto_sync = fals", "utf8"),
    Buffer.from([0xc3, 0x28]), // 非法 UTF-8 序列
    Buffer.from("e\n", "utf8"),
  ]);
  fs.writeFileSync(target, evil);
  fs.utimesSync(target, stat.mtime, stat.mtime);
  const after = realSnapshotDataDir(dataDir);
  const changes = realDataDirChanges(before, after);
  assert.ok(
    changes.includes("settings.toml"),
    `字节快照必须检出非法字节改写：${JSON.stringify({ before: before.settings, after: after.settings })}`,
  );
  // 且快照值本身逐字节可辨（不是宽松解码后的同串）。
  assert.notEqual(before.settings.bytesHex, after.settings.bytesHex);
});
