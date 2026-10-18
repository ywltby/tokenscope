// @vitest-environment node
// UP09（计划 §4.8、§6.2）反例单测：端口计算、端点归属、重试决策、宿主退出即失败、
// 65 秒预算不延长、退出确认与 launches[] 证据。
// 本文件必须跑在 node 环境（无 happy-dom），见 vitest.config.ts 的 include 与第一条用例。
import fs from "node:fs";
import { describe, expect, it } from "vitest";
import {
  CDP_READY_BUDGET_MS,
  CONNECT_RETRY_INTERVAL_MS,
  LAUNCH_BUDGET_MS,
  PAGE_TARGET_BUDGET_MS,
  PROBE_SHELL,
  PROCESS_PROBE_COMMAND,
  STOP_TIMEOUT_MS,
  buildLaunchesEvidence,
  classifyPortOwner,
  collectDescendants,
  connectDecision,
  createLaunchRecord,
  describeTargets,
  descendantsAliveByName,
  hostLogTail,
  isRetryableConnectError,
  launchPort,
  listenersOnPort,
  mergePids,
  normalizeListeners,
  normalizeProcessTable,
  pageTargetDecision,
  parseProbeOutput,
  pickTauriTarget,
  probeSpawnArgs,
  recordConnectAttempt,
  recordOwnerCheck,
  recordProbe,
  stopDecision,
} from "./native-instance.mjs";

const UI_E2E = new URL("../../../.github/workflows/ui-e2e.yml", import.meta.url);
const workflowText = () => fs.readFileSync(UI_E2E, "utf8");

describe("运行环境", () => {
  it("用 node 环境运行，不受 happy-dom 影响", () => {
    expect(typeof window).toBe("undefined");
    expect(typeof process.versions.node).toBe("string");
  });
});

describe("端口计算（§4.8 第 1 条）", () => {
  it("第 n 次启动用 --port + n，端口不复用", () => {
    expect(launchPort(9451, 1)).toBe(9452);
    expect(launchPort(9451, 2)).toBe(9453);
    expect(launchPort(9460, 1)).toBe(9461);
    expect(launchPort(9490, 2)).toBe(9492);
    const ports = Array.from({ length: 21 }, (_, i) => launchPort(9600, i + 1));
    expect(new Set(ports).size).toBe(21);
  });

  it("旧宿主退出后仍被 WebView2 子进程监听的端口不会被新实例复用（§6.2 行 1）", () => {
    const oldPort = launchPort(9460, 1);
    const childStillListening = listenersOnPort(
      normalizeListeners([
        { LocalPort: oldPort, OwningProcess: 4242, State: "Listen" },
        { LocalPort: 9460, OwningProcess: 99, State: "Listen" },
      ]),
      oldPort,
    );
    expect(childStillListening).toEqual([4242]);
    const newPort = launchPort(9460, 2);
    expect(newPort).not.toBe(oldPort);
    expect(childStillListening).not.toContain(newPort);
  });

  it("非法基准端口与序号直接报错，不做静默回绕", () => {
    expect(() => launchPort(9451, 0)).toThrow();
    expect(() => launchPort(9451, -1)).toThrow();
    expect(() => launchPort("not-a-port", 1)).toThrow();
    expect(() => launchPort(65535, 1)).toThrow();
  });

  it("工作流各步起始端口互不重叠（§4.8 第 1 条）", () => {
    const bases = [...workflowText().matchAll(/\b9\d{3}\b/g)].map((m) => Number(m[0]));
    const unique = [...new Set(bases)].sort((a, b) => a - b);
    // 9470 复活为 ui-collection-status 的起始端口（旧用途是强制缩放 1 轮，
    // 已因证据重复删除）；该场景 3 次启动用 9471–9473。
    // PR3：ui-source-paths 起始 9455（2 次启动用 9455–9456）。
    expect(unique).toEqual([9451, 9455, 9460, 9470, 9480, 9490]);
    // 单步最多 3 次启动：ui-refresh 首次 + 2 次重启，ui-system 首次 + 1 次重启，
    // ui-collection-status 首次 + 2 次重启。
    const stepSize = 3;
    const segments = unique.map((base) => [base + 1, base + stepSize]);
    for (const [from, to] of segments) {
      for (const [otherFrom, otherTo] of segments) {
        if (from === otherFrom) continue;
        expect(from > otherTo || to < otherFrom).toBe(true);
      }
    }
  });
});

describe("一次 PowerShell 探测（§4.8 第 6 条）", () => {
  it("单次调用同时取回进程表与监听表", () => {
    const args = probeSpawnArgs();
    expect(args).toHaveLength(4);
    expect(args.slice(0, 3)).toEqual(["-NoProfile", "-NonInteractive", "-Command"]);
    expect(PROBE_SHELL).toBe("powershell.exe");
    expect(PROCESS_PROBE_COMMAND).toContain("Get-CimInstance Win32_Process");
    expect(PROCESS_PROBE_COMMAND).toContain("Get-NetTCPConnection");
    expect(PROCESS_PROBE_COMMAND.match(/Get-CimInstance/g)).toHaveLength(1);
    expect(PROCESS_PROBE_COMMAND.match(/Get-NetTCPConnection/g)).toHaveLength(1);
  });

  it("解析单元素与数组两种 JSON 形态，字段名归一", () => {
    const single = parseProbeOutput(
      JSON.stringify({
        processes: { ProcessId: 10, ParentProcessId: 4, Name: "tokenscope.exe" },
        listeners: { LocalPort: 9452, OwningProcess: 10, State: "Listen" },
      }),
    );
    expect(single.ok).toBe(true);
    expect(single.processes).toEqual([{ pid: 10, ppid: 4, name: "tokenscope.exe" }]);
    expect(single.listeners).toEqual([{ port: 9452, pid: 10, state: "Listen" }]);
  });

  it("告警混进 stdout 时取最后一行 JSON；完全不是 JSON 时判探测失败", () => {
    const mixed = parseProbeOutput(
      `WARNING: 需要管理员权限\n${JSON.stringify({ processes: [], listeners: [] })}`,
    );
    expect(mixed.ok).toBe(true);
    expect(parseProbeOutput("WARNING: 需要管理员权限").ok).toBe(false);
    expect(parseProbeOutput("").ok).toBe(false);
    expect(normalizeProcessTable([{ pid: 0 }])).toEqual([]);
  });
});

describe("后代进程递归记录（§4.8 第 2 条）", () => {
  const table = normalizeProcessTable([
    { ProcessId: 100, ParentProcessId: 1, Name: "tokenscope.exe" },
    { ProcessId: 200, ParentProcessId: 100, Name: "msedgewebview2.exe" },
    { ProcessId: 300, ParentProcessId: 200, Name: "msedgewebview2.exe" },
    { ProcessId: 400, ParentProcessId: 300, Name: "msedgewebview2.exe" },
    { ProcessId: 500, ParentProcessId: 1, Name: "unrelated.exe" },
    { ProcessId: 600, ParentProcessId: 600, Name: "self-parent.exe" },
  ]);

  it("按父 PID 递归到孙进程，且不含宿主自身", () => {
    expect(collectDescendants(table, 100)).toEqual([200, 300, 400]);
    expect(collectDescendants(table, 999)).toEqual([]);
  });

  it("合并多次快照不会丢已记录的后代", () => {
    expect(mergePids([200], [200, 300])).toEqual([200, 300]);
    expect(mergePids(null, [])).toEqual([]);
  });

  // PR #14 CI 实测：宿主干净退出后 PID 被系统进程复用，从复用者出发的后代
  // 遍历把整棵系统树记成"后代"，退出确认 20 秒超时。存活判定必须带进程名
  // 身份校验——复用 PID 的新名字不属于本应用家族，不得算残留或进强杀清单。
  it("descendantsAliveByName 只认本应用家族的名字，未知 PID 保守保留", () => {
    const rows = normalizeProcessTable([
      { ProcessId: 200, ParentProcessId: 752, Name: "svchost.exe" },
      { ProcessId: 300, ParentProcessId: 752, Name: "msedgewebview2.exe" },
      { ProcessId: 400, ParentProcessId: 1, Name: "tokenscope.exe" },
    ]);
    expect(descendantsAliveByName([200, 300, 400, 999], rows, () => true)).toEqual([300, 400, 999]);
    expect(descendantsAliveByName([300], rows, () => false)).toEqual([]);
    expect(descendantsAliveByName([300], null, () => true)).toEqual([300]);
    expect(descendantsAliveByName(null, rows, () => true)).toEqual([]);
  });
});

describe("端点归属判定（§4.8 第 3 条）", () => {
  it("无监听者继续等", () => {
    const owner = classifyPortOwner({ listenerPids: [], hostPid: 100, descendantPids: [200] });
    expect(owner.kind).toBe("none");
  });

  it("宿主与已记录后代算自己人", () => {
    expect(
      classifyPortOwner({ listenerPids: [100], hostPid: 100, descendantPids: [200] }).kind,
    ).toBe("host");
    expect(
      classifyPortOwner({ listenerPids: [200], hostPid: 100, descendantPids: [200] }).kind,
    ).toBe("descendant");
  });

  it("外来监听者不被认领：不连接、不结束它，并记录归属不符（§6.2 行 2）", () => {
    const owner = classifyPortOwner({ listenerPids: [777], hostPid: 100, descendantPids: [200] });
    expect(owner.kind).toBe("foreign");
    expect(owner.foreignPids).toEqual([777]);
    expect(owner.ownedPids).toEqual([]);
    expect(owner.detail).toContain("777");
    // 混合情况（自己人 + 外来进程）同样按外来处理，外来 PID 必须留在证据里
    const mixed = classifyPortOwner({
      listenerPids: [200, 777],
      hostPid: 100,
      descendantPids: [200],
    });
    expect(mixed.kind).toBe("foreign");
    expect(mixed.foreignPids).toEqual([777]);
    expect(mixed.ownedPids).toEqual([200]);
  });

  it("监听表只取 Listen 状态，且记录端口所属 PID", () => {
    const listeners = normalizeListeners([
      { LocalPort: 9452, OwningProcess: 10, State: "Listen" },
      { LocalPort: 9452, OwningProcess: 11, State: "Established" },
      { LocalPort: 9453, OwningProcess: 12, State: "Listen" },
    ]);
    expect(listenersOnPort(listeners, 9452)).toEqual([10]);
    expect(listenersOnPort(listeners, "9453")).toEqual([12]);
    expect(listenersOnPort(listeners, 9999)).toEqual([]);
  });

  it("PowerShell 的 State 是数字枚举（Listen=2）时同样算监听者", () => {
    // 实测形态：Get-NetTCPConnection | ConvertTo-Json 给出 {"State":2}
    const listeners = normalizeListeners([
      { LocalPort: 9611, OwningProcess: 10, State: 2 },
      { LocalPort: 9611, OwningProcess: 11, State: 5 },
    ]);
    expect(listenersOnPort(listeners, 9611)).toEqual([10]);
    expect(
      classifyPortOwner({
        listenerPids: listenersOnPort(listeners, 9611),
        hostPid: 10,
        descendantPids: [],
      }).kind,
    ).toBe("host");
  });
});

describe("启动预算与重试（§4.8 第 4 条）", () => {
  it("总预算 65 秒 = CDP 就绪 45 秒 + 页面 target 20 秒，不收紧也不放宽", () => {
    expect(CDP_READY_BUDGET_MS).toBe(45000);
    expect(PAGE_TARGET_BUDGET_MS).toBe(20000);
    expect(LAUNCH_BUDGET_MS).toBe(65000);
    expect(CONNECT_RETRY_INTERVAL_MS).toBe(300);
    expect(STOP_TIMEOUT_MS).toBe(20000);
  });

  it("socket hang up 属可重试瞬断，第二次成功（§6.2 行 3）", () => {
    expect(isRetryableConnectError("browserType.connectOverCDP: socket hang up")).toBe(true);
    const first = connectDecision({
      error: "browserType.connectOverCDP: socket hang up",
      hostExitCode: null,
      ownerKind: "host",
      elapsedMs: 5000,
      budgetMs: LAUNCH_BUDGET_MS,
    });
    expect(first.action).toBe("retry");
    expect(first.retryAfterMs).toBe(300);

    const record = createLaunchRecord({ index: 2, port: 9462, startedAtMs: 0 });
    recordConnectAttempt(record, { atMs: 5000, ok: false, error: "socket hang up" });
    recordConnectAttempt(record, { atMs: 5300, ok: true });
    const [evidence] = buildLaunchesEvidence([record]);
    expect(evidence.connect_attempts).toHaveLength(2);
    expect(evidence.connect_attempts[0]).toMatchObject({
      at_ms: 5000,
      ok: false,
      error: "socket hang up",
    });
    expect(evidence.connect_attempts[1]).toMatchObject({ at_ms: 5300, ok: true });
  });

  it("ECONNRESET / ECONNREFUSED / target 被销毁都算瞬断", () => {
    expect(isRetryableConnectError("read ECONNRESET")).toBe(true);
    expect(isRetryableConnectError("connect ECONNREFUSED 127.0.0.1:9452")).toBe(true);
    expect(isRetryableConnectError("Target page, context or browser has been closed")).toBe(true);
    expect(isRetryableConnectError("Unexpected token < in JSON at position 0")).toBe(false);
    // undici 把真实原因放在 cause；错误链要一起判断，否则会误判成致命错误
    expect(isRetryableConnectError("fetch failed")).toBe(true);
    expect(isRetryableConnectError("fetch failed ← connect ECONNREFUSED 127.0.0.1:9601")).toBe(
      true,
    );
    expect(
      isRetryableConnectError("fetch failed ← The operation was aborted due to timeout"),
    ).toBe(true);
    const fatal = connectDecision({
      error: "Unexpected token < in JSON at position 0",
      elapsedMs: 1000,
      budgetMs: LAUNCH_BUDGET_MS,
    });
    expect(fatal.action).toBe("fail");
    expect(fatal.kind).toBe("connect-error");
  });

  it("重试期间宿主退出：立即失败并报告退出码（0 与非 0 同样立即失败，§6.2 行 4）", () => {
    for (const code of [0, 3]) {
      const decision = connectDecision({
        error: "socket hang up",
        hostExitCode: code,
        ownerKind: "host",
        elapsedMs: 4000,
        budgetMs: LAUNCH_BUDGET_MS,
      });
      expect(decision.action).toBe("fail");
      expect(decision.kind).toBe("host-exited");
      expect(decision.detail).toContain(`退出码=${code}`);
      expect(decision.retryAfterMs).toBe(0);
    }
    expect(
      pageTargetDecision({ hostExitCode: 1, urls: ["about:blank"], elapsedMs: 100 }).action,
    ).toBe("fail");
  });

  it("直到 65 秒仍连不上：按原失败处理，总时限不延长（§6.2 行 5）", () => {
    expect(
      connectDecision({
        error: "socket hang up",
        elapsedMs: LAUNCH_BUDGET_MS - 1,
        budgetMs: LAUNCH_BUDGET_MS,
      }).action,
    ).toBe("retry");
    const exhausted = connectDecision({
      error: "socket hang up",
      elapsedMs: LAUNCH_BUDGET_MS,
      budgetMs: LAUNCH_BUDGET_MS,
    });
    expect(exhausted.action).toBe("fail");
    expect(exhausted.kind).toBe("budget-exhausted");
    expect(exhausted.detail).toContain("65000");
  });

  it("页面 target 先是 about:blank，随后才挂 tauri（§6.2 行 6）", () => {
    expect(pickTauriTarget(["about:blank"])).toBeNull();
    expect(pageTargetDecision({ urls: ["about:blank"], elapsedMs: 3000 }).action).toBe("retry");
    expect(pickTauriTarget(["about:blank", "tauri://localhost/index.html"])).toBe(
      "tauri://localhost/index.html",
    );
    expect(
      pageTargetDecision({ urls: ["about:blank", "tauri://localhost/"], elapsedMs: 3000 }),
    ).toMatchObject({ action: "found", target: "tauri://localhost/" });
  });

  it("target 超时失败时把 target 列表写进证据（§6.2 行 6）", () => {
    const timedOut = pageTargetDecision({
      urls: ["about:blank", "devtools://devtools/bundled/x.html"],
      elapsedMs: PAGE_TARGET_BUDGET_MS,
    });
    expect(timedOut.action).toBe("fail");
    expect(timedOut.kind).toBe("target-timeout");
    expect(timedOut.detail).toContain("about:blank");
    expect(timedOut.targets).toEqual(["about:blank", "devtools://devtools/bundled/x.html"]);
    expect(describeTargets([])).toContain("没有 target");
  });
});

describe("退出确认（§4.8 第 2 条）", () => {
  it("宿主与后代都退出且端口释放才算结束", () => {
    const decision = stopDecision({
      hostPid: 100,
      hostExited: true,
      aliveDescendantPids: [],
      portListening: false,
      elapsedMs: 1200,
    });
    expect(decision.action).toBe("done");
    expect(decision.residualPids).toEqual([]);
  });

  it("旧 WebView2 子进程仍监听旧端口：等到 20 秒才失败并列出残留 PID（§6.2 行 1）", () => {
    const waiting = stopDecision({
      hostPid: 100,
      hostExited: true,
      aliveDescendantPids: [200],
      portListening: true,
      elapsedMs: STOP_TIMEOUT_MS - 1,
    });
    expect(waiting.action).toBe("wait");
    expect(waiting.residualPids).toEqual([200]);
    const timedOut = stopDecision({
      hostPid: 100,
      hostExited: true,
      aliveDescendantPids: [200],
      portListening: true,
      elapsedMs: STOP_TIMEOUT_MS,
    });
    expect(timedOut.action).toBe("timeout");
    expect(timedOut.residualPids).toEqual([200]);
    expect(timedOut.detail).toContain("200");
    expect(timedOut.detail).toContain("20000");
  });

  it("宿主未退出时宿主自身也算残留进程；端口仍被监听单独说明", () => {
    const decision = stopDecision({
      hostPid: 100,
      hostExited: false,
      aliveDescendantPids: [],
      portListening: true,
      elapsedMs: STOP_TIMEOUT_MS,
    });
    expect(decision.residualPids).toEqual([100]);
    expect(decision.kind).toBe("port-still-listening");
    expect(stopDecision({ hostPid: 100, hostExited: false, aliveDescendantPids: [] }).action).toBe(
      "wait",
    );
  });
});

describe("launches[] 证据（§4.8 第 5 条、§6.2 行 7）", () => {
  it("记录序号、端口、宿主 PID 与退出码、后代 PID、端口所属 PID、阶段耗时", () => {
    const record = createLaunchRecord({
      index: 1,
      port: 9452,
      root: "C:/tmp/root",
      logPath: "C:/tmp/process.log",
      browserArgs: "--remote-debugging-port=9452",
      startedAtMs: 1000,
    });
    record.hostPid = 100;
    record.descendantPids = [200, 300];
    recordProbe(record, { atMs: 100, ms: 1800, listenerPids: [100] });
    recordOwnerCheck(record, {
      atMs: 6400,
      owner: { kind: "host", listenerPids: [100], foreignPids: [] },
      probe_ms: 1800,
    });
    record.pageTargetUrl = "tauri://localhost/index.html";
    record.pageTargets = ["about:blank", "tauri://localhost/index.html"];
    record.stageMs = { spawn: 120, cdp_ready: 6400, page_target: 900, total: 7420 };
    record.hostExitCode = 0;
    const [evidence] = buildLaunchesEvidence([record]);
    expect(evidence).toMatchObject({
      index: 1,
      port: 9452,
      host_pid: 100,
      host_exit_code: 0,
      descendant_pids: [200, 300],
      port_owner_pids: [100],
      page_target_url: "tauri://localhost/index.html",
      stage_ms: { spawn: 120, cdp_ready: 6400, page_target: 900, total: 7420 },
    });
    expect(evidence.owner_checks).toHaveLength(1);
    expect(evidence.probes).toEqual([{ at_ms: 100, ms: 1800, ok: true, error: null }]);
  });

  it("首次启动失败也留下 launches[] 与宿主日志尾部（§6.2 行 7）", () => {
    const record = createLaunchRecord({ index: 1, port: 9452, startedAtMs: 0 });
    record.hostPid = 100;
    record.hostExitCode = 2;
    record.failure = "宿主在启动阶段退出（退出码=2），立即失败且不再重试";
    recordConnectAttempt(record, { atMs: 3000, ok: false, error: "ECONNREFUSED" });
    const [evidence] = buildLaunchesEvidence([record]);
    expect(evidence.failure).toContain("退出码=2");
    expect(evidence.connect_attempts).toHaveLength(1);
    const tail = hostLogTail(Array.from({ length: 200 }, (_, i) => `line-${i}`).join("\n"), {
      maxLines: 5,
      maxChars: 1000,
    });
    expect(tail.split("\n")).toHaveLength(5);
    expect(tail).toContain("line-199");
    expect(tail).not.toContain("line-194");
    expect(hostLogTail("a".repeat(50), { maxChars: 10 })).toBe(`…${"a".repeat(10)}`);
    expect(hostLogTail("")).toBe("");
  });
});
