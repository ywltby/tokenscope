// @vitest-environment node
// UP09 复核修复（计划 §4.8 第 4 条、§6.2 行 3/4/5）：**真实异步组合**的契约测试。
//
// 与 native-instance.test.mjs 的分工：那边验证决策函数（给定时钟与状态应得什么结论），
// 这里验证"等待真的被剩余预算约束、宿主退出真的能中止等待、被取消的连接真的不悬空"——
// 用真实计时器、真实 Promise 与真实子进程，而不是只喂状态给纯函数。
// 本文件跑在 node 环境（vitest.config.ts 的 include 覆盖 scripts/lib/**/*.test.mjs）。
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { afterAll, describe, expect, it } from "vitest";
import {
  BudgetExhaustedError,
  HOST_EXIT_POLL_MS,
  HostExitError,
  PROBE_TIMEOUT_MS,
  awaitWithHostExit,
  buildLaunchesEvidence,
  clampTimeoutToBudget,
  connectWithBudget,
  createHostExitSignal,
  createLaunchRecord,
  createProbeFn,
  hostExitDetail,
  recordConnectAttempt,
  remainingBudgetMs,
} from "./native-instance.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** 真实子进程一律登记，测试结束确保收掉（不依赖测试自身是否通过）。 */
const children = [];
afterAll(() => {
  for (const child of children) {
    try {
      child.kill();
    } catch {}
  }
});

/** 与 native-acceptance.mjs 同源的存活查询（`kill(pid, 0)`，EPERM 也算活着）。 */
function isProcessAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e?.code === "EPERM";
  }
}

/** 起一个真实子进程：`delayMs` 后以 `code` 自行退出，并装上与驱动侧同构的退出监听。 */
function spawnExitingChild(code, delayMs) {
  const child = spawn(process.execPath, ["-e", `setTimeout(() => process.exit(${code}), ${delayMs})`], {
    stdio: "ignore",
  });
  children.push(child);
  const watcher = createHostExitSignal({
    isAlive: () => isProcessAlive(child.pid),
    readExitCode: () => child.exitCode,
    intervalMs: HOST_EXIT_POLL_MS,
  });
  child.on("exit", (c) => watcher.notify(c));
  watcher.start();
  return { child, watcher };
}

/** 探测用的假子进程：只提供驱动侧真正用到的接口，不产生真实进程。 */
function fakeProbeChild() {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.killed = 0;
  child.kill = () => {
    child.killed += 1;
  };
  return child;
}

describe("宿主退出信号（§4.8 第 4 条 / §6.2 行 4）", () => {
  it("真实子进程退出：exit 事件与 250 ms 轮询取先到者，数百毫秒内给出退出码", async () => {
    const startedAt = Date.now();
    const { watcher } = spawnExitingChild(3, 150);
    const info = await watcher.signal;
    const elapsed = Date.now() - startedAt;
    expect(["exit-event", "poll"]).toContain(info.source);
    if (info.source === "exit-event") {
      expect(info.code).toBe(3);
    } else {
      // 轮询先到时，Windows 内核层可能已判死而父进程还没回收子进程，
      // child.exitCode 仍为 null（产品按“退出码=unknown”处理）；已回收则必须是 3。
      expect([3, null]).toContain(info.code);
    }
    expect(elapsed).toBeLessThan(1500);
    expect(elapsed).toBeGreaterThanOrEqual(100);
  });

  it("没有 exit 事件时靠存活轮询发现（两种来源缺一不可）", async () => {
    let alive = true;
    setTimeout(() => {
      alive = false;
    }, 120);
    const watcher = createHostExitSignal({
      isAlive: () => alive,
      readExitCode: () => 0,
      intervalMs: HOST_EXIT_POLL_MS,
    });
    watcher.start();
    const info = await watcher.signal;
    expect(info).toMatchObject({ code: 0, source: "poll" });
    expect(watcher.polls).toBeGreaterThanOrEqual(1);
    expect(watcher.isSettled()).toBe(true);
  });

  it("退出信号一旦成立就不再轮询（stop 后不残留计时器）", async () => {
    const watcher = createHostExitSignal({ isAlive: () => true, intervalMs: HOST_EXIT_POLL_MS });
    watcher.start();
    await sleep(30);
    expect(watcher.isSettled()).toBe(false);
    watcher.stop();
    await sleep(40);
    expect(watcher.polls).toBeGreaterThanOrEqual(1);
    expect(watcher.isSettled()).toBe(false);
  });
});

describe("等待被宿主退出中止（§6.2 行 4：退出码 0 与非 0 都立即失败）", () => {
  for (const code of [0, 3]) {
    it(`等待期间收到退出码 ${code}：立即失败、错误里带退出码、不再等满超时`, async () => {
      const watcher = createHostExitSignal({ isAlive: () => true, intervalMs: HOST_EXIT_POLL_MS });
      watcher.start();
      setTimeout(() => watcher.notify(code), 80);
      const startedAt = Date.now();
      const error = await awaitWithHostExit({
        task: () => new Promise(() => {}), // 模拟挂住的 WebSocket 建立
        hostExit: watcher,
        timeoutMs: 45000,
        timeoutDetail: "connectOverCDP 超过剩余预算 45000 ms（WebSocket 未建立）",
      }).then(
        () => null,
        (e) => e,
      );
      const elapsed = Date.now() - startedAt;
      expect(error).toBeInstanceOf(HostExitError);
      expect(error.kind).toBe("host-exited");
      expect(error.hostExitCode).toBe(code);
      expect(error.message).toContain(`退出码=${code}`);
      expect(error.message).toContain("立即失败且不再重试");
      expect(elapsed).toBeLessThan(1000); // 而不是 45 秒上限
      watcher.stop();
    });
  }

  it("真实子进程退出时，挂住的等待被中止（端到端组合）", async () => {
    const { watcher } = spawnExitingChild(1, 150);
    const startedAt = Date.now();
    const error = await awaitWithHostExit({
      task: () => sleep(30000),
      hostExit: watcher,
      timeoutMs: 45000,
    }).then(
      () => null,
      (e) => e,
    );
    expect(error).toBeInstanceOf(HostExitError);
    expect(error.message).toMatch(/退出码=(1|unknown)/);
    expect(Date.now() - startedAt).toBeLessThan(1500);
    watcher.stop();
  });

  it("错误文案统一由 hostExitDetail 生成，0 与非 0 都不省略退出码", () => {
    expect(hostExitDetail({ code: 0 })).toContain("退出码=0");
    expect(hostExitDetail({ code: 3 })).toContain("退出码=3");
    expect(hostExitDetail({ code: null })).toContain("退出码=unknown");
    expect(new HostExitError({ code: 0 }).message).toBe(hostExitDetail({ code: 0 }));
  });
});

describe("剩余预算约束等待（§6.2 行 5：总时限不延长）", () => {
  it("等待超时取 min(自身超时, 剩余预算)；下限不突破预算；耗尽返回 0", () => {
    const now = 1_000_000;
    expect(clampTimeoutToBudget({ timeoutMs: 30000, deadlineMs: now + 65000, nowMs: now })).toBe(30000);
    expect(clampTimeoutToBudget({ timeoutMs: 30000, deadlineMs: now + 5000, nowMs: now })).toBe(5000);
    // 300 ms 下限不得把等待抬到剩余预算之外（旧实现 Math.max(300, remaining) 的缺陷）
    expect(clampTimeoutToBudget({ timeoutMs: 3000, deadlineMs: now + 100, nowMs: now, minMs: 300 })).toBe(100);
    expect(clampTimeoutToBudget({ timeoutMs: 3000, deadlineMs: now - 1, nowMs: now })).toBe(0);
    expect(clampTimeoutToBudget({ timeoutMs: 3000, deadlineMs: now, nowMs: now })).toBe(0);
    expect(remainingBudgetMs(now + 2500, now)).toBe(2500);
    expect(remainingBudgetMs(now - 2500, now)).toBe(0);
    expect(remainingBudgetMs(null, now)).toBe(Number.POSITIVE_INFINITY);
  });

  it("预算耗尽：不再开启新的连接尝试，第二次 connect 根本不发起", async () => {
    const deadlineMs = Date.now() + 300;
    let connectCalls = 0;
    const connectOnce = () =>
      awaitWithHostExit({
        task: async () => {
          connectCalls++;
          throw new Error("browserType.connectOverCDP: socket hang up");
        },
        timeoutMs: clampTimeoutToBudget({ timeoutMs: remainingBudgetMs(deadlineMs), deadlineMs }),
      }).then(
        () => null,
        (e) => e,
      );

    const first = await connectOnce();
    expect(first.message).toContain("socket hang up");
    expect(first).not.toBeInstanceOf(BudgetExhaustedError);
    expect(connectCalls).toBe(1);

    await sleep(400); // 预算用尽
    expect(remainingBudgetMs(deadlineMs)).toBe(0);
    expect(clampTimeoutToBudget({ timeoutMs: 45000, deadlineMs })).toBe(0);

    const second = await connectOnce();
    expect(second).toBeInstanceOf(BudgetExhaustedError);
    expect(connectCalls).toBe(1); // 不再发起新的尝试
  });

  it("等待超过剩余预算：被计时的竞速中止，耗时不超过预算", async () => {
    const startedAt = Date.now();
    const error = await awaitWithHostExit({
      task: () => sleep(5000),
      timeoutMs: 200,
      timeoutDetail: "connectOverCDP 超过剩余预算 200 ms（WebSocket 未建立）",
    }).then(
      () => null,
      (e) => e,
    );
    const elapsed = Date.now() - startedAt;
    expect(error).toBeInstanceOf(BudgetExhaustedError);
    expect(error.message).toContain("超过剩余预算 200 ms");
    expect(elapsed).toBeLessThan(600);
    expect(elapsed).toBeGreaterThanOrEqual(150);
  });

  it("取消后迟到的连接被关闭，且不作为新的尝试结果返回（要求 3）", async () => {
    let closed = 0;
    let cancelled = 0;
    const lateBrowser = {
      close: async () => {
        closed++;
      },
    };
    let connectCalls = 0;
    const error = await awaitWithHostExit({
      task: () => {
        connectCalls++;
        return sleep(400).then(() => lateBrowser);
      },
      timeoutMs: 120,
      timeoutDetail: "connectOverCDP 超过剩余预算 120 ms（WebSocket 未建立）",
      onCancel: () => {
        cancelled++;
      },
      onLateResult: (browser) => browser.close(),
    }).then(
      () => null,
      (e) => e,
    );
    expect(error).toBeInstanceOf(BudgetExhaustedError);
    expect(connectCalls).toBe(1);
    expect(cancelled).toBe(1); // 取消当下就执行，而不是等迟到结果
    expect(closed).toBe(0); // 尚未迟到
    await sleep(500);
    expect(closed).toBe(1); // 迟到的连接已关闭，没有悬空
  });

  it("socket hang up 后第二次成功：期限内重试成功且两次尝试都进 launches[]（§6.2 行 3）", async () => {
    const deadlineMs = Date.now() + 5000;
    const record = createLaunchRecord({ index: 2, port: 9462, startedAtMs: Date.now() });
    let calls = 0;
    const connectOnce = () =>
      awaitWithHostExit({
        task: async () => {
          calls++;
          if (calls === 1) throw new Error("browserType.connectOverCDP: socket hang up");
          return { contexts: () => [{ pages: () => [] }] };
        },
        timeoutMs: clampTimeoutToBudget({ timeoutMs: remainingBudgetMs(deadlineMs), deadlineMs }),
      });

    let browser = null;
    const failures = [];
    while (!browser && calls < 5) {
      const attemptStartedAt = Date.now();
      try {
        browser = await connectOnce();
        recordConnectAttempt(record, {
          atMs: Date.now() - record.startedAtMs,
          phase: "cdp",
          ok: true,
          ms: Date.now() - attemptStartedAt,
        });
      } catch (error) {
        failures.push(error.message);
        recordConnectAttempt(record, {
          atMs: Date.now() - record.startedAtMs,
          phase: "cdp",
          ok: false,
          error: error.message,
          ms: Date.now() - attemptStartedAt,
        });
        await sleep(50);
      }
    }
    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("socket hang up");
    expect(browser).not.toBeNull();
    expect(calls).toBe(2);
    const [evidence] = buildLaunchesEvidence([record]);
    expect(evidence.connect_attempts).toHaveLength(2);
    expect(evidence.connect_attempts[0]).toMatchObject({ ok: false, aborted: null });
    expect(evidence.connect_attempts[0].error).toContain("socket hang up");
    expect(evidence.connect_attempts[1]).toMatchObject({ ok: true, aborted: null });
  });
});

describe("探测受剩余预算与宿主退出约束（§4.8 第 4 条题目：探测不得等满自身 30 秒）", () => {
  it("探测自身超时被剩余预算截断，而不是等满 30000 ms", async () => {
    let spawned = 0;
    const probe = createProbeFn({
      spawnFn: () => {
        spawned++;
        return fakeProbeChild();
      },
    });
    const startedAt = Date.now();
    const result = await probe({ deadlineMs: startedAt + 400 });
    const elapsed = Date.now() - startedAt;
    expect(result.ok).toBe(false);
    // 截断后的超时贴着剩余预算：不写死 400，避免真实时钟在两次调用之间推进 1 ms 就误报
    expect(result.timeout_ms).toBeGreaterThan(350);
    expect(result.timeout_ms).toBeLessThanOrEqual(400); // min(30000, 400)
    expect(elapsed).toBeLessThan(1200);
    expect(elapsed).toBeGreaterThanOrEqual(350);
    expect(result.error).toContain("已按本次启动剩余预算截断");
    expect(spawned).toBe(1);
    expect(PROBE_TIMEOUT_MS).toBe(30000);
  });

  it("预算已耗尽：探测根本不 spawn", async () => {
    let spawned = 0;
    const probe = createProbeFn({
      spawnFn: () => {
        spawned++;
        return fakeProbeChild();
      },
    });
    const result = await probe({ deadlineMs: Date.now() - 1 });
    expect(result.ok).toBe(false);
    expect(result.timeout_ms).toBe(0);
    expect(spawned).toBe(0);
    expect(result.error).toContain("剩余预算已耗尽");
  });

  it("宿主退出：探测立刻中止并终止 PowerShell，不等自身超时", async () => {
    const childrenOfProbe = [];
    let firstProbeTime = null;
    const probe = createProbeFn({
      // 预算起点只取一次，避免传参到内部首次取时推进 1ms 后正确截断。
      // 后续计时、真实子进程及真实 timer 均保留，不改变任何观察窗口。
      now: () => {
        const current = firstProbeTime ?? Date.now();
        firstProbeTime = null;
        return current;
      },
      spawnFn: () => {
        const child = fakeProbeChild();
        childrenOfProbe.push(child);
        return child;
      },
    });
    const { watcher } = spawnExitingChild(0, 150);
    const startedAt = Date.now();
    firstProbeTime = startedAt;
    const result = await probe({ deadlineMs: startedAt + PROBE_TIMEOUT_MS, hostExit: watcher });
    expect(result.ok).toBe(false);
    expect(result.hostExited).toBe(true);
    expect(result.timeout_ms).toBe(PROBE_TIMEOUT_MS); // 预算充足，自身超时未被截断
    expect(result.error).toMatch(/宿主已退出（退出码=(0|unknown)），探测中止/);
    expect(Date.now() - startedAt).toBeLessThan(1500);
    expect(childrenOfProbe[0].killed).toBe(1);
    watcher.stop();
  });

  it("探测正常返回时解析进程表与监听表（组合仍走一次 PowerShell 调用）", async () => {
    const probe = createProbeFn({
      spawnFn: () => {
        const child = fakeProbeChild();
        setTimeout(() => {
          child.stdout.emit(
            "data",
            JSON.stringify({
              processes: [{ ProcessId: 10, ParentProcessId: 4, Name: "tokenscope.exe" }],
              listeners: [{ LocalPort: 9452, OwningProcess: 10, State: 2 }],
            }),
          );
          child.emit("close");
        }, 20);
        return child;
      },
    });
    const result = await probe({ deadlineMs: Date.now() + 5000 });
    expect(result.ok).toBe(true);
    expect(result.processes).toEqual([{ pid: 10, ppid: 4, name: "tokenscope.exe" }]);
    expect(result.listeners).toEqual([{ port: 9452, pid: 10, state: "2" }]);
    // 自身 30 秒被剩余预算截断（同样不写死毫秒数，真实时钟会推进）
    expect(result.timeout_ms).toBeGreaterThan(4900);
    expect(result.timeout_ms).toBeLessThanOrEqual(5000);
  });
});

describe("取消动作在取消当下执行（复核 P2：onCancel 与 onLateResult 分离）", () => {
  it("超时分支：底层任务还没结束，取消动作已经发生", async () => {
    let cancelled = 0;
    let late = 0;
    let taskFinished = false;
    const startedAt = Date.now();
    const error = await awaitWithHostExit({
      task: () =>
        new Promise(() => {}).then(() => {
          taskFinished = true;
        }),
      timeoutMs: 40,
      timeoutDetail: "超过剩余预算 40 ms",
      onCancel: () => {
        cancelled++;
      },
      onLateResult: () => {
        late++;
      },
    }).then(
      () => null,
      (e) => e,
    );
    expect(error).toBeInstanceOf(BudgetExhaustedError);
    expect(cancelled).toBe(1);
    expect(taskFinished).toBe(false);
    expect(late).toBe(0);
    expect(Date.now() - startedAt).toBeLessThan(500);
  });

  it("宿主退出分支：底层任务还没结束，取消动作已经发生", async () => {
    let cancelled = 0;
    let fireExit = null;
    const hostExit = {
      signal: new Promise((resolve) => {
        fireExit = resolve;
      }),
    };
    const pending = awaitWithHostExit({
      task: () => new Promise(() => {}),
      hostExit,
      timeoutMs: 45000,
      onCancel: () => {
        cancelled++;
      },
    }).then(
      () => null,
      (e) => e,
    );
    await sleep(30);
    expect(cancelled).toBe(0); // 还没取消
    fireExit({ code: 0 });
    const error = await pending;
    expect(error).toBeInstanceOf(HostExitError);
    expect(cancelled).toBe(1); // 取消当下就执行，不等底层自己结束
  });

  it("取消后迟到成功：只交给收尾回调一次，且不再重复取消", async () => {
    let cancelled = 0;
    const closed = [];
    const lateBrowser = {
      close: async () => {
        closed.push("closed");
      },
    };
    const error = await awaitWithHostExit({
      task: () => sleep(120).then(() => lateBrowser),
      timeoutMs: 40,
      timeoutDetail: "超过剩余预算 40 ms",
      onCancel: () => {
        cancelled++;
      },
      onLateResult: (browser) => browser.close(),
    }).then(
      () => null,
      (e) => e,
    );
    expect(error).toBeInstanceOf(BudgetExhaustedError);
    expect(cancelled).toBe(1);
    expect(closed).toHaveLength(0);
    await sleep(250);
    expect(cancelled).toBe(1); // 迟到结果不会再次触发取消
    expect(closed).toEqual(["closed"]);
  });

  it("预算耗尽：底层任务根本不启动，取消动作仍然执行", async () => {
    let cancelled = 0;
    let started = 0;
    const error = await awaitWithHostExit({
      task: () => {
        started++;
      },
      timeoutMs: 0,
      onCancel: () => {
        cancelled++;
      },
    }).then(
      () => null,
      (e) => e,
    );
    expect(error).toBeInstanceOf(BudgetExhaustedError);
    expect(started).toBe(0);
    expect(cancelled).toBe(1);
  });

  it("宿主退出后真实 fetch 立刻被 abort（请求不再挂着）", async () => {
    const { createServer } = await import("node:http");
    let observed = 0;
    const server = createServer((req, res) => {
      req.on("aborted", () => {
        observed++;
      });
      res.on("close", () => {
        if (!res.writableEnded) observed++;
      });
      // 故意不响应：请求会一直挂着，除非被取消
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const port = server.address().port;
    const controller = new AbortController();
    let fireExit = null;
    const hostExit = {
      signal: new Promise((resolve) => {
        fireExit = resolve;
      }),
    };
    const pending = awaitWithHostExit({
      task: () => fetch(`http://127.0.0.1:${port}/json/version`, { signal: controller.signal }),
      hostExit,
      timeoutMs: 45000,
      onCancel: () => controller.abort(),
    }).then(
      () => null,
      (e) => e,
    );
    await sleep(150); // 让请求真正到达服务端
    fireExit({ code: 0 });
    const error = await pending;
    expect(error).toBeInstanceOf(HostExitError);
    await sleep(250);
    expect(observed).toBeGreaterThan(0); // 服务端看到连接被关闭，而不是一直挂着
    await new Promise((resolve) => server.close(resolve));
  });

  it("connectWithBudget：底层拿到同一份超时，取消后迟到连接被关闭", async () => {
    const seen = [];
    let closed = 0;
    const error = await connectWithBudget({
      connect: (url, options) => {
        seen.push({ url, timeout: options?.timeout });
        return sleep(150).then(() => ({
          close: async () => {
            closed++;
          },
        }));
      },
      url: "http://127.0.0.1:9601",
      timeoutMs: 50,
      timeoutDetail: "connectOverCDP 超过剩余预算 50 ms（WebSocket 未建立）",
    }).then(
      () => null,
      (e) => e,
    );
    expect(error).toBeInstanceOf(BudgetExhaustedError);
    expect(seen).toEqual([{ url: "http://127.0.0.1:9601", timeout: 50 }]);
    await sleep(250);
    expect(closed).toBe(1);
  });
});
