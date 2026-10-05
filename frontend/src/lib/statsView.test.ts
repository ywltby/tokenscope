// Task 8：pricing_match_source_visible / excluded_scope_visible 的纯函数回归。
import { describe, expect, it } from "vitest";
import { buildSourceLines, priceSourceLine } from "./statsView";
import type { PricingEntry, SourceStat } from "../types";

describe("priceSourceLine（pricing_match_source_visible）", () => {
  it("完整条目只显示来源", () => {
    const e = {
      prefix: "x",
      input: 1,
      output: 2,
      cache_write: 0,
      cache_read: 0,
      incomplete: false,
      source: "内置",
    } as PricingEntry;
    expect(priceSourceLine(e)).toBe("来源：内置");
  });

  it("不完整条目必须可见（部分分项价未知按 0 展示但未计费）", () => {
    const e = {
      prefix: "x",
      input: 1,
      output: 2,
      cache_write: 0,
      cache_read: 0,
      incomplete: true,
      source: "models.dev",
    } as PricingEntry;
    const line = priceSourceLine(e);
    expect(line).toContain("来源：models.dev");
    expect(line).toContain("不完整");
  });
});

describe("buildSourceLines（excluded_scope_visible）", () => {
  it("非零排除项与读取失败必须出现在来源统计行", () => {
    const sources: SourceStat[] = [
      {
        agent: "claude-code",
        stats: {
          files_scanned: 3,
          lines_seen: 100,
          events: 90,
          duplicates_dropped: 5,
          bad_lines: 2,
          skipped_sidechain: 1,
          skipped_synthetic: 1,
          skipped_zero_usage: 1,
          skipped_no_model: 0,
          ignored_token_usage_record: 0,
          io_errors: 2,
        },
      },
    ];
    const lines = buildSourceLines(sources);
    expect(lines).toHaveLength(1);
    const joined = lines[0].parts.join(" · ");
    expect(joined).toContain("去重丢弃 5");
    expect(joined).toContain("坏行 2");
    expect(joined).toContain("读取失败 2");
    // 零值不展示（除文件/事件基数外）
    expect(joined).not.toContain("跳过无模型");
  });
});
