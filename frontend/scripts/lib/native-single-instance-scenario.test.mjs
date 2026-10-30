// @vitest-environment node
// AC10 场景编排的合成契约：用假实例/假外部观察跑完整 ui-single-instance 流程，
// 区分“全部实际观察到”“托盘观察不可用”“前台观察不可用”“第二次启动超时”。
// 不启动 Native；尤其验证托盘 pending 不会被进程数或场景结构替身成通过。
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { nativeSingleInstance } from "../contracts/native-single-instance.mjs";
import { acceptanceIdentifier } from "./native-single-instance.mjs";

const BASE = "io.github.ywltby.tokenscope";
const ACCEPTANCE = acceptanceIdentifier(BASE);
const REAL_DATA = path.join(os.homedir(), ".tokenscope");
const ROOT = path.join(os.tmpdir(), "tokenscope-ac10-scenario");

function ledger() {
  const rows = [];
  const pending = [];
  return {
    rows,
    pending,
    ck: {
      add: (name, ok, detail = "") => rows.push({ name, ok: !!ok, detail: String(detail) }),
      pending: (name, detail = "") => pending.push({ name, detail: String(detail) }),
    },
  };
}

function observations(overrides = {}) {
  return {
    tray: {
      ok: true,
      visible_count: 3,
      overflow_count: 1,
      total_count: 4,
      token_scope_count: 1,
      matches: [{ area: "visible", text: "TokenScope" }],
      ...overrides.tray,
    },
    stateMinimized: {
      ok: true,
      found: true,
      hwnd: "101",
      visible: true,
      minimized: true,
      rect: "0,0,900,700",
      ...overrides.stateMinimized,
    },
    stateRestored: {
      ok: true,
      found: true,
      hwnd: "101",
      visible: true,
      minimized: false,
      rect: "0,0,900,700",
      ...overrides.stateRestored,
    },
    foregroundBefore: {
      ok: true,
      match: false,
      pid: "101",
      hwnd: "101",
      foreground_pid: "999",
      foreground_hwnd: "999",
      ...overrides.foregroundBefore,
    },
    foregroundAfter: {
      ok: true,
      match: true,
      pid: "101",
      hwnd: "101",
      foreground_pid: "101",
      foreground_hwnd: "101",
      ...overrides.foregroundAfter,
    },
  };
}

function fakeInstance({
  secondary,
  onSecond = null,
  restoreOnSecond = true,
  pageScreenshot = async () => {},
}) {
  let restored = false;
  return {
    instance: {
      root: ROOT,
      opts: {
        baseIdentifier: BASE,
        acceptanceIdentifier: ACCEPTANCE,
        realDataDir: REAL_DATA,
        shots: os.tmpdir(),
      },
      proc: { pid: 101, exitCode: null },
      waitForData: async () => {},
      launchSecondary: async () => {
        restored = restoreOnSecond;
        onSecond?.();
        return secondary;
      },
      page: { screenshot: pageScreenshot },
    },
    isRestored: () => restored,
  };
}

function secondaryRecord(overrides = {}) {
  return {
    hostPid: 202,
    role: "secondary",
    owner: "acceptance-driver",
    parentPid: 999,
    root: ROOT,
    sameRoot: true,
    acceptanceIdentifier: ACCEPTANCE,
    hostExitCode: 0,
    hostSignal: null,
    exitElapsedMs: 120,
    exitBudgetMs: 15000,
    exitWithinBudget: true,
    exitTimedOut: false,
    exitConfirmed: true,
    exitBeforeTerminate: true,
    exitAfterTerminate: false,
    terminateRequested: false,
    terminateError: null,
    terminateUnconfirmed: false,
    ...overrides,
  };
}

async function runScenario({
  secondary,
  obs,
  windowOverride = null,
  restoreOnSecond = true,
  onTrayAfter = null,
  trayBaseline = {
    ok: true,
    visible_count: 3,
    overflow_count: 1,
    total_count: 4,
    token_scope_count: 0,
    matches: [],
  },
}) {
  const { instance, isRestored } = fakeInstance({ secondary, restoreOnSecond });
  const { rows, pending, ck } = ledger();
  const ev = { tray_baseline_before_primary: trayBaseline };
  let trayReads = 0;
  await nativeSingleInstance({
    inst: instance,
    ck,
    ev,
    observeWindow:
      windowOverride ??
      ((_, action) => {
        if (action === "state") {
          return isRestored() ? obs.stateRestored : obs.stateMinimized;
        }
        return isRestored() ? obs.foregroundAfter : obs.foregroundBefore;
      }),
    observeTray: () => {
      trayReads += 1;
      if (trayReads > 1) onTrayAfter?.(instance);
      return obs.tray;
    },
    runWindow: () => ({ status: 0, stdout: "PID 101 STATE_CHANGED true", stderr: "" }),
    trayBudgetMs: 80,
    restoreBudgetMs: 120,
    pollIntervalMs: 5,
  });
  return { rows, pending, ev };
}

const byName = (rows, name) => rows.find((row) => row.name === name);
const pendingNames = (pending) => pending.map((row) => row.name);

describe("ui-single-instance 场景编排（假实例）", () => {
  it("所有外部观察都可用时，双 PID/限时退出/恢复前台/唯一托盘全部通过", async () => {
    const { rows, pending, ev } = await runScenario({
      secondary: { record: secondaryRecord(), verdict: { status: "passed", reason: "ok" } },
      obs: observations(),
    });
    expect(pending).toEqual([]);
    for (const name of [
      "acceptance_root_isolated_from_real_data",
      "acceptance_identifier_differs_from_product",
      "single_tray_icon_before_second",
      "first_window_minimized_before_second",
      "first_window_state_observed_minimized",
      "second_process_same_isolated_root",
      "secondary_pid_differs_from_primary",
      "second_process_exit_confirmed",
      "second_process_exited_within_budget",
      "second_process_exit_code_zero",
      "second_process_exit_verdict",
      "first_window_restored_visible",
      "first_window_foreground",
      "single_tray_icon_after_second",
      "tray_count_unchanged_after_second",
      "first_instance_alive_after_second",
    ]) {
      expect(byName(rows, name)?.ok, name).toBe(true);
    }
    expect(ev.single_instance.secondary).toMatchObject({
      pid: 202,
      role: "secondary",
      same_root: true,
      exit_code: 0,
    });
    expect(ev.single_instance.restore.verdict.status).toBe("passed");
  });

  it("正式实例已占一个托盘图标时，按基线+1 核对验收实例只新增一个", async () => {
    const { rows, pending, ev } = await runScenario({
      secondary: { record: secondaryRecord(), verdict: { status: "passed", reason: "ok" } },
      obs: observations({
        tray: {
          token_scope_count: 2,
          matches: [
            { area: "visible", text: "TokenScope" },
            { area: "overflow", text: "TokenScope hidden" },
          ],
        },
      }),
      trayBaseline: {
        ok: true,
        visible_count: 3,
        overflow_count: 1,
        total_count: 4,
        token_scope_count: 1,
        matches: [{ area: "overflow", text: "TokenScope" }],
      },
    });
    expect(pending).toEqual([]);
    expect(byName(rows, "single_tray_icon_before_second")?.ok).toBe(true);
    expect(byName(rows, "single_tray_icon_after_second")?.ok).toBe(true);
    expect(byName(rows, "tray_count_unchanged_after_second")?.ok).toBe(true);
    expect(ev.single_instance.expected_tray_count).toBe(2);
  });

  it("托盘观察不可用时三项均为 pending，绝不因场景/进程数变成通过", async () => {
    const { rows, pending } = await runScenario({
      secondary: { record: secondaryRecord(), verdict: { status: "passed", reason: "ok" } },
      obs: observations({
        tray: { ok: false, error: "tray probe failed", token_scope_count: null },
      }),
    });
    expect(pendingNames(pending)).toEqual(
      expect.arrayContaining([
        "single_tray_icon_before_second",
        "single_tray_icon_after_second",
        "tray_count_unchanged_after_second",
      ]),
    );
    expect(byName(rows, "single_tray_icon_before_second")).toBeUndefined();
    expect(byName(rows, "single_tray_icon_after_second")).toBeUndefined();
    expect(byName(rows, "second_process_exit_code_zero")?.ok).toBe(true);
  });

  it("前台观察不可用时只把前台检查记 pending，可见性仍可独立通过", async () => {
    const obs = observations();
    const { rows, pending } = await runScenario({
      secondary: { record: secondaryRecord(), verdict: { status: "passed", reason: "ok" } },
      obs,
      windowOverride: (_, action) =>
        action === "state" ? obs.stateRestored : { ok: false, error: "foreground probe failed" },
    });
    expect(pendingNames(pending)).toContain("first_window_foreground");
    expect(byName(rows, "first_window_restored_visible")?.ok).toBe(true);
    expect(byName(rows, "first_window_foreground")).toBeUndefined();
  });

  it("退出码不可读时退出耗时仍可判定，但退出码/verdict 记 pending", async () => {
    const { rows, pending } = await runScenario({
      secondary: {
        record: secondaryRecord({
          hostExitCode: null,
          hostSignal: null,
          exitWithinBudget: true,
          exitTimedOut: false,
        }),
        verdict: { status: "pending", reason: "第二次启动的退出码不可读" },
      },
      obs: observations(),
    });
    expect(pendingNames(pending)).toEqual(
      expect.arrayContaining(["second_process_exit_code_zero", "second_process_exit_verdict"]),
    );
    expect(byName(rows, "second_process_exited_within_budget")?.ok).toBe(true);
    expect(byName(rows, "second_process_exit_code_zero")).toBeUndefined();
  });

  it("第二次启动超时/非零退出时，退出与恢复检查为失败（不是 pending）", async () => {
    const { rows, pending } = await runScenario({
      secondary: {
        record: secondaryRecord({
          hostExitCode: null,
          exitWithinBudget: false,
          exitTimedOut: true,
          exitConfirmed: false,
          terminateRequested: true,
          terminateError: null,
          terminateUnconfirmed: true,
        }),
        verdict: { status: "failed", reason: "第二次启动未在 15000 ms 内退出" },
      },
      obs: observations(),
      restoreOnSecond: false,
    });
    expect(pending).toEqual([]);
    expect(byName(rows, "second_process_exit_confirmed")?.ok).toBe(false);
    expect(byName(rows, "second_process_exited_within_budget")?.ok).toBe(false);
    expect(byName(rows, "second_process_exit_code_zero")?.ok).toBe(false);
    expect(byName(rows, "second_process_exit_verdict")?.ok).toBe(false);
    expect(byName(rows, "first_window_restored_visible")?.ok).toBe(false);
    expect(byName(rows, "first_window_foreground")?.ok).toBe(false);
  });

  it("第一个实例以 signalCode 退出时存活检查失败", async () => {
    const { rows } = await runScenario({
      secondary: { record: secondaryRecord(), verdict: { status: "passed", reason: "ok" } },
      obs: observations(),
      onTrayAfter: (instance) => {
        instance.proc.signalCode = "SIGTERM";
      },
    });
    expect(byName(rows, "first_instance_alive_after_second")?.ok).toBe(false);
  });
});
