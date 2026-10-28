// P04：政策渲染器的确定性测试（离线、无 HTML、无外部资源）。
import { describe, expect, it } from "vitest";
import { inlineText, renderPolicyMarkdown } from "./policyMarkdown";

describe("政策渲染（policyMarkdown）", () => {
  it("renders_headings_list_and_paragraphs_in_source_order", () => {
    const blocks = renderPolicyMarkdown(
      ["# 标题", "", "## 小节", "", "- 第一项", "- 第二项", "", "普通段落。"].join("\n"),
    );
    expect(blocks).toEqual([
      { type: "h1", text: "标题" },
      { type: "h2", text: "小节" },
      { type: "ul", items: ["第一项", "第二项"] },
      { type: "p", text: "普通段落。" },
    ]);
  });

  it.each(["\n", "\r\n"])("行尾反斜杠产生段落内换行（%j）", (lineEnding) => {
    const markdown = ["更新日期：今天\\", "生效日期：明天\\", "适用范围：桌面应用"].join(
      lineEnding,
    );
    expect(renderPolicyMarkdown(markdown)).toEqual([
      { type: "p", text: "更新日期：今天\n生效日期：明天\n适用范围：桌面应用" },
    ]);
  });

  it("列表项的行尾反斜杠保留项内换行", () => {
    const markdown = ["- 第一行\\", "  续行 **重点**", "- 第二项"].join("\n");
    expect(renderPolicyMarkdown(markdown)).toEqual([
      { type: "ul", items: ["第一行\n续行 重点", "第二项"] },
    ]);
  });

  it("普通换行与转义反斜杠不产生硬换行", () => {
    const markdown = ["普通第一行", "普通第二行", "路径\\\\", "后续文字"].join("\n");
    const blocks = renderPolicyMarkdown(markdown);
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ type: "p" });
    if (blocks[0].type !== "p") throw new Error("预期段落");
    expect(blocks[0].text).not.toContain("\n");
    expect(blocks[0].text).toContain("路径\\");
    expect(blocks[0].text).toContain("后续文字");
  });

  it("段落结束处和反斜杠后有空格时不误判为硬换行", () => {
    expect(renderPolicyMarkdown("段尾\\\n\n下一段")).toEqual([
      { type: "p", text: "段尾\\" },
      { type: "p", text: "下一段" },
    ]);
    expect(renderPolicyMarkdown("有空格\\ \n续行")).toEqual([{ type: "p", text: "有空格\\ 续行" }]);
  });

  it("跨行行内代码中的反斜杠保持为文本", () => {
    const markdown = ["路径 `C:\\", "目录`"].join("\n");
    expect(renderPolicyMarkdown(markdown)).toEqual([{ type: "p", text: "路径 C:\\ 目录" }]);
  });

  it("跨硬换行的加粗文本仍去除标记", () => {
    const markdown = ["**请注意\\", "以下内容**"].join("\n");
    expect(renderPolicyMarkdown(markdown)).toEqual([{ type: "p", text: "请注意\n以下内容" }]);
  });

  it("inline_markup_is_flattened_without_html", () => {
    const blocks = renderPolicyMarkdown(
      "**请特别注意：**这是 `代码`，参见 [models.dev](https://models.dev/)。",
    );
    expect(blocks).toEqual([
      {
        type: "p",
        text: "请特别注意：这是 代码，参见 models.dev（https://models.dev/）。",
      },
    ]);
    // HTML-ish 输入按纯文本原样保留（渲染层用插值，不做 HTML 解析）
    const raw = "<script>alert(1)</script>";
    expect(renderPolicyMarkdown(raw)).toEqual([{ type: "p", text: raw }]);
  });

  it("consent_dialog_renders_text_without_raw_html_or_remote_resources", () => {
    // 组件源码层面的静态证据：只做插值渲染（无 v-html / 外链 / 远程资源）。
    // （源码扫描由 frontend/scripts/check-ui-contracts.mjs 的静态契约承担，
    //  这里的断言保证渲染器本身不会把标记还原成 HTML。）
    const blocks = renderPolicyMarkdown("<b>粗体</b> [x](https://example.com/a)");
    const text = blocks.map((b) => (b.type === "ul" ? b.items.join("") : b.text)).join("");
    expect(text).toContain("<b>粗体</b>");
    expect(text).not.toContain("<a href");
    expect(text).toContain("x（https://example.com/a）");
  });

  it("inline_text_keeps_plain_text_untouched", () => {
    expect(inlineText("更新日期：2026 年 10 月 9 日")).toBe("更新日期：2026 年 10 月 9 日");
    expect(inlineText("普通 **加粗** 文本")).toBe("普通 加粗 文本");
  });

  it("empty_document_renders_nothing", () => {
    expect(renderPolicyMarkdown("")).toEqual([]);
    expect(renderPolicyMarkdown("\n\n")).toEqual([]);
  });
});
