// Task 8：来源采集统计行与价格来源行的纯函数（可测，供组件消费）。
import { fmtNum, type PricingEntry, type SourceStat, STAT_KEYS } from "../types";

/// 来源采集统计行（excluded_scope_visible）：只展示非零计数与关键基数。
export function buildSourceLines(sources: SourceStat[]): {
  agent: string;
  parts: string[];
}[] {
  return sources.map((s) => ({
    agent: s.agent,
    parts: STAT_KEYS.filter(
      (k) => (s.stats[k.key] ?? 0) > 0 || k.key === "files_scanned" || k.key === "events",
    ).map((k) => `${k.label} ${fmtNum(s.stats[k.key] ?? 0)}`),
  }));
}

/// 价格表悬浮的来源行（pricing_match_source_visible）：不完整条目必须可见。
export function priceSourceLine(e: PricingEntry): string {
  return e.incomplete
    ? `来源：${e.source}（不完整：部分分项价格未知，按 0 展示但未计费）`
    : `来源：${e.source}`;
}
