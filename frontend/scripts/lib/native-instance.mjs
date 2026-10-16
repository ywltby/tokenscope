// UP09（计划 §4.8）：原生验收实例生命周期的纯逻辑。
//
// 这里只放可单测的纯函数与常量：端口计算、端口归属判定、连接重试决策、退出确认、
// 证据拼装。真正的进程与网络查询由 `native-acceptance.mjs` 注入——沿用脚本现有的
// **一次 PowerShell 调用**（`Get-CimInstance Win32_Process` + `Get-NetTCPConnection`）
// 取回进程表与监听表，不另建适配器脚本（计划 §4.8 第 6 条）。

/** CDP 就绪（`/json/version` + `connectOverCDP`）阶段上限；与改动前的 45 秒一致。 */
export const CDP_READY_BUDGET_MS = 45000;
/** 等待应用页面 target 阶段上限；与改动前的 20 秒一致。 */
export const PAGE_TARGET_BUDGET_MS = 20000;
/** 单次启动总预算 = 两段之和（计划 §4.8 第 4 条：65 秒，不收紧也不放宽）。 */
export const LAUNCH_BUDGET_MS = CDP_READY_BUDGET_MS + PAGE_TARGET_BUDGET_MS;
/** 连接类瞬断的重试间隔（计划 §4.8 第 4 条）。 */
export const CONNECT_RETRY_INTERVAL_MS = 300;
/** 退出确认上限（计划 §4.8 第 2 条）。 */
export const STOP_TIMEOUT_MS = 20000;
/**
 * 宿主退出轮询间隔（计划 §4.8 第 4 条：宿主一旦退出立即失败）。
 * 与驱动进程里的子进程 `exit` 事件取先到者，保证数百毫秒内发现退出。
 */
export const HOST_EXIT_POLL_MS = 250;
/**
 * 进程/监听表探测自身的超时上限。实际等待取
 * `min(PROBE_TIMEOUT_MS, 本次启动剩余预算)`——预算耗尽时不再等待（计划 §4.8 第 4 条）。
 */
export const PROBE_TIMEOUT_MS = 30000;

/** 进程表 + 监听表探测所用 shell。沿用现有 `stop()` 用的 Windows PowerShell。 */
export const PROBE_SHELL = "powershell.exe";

/**
 * 进程表与监听表的一次 PowerShell 调用（单次进程启动，输出一段 JSON）。
 * 计划 §4.8 第 6 条要求沿用现有方式，不另建适配器脚本。
 */
export const PROCESS_PROBE_COMMAND = [
  "$ErrorActionPreference = 'SilentlyContinue'",
  "$procs = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name)",
  "$listen = @(Get-NetTCPConnection -State Listen | Select-Object LocalPort, OwningProcess, State)",
  "[pscustomobject]@{ processes = $procs; listeners = $listen } | ConvertTo-Json -Depth 3 -Compress",
].join("\n");

/** 探测进程的启动参数（测试固定住 shell 与单次调用形态）。 */
export function probeSpawnArgs() {
  return ["-NoProfile", "-NonInteractive", "-Command", PROCESS_PROBE_COMMAND];
}

/**
 * 本次启动的剩余预算（计划 §4.8 第 4 条：单次启动从启动进程到拿到应用页面共 65 秒，
 * 任何等待都不得超过 `启动开始 + 65 秒`，也不得超过所选阶段的原有上限）。
 * 调用方把「阶段上限与总上限取更严者」算好后传进来；`null`/`Infinity` 表示不受启动预算约束
 * （例如场景收尾的退出确认），此时返回 `Infinity`。
 */
export function remainingBudgetMs(deadlineMs, nowMs = Date.now()) {
  // `null` / `undefined` / 空串表示"没有启动预算约束"，不能被 Number() 折成 0（那就成了"预算已耗尽"）
  if (deadlineMs === null || deadlineMs === undefined || deadlineMs === "") {
    return Number.POSITIVE_INFINITY;
  }
  const deadline = Number(deadlineMs);
  if (!Number.isFinite(deadline)) return Number.POSITIVE_INFINITY;
  const now = Number(nowMs);
  return Math.max(0, Math.round(deadline - (Number.isFinite(now) ? now : Date.now())));
}

/**
 * 把某个等待自身的超时压到剩余预算之内：`min(max(自身超时, 下限), 剩余预算)`。
 * 返回 0 表示预算已耗尽——调用方**不得**再开启新的等待/连接/探测，
 * 而不是把等待抬到自己的最低值（那正是"探测仍可等 30 秒"的缺陷）。
 */
export function clampTimeoutToBudget({
  timeoutMs = 0,
  deadlineMs = null,
  nowMs = Date.now(),
  minMs = 0,
} = {}) {
  const remaining = remainingBudgetMs(deadlineMs, nowMs);
  const own = Number(timeoutMs);
  const floor = Math.max(0, Number(minMs) || 0);
  const wanted = Math.max(Number.isFinite(own) && own > 0 ? own : 0, floor);
  if (!Number.isFinite(remaining)) return Math.round(wanted);
  if (remaining <= 0) return 0;
  return Math.round(Math.min(wanted || remaining, remaining));
}

/**
 * 宿主退出失败的统一文案（计划 §4.8 第 4 条 / §6.2 行 4）：0 与非 0 都立即失败并报告退出码。
 * `connectDecision` 与 `HostExitError` 共用同一文本，避免两处措辞漂移。
 */
export function hostExitDetail({ code = null, atMs = null, source = null } = {}) {
  const codeText = code === null || code === undefined ? "unknown" : code;
  const detected =
    atMs === null || atMs === undefined
      ? ""
      : `（于启动后 ${Math.round(atMs)} ms 经${source === "exit-event" ? "子进程 exit 事件" : "存活轮询"}发现）`;
  return `宿主在启动阶段退出（退出码=${codeText}），立即失败且不再重试${detected}`;
}

/** 等待期间宿主退出：等待立即中止，错误里带退出码（§6.2 行 4）。 */
export class HostExitError extends Error {
  constructor(info = {}) {
    const code = info?.code ?? null;
    super(hostExitDetail({ code, atMs: info?.atMs ?? null, source: info?.source ?? null }));
    this.name = "HostExitError";
    this.kind = "host-exited";
    this.abortedBy = "host-exit";
    this.hostExitCode = code;
    this.detectedAtMs = info?.atMs ?? null;
  }
}

/** 本次启动剩余预算耗尽：不再开启新的等待/连接/探测（§6.2 行 5：总时限不延长）。 */
export class BudgetExhaustedError extends Error {
  constructor({ detail = "", timeoutMs = 0 } = {}) {
    super(detail || `本次启动剩余预算耗尽（已等 ${Math.round(timeoutMs)} ms），不再开启新的等待`);
    this.name = "BudgetExhaustedError";
    this.kind = "budget-exhausted";
    this.abortedBy = "budget";
    this.timeoutMs = Math.round(timeoutMs);
  }
}

/**
 * 连接类瞬断：`socket hang up`、`ECONNRESET`、`ECONNREFUSED`、target 被销毁等
 * （计划 §4.8 第 4 条）。`fetch failed` 是 undici 的包裹错误——真实原因在 `cause`，
 * 调用方应把错误链一起传进来（见 native-acceptance.mjs 的 errorChain）。
 * 非此类错误不重试，直接按失败处理。
 */
const RETRYABLE_CONNECT_ERROR =
  /socket hang up|ECONNRESET|ECONNREFUSED|EPIPE|ETIMEDOUT|ECONNABORTED|fetch failed|aborted|timeout|Target closed|Target page, context or browser has been closed|browser has been closed|WebSocket error|WebSocket is not open|websocket url/i;

export function isRetryableConnectError(message) {
  return RETRYABLE_CONNECT_ERROR.test(String(message ?? ""));
}

/**
 * 同一驱动进程内第 n 次启动的端口：`--port + n`（计划 §4.8 第 1 条）。
 * `--port` 是本次运行的端口序列起点，启动序号从 1 起，端口不复用。
 */
export function launchPort(basePort, launchNumber) {
  const base = Number(basePort);
  const n = Number(launchNumber);
  if (!Number.isInteger(base) || base <= 0 || base > 65535)
    throw new Error(`非法基准端口：${basePort}`);
  if (!Number.isInteger(n) || n < 1) throw new Error(`非法启动序号：${launchNumber}`);
  const port = base + n;
  if (port > 65535) throw new Error(`启动序号越界：${base} + ${n} 超过 65535`);
  return port;
}

function uniqueSortedPids(values) {
  const set = new Set();
  for (const raw of values ?? []) {
    const pid = Number(raw);
    if (Number.isInteger(pid) && pid > 0) set.add(pid);
  }
  return [...set].sort((a, b) => a - b);
}

/** 合并两批 PID（证据里后代列表要累计，不能因为换了一次快照就丢）。 */
export function mergePids(left, right) {
  return uniqueSortedPids([...(left ?? []), ...(right ?? [])]);
}

function asArray(value) {
  if (value === null || value === undefined || value === "") return [];
  return Array.isArray(value) ? value : [value];
}

/** `Get-CimInstance Win32_Process` 的一行 → `{ pid, ppid, name }`。 */
export function normalizeProcessTable(raw) {
  return asArray(raw)
    .map((item) => ({
      pid: Number(item?.ProcessId ?? item?.pid ?? item?.PID),
      ppid: Number(item?.ParentProcessId ?? item?.ppid ?? item?.ParentPid),
      name: String(item?.Name ?? item?.name ?? ""),
    }))
    .filter((item) => Number.isInteger(item.pid) && item.pid > 0);
}

/** `Get-NetTCPConnection -State Listen` 的一行 → `{ port, pid, state }`。 */
export function normalizeListeners(raw) {
  return asArray(raw)
    .map((item) => ({
      port: Number(item?.LocalPort ?? item?.port ?? item?.Port),
      pid: Number(item?.OwningProcess ?? item?.pid ?? item?.OwningPid),
      state: String(item?.State ?? item?.state ?? "Listen"),
    }))
    .filter((item) => Number.isInteger(item.port) && item.port > 0 && Number.isInteger(item.pid));
}

/**
 * 解析一次探测输出。PowerShell 的告警可能混进 stdout，因此先整体解析，
 * 失败再退回“最后一行 JSON”。解析不出返回 `ok:false`，由调用方按探测失败处理。
 */
export function parseProbeOutput(text) {
  const trimmed = String(text ?? "").trim();
  if (!trimmed) return { ok: false, error: "探测输出为空", processes: [], listeners: [] };
  const candidates = [trimmed, ...trimmed.split(/\r?\n/).reverse()];
  for (const candidate of candidates) {
    const line = candidate.trim();
    if (!line.startsWith("{")) continue;
    try {
      const data = JSON.parse(line);
      return {
        ok: true,
        error: null,
        processes: normalizeProcessTable(data?.processes),
        listeners: normalizeListeners(data?.listeners),
      };
    } catch {
      /* 换下一个候选 */
    }
  }
  return {
    ok: false,
    error: `探测输出不是 JSON：${trimmed.slice(0, 160)}`,
    processes: [],
    listeners: [],
  };
}

/**
 * 宿主退出信号（计划 §4.8 第 4 条）：驱动进程里的子进程 `exit` 事件（`notify`）
 * 与短间隔存活轮询取先到者。resolve 出 `{ code, atMs, source }`；宿主不退出则永不 resolve。
 *
 * 单独用 exit 事件不够：`spawn` 的宿主若被别处结束、或事件在等待中途丢失，
 * 等待会一直挂到自己的超时；单独用轮询也不够：至少要等一个轮询周期才发现。
 * 两者一起才满足"数百毫秒内中止"。
 */
export function createHostExitSignal({
  isAlive = () => true,
  readExitCode = () => null,
  intervalMs = HOST_EXIT_POLL_MS,
  now = () => Date.now(),
  onExit = null,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
} = {}) {
  let resolveSignal;
  const signal = new Promise((resolve) => {
    resolveSignal = resolve;
  });
  let settled = false;
  let timer = null;
  let polls = 0;
  let explicitCode;
  const clear = () => {
    if (timer !== null) {
      clearTimer(timer);
      timer = null;
    }
  };
  const settle = (source, code) => {
    if (settled) return;
    settled = true;
    clear();
    const value = code === undefined || code === null ? readExitCode() : code;
    const info = { code: value ?? null, atMs: now(), source };
    try {
      onExit?.(info);
    } catch {
      /* 记录回调失败不影响退出信号本身 */
    }
    resolveSignal(info);
  };
  const poll = () => {
    timer = null;
    if (settled) return;
    polls++;
    let alive = true;
    try {
      alive = isAlive() !== false;
    } catch {
      alive = false;
    }
    if (!alive) {
      settle("poll");
      return;
    }
    timer = setTimer(poll, Math.max(1, Math.round(intervalMs)));
  };
  return {
    signal,
    start() {
      if (!settled && timer === null) timer = setTimer(poll, 0);
      return this;
    },
    /** 由驱动侧的 `child.on("exit")` 调用；即使退出码为 null（信号终止）也要触发。 */
    notify(code) {
      if (code !== undefined && code !== null) explicitCode = code;
      settle("exit-event", explicitCode);
      return this;
    },
    stop() {
      clear();
      return this;
    },
    isSettled: () => settled,
    exitInfo: () => (settled ? { code: explicitCode ?? null } : null),
    get polls() {
      return polls;
    },
  };
}

/**
 * 让一个可能不可取消的等待（`connectOverCDP`、`fetch`、重试间隔）在
 * 「目标完成」「宿主退出」「预算耗尽」之间取先到者。
 *
 * 取消分支抛出 `HostExitError` / `BudgetExhaustedError`；**迟到的成功结果不返回给调用方**，
 * 而是交给 `onAbandon` 清理（例如 `browser.close()`）——这样被取消的旧连接不会被当成
 * 新的尝试结果，底层连接也不会悬空。
 */
export function awaitWithHostExit({
  task,
  hostExit = null,
  timeoutMs = 0,
  timeoutDetail = "",
  onAbandon = null,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
} = {}) {
  const waitMs = Math.max(0, Math.round(Number(timeoutMs) || 0));
  if (waitMs <= 0) {
    // 预算已耗尽：不启动新的等待/连接（§6.2 行 5）
    return Promise.reject(new BudgetExhaustedError({ timeoutMs: 0, detail: timeoutDetail }));
  }
  const abandon = (value) => {
    if (typeof onAbandon !== "function") return;
    try {
      onAbandon(value);
    } catch {
      /* 清理失败不影响主流程 */
    }
  };
  return new Promise((resolve, reject) => {
    let settled = false;
    let timer = null;
    const finish = (action) => {
      if (settled) return;
      settled = true;
      if (timer !== null) {
        clearTimer(timer);
        timer = null;
      }
      action();
    };
    timer = setTimer(
      () => finish(() => reject(new BudgetExhaustedError({ timeoutMs: waitMs, detail: timeoutDetail }))),
      waitMs,
    );
    const observed = Promise.resolve()
      .then(task)
      .then(
        (value) => ({ ok: true, value }),
        (error) => ({ ok: false, error }),
      );
    if (hostExit?.signal) {
      hostExit.signal.then(
        (info) => finish(() => reject(new HostExitError(info))),
        () => {},
      );
    }
    observed.then((result) => {
      if (settled) {
        // 已经被取消：迟到的成功结果交给清理逻辑，不回传
        if (result.ok) abandon(result.value);
        return;
      }
      finish(() => (result.ok ? resolve(result.value) : reject(result.error)));
    });
  });
}

/**
 * 一次进程/监听表探测（沿用一次 PowerShell 调用，§4.8 第 6 条）：
 *   - 自身超时取 `min(PROBE_TIMEOUT_MS, 本次启动剩余预算)`；预算耗尽时**不 spawn**；
 *   - 宿主退出信号先到时立刻结束本次探测并终止 PowerShell（由调用方的决策点抛出带退出码的错误）。
 * 返回 `{ ok, processes, listeners, ms, timeout_ms, error, hostExited? }`；探测失败不抛错。
 */
export function createProbeFn({
  spawnFn,
  shell = PROBE_SHELL,
  spawnArgs = probeSpawnArgs,
  ownTimeoutMs = PROBE_TIMEOUT_MS,
  now = () => Date.now(),
  setTimer = setTimeout,
  clearTimer = clearTimeout,
} = {}) {
  if (typeof spawnFn !== "function") throw new Error("createProbeFn 需要注入 spawn 实现");
  return function probe({ deadlineMs = null, hostExit = null } = {}) {
    return new Promise((resolve) => {
      const startedAt = now();
      const timeoutMs = clampTimeoutToBudget({ timeoutMs: ownTimeoutMs, deadlineMs, nowMs: startedAt });
      if (timeoutMs <= 0) {
        resolve({
          ok: false,
          error: `本次启动剩余预算已耗尽，不开启新的探测（自身超时 ${ownTimeoutMs} ms 被截断）`,
          processes: [],
          listeners: [],
          timeout_ms: 0,
          ms: 0,
        });
        return;
      }
      let child;
      try {
        child = spawnFn(shell, spawnArgs(), { windowsHide: true });
      } catch (e) {
        resolve({
          ok: false,
          error: `探测进程启动失败：${String(e?.message ?? e)}`,
          processes: [],
          listeners: [],
          timeout_ms: timeoutMs,
          ms: now() - startedAt,
        });
        return;
      }
      let out = "";
      let err = "";
      let done = false;
      let timer = null;
      const finish = (result) => {
        if (done) return;
        done = true;
        if (timer !== null) clearTimer(timer);
        resolve({ ...result, timeout_ms: timeoutMs, ms: now() - startedAt });
      };
      timer = setTimer(() => {
        try {
          child.kill();
        } catch {}
        finish({
          ok: false,
          error: `探测自身超过 ${timeoutMs} ms（已按本次启动剩余预算截断），已终止本次探测`,
          processes: [],
          listeners: [],
        });
      }, timeoutMs);
      if (child.stdout && child.stderr) {
        child.stdout.on("data", (d) => (out += d));
        child.stderr.on("data", (d) => (err += d));
      }
      child.on("error", (e) =>
        finish({
          ok: false,
          error: `探测进程错误：${String(e?.message ?? e)}`,
          processes: [],
          listeners: [],
        }),
      );
      child.on("close", () => {
        const parsed = parseProbeOutput(out);
        finish({
          ...parsed,
          error: parsed.error ? `${parsed.error}${err.trim() ? `；stderr: ${err.trim().slice(0, 200)}` : ""}` : null,
        });
      });
      if (hostExit?.signal) {
        hostExit.signal.then(
          (info) => {
            try {
              child.kill();
            } catch {}
            finish({
              ok: false,
              hostExited: true,
              hostExitCode: info?.code ?? null,
              error: `宿主已退出（退出码=${info?.code ?? "unknown"}），探测中止`,
              processes: [],
              listeners: [],
            });
          },
          () => {},
        );
      }
    });
  };
}

/**
 * 递归记录宿主后代（按父 PID），用于端口归属判定与关闭确认（计划 §4.8 第 2、3 条）。
 * 返回不含宿主自身的 PID 列表。
 */
export function collectDescendants(processes, rootPid) {
  const hostPid = Number(rootPid);
  if (!Number.isInteger(hostPid) || hostPid <= 0) return [];
  const rows = (processes ?? []).filter((p) => Number.isInteger(p?.pid) && p.pid > 0);
  const found = new Set();
  let frontier = [hostPid];
  while (frontier.length) {
    const next = [];
    for (const parent of frontier) {
      for (const row of rows) {
        if (row.ppid !== parent || row.pid === hostPid || found.has(row.pid)) continue;
        found.add(row.pid);
        next.push(row.pid);
      }
    }
    frontier = next;
  }
  return uniqueSortedPids([...found]);
}

/**
 * `State` 在 PowerShell 里是 `TcpState` 枚举，`ConvertTo-Json` 会序列化成数字
 * （Listen = 2），也可能直接给字符串。两种形态都要算监听者——实测只认字符串会把
 * 真实监听者全部漏掉，端点归属永远判成“无监听者”。
 */
const LISTEN_STATES = new Set(["listen", "2"]);

/** 监听指定端口的进程 PID（只算 Listen 状态）。 */
export function listenersOnPort(listeners, port) {
  const wanted = Number(port);
  return uniqueSortedPids(
    (listeners ?? [])
      .filter(
        (l) =>
          Number(l?.port) === wanted &&
          LISTEN_STATES.has(String(l?.state ?? "Listen").toLowerCase()),
      )
      .map((l) => l.pid),
  );
}

/**
 * 端点归属判定（计划 §4.8 第 3 条）：监听者必须是宿主或本次已记录的后代。
 * 无监听者 → `none`（继续等）；出现任何不属于本次实例的监听者 → `foreign`（失败）。
 */
export function classifyPortOwner({ listenerPids, hostPid, descendantPids } = {}) {
  const pids = uniqueSortedPids(listenerPids);
  const host = Number(hostPid);
  const owned = new Set(uniqueSortedPids([host, ...(descendantPids ?? [])]));
  if (!pids.length) {
    return {
      kind: "none",
      listenerPids: [],
      ownedPids: [],
      foreignPids: [],
      detail: "端口没有监听者",
    };
  }
  const foreignPids = pids.filter((pid) => !owned.has(pid));
  const ownedPids = pids.filter((pid) => owned.has(pid));
  if (foreignPids.length) {
    return {
      kind: "foreign",
      listenerPids: pids,
      ownedPids,
      foreignPids,
      detail: `端口监听者 PID ${foreignPids.join(",")} 既不是宿主 ${host} 也不在已记录后代 [${[...(descendantPids ?? [])].join(",")}] 中`,
    };
  }
  const kind = pids.includes(host) ? "host" : "descendant";
  return {
    kind,
    listenerPids: pids,
    ownedPids,
    foreignPids: [],
    detail: `端口由${kind === "host" ? "宿主" : "已记录后代"} PID ${pids.join(",")} 监听`,
  };
}

/**
 * 启动阶段的下一步动作（计划 §4.8 第 2、3、4 条）：
 * 宿主退出立即失败并报退出码；外来监听者立即失败且不结束它；瞬断在预算内重试。
 */
export function connectDecision({
  error = null,
  hostExitCode = null,
  ownerKind = "none",
  ownerDetail = "",
  elapsedMs = 0,
  budgetMs = CDP_READY_BUDGET_MS,
} = {}) {
  if (hostExitCode !== null && hostExitCode !== undefined) {
    return {
      action: "fail",
      kind: "host-exited",
      detail: hostExitDetail({ code: hostExitCode }),
      retryAfterMs: 0,
    };
  }
  if (ownerKind === "foreign") {
    return {
      action: "fail",
      kind: "foreign-listener",
      detail: `端口归属不符：${ownerDetail}；不连接、不结束外来进程`,
      retryAfterMs: 0,
    };
  }
  if (error && !isRetryableConnectError(error)) {
    return {
      action: "fail",
      kind: "connect-error",
      detail: `连接失败且不属于可重试瞬断：${error}`,
      retryAfterMs: 0,
    };
  }
  if (elapsedMs >= budgetMs) {
    return {
      action: "fail",
      kind: "budget-exhausted",
      detail: `启动预算 ${budgetMs} ms 用尽（已用 ${Math.round(elapsedMs)} ms）${error ? `；最后一次错误：${error}` : ""}`,
      retryAfterMs: 0,
    };
  }
  return {
    action: "retry",
    kind: error
      ? "transient-connect-error"
      : ownerKind === "none"
        ? "no-listener"
        : "not-connected",
    detail: error
      ? `可重试瞬断：${error}`
      : ownerKind === "none"
        ? "端口尚无监听者，继续等待"
        : "端点已存在但尚未连接成功",
    retryAfterMs: CONNECT_RETRY_INTERVAL_MS,
  };
}

/** 应用页面 target：沿用现有 `/tauri/i` 匹配（计划 §19 保留）。 */
export function pickTauriTarget(urls) {
  const list = (urls ?? []).filter((u) => typeof u === "string");
  return list.find((u) => /tauri/i.test(u)) ?? null;
}

/** target 列表文案：等待超时与失败证据都要记录它（计划 §6.2）。 */
export function describeTargets(urls) {
  const list = (urls ?? []).filter((u) => typeof u === "string");
  return list.length ? list.map((u) => JSON.stringify(u)).join(", ") : "（当前没有 target）";
}

/** 页面 target 阶段的下一步动作；阶段上限 20 秒，超时把 target 列表写进证据。 */
export function pageTargetDecision({
  hostExitCode = null,
  urls = [],
  elapsedMs = 0,
  budgetMs = PAGE_TARGET_BUDGET_MS,
} = {}) {
  if (hostExitCode !== null && hostExitCode !== undefined) {
    return {
      action: "fail",
      kind: "host-exited",
      detail: `宿主在等待页面 target 时退出（退出码=${hostExitCode}）`,
    };
  }
  const target = pickTauriTarget(urls);
  if (target) return { action: "found", kind: "tauri-target", detail: target, target };
  if (elapsedMs >= budgetMs) {
    return {
      action: "fail",
      kind: "target-timeout",
      detail: `没有 tauri 页面 target（等待 ${budgetMs} ms；现有 target：${describeTargets(urls)}）`,
      targets: [...urls],
    };
  }
  return {
    action: "retry",
    kind: "waiting-target",
    detail: `等待 tauri 页面 target（现有：${describeTargets(urls)}）`,
  };
}

/**
 * 退出确认决策（计划 §4.8 第 2 条）：宿主与全部已记录后代退出、端口不再监听才算结束，
 * 上限 20 秒；超时返回残留 PID 列表。
 */
export function stopDecision({
  hostPid = null,
  hostExited = false,
  aliveDescendantPids = [],
  portListening = false,
  elapsedMs = 0,
  timeoutMs = STOP_TIMEOUT_MS,
} = {}) {
  const host = Number(hostPid);
  const alive = uniqueSortedPids(aliveDescendantPids);
  const residualPids = uniqueSortedPids([...(hostExited ? [] : [host]), ...alive]);
  if (hostExited && !residualPids.length && !portListening) {
    return {
      action: "done",
      kind: "exited-and-port-released",
      residualPids: [],
      detail: `宿主与后代已退出、端口已释放（${Math.round(elapsedMs)} ms）`,
    };
  }
  if (elapsedMs >= timeoutMs) {
    return {
      action: "timeout",
      kind: portListening ? "port-still-listening" : "processes-still-alive",
      residualPids,
      detail: `超过 ${timeoutMs} ms 仍未确认退出：残留 PID [${residualPids.join(",") || "无"}]${portListening ? "；端口仍在监听" : ""}`,
    };
  }
  return {
    action: "wait",
    kind: "waiting-exit",
    residualPids,
    detail: `等待宿主与后代退出、端口释放（已等 ${Math.round(elapsedMs)} ms）`,
  };
}

/** 一次启动的证据骨架（计划 §4.8 第 5 条）。 */
export function createLaunchRecord({
  index,
  port,
  root = "",
  logPath = "",
  browserArgs = "",
  startedAtMs = Date.now(),
} = {}) {
  return {
    index,
    port,
    root,
    logPath,
    browserArgs,
    startedAtMs,
    startedAt: new Date(startedAtMs).toISOString(),
    // 启动预算（§4.8 第 4 条）：总上限与当前阶段上限取更严者作为一切等待的截止时刻
    launchDeadlineMs: null,
    phaseDeadlineMs: null,
    hostExitDetected: null,
    attached: false,
    hostPid: null,
    hostExitCode: null,
    descendantPids: [],
    portOwnerPids: [],
    ownerChecks: [],
    probes: [],
    attempts: [],
    pageTargets: [],
    pageTargetUrl: null,
    stageMs: { spawn: null, cdp_ready: null, page_target: null, total: null },
    stop: null,
    failure: null,
  };
}

/** 记录一次 PowerShell 探测：耗时与结果都进证据。 */
export function recordProbe(
  record,
  { atMs = 0, ms = 0, ok = true, error = null, listenerPids = [] } = {},
) {
  if (!record) return record;
  record.probes.push({
    at_ms: Math.round(atMs),
    ms: Math.round(ms),
    ok: !!ok,
    error: error ?? null,
  });
  record.portOwnerPids = mergePids(record.portOwnerPids, listenerPids);
  return record;
}

/** 记录一次连接尝试（时间与错误），重试成功时两次都要留在证据里。 */
export function recordConnectAttempt(
  record,
  { atMs = 0, phase = "cdp", ok = true, error = null, ms = 0, aborted = null } = {},
) {
  if (!record) return record;
  record.attempts.push({
    at_ms: Math.round(atMs),
    phase,
    ok: !!ok,
    error: error ?? null,
    ms: Math.round(ms),
    // 被宿主退出/预算取消的尝试单独标注，便于从证据里区分"自己超时"与"被中止"
    aborted: aborted ?? null,
  });
  return record;
}

/** 记录一次端口归属核实。 */
export function recordOwnerCheck(record, { atMs = 0, owner, probe_ms = 0 } = {}) {
  if (!record) return record;
  record.ownerChecks.push({
    at_ms: Math.round(atMs),
    kind: owner?.kind ?? "unknown",
    listener_pids: [...(owner?.listenerPids ?? [])],
    foreign_pids: [...(owner?.foreignPids ?? [])],
    probe_ms: Math.round(probe_ms),
  });
  return record;
}

/** 宿主日志尾部：失败证据要能直接看到应用为什么没起来（计划 §4.8 第 5 条）。 */
export function hostLogTail(text, { maxLines = 80, maxChars = 6000 } = {}) {
  const lines = String(text ?? "").split(/\r?\n/);
  const tail = lines.slice(Math.max(0, lines.length - maxLines));
  let body = tail.join("\n");
  if (body.length > maxChars) body = `…${body.slice(body.length - maxChars)}`;
  return body;
}

/** `launches[]`：序号、端口、宿主 PID 与退出码、后代 PID、端口所属 PID、尝试、target、阶段耗时。 */
export function buildLaunchesEvidence(records) {
  return (records ?? []).map((record) => ({
    index: record.index,
    port: record.port,
    attached: record.attached === true,
    host_pid: record.hostPid,
    host_exit_code: record.hostExitCode,
    host_exit_detected: record.hostExitDetected ? { ...record.hostExitDetected } : null,
    budget_ms:
      Number.isFinite(Number(record.launchDeadlineMs)) && Number.isFinite(Number(record.startedAtMs))
        ? Math.round(Number(record.launchDeadlineMs) - Number(record.startedAtMs))
        : null,
    descendant_pids: [...record.descendantPids],
    port_owner_pids: [...record.portOwnerPids],
    browser_args: record.browserArgs,
    page_target_url: record.pageTargetUrl,
    page_targets: [...record.pageTargets],
    connect_attempts: record.attempts.map((a) => ({ ...a })),
    owner_checks: record.ownerChecks.map((c) => ({ ...c })),
    probes: record.probes.map((p) => ({ ...p })),
    stage_ms: { ...record.stageMs },
    started_at: record.startedAt,
    failure: record.failure,
    stop: record.stop,
  }));
}
