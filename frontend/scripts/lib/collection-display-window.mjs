// 全部边界来自同一 WebView 的 performance.now()；轮询值只作对照。
// failed 检查真实开始至 UI 结束；succeeded 还检查 UI 自身最短显示期。
// 二者都要求 reading 连续保持到对应终态回调处理完毕，缺证据即失败。
export function collectionDisplayWindow({ runId, callbacks, transitions, phase = "failed" }) {
  const valid = (records) =>
    Array.isArray(records) &&
    records.length > 0 &&
    records.every(
      (s, i) => Number.isFinite(s.at_ms) && (i === 0 || s.at_ms >= records[i - 1].at_ms),
    );
  // query 的 at_ms 是注册时刻、记录却在 resolve 时追加，不能拿它校验
  // 状态回调的顺序；状态 enter/exit 才是本断言的直接证据。
  const statusCallbacks = Array.isArray(callbacks)
    ? callbacks.filter((s) => s.kind === "status-enter" || s.kind === "status-exit")
    : [];
  const tracesValid = valid(statusCallbacks) && valid(transitions);
  const start = statusCallbacks.find(
    (s) => s.kind === "status-enter" && s.phase === "running" && s.runId === runId,
  );
  const terminal = statusCallbacks.find(
    (s) => s.kind === "status-enter" && s.phase === phase && s.runId === runId,
  );
  const terminalExit = statusCallbacks.find(
    (s) => s.kind === "status-exit" && s.phase === phase && s.runId === runId,
  );
  const readingIndex =
    transitions?.findIndex?.((s) => s.state === "reading" && s.runId === runId) ?? -1;
  const reading = readingIndex >= 0 ? transitions[readingIndex] : null;
  // 首次离开 reading 就是结束，不能跳过中途消失、串轮次或提前停下。
  const end =
    readingIndex >= 0
      ? transitions.slice(readingIndex + 1).find((s) => s.state !== "reading" || s.runId !== runId)
      : null;
  const displayMs = reading && end ? end.at_ms - reading.at_ms : null;
  const backendElapsedMs = start && end ? end.at_ms - start.at_ms : null;
  const endAfterCallbackMs = terminalExit && end ? end.at_ms - terminalExit.at_ms : null;
  const ok = Boolean(
    tracesValid &&
    Number.isInteger(runId) &&
    runId > 0 &&
    ["failed", "succeeded"].includes(phase) &&
    start &&
    terminal &&
    terminalExit &&
    reading &&
    !reading.initial &&
    end &&
    end.runId === runId &&
    end.state === (phase === "failed" ? "failed" : "confirmed") &&
    reading.at_ms >= start.at_ms &&
    reading.at_ms <= terminal.at_ms &&
    terminal.at_ms >= start.at_ms &&
    terminalExit.at_ms >= terminal.at_ms &&
    endAfterCallbackMs >= 0 &&
    (phase === "failed" ? backendElapsedMs : displayMs) >= 2300,
  );
  const basis =
    `页面单调时钟：runId=${runId}；真实开始=${start?.at_ms ?? "缺失"}ms；` +
    `${phase}回调进入/结束=${terminal?.at_ms ?? "缺失"}/${terminalExit?.at_ms ?? "缺失"}ms；` +
    `reading开始/结束=${reading?.at_ms ?? "缺失"}/${end?.at_ms ?? "缺失"}ms；` +
    `UI显示=${displayMs ?? "缺失"}ms；真实开始至UI结束=${backendElapsedMs ?? "缺失"}ms；` +
    `UI结束晚于回调=${endAfterCallbackMs ?? "缺失"}ms；` +
    `阈值2300ms（${phase === "failed" ? "真实开始至UI结束" : "UI显示"}）；证据完整且成立=${ok}`;
  return {
    ok,
    basis,
    displayMs,
    backendElapsedMs,
    endAfterCallbackMs,
    start,
    terminal,
    terminalExit,
    reading,
    end,
  };
}

export function legacyCollectionWindow(timeline, runId) {
  const startIndex = timeline.findIndex((s) => s.runId === runId && s.state === "reading");
  const endIndex =
    startIndex < 0
      ? -1
      : timeline.findIndex((s, i) => i > startIndex && s.runId === runId && s.state !== "reading");
  const start = timeline[startIndex];
  const end = timeline[endIndex];
  const beforeStart = timeline[startIndex - 1];
  const beforeEnd = timeline[endIndex - 1];
  return {
    observedDisplayMs: start && end ? end.at_ms - start.at_ms : null,
    pageObservedDisplayMs: start && end ? end.page_ms - start.page_ms : null,
    displayLowerBoundMs: start && end && beforeEnd ? beforeEnd.page_ms - start.page_ms : null,
    displayUpperBoundMs: start && end && beforeStart ? end.page_ms - beforeStart.page_ms : null,
  };
}
