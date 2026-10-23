// S00（PR3 计划 §9.1）：原生验收场景的共用阶段记录、有界命令与看门狗收尾。
//
// 背景：main 6393bfc 的 Native UI E2E `ui-system --scale 2` 卡死至 45 分钟超时且
// evidence.json 未写出——spawnSync 无 timeout、`browser.close()` 无上限、失败截图
// 落在场景竞速保护之外。本模块只放可单测的共用逻辑，不重写实例管理
// （Instance 仍在 native-acceptance.mjs，端口/退出确认决策仍在 native-instance.mjs）。
//
// 边界（计划 §9.1，不放宽）：
//   - 同步子进程一律 30 秒上限；检查 error/status/signal，不只看 stdout；
//   - CDP `browser.close()` 一律 10 秒上限，超时记为 close timeout 后继续既有流程；
//   - 场景看门狗到点：置取消标志 → 同步写最小证据 → 有界收尾（复制应用日志、终止
//     **本次自己记录**的 PID）→ 非零退出；attach 模式不认领、不终止外部实例；
//   - Promise.race 不会取消另一分支：取消标志由场景与 Instance 在关闭/重启前自查，
//     超时后不得再次 launch；
//   - 同步阻塞会推迟 JS 定时器，场景预算不是精确墙钟强杀点——预算最多还需容纳
//     当前这一次有界同步调用（30 秒）。

import { spawnSync } from "node:child_process";

/** 单次同步子进程上限（计划 §9.1 第 2 条）。 */
export const SYNC_COMMAND_TIMEOUT_MS = 30000;
/** CDP `browser.close()` 上限（计划 §6 S00 第 2 条）。 */
export const BROWSER_CLOSE_TIMEOUT_MS = 10000;
/** 失败截图/DOM 取证的短上限（计划 §9.1 第 5 条：无法完成就跳过并记原因）。 */
export const FAILURE_CAPTURE_TIMEOUT_MS = 8000;
/** 普通场景整体上限（计划 §6 S00 第 3 条：5 分钟）。 */
export const DEFAULT_SCENARIO_TIMEOUT_MS = 300000;
/** 有明确长时目的的场景保留显式独立预算，不按 5 分钟截断合法工作。 */
export const SCENARIO_TIMEOUT_OVERRIDES = {
  "expire-cycle": 750000,
  "ui-relaunch": 1800000,
  "ui-collection-status": 480000,
};

/** 场景预算：有显式独立预算的场景用原值，其余 5 分钟。 */
export function scenarioBudgetMs(scenario) {
  return SCENARIO_TIMEOUT_OVERRIDES[scenario] ?? DEFAULT_SCENARIO_TIMEOUT_MS;
}

/** 场景超时后的迟到分支（关闭/重启/再次启动）必须抛错，不得继续产生副作用。 */
export class ScenarioCanceledError extends Error {
  constructor(action) {
    super(`场景已超时取消，不再执行「${action}」`);
    this.name = "ScenarioCanceledError";
    this.kind = "scenario-canceled";
    this.action = action;
  }
}

/** 取消标志已置位时抛出 `ScenarioCanceledError`；未取消时什么也不做。 */
export function ensureNotCanceled(isCanceled, action) {
  if (typeof isCanceled === "function" && isCanceled()) {
    throw new ScenarioCanceledError(action);
  }
}

/**
 * 有界 spawnSync（计划 §9.1 第 2 条）：统一 30 秒上限；检查 error / signal，
 * 非零退出码原样返回交给调用方按既有语义判定（保持各调用点现有的 status/stdout 检查）。
 * 超时（进程已被杀）与 spawn 失败抛出含动作、阶段与命令退出情况的错误；不输出凭据。
 * 2026-10-23 取证装配：超时时子进程已写出的 stdout/stderr 尾部（有界截断）随错误
 * 保留——prepare 类脚本按阶段输出进度，只剩外层 ETIMEDOUT 就无法定位阻塞阶段。
 */
const TIMEOUT_OUTPUT_TAIL_CHARS = 2000;

function tailText(text) {
  const s = String(text ?? "");
  if (!s.trim()) return "";
  return s.length > TIMEOUT_OUTPUT_TAIL_CHARS ? `…${s.slice(-TIMEOUT_OUTPUT_TAIL_CHARS)}` : s;
}

/**
 * 2026-10-23 取证装配：超时/启动失败的错误对象携带 `outputTail`（子进程已
 * 产出的 stdout/stderr 尾部，各自有界截断），消息里保留带前缀的摘要。
 * evidence 组装处经 [`errorEvidenceFields`] 把尾部写成独立证据字段——若只
 * 拼进 message，会被外层 800 字符错误摘要截掉，证据在落盘时丢失。
 */
function outputTail(text) {
  const s = String(text ?? "");
  if (!s.trim()) return "";
  const tail = s.length > TIMEOUT_OUTPUT_TAIL_CHARS ? `…${s.slice(-TIMEOUT_OUTPUT_TAIL_CHARS)}` : s;
  return `（子进程已产出输出尾部）: ${tail.replace(/\r?\n$/, "")}`;
}

/**
 * evidence 错误字段的统一组装：`error` 保持既有 800 字符摘要上限；
 * `command_output_tail` 承载 [`spawnSyncBounded`] 错误对象上的
 * `outputTail`（无则 null）。返回对象直接并入 evidence，序列化后字段
 * 仍完整（尾部不在 800 字符摘要内，不受其截断影响）。
 */
export function errorEvidenceFields(error) {
  const fields = { error: String(error?.stack ?? error).slice(0, 800) };
  const tail = error?.outputTail;
  if (tail && (tail.stdout || tail.stderr)) {
    fields.command_output_tail = { stdout: tail.stdout ?? "", stderr: tail.stderr ?? "" };
  }
  return fields;
}

export function spawnSyncBounded(
  command,
  args,
  { action = "sync-command", stage = null, timeoutMs = SYNC_COMMAND_TIMEOUT_MS, options = {} } = {},
) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    windowsHide: true,
    timeout: Math.max(1, Math.round(Number(timeoutMs) || SYNC_COMMAND_TIMEOUT_MS)),
    ...options,
  });
  const where = `动作=${action}${stage ? ` 阶段=${stage}` : ""} 命令=${command}`;
  if (result.error) {
    const tail = { stdout: tailText(result.stdout), stderr: tailText(result.stderr) };
    const tailNote = [
      outputTail(result.stdout),
      outputTail(result.stderr).replace("（子进程已产出输出尾部）: ", "（stderr 尾部）: "),
    ]
      .filter(Boolean)
      .join("");
    const failure = new Error(
      `同步命令超时或启动失败：${where} errno=${result.error.errno ?? "unknown"} status=${result.status} signal=${result.signal ?? "none"}（${result.error.message}）${tailNote}`,
    );
    failure.outputTail = tail;
    throw failure;
  }
  if (result.signal) {
    throw new Error(`同步命令被信号终止（超时上限 ${timeoutMs} ms）：${where} signal=${result.signal} status=${result.status}`);
  }
  return result;
}

/**
 * 10 秒有界 CDP 关闭（计划 §9.1 第 5 条）：所有断开 CDP 的路径（Instance.stop、
 * 契约直接调用、expire-start）复用。超时记为 close timeout，绝不抛出——调用方
 * 拿着返回值继续既有退出确认与进程终止流程，不能因关闭挂起而卡死。
 */
export async function closeBrowserBounded(browser, { timeoutMs = BROWSER_CLOSE_TIMEOUT_MS } = {}) {
  if (!browser || typeof browser.close !== "function") {
    return { closed: false, reason: "no-browser" };
  }
  let timer = null;
  try {
    return await Promise.race([
      Promise.resolve()
        .then(() => browser.close())
        .then(
          () => ({ closed: true, reason: null }),
          (error) => ({ closed: false, reason: `close error: ${String(error?.message ?? error)}` }),
        ),
      new Promise((resolve) => {
        timer = setTimeout(() => resolve({ closed: false, reason: "close timeout" }), Math.max(1, timeoutMs));
      }),
    ]);
  } finally {
    if (timer !== null) clearTimeout(timer);
  }
}

/**
 * 有界取证（计划 §9.1 第 5 条）：失败截图/DOM 查询超过短上限就放弃并记原因。
 * 底层 Promise 无法真正取消——放弃后它可能仍挂着，但不阻塞收尾；残余连接由
 * 调用方最终的 process.exit 结束。
 */
export function withTimeout(task, timeoutMs, detail) {
  let timer = null;
  return Promise.race([
    Promise.resolve().then(task),
    new Promise((_, reject) => {
      timer = setTimeout(
        () => reject(new Error(`${detail}（${Math.round(timeoutMs)} ms 上限，已跳过并记原因）`)),
        Math.max(1, timeoutMs),
      );
    }),
  ]).finally(() => {
    if (timer !== null) clearTimeout(timer);
  });
}

/**
 * 共用阶段记录（计划 §9.1 第 3 条）：调用前写「阶段 <名称>」并记录启动序号与时间，
 * 调用后 markDone 记录完成。证据保留完整序列；`last_stage` 取最后一个开始阶段
 * （done_at 缺失即卡在该阶段）。收尾阶段不得用「cleanup」之类名字覆盖真正失败阶段。
 */
export function createStageTracker({ log = () => {}, launchIndexOf = () => null } = {}) {
  const entries = [];
  return {
    mark(name) {
      const entry = {
        stage: name,
        at: new Date().toISOString(),
        launch: launchIndexOf() ?? null,
        done_at: null,
      };
      entries.push(entry);
      log(`阶段 ${name}`);
      return entry;
    },
    markDone(name) {
      for (let i = entries.length - 1; i >= 0; i--) {
        if (entries[i].stage === name && entries[i].done_at === null) {
          entries[i].done_at = new Date().toISOString();
          return entries[i];
        }
      }
      return null;
    },
    list: () => entries.map((e) => ({ ...e })),
    last: () => {
      const entry = entries.at(-1);
      return entry ? { ...entry } : null;
    },
  };
}

/**
 * 场景级看门狗（计划 §6 S00 第 3 条 / §9.1 第 4、6 条）：
 *   - 到点先置取消标志，再同步执行 onTimeout（追加失败检查并写最小证据——同步写，
 *     不依赖 CDP；失败截图等后续增强不能阻止它落盘）；
 *   - 竞速沿用既有入口形态：看门狗分支 resolve 一个 timeout 结论，场景正常结束或
 *     抛错时解除计时器；不另套一层无人收尾的 Promise.race；
 *   - 场景迟到分支由调用方通过 cancelFlag 自查（本函数只置位，不取消底层任务）。
 * 返回 `{ kind: "completed", value }` 或 `{ kind: "timeout", timeoutMs }`；场景抛错原样上抛。
 */
export function runScenarioBounded({
  scenarioFn,
  timeoutMs,
  cancelFlag = { requested: false },
  onTimeout = () => {},
  hb = () => {},
  setTimer = setTimeout,
  clearTimer = clearTimeout,
} = {}) {
  const budget = Math.max(1, Math.round(Number(timeoutMs) || DEFAULT_SCENARIO_TIMEOUT_MS));
  return new Promise((resolve, reject) => {
    let timer = null;
    let settled = false;
    const fired = new Promise((resolveFired) => {
      timer = setTimer(() => {
        try {
          cancelFlag.requested = true;
          hb(`场景看门狗触发：超过 ${budget} ms 未完成，置取消标志并写最小证据`);
          try {
            onTimeout();
          } catch (e) {
            hb(`最小证据写出失败：${String(e?.message ?? e)}`);
          }
          resolveFired({ kind: "timeout", timeoutMs: budget });
        } catch (e) {
          // 看门狗自身异常也不能吞掉超时结论
          cancelFlag.requested = true;
          resolveFired({ kind: "timeout", timeoutMs: budget });
        }
      }, budget);
    });
    const finish = (fn) => {
      if (settled) return;
      settled = true;
      if (timer !== null) clearTimer(timer);
      timer = null;
      fn();
    };
    fired.then((outcome) => finish(() => resolve(outcome)));
    Promise.resolve()
      .then(() => scenarioFn({ isCanceled: () => cancelFlag.requested === true }))
      .then(
        (value) => finish(() => resolve({ kind: "completed", value })),
        (error) => finish(() => reject(error)),
      );
  });
}

/**
 * 超时后的有界收尾（计划 §9.1 第 4、6 条）：先按 10 秒上限解除 CDP，再复制应用日志，
 * 然后按既有流程终止**本次自己记录**的 PID；attach 模式不认领、不终止外部实例。
 * 每一步失败都记录后继续，不抛出——收尾异常不能阻止随后的非零退出。
 */
export async function finalizeScenarioTimeout({
  attached = false,
  record = null,
  terminateRecordedPids = null,
  closeBrowser = null,
  copyAppLogs = null,
  hb = () => {},
} = {}) {
  const result = {
    browser_close: { closed: false, reason: "not-attempted" },
    app_logs_copied: false,
    terminate: attached ? "skipped-attach" : "not-attempted",
    errors: [],
  };
  if (typeof closeBrowser === "function") {
    try {
      result.browser_close = await closeBrowser();
    } catch (e) {
      result.errors.push(`closeBrowser: ${String(e?.message ?? e)}`);
    }
  }
  if (typeof copyAppLogs === "function") {
    try {
      copyAppLogs();
      result.app_logs_copied = true;
    } catch (e) {
      result.errors.push(`copyAppLogs: ${String(e?.message ?? e)}`);
    }
  }
  if (!attached && typeof terminateRecordedPids === "function") {
    if (record?.hostPid) {
      try {
        const t = await terminateRecordedPids(record, "scenario-timeout-cleanup");
        result.terminate = { reason: t?.reason ?? "scenario-timeout-cleanup", pids: t?.pids ?? [] };
      } catch (e) {
        result.terminate = "failed";
        result.errors.push(`terminateRecordedPids: ${String(e?.message ?? e)}`);
      }
    } else {
      result.terminate = "no-recorded-host";
    }
  }
  hb(`有界收尾完成：${JSON.stringify(result)}`);
  return result;
}

/**
 * 等查询身份稳定（PR3 补修复核）：吸收预设切换的迟到重复批次，给恢复/切页检查
 * 取可信基线。`read()` 返回 `{ summary, events }`（汇总与明细的 query_id）。
 *
 * 判定：两者非空且相同才算有效读数；同一有效身份连续保持 `quietMs` 才返回；
 * `budgetMs` 内未稳定即抛错，错误带观察到的身份变化序列——不得把未稳定当成功。
 * 计时按累计等待间隔，不含读取本身耗时（读取为毫秒级 DOM 属性）。
 */
export async function waitStableQuery({
  read,
  sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  intervalMs = 500,
  quietMs = 2000,
  budgetMs = 10000,
}) {
  const observed = [];
  let candidate = null;
  let since = 0;
  for (let elapsed = 0; ; elapsed += intervalMs) {
    const { summary = null, events = null } = (await read()) ?? {};
    const last = observed.at(-1);
    if (!last || last.summary !== summary || last.events !== events) {
      observed.push({ at_ms: elapsed, summary, events });
    }
    const valid = !!summary && summary === events;
    if (valid && summary === candidate) {
      if (elapsed - since >= quietMs) return summary;
    } else {
      candidate = valid ? summary : null;
      since = elapsed;
    }
    if (elapsed >= budgetMs) {
      const error = new Error(
        `查询身份 ${budgetMs} ms 内未稳定（需非空、汇总与明细一致并保持 ${quietMs} ms）：${JSON.stringify(observed)}`,
      );
      error.kind = "query-unstable";
      error.observed = observed;
      throw error;
    }
    await sleep(intervalMs);
  }
}
