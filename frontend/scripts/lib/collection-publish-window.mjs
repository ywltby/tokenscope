// query_publishes_within_reading_window 的判定（2026-10-19 修订，纯函数）。
//
// 背景：Native UI E2E 39177008549 暴露原判定"统计落地的那次取样仍是读取
// 态"的时序前提——它要求"采集写入 + 查询"在 1000ms 最短显示期内完成；
// 该运行采集 ~465ms + 查询 ~517ms ≈ 982ms，落地与显示期同时到点，取样
// 已翻到 confirmed。产品承诺的其实是"查询在读取期内已经开始"（查询不等
// 待动画、查询早结束不推迟数据发布——后者由可控时钟单测证明）。判定：
//   1. 落地前的读取期内观察到新代次查询开始（generation 前进到落地代次，
//      或胶囊显示"刷新中…"）；或
//   2. 落地取样本身仍是读取态（查询快于最短显示期的特例）。
// 只读、有界；供 native-collection-status 契约与单测共用。

/**
 * 判定新代次的查询是否在读取期内开始。
 *
 * @param timeline 按时间升序的取样（与 evidence timeline 同形：state /
 *   text / generation / requests / at_ms 等）。
 * @param baseline 追加事件前的取样（requests 为统计基线）。
 * @returns { ok, basis, publish, readingStartMs, queryStartMs }；basis 为
 *   中文依据串（读取开始、查询开始、落地时刻、落地时的状态）。
 */
export function queryPublishesDuringReading({ timeline, baseline }) {
  const index = timeline.findIndex((s) => s.requests === baseline.requests + 1);
  if (index < 0) {
    return {
      ok: false,
      basis: "时间线中没有统计落地取样（requests 未前进）",
      publish: null,
      readingStartMs: null,
      queryStartMs: null,
    };
  }
  const publish = timeline[index];
  // 本轮读取期：与落地同 runId、发生在落地之前（含落地取样本身）的读取态样本。
  const reading = timeline
    .slice(0, index + 1)
    .filter((s) => s.runId === publish.runId && s.state === "reading");
  const readingStartMs = reading.length > 0 ? reading[0].at_ms : null;
  const started = reading.find((s) => s.generation === publish.generation || s.text === "刷新中…");
  const queryStartMs = started ? started.at_ms : null;
  const ok = Boolean(started) || publish.state === "reading";
  const basis =
    `读取开始=${readingStartMs ?? "未观察到"}ms；查询开始=${queryStartMs ?? "未观察到"}ms；` +
    `落地=${publish.at_ms}ms；落地时状态=${publish.state}/${publish.text}` +
    (ok ? "" : "；读取期内未见新代次查询开始");
  return { ok, basis, publish, readingStartMs, queryStartMs };
}
