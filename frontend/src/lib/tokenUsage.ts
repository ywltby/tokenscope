import type { TokenCounts } from "../types";

/** IPC 保留互斥计费桶；所有用量展示的输入统一包含两类缓存。 */
export function totalInput(
  tokens: Pick<TokenCounts, "input" | "cache_write" | "cache_read">,
): number {
  return tokens.input + tokens.cache_write + tokens.cache_read;
}

export function displayTokens(tokens: TokenCounts): TokenCounts {
  return { ...tokens, input: totalInput(tokens) };
}

export function totalTokens(tokens: TokenCounts): number {
  return totalInput(tokens) + tokens.output;
}
