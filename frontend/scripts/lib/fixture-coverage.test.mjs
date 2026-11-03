// @vitest-environment node
import assert from "node:assert/strict";
import { describe, expect, it } from "vitest";

// AC08：fixture 覆盖表聚合——覆盖必须来自当次运行产物（巡检行与契约
// 实际打开记录），不能用源码名单或 required 清单冒充。
describe("fixture 覆盖表聚合（AC08）", () => {
  it("缺失的覆盖数据不被冒充为已覆盖", async () => {
    const { computeFixtureCoverage } = await import("./fixture-coverage.mjs");
    const registered = ["normal", "large-pricing", "privacy-consent-pending"];
    // 巡检只跑了 normal；没有任何契约打开记录。
    const coverage = computeFixtureCoverage({
      registered,
      patrol: [{ fixture: "normal", ok: true }],
      contracts: [],
    });
    assert.deepEqual(coverage.uncovered, ["large-pricing", "privacy-consent-pending"]);
    const byName = Object.fromEntries(coverage.rows.map((r) => [r.fixture, r]));
    assert.equal(byName.normal.patrol, true);
    assert.deepEqual(byName.normal.contracts, []);
    assert.equal(byName["large-pricing"].patrol, false);
    assert.deepEqual(byName["large-pricing"].contracts, []);
  });

  it("契约实际打开的 fixture 记入该契约，且重复打开去重", async () => {
    const { computeFixtureCoverage } = await import("./fixture-coverage.mjs");
    const registered = ["normal", "settings-deferred-race"];
    const coverage = computeFixtureCoverage({
      registered,
      patrol: [{ fixture: "normal", ok: true }],
      contracts: [
        { contract: "saved_settings_survive_delayed_read", fixtures: ["settings-deferred-race"] },
        {
          contract: "settings_failure_retry_restores_all_controls",
          fixtures: ["settings-deferred-race", "normal"],
        },
      ],
    });
    assert.deepEqual(coverage.uncovered, []);
    const byName = Object.fromEntries(coverage.rows.map((r) => [r.fixture, r]));
    assert.deepEqual(byName["settings-deferred-race"].contracts, [
      "saved_settings_survive_delayed_read",
      "settings_failure_retry_restores_all_controls",
    ]);
    assert.equal(byName["settings-deferred-race"].patrol, false);
    assert.equal(byName.normal.patrol, true);
    assert.deepEqual(byName.normal.contracts, ["settings_failure_retry_restores_all_controls"]);
  });

  it("巡检失败的 fixture 仍算运行过（覆盖记录运行事实，不冒充通过）", async () => {
    const { computeFixtureCoverage } = await import("./fixture-coverage.mjs");
    const registered = ["broken-fixture"];
    const coverage = computeFixtureCoverage({
      registered,
      patrol: [{ fixture: "broken-fixture", ok: false }],
      contracts: [],
    });
    assert.deepEqual(coverage.uncovered, []);
    assert.equal(coverage.rows[0].patrol, true);
  });

  it("未注册的 fixture 出现在运行产物中时不得静默丢弃", async () => {
    const { computeFixtureCoverage } = await import("./fixture-coverage.mjs");
    const coverage = computeFixtureCoverage({
      registered: ["normal"],
      patrol: [{ fixture: "normal", ok: true }],
      contracts: [{ contract: "x", fixtures: ["ghost-fixture"] }],
    });
    assert.deepEqual(coverage.unregistered, ["ghost-fixture"]);
  });
});
