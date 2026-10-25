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

const source = fs.readFileSync(new URL("../native-acceptance.mjs", import.meta.url), "utf8");

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
  return {
    business: snapshotBusinessFiles(dataDir),
    settings: fs.existsSync(settingsPath) ? fs.readFileSync(settingsPath, "utf8") : null,
  };
}

function realDataDirChanges(before, after) {
  return [
    ...changedBusinessFiles(before.business, after.business),
    ...(before.settings === after.settings ? [] : ["settings.toml"]),
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
      snapshotDataDir: realSnapshotDataDir,
      dataDirChanges: realDataDirChanges,
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
        prelaunchSettings: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
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
        prelaunchSettings: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
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
        prelaunchSettings: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
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
        prelaunchSettings: fs.readFileSync(path.join(dataDir, "settings.toml"), "utf8"),
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

function runAcceptOrder({ records, settingsText }) {
  const box = { x: 100, y: 200, width: 120, height: 40 };
  const clicksLog = [];
  const checks = [];
  const run = extractScenario(
    "privacy-accept-order",
    "\n  /**\n   * AC06 场景 4a",
    {
      path,
      settingsFingerprint,
      fs: {
        readFileSync(_file, encoding) {
          return encoding === "utf8" ? settingsText : Buffer.from(settingsText);
        },
      },
      readAcceptanceTrace() {
        return { violations: [], records };
      },
    },
  );
  return run({
    inst: {
      page: {
        async waitForSelector() {},
        async waitForTimeout() {},
        locator() {
          return {
            first() {
              return { async boundingBox() { return box; } };
            },
          };
        },
        async evaluate(fn) {
          if (fn.toString().includes("addEventListener")) {
            clicksLog.length = 0;
            return undefined;
          }
          return clicksLog;
        },
        mouse: {
          async click() {
            clicksLog.push({
              type: "click",
              ts: 1030 + clicksLog.length * 30,
              x: box.x + box.width / 2,
              y: box.y + box.height / 2,
            });
          },
        },
        async screenshot() {},
      },
      root: "C:/synthetic-root",
      opts: { shots: "C:/synthetic-shots" },
    },
    ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
    ev: {},
  }).then(() => ({ checks }));
}

function consentRecords(settingsText, { hash } = {}) {
  const fingerprint = settingsFingerprint(Buffer.from(settingsText));
  return [
    {
      seq: 1,
      event: "consent_saved",
      settings_hash: { algorithm: "fnv1a64", value: hash ?? fingerprint.value },
    },
  ];
}

const ORDER_SETTINGS = "privacy_policy_accepted = true\nprivacy_policy_revision = 2\n";

test("accept-order: 同意 → 解锁 → 首读严格成序且指纹一致才通过", async () => {
  const records = [
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
    ...consentRecords(ORDER_SETTINGS).map((row) => ({ ...row, seq: 2 })),
    { seq: 3, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const failed = checks.filter((check) => !check.pass);
  assert.ok(failed.length > 0, "颠倒顺序不得通过");
  assert.ok(failed.some((check) => check.name === "consent_saved_precedes_business_ready"));
});

test("accept-order: consent_saved 整条缺失必须失败", async () => {
  const records = [{ seq: 1, event: "business_ready" }, { seq: 2, event: "history_open" }];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const failed = checks.filter((check) => !check.pass);
  assert.ok(failed.length > 0, "缺事件不得通过（旧断言 read>max(空,ready) 会放行）");
  assert.ok(failed.some((check) => check.name === "consent_saved_precedes_business_ready"));
});

test("accept-order: 落盘指纹与最终 settings 不一致必须失败", async () => {
  const records = [
    ...consentRecords(ORDER_SETTINGS, { hash: "0000000000000000" }),
    { seq: 2, event: "business_ready" },
    { seq: 3, event: "history_open" },
  ];
  const { checks } = await runAcceptOrder({ records, settingsText: ORDER_SETTINGS });
  const mismatch = checks.find((check) => check.name === "consent_saved_single_atomic_replace");
  assert.ok(mismatch, "缺少单次原子落盘指纹断言");
  assert.equal(mismatch.pass, false);
});

// ── privacy-accept-order：双击输入真实性（PR52 复盘缺陷 4）───────────────
// 旧实现第二次 locator.click 失败被 catch 吞掉——双击可能只发了一次输入
// 仍全绿（反例 secondClickCounterexample：successfulClicks=1 通过）。

function runAcceptOrderClicks({ deliveredClicks, gapMs, failSecondDispatch, records }) {
  const checks = [];
  const box = { x: 100, y: 200, width: 120, height: 40 };
  const centerX = box.x + box.width / 2;
  const centerY = box.y + box.height / 2;
  const clicksLog = [];
  let dispatched = 0;
  const run = extractScenario(
    "privacy-accept-order",
    "\n  /**\n   * AC06 场景 4a",
    {
      path,
      settingsFingerprint,
      fs: {
        readFileSync(_file, encoding) {
          return encoding === "utf8" ? ORDER_SETTINGS : Buffer.from(ORDER_SETTINGS);
        },
      },
      readAcceptanceTrace() {
        return { violations: [], records };
      },
    },
  );
  const invocation = run({
    inst: {
      page: {
        async waitForSelector() {},
        async waitForTimeout() {},
        locator() {
          return {
            first() {
              return { async boundingBox() { return box; } };
            },
          };
        },
        async evaluate(fn) {
          const text = fn.toString();
          if (text.includes("addEventListener")) {
            clicksLog.length = 0;
            return undefined;
          }
          return clicksLog;
        },
        mouse: {
          async click() {
            dispatched += 1;
            if (failSecondDispatch && dispatched === 2) throw new Error("CDP 输入通道错误");
            if (dispatched <= deliveredClicks) {
              clicksLog.push({
                type: "click",
                ts: 1000 + (dispatched - 1) * gapMs,
                x: centerX,
                y: centerY,
              });
            }
          },
        },
        async screenshot() {},
      },
      root: "C:/synthetic-root",
      opts: { shots: "C:/synthetic-shots" },
    },
    ck: { add: (name, pass, detail) => checks.push({ name, pass, detail }) },
    ev: {},
  });
  return invocation.then(() => ({ checks, dispatched }));
}

function orderedRecords() {
  return [
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
  await assert.rejects(
    runAcceptOrderClicks({ deliveredClicks: 2, gapMs: 30, failSecondDispatch: true, records: orderedRecords() }),
    /CDP 输入通道错误/,
  );
});
