/**
 * 查询错误分类（RC02）：后端 `Result<_, String>` 的错误串以结构化 **code**
 * 前缀表达失败类别，这里集中判定，避免在组件里散落中文字符串匹配。
 *
 * 目前只有一个需要区分的 code：
 * - `query_expired`：查询会话不存在 / 被淘汰 / 跨进程失效 / 游标不属于当前
 *   会话或指纹不匹配。语义是「当前批次已死」——恢复必须**新建批次**，
 *   不能复用原 beginPromise 重发同一会话。
 *
 * 注意：**不**把未识别的失败一律当成过期（那会让普通请求失败也触发整批
 * 重查，丢掉局部重试的能力）。只有显式携带该 code 的错误才按过期处理。
 */
export const QUERY_EXPIRED_CODE = "query_expired";

/** 统一取错误文本（Error / string / 其他）。 */
export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (e == null) return "";
  return String(e);
}

/**
 * 是否「会话已过期」类错误。按 code 词边界匹配（不区分大小写）：既能命中
 * `query_expired: ...` 前缀，也能命中被 anyhow 上下文包裹后出现在串中段的
 * 同一 code；不会把普通失败误判为过期。
 */
export function isQueryExpired(e: unknown): boolean {
  const msg = errorText(e);
  if (!msg) return false;
  return new RegExp(`(^|[^a-z_])${QUERY_EXPIRED_CODE}([^a-z_]|$)`, "i").test(msg);
}
