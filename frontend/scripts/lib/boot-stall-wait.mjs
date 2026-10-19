// BS04 复核补修：迟到 bootstrap 响应的落定等待——契约断言「代次保护」
// 的前提是旧响应真的送达 Node 侧；响应未落定就断言「状态没退回」是
// 误通过路径（界面当然不会退回）。等待超时必须让契约失败。
// 纯函数化（注入 sleep/now）以便单测；契约侧传入 Node 真实 sleep。

/**
 * 等待一条 IPC 调用在 Node 侧落定（outcome 已写入）。
 * @param {{outcome?: string}} call Node 侧 IPC 记录（exposeFunction 包装在
 *   fixture 返回后写入 outcome；挂起的调用没有 outcome）。
 * @param {{timeoutMs?: number, intervalMs?: number, sleep?: (ms: number) => Promise<void>, now?: () => number}} [opts]
 * @returns {Promise<string>} 落定的 outcome（"ok" | "reject" | "error"）。
 * @throws {Error} 超时仍未落定——响应未送达或 fixture 未释放，此时断言
 *   「状态没退回」不能证明代次保护，调用方（契约）必须失败。
 */
export async function waitForIpcSettled(call, opts = {}) {
  const timeoutMs = opts.timeoutMs ?? 5000;
  const intervalMs = opts.intervalMs ?? 50;
  const sleep = opts.sleep ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
  const now = opts.now ?? (() => Date.now());
  const deadline = now() + timeoutMs;
  while (call.outcome === undefined) {
    if (now() >= deadline) {
      throw new Error(
        `迟到的 bootstrap 响应 ${timeoutMs}ms 内未在 Node 侧落定（outcome 恒缺）——` +
          "响应未送达或 fixture 未释放，无法证明代次保护，契约失败",
      );
    }
    await sleep(intervalMs);
  }
  return call.outcome;
}
