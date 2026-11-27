import type { EventRow } from "../types";

/**
 * 跨查询刷新稳定的请求明细行标识（与会话绑定的分页游标 `row.cursor` 解耦）：
 * - 优先使用后端下发的 `row.row_id`（原生请求 = `event:<event_id>`，
 *   CCS 日汇总 = `rollup:[day,model_key,source_tz,app]`），不随 query_id、页内位置或 ridx 变化；
 * - 兼容旧版缓存快照或极简测试行未携带 `row_id` 的情形：日汇总按四元组构造，
 *   原生请求尝试从 v2 JSON 游标剥离会话字段（qid/mfp/dfp），非 JSON 游标时回退原串。
 */
export function stableEventRowKey(row: EventRow): string {
  if (row.row_id) return row.row_id;
  if (row.rollup) {
    return `rollup:${JSON.stringify([row.rollup.day, row.model, row.rollup.source_tz, row.agent])}`;
  }
  if (row.cursor.startsWith("{")) {
    try {
      const parsed = JSON.parse(row.cursor) as {
        kind?: string;
        ts?: string;
        rid?: string;
        seq?: number;
      };
      if (
        parsed &&
        parsed.kind !== "rollup" &&
        typeof parsed.ts === "string" &&
        typeof parsed.rid === "string" &&
        typeof parsed.seq === "number"
      ) {
        return `cursor-event:${JSON.stringify([parsed.ts, parsed.rid, parsed.seq])}`;
      }
    } catch {
      // 非 JSON 游标，回退原串
    }
  }
  return row.cursor;
}
