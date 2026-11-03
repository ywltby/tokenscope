// @vitest-environment node
// 政策由 Rust include_str! 打包，验证仓库政策正文的实际换行。
import fs from "node:fs";
import { expect, it } from "vitest";
import { renderPolicyMarkdown } from "../../src/lib/policyMarkdown.ts";

it("实际隐私政策的日期与适用范围按行渲染", () => {
  const markdown = fs.readFileSync(new URL("../../../docs/privacy.md", import.meta.url), "utf8");
  const lines = markdown.split(/\r?\n/);
  const dateIndex = lines.findIndex((line) => line.startsWith("更新日期："));
  expect(dateIndex).toBeGreaterThanOrEqual(0);
  expect(lines[dateIndex]).toMatch(/\\$/);
  expect(lines[dateIndex + 1]).toMatch(/^生效日期：.*\\$/);
  expect(lines[dateIndex + 2]).toMatch(/^适用范围：/);

  const expected = [
    lines[dateIndex].slice(0, -1),
    lines[dateIndex + 1].slice(0, -1),
    lines[dateIndex + 2],
  ].join("\n");
  const firstParagraph = renderPolicyMarkdown(markdown).find((block) => block.type === "p");
  expect(firstParagraph).toEqual({ type: "p", text: expected });
});
