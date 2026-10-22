// 序列化到 WebView 内运行：不依赖模块闭包、不改 DOM、不加轮询。
// 观察稳定祖先，胶囊替换后也继续记录；MutationObserver 在 DOM 提交后的
// 微任务检查点取 performance.now()，不再混入 CDP/100ms 轮询的往返延迟。
export function observeCollectionTransitions(document, now, MutationObserver) {
  const records = [];
  let previous;
  let previousPill;
  const record = (initial = false) => {
    const pill = document.querySelector(".ts-status-pill");
    const state = pill?.getAttribute("data-collection-state") ?? null;
    const rawRun = pill?.getAttribute("data-collection-run-id");
    const runId = rawRun ? Number(rawRun) : null;
    if (
      !previous ||
      pill !== previousPill ||
      previous.state !== state ||
      previous.runId !== runId
    ) {
      const entry = { at_ms: now(), state, runId, initial };
      records.push(entry);
      previous = entry;
      previousPill = pill;
    }
  };
  const observer = new MutationObserver(() => record());
  observer.observe(document.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["data-collection-state", "data-collection-run-id"],
  });
  record(true);
  return {
    read: () => records.map((s) => ({ ...s })),
    dispose: () => observer.disconnect(),
  };
}
