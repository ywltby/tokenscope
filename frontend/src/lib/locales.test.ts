// 翻译资源的结构约束：zh-CN 与 en-US 必须同构（同键、同占位符、无空值）。
//
// 这是"以后增加语言也不漏键"的守卫：`AppMessages` 类型只约束编译期能看见
// 的字面量键，跨文件的动态拼接（`t(SECTION_LABEL_KEY[id])`）与运行期插值
// 仍需要这层断言。占位符必须逐键一致——否则某个语言会静默丢掉数字/名称。
import { describe, expect, it } from "vitest";
import zhCN from "./locales/zh-CN";
import enUS from "./locales/en-US";

type Catalog = { [key: string]: string | Catalog };

function flatten(prefix: string, value: Catalog, out: Map<string, string>): void {
  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof child === "string") out.set(path, child);
    else flatten(path, child as Catalog, out);
  }
}

const zh = new Map<string, string>();
flatten("", zhCN as unknown as Catalog, zh);
const en = new Map<string, string>();
flatten("", enUS as unknown as Catalog, en);

/** `{name}` 形式的插值占位符（vue-i18n 具名插值）。 */
function placeholders(text: string): string[] {
  return [...text.matchAll(/\{([A-Za-z0-9_]+)\}/g)].map((m) => m[1]).sort();
}

describe("翻译资源结构（zh-CN / en-US）", () => {
  it("both_locales_define_the_same_keys：两种语言的键集合完全一致", () => {
    const missingInEn = [...zh.keys()].filter((k) => !en.has(k)).sort();
    const missingInZh = [...en.keys()].filter((k) => !zh.has(k)).sort();
    expect(missingInEn, "en-US 缺失的键").toEqual([]);
    expect(missingInZh, "zh-CN 缺失的键").toEqual([]);
  });

  it("placeholders_match_per_key：每个键的插值占位符两侧一致", () => {
    const mismatched: string[] = [];
    for (const [key, zhText] of zh) {
      const enText = en.get(key);
      if (enText === undefined) continue;
      if (placeholders(zhText).join(",") !== placeholders(enText).join(",")) {
        mismatched.push(`${key}: zh=[${placeholders(zhText)}] en=[${placeholders(enText)}]`);
      }
    }
    expect(mismatched, "占位符不一致会导致某个语言丢参数").toEqual([]);
  });

  it("no_empty_translations：不允许空字符串（漏译必须显式写出原文）", () => {
    const empties = [...zh.entries(), ...en.entries()]
      .filter(([, text]) => text.trim() === "")
      .map(([key]) => key);
    expect(empties).toEqual([]);
  });

  it("en_us_differs_from_zh_cn_for_ui_copy：英文不是中文占位（同一键必须真的换语言）", () => {
    // 品牌/技术标识允许相同（如 "models.dev"、"TokenScope"），但界面文案必须不同。
    const identical: string[] = [];
    for (const [key, zhText] of zh) {
      const enText = en.get(key);
      if (enText === zhText && /[\u4e00-\u9fa5]/.test(zhText)) identical.push(key);
    }
    expect(identical, "含中文的键在英文包里仍是中文原文").toEqual([]);
  });

  it("critical_namespaces_are_present：主界面/设置/提示/托盘等必备命名空间存在", () => {
    for (const namespace of [
      "app",
      "consent",
      "close",
      "dashboard",
      "pricing",
      "settings",
      "range",
      "status",
      "cost",
      "tiered",
      "tz",
      "buckets",
    ]) {
      expect(
        [...zh.keys()].some((k) => k.startsWith(`${namespace}.`)),
        namespace,
      ).toBe(true);
    }
    // 托盘菜单由后端渲染，但文案清单必须在前端资源里有对应项（同一口径）
    expect(zh.get("app.trayShow")).toBe("显示主窗口");
    expect(en.get("app.trayShow")).toBe("Show main window");
  });
});
