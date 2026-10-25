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
import { changedBusinessFiles, snapshotBusinessFiles } from "./acceptance-io.mjs";

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
