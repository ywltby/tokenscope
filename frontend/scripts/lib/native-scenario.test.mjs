// @vitest-environment node
// S00（PR3 计划 §9.1 第 7 条）反例单测：同步命令超时、CDP close 永不返回、场景永不
// 返回、失败取证永不返回、正常路径解除计时器、超时后迟到分支不重启、attach 不终止
// 外部进程；至少一条用 Node 子进程跑共用收尾入口（父测试设上限，断言磁盘证据与非零退出）。
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";
import {
  BROWSER_CLOSE_TIMEOUT_MS,
  DEFAULT_SCENARIO_TIMEOUT_MS,
  FAILURE_CAPTURE_TIMEOUT_MS,
  SCENARIO_TIMEOUT_OVERRIDES,
  SYNC_COMMAND_TIMEOUT_MS,
  ScenarioCanceledError,
  closeBrowserBounded,
  createStageTracker,
  ensureNotCanceled,
  finalizeScenarioTimeout,
  runScenarioBounded,
  scenarioBudgetMs,
  spawnSyncBounded,
  waitStableQuery,
  withTimeout,
} from "./native-scenario.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

describe("预算常量（计划 §6 S00 第 3 条）", () => {
  it("同步命令 30 秒、CDP 关闭 10 秒、失败取证短上限、普通场景 5 分钟", () => {
    expect(SYNC_COMMAND_TIMEOUT_MS).toBe(30000);
    expect(BROWSER_CLOSE_TIMEOUT_MS).toBe(10000);
    expect(FAILURE_CAPTURE_TIMEOUT_MS).toBeLessThan(SYNC_COMMAND_TIMEOUT_MS);
    expect(DEFAULT_SCENARIO_TIMEOUT_MS).toBe(300000);
  });

  it("有明确长时目的的场景保留显式独立预算，其余用 5 分钟默认", () => {
    expect(scenarioBudgetMs("ui-system")).toBe(300000);
    expect(scenarioBudgetMs("first-frame")).toBe(300000);
    expect(SCENARIO_TIMEOUT_OVERRIDES["expire-cycle"]).toBe(750000);
    expect(SCENARIO_TIMEOUT_OVERRIDES["ui-relaunch"]).toBe(1800000);
    expect(SCENARIO_TIMEOUT_OVERRIDES["ui-collection-status"]).toBe(480000);
  });
});

describe("有界同步命令（计划 §9.1 第 2 条）", () => {
  it("同步命令挂起：按上限杀掉并抛出含动作、阶段与命令退出情况的错误", () => {
    const startedAt = Date.now();
    let message = "";
    try {
      spawnSyncBounded(process.execPath, ["-e", "setTimeout(() => {}, 10000)"], {
        action: "window-close",
        stage: "close-command",
        timeoutMs: 300,
      });
    } catch (e) {
      message = e.message;
    }
    // Node 超时杀进程后 error=ETIMEDOUT、signal=SIGTERM、status=null：三种退出情况都要出现
    expect(message).toContain("同步命令超时或启动失败");
    expect(message).toContain("动作=window-close");
    expect(message).toContain("阶段=close-command");
    expect(message).toMatch(/ETIMEDOUT/);
    expect(message).toContain("signal=SIGTERM");
    // 不等满命令自身的 10 秒：上限到点即返回
    expect(Date.now() - startedAt).toBeLessThan(5000);
  });

  it("spawn 失败（命令不存在）也抛出含动作的错误，不静默返回", () => {
    expect(() =>
      spawnSyncBounded("tokenscope-definitely-not-a-command.exe", [], { action: "probe-action" }),
    ).toThrow(/probe-action/);
  });

  it("正常命令原样返回；非零退出码不吞掉，由调用方按既有语义判定", () => {
    const ok = spawnSyncBounded(process.execPath, ["-e", "process.stdout.write('fine')"], {
      action: "ok-action",
    });
    expect(ok.status).toBe(0);
    expect(ok.stdout).toContain("fine");
    const nonzero = spawnSyncBounded(process.execPath, ["-e", "process.exit(3)"], {
      action: "nonzero-action",
    });
    expect(nonzero.status).toBe(3);
    expect(nonzero.error).toBeUndefined();
  });

  it("默认上限是 30 秒常量", () => {
    const quick = spawnSyncBounded(process.execPath, ["-e", "process.exit(0)"], {
      action: "default-timeout",
      timeoutMs: 0, // 非法值必须回退到常量，而不是 0/NaN
    });
    expect(quick.status).toBe(0);
  });
});

describe("有界 CDP 关闭（计划 §9.1 第 5 条）", () => {
  it("close 永不返回：到时返回 closed:false 与 close timeout，不抛出、不挂起", async () => {
    const startedAt = Date.now();
    const hanging = {
      close: () => new Promise(() => {}),
    };
    const result = await closeBrowserBounded(hanging, { timeoutMs: 80 });
    expect(result).toEqual({ closed: false, reason: "close timeout" });
    expect(Date.now() - startedAt).toBeLessThan(2000);
  });

  it("close 正常完成返回 closed:true；close 抛错记为 close error 而不是外抛", async () => {
    expect(await closeBrowserBounded({ close: async () => {} })).toEqual({
      closed: true,
      reason: null,
    });
    const boom = await closeBrowserBounded({
      close: async () => {
        throw new Error("boom");
      },
    });
    expect(boom.closed).toBe(false);
    expect(boom.reason).toContain("boom");
  });

  it("没有 browser 时直接返回 no-browser", async () => {
    expect(await closeBrowserBounded(null)).toEqual({ closed: false, reason: "no-browser" });
    expect(await closeBrowserBounded({})).toEqual({ closed: false, reason: "no-browser" });
  });
});

describe("有界取证（计划 §9.1 第 5 条：失败截图/DOM 查询）", () => {
  it("取证永不返回：短上限后被拒绝并带原因，不阻塞收尾", async () => {
    const startedAt = Date.now();
    await expect(
      withTimeout(() => new Promise(() => {}), 60, "失败取证（截图）未能完成"),
    ).rejects.toThrow(/失败取证（截图）未能完成/);
    expect(Date.now() - startedAt).toBeLessThan(2000);
  });

  it("及时完成的取证原样返回结果", async () => {
    await expect(withTimeout(async () => 42, 1000, "x")).resolves.toBe(42);
  });
});

describe("阶段记录（计划 §9.1 第 3 条）", () => {
  it("mark 写「阶段 <名称>」并记录启动序号与时间；markDone 补完成时刻", () => {
    const lines = [];
    const tracker = createStageTracker({
      log: (line) => lines.push(line),
      launchIndexOf: () => 2,
    });
    tracker.mark("close-command");
    tracker.markDone("close-command");
    tracker.mark("relaunch");
    expect(lines).toEqual(["阶段 close-command", "阶段 relaunch"]);
    const list = tracker.list();
    expect(list).toHaveLength(2);
    expect(list[0]).toMatchObject({ stage: "close-command", launch: 2, done_at: expect.any(String) });
    expect(list[1]).toMatchObject({ stage: "relaunch", done_at: null });
    expect(tracker.last()).toMatchObject({ stage: "relaunch", done_at: null });
  });

  it("没有阶段时 last 为 null；同名阶段只补最近一个未完成项", () => {
    const tracker = createStageTracker({});
    expect(tracker.last()).toBeNull();
    tracker.mark("stop");
    tracker.mark("stop");
    tracker.markDone("stop");
    const list = tracker.list();
    expect(list[0].done_at).toBeNull();
    expect(list[1].done_at).not.toBeNull();
  });
});

describe("场景看门狗（计划 §6 S00 第 3 条 / §9.1 第 4、6 条）", () => {
  it("场景永不返回：到点置取消标志、onTimeout 恰好一次、返回 timeout 结论", async () => {
    const cancelFlag = { requested: false };
    let timeoutCalls = 0;
    const startedAt = Date.now();
    const outcome = await runScenarioBounded({
      scenarioFn: () => new Promise(() => {}),
      timeoutMs: 80,
      cancelFlag,
      onTimeout: () => {
        timeoutCalls++;
      },
    });
    expect(outcome).toEqual({ kind: "timeout", timeoutMs: 80 });
    expect(cancelFlag.requested).toBe(true);
    expect(timeoutCalls).toBe(1);
    expect(Date.now() - startedAt).toBeLessThan(2000);
  });

  it("超时后迟到的场景分支不会再次触发 onTimeout，取消标志保持置位", async () => {
    const cancelFlag = { requested: false };
    let timeoutCalls = 0;
    let lateResolved = false;
    const outcome = await runScenarioBounded({
      scenarioFn: () =>
        sleep(250).then(() => {
          lateResolved = true;
          return "late";
        }),
      timeoutMs: 60,
      cancelFlag,
      onTimeout: () => {
        timeoutCalls++;
      },
    });
    expect(outcome.kind).toBe("timeout");
    expect(timeoutCalls).toBe(1);
    // 迟到分支要真的跑完再断言：竞速提前返回，另一分支不受影响地完成
    await sleep(300);
    expect(lateResolved).toBe(true);
    expect(timeoutCalls).toBe(1);
    expect(cancelFlag.requested).toBe(true);
  });

  it("正常完成：解除计时器（clearTimer 收到句柄）、返回 completed、不触发 onTimeout", async () => {
    let nextHandle = 0;
    const cleared = [];
    const outcome = await runScenarioBounded({
      scenarioFn: async () => "done",
      timeoutMs: 60000,
      cancelFlag: { requested: false },
      setTimer: () => ++nextHandle,
      clearTimer: (h) => cleared.push(h),
    });
    expect(outcome).toMatchObject({ kind: "completed" });
    expect(cleared).toEqual([1]);
  });

  it("场景抛错原样上抛，计时器同样解除", async () => {
    const cleared = [];
    await expect(
      runScenarioBounded({
        scenarioFn: async () => {
          throw new Error("场景内失败");
        },
        timeoutMs: 60000,
        setTimer: () => 1,
        clearTimer: (h) => cleared.push(h),
      }),
    ).rejects.toThrow("场景内失败");
    expect(cleared).toEqual([1]);
  });

  it("onTimeout 抛错不吞掉超时结论（最小证据失败也要返回 timeout）", async () => {
    const outcome = await runScenarioBounded({
      scenarioFn: () => new Promise(() => {}),
      timeoutMs: 60,
      cancelFlag: { requested: false },
      onTimeout: () => {
        throw new Error("磁盘已满");
      },
    });
    expect(outcome.kind).toBe("timeout");
  });

  it("取消标志由场景迟到分支自查：ensureNotCanceled 抛 ScenarioCanceledError", () => {
    const cancelFlag = { requested: false };
    expect(() => ensureNotCanceled(() => cancelFlag.requested, "launch")).not.toThrow();
    cancelFlag.requested = true;
    expect(() => ensureNotCanceled(() => cancelFlag.requested, "launch")).toThrow(
      ScenarioCanceledError,
    );
    try {
      ensureNotCanceled(() => cancelFlag.requested, "close-command");
      expect.unreachable("必须抛出");
    } catch (e) {
      expect(e.action).toBe("close-command");
      expect(e.message).toContain("close-command");
    }
  });

  it("迟到分支不得再次 launch/stop：Instance 的取消自查接线存在于主驱动（源级契约）", async () => {
    const source = fs.readFileSync(
      fileURLToPath(new URL("../native-acceptance.mjs", import.meta.url)),
      "utf8",
    );
    expect(source).toContain("ensureNotCanceled(this.opts.isCanceled, \"launch\")");
    expect(source).toContain("ensureNotCanceled(this.opts.isCanceled, \"stop\")");
  });
});

describe("超时后的有界收尾（计划 §9.1 第 4、6 条）", () => {
  it("attach 模式不认领、不终止外部实例", async () => {
    let terminateCalls = 0;
    const result = await finalizeScenarioTimeout({
      attached: true,
      record: { hostPid: 4242, descendantPids: [4243] },
      terminateRecordedPids: async () => {
        terminateCalls++;
        return { reason: "x", pids: [] };
      },
      closeBrowser: async () => ({ closed: true, reason: null }),
    });
    expect(terminateCalls).toBe(0);
    expect(result.terminate).toBe("skipped-attach");
    expect(result.errors).toEqual([]);
  });

  it("非 attach 终止本次已记录 PID；收尾各步异常都记录后继续，不外抛", async () => {
    const calls = [];
    const result = await finalizeScenarioTimeout({
      attached: false,
      record: { hostPid: 100, descendantPids: [200] },
      terminateRecordedPids: async (record, reason) => {
        calls.push({ record, reason });
        return { reason, pids: [100, 200] };
      },
      closeBrowser: () => closeBrowserBounded({ close: () => new Promise(() => {}) }, { timeoutMs: 40 }),
      copyAppLogs: () => {
        throw new Error("日志目录不可读");
      },
    });
    expect(calls).toEqual([{ record: { hostPid: 100, descendantPids: [200] }, reason: "scenario-timeout-cleanup" }]);
    expect(result.terminate).toEqual({ reason: "scenario-timeout-cleanup", pids: [100, 200] });
    expect(result.browser_close).toEqual({ closed: false, reason: "close timeout" });
    expect(result.app_logs_copied).toBe(false);
    expect(result.errors.join(" ")).toContain("日志目录不可读");
  });

  it("没有已记录宿主时不动任何进程", async () => {
    let terminateCalls = 0;
    const result = await finalizeScenarioTimeout({
      attached: false,
      record: null,
      terminateRecordedPids: async () => {
        terminateCalls++;
      },
    });
    expect(terminateCalls).toBe(0);
    expect(result.terminate).toBe("no-recorded-host");
  });
});

describe("共用收尾入口的子进程端到端（计划 §9.1 第 7 条）", () => {
  it("父进程设上限：子进程到点写最小证据并以非零退出，不悬挂", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "native-scenario-test-"));
    const evidencePath = path.join(dir, "evidence.json");
    const scriptPath = path.join(dir, "run-scenario.mjs");
    const libPath = fileURLToPath(new URL("./native-scenario.mjs", import.meta.url));
    fs.writeFileSync(
      scriptPath,
      [
        `import fs from "node:fs";`,
        `import { runScenarioBounded } from ${JSON.stringify(pathToFileURL(libPath).href)};`,
        `const evidencePath = process.argv[2];`,
        `const outcome = await runScenarioBounded({`,
        `  scenarioFn: () => new Promise(() => {}),`,
        `  timeoutMs: 200,`,
        `  cancelFlag: { requested: false },`,
        `  onTimeout: () => {`,
        `    fs.writeFileSync(evidencePath, JSON.stringify({ scenario_timeout: true, minimal: true }), "utf8");`,
        `  },`,
        `  hb: () => {},`,
        `});`,
        `if (outcome.kind === "timeout") process.exit(7);`,
        `process.exit(0);`,
        "",
      ].join("\n"),
      "utf8",
    );
    const startedAt = Date.now();
    // 父测试上限 10 秒：子进程看门狗 200 ms，超时即 kill（不等它悬挂）
    const child = spawnSync(process.execPath, [scriptPath, evidencePath], {
      encoding: "utf8",
      timeout: 10000,
      killSignal: "SIGKILL",
    });
    expect(child.error ?? null).toBeNull();
    expect(child.status).toBe(7);
    expect(Date.now() - startedAt).toBeLessThan(9000);
    const evidence = JSON.parse(fs.readFileSync(evidencePath, "utf8"));
    expect(evidence).toEqual({ scenario_timeout: true, minimal: true });
  });
});

describe("waitStableQuery（PR3 补修复核：未稳定不得放行）", () => {
  // 按读数序列回放；超出序列后保持最后一个读数。sleep 不真等。
  const replay = (reads) => {
    let n = 0;
    return {
      read: async () => reads[Math.min(n++, reads.length - 1)],
      sleep: async () => {},
    };
  };
  const q = (summary, events = summary) => ({ summary, events });

  it("迟到批次后保持稳定：返回最终身份", async () => {
    const id = await waitStableQuery(replay([q("q1"), q("q2"), q("q2")]));
    expect(id).toBe("q2");
  });

  it("身份持续变化：到期抛错并带变化序列", async () => {
    const reads = Array.from({ length: 40 }, (_, i) => q(`q${i}`));
    const error = await waitStableQuery(replay(reads)).catch((e) => e);
    expect(error).toBeInstanceOf(Error);
    expect(error.kind).toBe("query-unstable");
    expect(error.observed.length).toBeGreaterThan(5);
    expect(error.message).toContain("q0");
  });

  it("身份始终为空：不当作稳定基线", async () => {
    const error = await waitStableQuery(replay([q(null)])).catch((e) => e);
    expect(error.kind).toBe("query-unstable");
    expect(error.observed).toEqual([{ at_ms: 0, summary: null, events: null }]);
  });

  it("汇总与明细不一致：不当作稳定基线", async () => {
    const error = await waitStableQuery(replay([q("q2", "q1")])).catch((e) => e);
    expect(error.kind).toBe("query-unstable");
  });
});
