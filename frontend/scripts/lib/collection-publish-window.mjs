// 原生验收只观察真实 IPC 回调，不改事件、计时器或查询调度。
// Tauri 2 的 callbacks 是其公开用于调试的 Map；接口不兼容时明确失败。
// 此函数会序列化到 WebView 执行，必须保持无模块外部依赖。
export function observeCollectionCallbacks(internals, now) {
  const callbacks = internals.callbacks;
  if (!(callbacks instanceof Map)) throw new Error("Tauri callback map unavailable");
  const records = [];
  const wrappers = new Map();
  const originalSet = callbacks.set;
  const ownSet = Object.getOwnPropertyDescriptor(callbacks, "set");
  function wrap(id, callback, registeredAt) {
    const wrapped = function (...args) {
      const value = args[0];
      const status = value?.event === "collection-status" ? value.payload : null;
      if (status) records.push({ kind: "status-enter", at_ms: now(), ...status });
      // query_begin 的成功响应提供身份；时间取其 resolve 回调注册时刻，
      // 就是 invoke 同步发起时刻，不把后端响应耗时算作前端延迟。
      if (
        registeredAt !== null &&
        typeof value?.queryId === "string" &&
        typeof value?.generation === "number"
      ) {
        records.push({
          kind: "query",
          at_ms: registeredAt,
          resolved_ms: now(),
          queryId: value.queryId,
          generation: value.generation,
        });
      }
      const result = callback.apply(this, args);
      if (status) records.push({ kind: "status-exit", at_ms: now(), ...status });
      return result;
    };
    wrappers.set(id, { callback, wrapped });
    originalSet.call(callbacks, id, wrapped);
  }
  for (const [id, callback] of callbacks) wrap(id, callback, null);
  callbacks.set = function (id, callback) {
    wrap(id, callback, now());
    return this;
  };
  return {
    read: () => records.slice(),
    dispose() {
      if (ownSet) Object.defineProperty(callbacks, "set", ownSet);
      else delete callbacks.set;
      for (const [id, { callback, wrapped }] of wrappers) {
        if (callbacks.get(id) === wrapped) originalSet.call(callbacks, id, callback);
      }
    },
  };
}

// 检查名为兼容历史证据保留，但判定不再假设采集能在 1 秒内完成。
// 当前可见、未翻页、无在途批次：真实终态回调处理结束之前必须已发起
// 最终落地的 query_begin。快/慢轮次一视同仁；缺证据直接失败。
export function queryPublishesDuringReading({ timeline, baseline, trace = [] }) {
  const publish = timeline.find((s) => s.requests === baseline.requests + 1) ?? null;
  const start = trace.find(
    (s) => s.kind === "status-enter" && s.phase === "running" && s.runId === publish?.runId,
  );
  const end = trace.find(
    (s) =>
      s.kind === "status-exit" &&
      s.phase === "succeeded" &&
      s.runId === publish?.runId &&
      s.latestGeneration === publish?.generation,
  );
  const query = trace.find(
    (s) =>
      s.kind === "query" &&
      s.queryId === publish?.summaryQuery &&
      s.generation === publish?.generation,
  );
  const ok = Boolean(
    publish &&
    start &&
    end &&
    query &&
    publish.summaryQuery === publish.eventsQuery &&
    publish.summaryQuery !== baseline.summaryQuery &&
    publish.generation > baseline.generation &&
    query.at_ms >= start.at_ms &&
    query.at_ms <= end.at_ms,
  );
  const basis =
    `页面单调时钟：读取开始=${start?.at_ms ?? "缺失"}ms；` +
    `终态处理结束=${end?.at_ms ?? "缺失"}ms；查询发起=${query?.at_ms ?? "缺失"}ms；` +
    `query_id=${query?.queryId ?? "缺失"}；终态处理结束前已发起对应查询=${ok}`;
  return {
    ok,
    basis,
    publish,
    readingStartMs: start?.at_ms ?? null,
    queryStartMs: query?.at_ms ?? null,
  };
}
