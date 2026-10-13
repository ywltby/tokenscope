import { describe, expect, it } from "vitest";

// C03（计划 2026-10-13 控件材质 Task C03）：
// 按钮变体白名单契约（DESIGN.md §5 / 计划 §4.3）：
// 应用代码里的 NButton 只允许四种形态：
// - 默认（次按钮）
// - type="primary"
// - type="error"
// - text
// 禁止使用 secondary, tertiary, quaternary, ghost, dashed, color=, text-color=, :bordered="false", bordered="false"。
// 并且 NButton 的焦点由 tokens.css 统一通过 .n-button:focus-visible 提供，不挂 ts-focusable。

const vueModules = import.meta.glob<string>("../**/*.vue", {
  query: "?raw",
  import: "default",
  eager: true,
});

interface TagMatch {
  file: string;
  tag: string;
  attrs: string;
}

function getButtonTags(): TagMatch[] {
  const result: TagMatch[] = [];
  const regex = /<NButton\b([^>]*)>/g;
  for (const [file, content] of Object.entries(vueModules)) {
    let match: RegExpExecArray | null;
    while ((match = regex.exec(content)) !== null) {
      result.push({
        file,
        tag: match[0],
        attrs: match[1],
      });
    }
  }
  return result;
}

const PROHIBITED_ATTRS = [
  { name: "secondary", pattern: /\bsecondary\b/ },
  { name: "tertiary", pattern: /\btertiary\b/ },
  { name: "quaternary", pattern: /\bquaternary\b/ },
  { name: "ghost", pattern: /\bghost\b/ },
  { name: "dashed", pattern: /\bdashed\b/ },
  { name: "color=", pattern: /(?:^|\s|:)color\s*=/ },
  { name: "text-color=", pattern: /(?:^|\s|:)text-color\s*=/ },
  { name: 'bordered="false"', pattern: /(?::|v-bind:)?bordered\s*=\s*["']false["']/ },
];

const FOCUSABLE_PATTERN = /\bts-focusable\b/;

describe("controlUsage：NButton 变体白名单与类名约束 (C03)", () => {
  it("扫描到了 vue 文件与 NButton 标签", () => {
    const fileCount = Object.keys(vueModules).length;
    expect(fileCount).toBeGreaterThan(0);
    const buttons = getButtonTags();
    expect(buttons.length).toBeGreaterThan(0);
  });

  it("所有 <NButton> 标签均不包含禁止的变体属性", () => {
    const buttons = getButtonTags();
    const violations: { file: string; rule: string; tag: string }[] = [];

    for (const b of buttons) {
      for (const { name, pattern } of PROHIBITED_ATTRS) {
        if (pattern.test(b.attrs)) {
          violations.push({
            file: b.file,
            rule: name,
            tag: b.tag.trim(),
          });
        }
      }
    }

    expect(
      violations,
      `发现禁止的 NButton 变体属性:\n${violations.map((v) => `  ${v.file} [${v.rule}]:\n    ${v.tag}`).join("\n")}`,
    ).toEqual([]);
  });

  it("所有 <NButton> 标签均不包含 ts-focusable 类", () => {
    const buttons = getButtonTags();
    const violations: { file: string; tag: string }[] = [];

    for (const b of buttons) {
      if (FOCUSABLE_PATTERN.test(b.attrs)) {
        violations.push({
          file: b.file,
          tag: b.tag.trim(),
        });
      }
    }

    expect(
      violations,
      `NButton 不得挂 ts-focusable 类（焦点由 .n-button:focus-visible 统一处理）:\n${violations.map((v) => `  ${v.file}:\n    ${v.tag}`).join("\n")}`,
    ).toEqual([]);
  });
});
