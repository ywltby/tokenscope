// AC10（N12 单实例）：同一隔离根启动第二个验收实例，核对第二个进程限时退出、
// 第一个窗口恢复可见并获得前台、验收实例只新增一个 TokenScope 托盘图标
// （按启动前基线 +1 核对，正式实例可能同时占用一个图标）。
//
// 外部观察全部来自独立进程（PowerShell/Win32）：窗口可见/最小化/前台与
// 托盘图标文本。观察不可用时对应检查记为 pending；绝不用进程数量代替
// 托盘证据，也不把“第一个进程仍存活”当作窗口恢复/前台。
import path from "node:path";
import { fileURLToPath } from "node:url";
import { ensureNotCanceled, spawnSyncBounded } from "../lib/native-scenario.mjs";
import {
  SINGLE_INSTANCE_RESTORE_BUDGET_MS,
  SINGLE_INSTANCE_TRAY_BUDGET_MS,
  acceptanceIdentifier,
  classifyTrayObservation,
  classifyWindowRestore,
  isIsolatedAcceptanceRoot,
  parseForeground,
  parseTrayObservation,
  parseWindowState,
} from "../lib/native-single-instance.mjs";

const WINDOW_ACTION = fileURLToPath(
  new URL("../../../scripts/native-window-action.ps1", import.meta.url),
);
const TRAY_READER = fileURLToPath(
  new URL("../../../scripts/read-tray-notification-area.ps1", import.meta.url),
);
const POLL_INTERVAL_MS = 150;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function runPowerShell(script, args, action) {
  return spawnSyncBounded(
    "pwsh",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, ...args],
    { action },
  );
}

function readWindow(inst, action) {
  let result;
  try {
    result = runPowerShell(
      WINDOW_ACTION,
      ["-ProcessId", String(inst.proc.pid), "-Action", action],
      `window-${action}`,
    );
  } catch (error) {
    return {
      ok: false,
      error: `${action} 观察命令失败：${String(error?.message ?? error).slice(0, 160)}`,
    };
  }
  if (result.status !== 0) {
    return {
      ok: false,
      error: `${action} 观察失败（pwsh 退出码 ${result.status}）：${(result.stderr || "").slice(0, 160)}`,
    };
  }
  return action === "state" ? parseWindowState(result.stdout) : parseForeground(result.stdout);
}

function readTray() {
  let result;
  try {
    result = runPowerShell(TRAY_READER, [], "tray-read");
  } catch (error) {
    return {
      ok: false,
      error: `托盘观察命令失败：${String(error?.message ?? error).slice(0, 160)}`,
      token_scope_count: null,
      matches: [],
    };
  }
  if (result.status !== 0) {
    return {
      ok: false,
      error: `托盘观察失败（pwsh 退出码 ${result.status}）：${(result.stderr || "").slice(0, 160)}`,
      token_scope_count: null,
      matches: [],
    };
  }
  return parseTrayObservation(result.stdout);
}

/** 启动第一个实例前的托盘基线：用于证明“验收实例只新增一个图标”，
 * 而不是要求系统总托盘数为 1（用户可能正开着正式实例）。 */
export function readTrayObservation() {
  return readTray();
}

/** 有界轮询：每次观察前检查场景取消与第一个实例存活；超时返回最后一次观察。 */
async function pollObservation({
  inst,
  isCanceled,
  action,
  observe,
  accept,
  budgetMs,
  intervalMs = POLL_INTERVAL_MS,
}) {
  const started = Date.now();
  let last = null;
  while (Date.now() - started <= budgetMs) {
    ensureNotCanceled(isCanceled, action);
    if (inst.proc.exitCode !== null) {
      throw new Error(`第一个实例在「${action}」期间退出（退出码=${inst.proc.exitCode}）`);
    }
    last = observe();
    if (accept(last)) return { matched: true, value: last, elapsedMs: Date.now() - started };
    await sleep(intervalMs);
  }
  return { matched: false, value: last, elapsedMs: Date.now() - started };
}

export async function nativeSingleInstance({
  inst,
  ck,
  ev,
  markStage = () => {},
  markStageDone = () => {},
  isCanceled = () => false,
  observeWindow = readWindow,
  observeTray = readTray,
  runWindow = (target, action) =>
    runPowerShell(
      WINDOW_ACTION,
      ["-ProcessId", String(target.proc.pid), "-Action", action],
      `window-${action}`,
    ),
  trayBudgetMs = SINGLE_INSTANCE_TRAY_BUDGET_MS,
  restoreBudgetMs = SINGLE_INSTANCE_RESTORE_BUDGET_MS,
  pollIntervalMs = POLL_INTERVAL_MS,
}) {
  const baseIdentifier = inst.opts.baseIdentifier;
  const acceptance = inst.opts.acceptanceIdentifier;
  const realDataDir = inst.opts.realDataDir;
  const rootCheck = isIsolatedAcceptanceRoot(inst.root, realDataDir);
  const expectedAcceptance = acceptanceIdentifier(baseIdentifier);
  const trayBaseline = ev.tray_baseline_before_primary ?? null;
  const trayBaselineOk =
    trayBaseline?.ok === true && Number.isInteger(trayBaseline.token_scope_count);
  const expectedTrayCount = trayBaselineOk ? trayBaseline.token_scope_count + 1 : null;
  ev.single_instance = {
    base_identifier: baseIdentifier,
    acceptance_identifier: acceptance,
    real_data_dir: realDataDir,
    root_check: rootCheck,
    tray_baseline: trayBaseline,
    expected_tray_count: expectedTrayCount,
  };
  ck.add("acceptance_root_isolated_from_real_data", rootCheck.ok, rootCheck.reason);
  ck.add(
    "acceptance_identifier_differs_from_product",
    acceptance === expectedAcceptance &&
      acceptance !== baseIdentifier &&
      acceptance.endsWith(".acceptance"),
    `${baseIdentifier} -> ${acceptance}`,
  );

  await inst.waitForData();

  // 第一个实例就绪后等待真实托盘图标出现；只观察 TokenScope 文本，不用进程数。
  // 基线可能包含用户正在运行的正式实例：只要求验收实例相对基线新增 1 个。
  markStage("tray-before-second");
  const trayBeforePoll = await pollObservation({
    inst,
    isCanceled,
    action: "tray-before-second",
    observe: observeTray,
    accept: (value) => {
      if (value.ok !== true || !Number.isInteger(value.token_scope_count)) return false;
      return trayBaselineOk ? value.token_scope_count >= expectedTrayCount : true;
    },
    budgetMs: trayBudgetMs,
    intervalMs: pollIntervalMs,
  });
  markStageDone("tray-before-second");
  const trayBefore = trayBeforePoll.value ?? { ok: false, error: "无观察结果" };
  const trayBeforeVerdict = trayBaselineOk
    ? classifyTrayObservation({ observation: trayBefore, expected: expectedTrayCount })
    : {
        status: "pending",
        reason: `启动前托盘基线不可用：${trayBaseline?.error ?? "无基线"}`,
      };
  ev.single_instance.tray_before_second = {
    observation: trayBefore,
    baseline_count: trayBaseline?.token_scope_count ?? null,
    expected_count: expectedTrayCount,
    matched: trayBeforePoll.matched,
    elapsed_ms: trayBeforePoll.elapsedMs,
    verdict: trayBeforeVerdict,
  };
  if (trayBeforeVerdict.status === "pending") {
    ck.pending("single_tray_icon_before_second", trayBeforeVerdict.reason);
  } else {
    ck.add(
      "single_tray_icon_before_second",
      trayBeforeVerdict.status === "passed",
      trayBeforeVerdict.reason,
    );
  }

  // 最小化第一个窗口：第二个实例的 single-instance 回调必须 show/unminimize/focus。
  const minimize = runWindow(inst, "minimize");
  const minimized = /STATE_CHANGED true/i.test(minimize.stdout || "");
  ck.add(
    "first_window_minimized_before_second",
    minimize.status === 0 && minimized,
    `${minimize.stdout || ""}${minimize.stderr || ""}`.trim().slice(0, 200),
  );
  const minimizedState = observeWindow(inst, "state");
  ev.single_instance.minimized_state = minimizedState;
  ck.add(
    "first_window_state_observed_minimized",
    minimizedState.ok === true &&
      minimizedState.found === true &&
      minimizedState.minimized === true,
    JSON.stringify(minimizedState).slice(0, 240),
  );

  // 同一隔离根启动第二个进程；只记录/等待退出，不接 CDP、不替换第一个实例。
  markStage("second-launch");
  const secondary = await inst.launchSecondary();
  markStageDone("second-launch");
  const secondaryRecord = secondary.record;
  ev.single_instance.secondary = {
    pid: secondaryRecord.hostPid,
    role: secondaryRecord.role,
    owner: secondaryRecord.owner,
    parent_pid: secondaryRecord.parentPid,
    root: secondaryRecord.root,
    same_root: secondaryRecord.sameRoot,
    acceptance_identifier: secondaryRecord.acceptanceIdentifier,
    exit_code: secondaryRecord.hostExitCode,
    exit_signal: secondaryRecord.hostSignal,
    exit_elapsed_ms: secondaryRecord.exitElapsedMs,
    exit_budget_ms: secondaryRecord.exitBudgetMs,
    exit_within_budget: secondaryRecord.exitWithinBudget,
    exit_timed_out: secondaryRecord.exitTimedOut,
    verdict: secondary.verdict,
  };
  ck.add(
    "second_process_same_isolated_root",
    secondaryRecord.sameRoot === true && secondaryRecord.acceptanceIdentifier === acceptance,
    `root=${secondaryRecord.root}; identifier=${secondaryRecord.acceptanceIdentifier}`,
  );
  ck.add(
    "second_process_exited_within_budget",
    secondaryRecord.exitWithinBudget === true && secondaryRecord.exitTimedOut !== true,
    `${secondaryRecord.exitElapsedMs} ms / 上限 ${secondaryRecord.exitBudgetMs} ms`,
  );
  if (
    secondaryRecord.hostExitCode === null &&
    secondaryRecord.hostSignal === null &&
    secondaryRecord.exitTimedOut !== true &&
    secondaryRecord.spawnError == null
  ) {
    ck.pending("second_process_exit_code_zero", "第二次启动退出码不可读");
  } else {
    ck.add(
      "second_process_exit_code_zero",
      secondaryRecord.hostExitCode === 0 && secondaryRecord.hostSignal === null,
      `exit=${secondaryRecord.hostExitCode ?? "unknown"}; signal=${secondaryRecord.hostSignal ?? "none"}`,
    );
  }
  if (secondary.verdict.status === "pending") {
    ck.pending("second_process_exit_verdict", secondary.verdict.reason);
  } else {
    ck.add(
      "second_process_exit_verdict",
      secondary.verdict.status === "passed",
      secondary.verdict.reason,
    );
  }

  // 第二个进程退出后观察第一个窗口：可见 + 未最小化 + 前台属于第一个 PID。
  markStage("restore-observe");
  const restorePoll = await pollObservation({
    inst,
    isCanceled,
    action: "restore-observe",
    observe: () => ({
      state: observeWindow(inst, "state"),
      foreground: observeWindow(inst, "foreground"),
    }),
    accept: (value) =>
      value.state.ok === true &&
      value.state.found === true &&
      value.state.visible === true &&
      value.state.minimized === false &&
      value.foreground.ok === true &&
      value.foreground.match === true,
    budgetMs: restoreBudgetMs,
    intervalMs: pollIntervalMs,
  });
  markStageDone("restore-observe");
  const restoreValue = restorePoll.value ?? {
    state: { ok: false, error: "无窗口状态观察结果" },
    foreground: { ok: false, error: "无前台观察结果" },
  };
  const restoreVerdict = classifyWindowRestore({
    state: restoreValue.state,
    foreground: restoreValue.foreground,
    elapsedMs: restorePoll.elapsedMs,
    budgetMs: restoreBudgetMs,
  });
  ev.single_instance.restore = {
    state: restoreValue.state,
    foreground: restoreValue.foreground,
    matched: restorePoll.matched,
    elapsed_ms: restorePoll.elapsedMs,
    verdict: restoreVerdict,
  };
  for (const [name, result] of [
    ["first_window_restored_visible", restoreVerdict.visibility],
    ["first_window_foreground", restoreVerdict.foreground],
  ]) {
    if (result.status === "pending") ck.pending(name, result.reason);
    else ck.add(name, result.status === "passed", result.reason);
  }

  // 第二个实例退出后再读一次托盘：TokenScope 计数必须仍等于基线+1，且两次一致。
  const trayAfter = observeTray();
  const trayAfterVerdict = trayBaselineOk
    ? classifyTrayObservation({ observation: trayAfter, expected: expectedTrayCount })
    : {
        status: "pending",
        reason: `启动前托盘基线不可用：${trayBaseline?.error ?? "无基线"}`,
      };
  ev.single_instance.tray_after_second = {
    observation: trayAfter,
    baseline_count: trayBaseline?.token_scope_count ?? null,
    expected_count: expectedTrayCount,
    verdict: trayAfterVerdict,
  };
  if (trayAfterVerdict.status === "pending") {
    ck.pending("single_tray_icon_after_second", trayAfterVerdict.reason);
  } else {
    ck.add(
      "single_tray_icon_after_second",
      trayAfterVerdict.status === "passed",
      trayAfterVerdict.reason,
    );
  }
  if (trayBaselineOk && trayBefore.ok === true && trayAfter.ok === true) {
    ck.add(
      "tray_count_unchanged_after_second",
      trayBefore.token_scope_count === expectedTrayCount &&
        trayAfter.token_scope_count === expectedTrayCount,
      `基线 ${trayBaseline.token_scope_count} -> 启动后 ${trayBefore.token_scope_count} -> 第二个后 ${trayAfter.token_scope_count}（期望 ${expectedTrayCount}）`,
    );
  } else {
    ck.pending(
      "tray_count_unchanged_after_second",
      `基线=${trayBaseline?.error ?? "ok"}；前=${trayBefore.error ?? "ok"}；后=${trayAfter.error ?? "ok"}`,
    );
  }

  ck.add(
    "first_instance_alive_after_second",
    inst.proc.exitCode === null,
    `pid=${inst.proc.pid}; exit=${inst.proc.exitCode ?? "alive"}`,
  );
  try {
    await inst.page.screenshot({
      path: path.join(inst.opts.shots, "single-instance-restored.png"),
    });
  } catch (error) {
    ev.single_instance.screenshot_error = String(error?.message ?? error);
  }
}
