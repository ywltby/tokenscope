// 隐私弹窗的协议入口（十九轮复核 P2）：政策引用 terms.md，同意前必须能在
// 弹窗内离线阅读协议全文——内容来自后端 PolicyInfo.terms（编译进二进制），
// 不依赖安装资源路径、不产生可点击外链。
import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";

vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const NModalStub = dc({
    name: "NModal",
    props: { show: { type: Boolean, default: false } },
    setup(props, { slots }) {
      return () => (props.show ? h("div", { class: "modal-stub" }, slots.default?.()) : null);
    },
  });
  const NButtonStub = dc({
    name: "NButton",
    setup(_, { slots, attrs }) {
      return () => h("button", attrs, slots.default?.());
    },
  });
  return { ...actual, NModal: NModalStub, NButton: NButtonStub };
});

import PrivacyConsentDialog from "./PrivacyConsentDialog.vue";

const POLICY = {
  title: "TokenScope 隐私政策",
  date: "2026 年 10 月 28 日",
  markdown: "# TokenScope 隐私政策\n\n正文示例。关于许可请参阅独立的 [协议](terms.md)。\n",
  terms: "# TokenScope 最终用户许可与免责协议\n\n## 一、许可\n\n本协议正文示例。\n",
};

function mountDialog(policy: typeof POLICY | Omit<typeof POLICY, "terms">) {
  return mount(PrivacyConsentDialog, {
    props: {
      open: true,
      policy,
      saving: false,
      blocked: false,
      message: null,
      updateNotice: null,
    },
  });
}

describe("隐私弹窗：协议离线入口（十九轮 P2）", () => {
  it("有协议全文时提供展开入口，展开后正文可见", async () => {
    const wrapper = mountDialog(POLICY);
    const toggle = wrapper.find(".privacy-terms-toggle");
    expect(toggle.exists()).toBe(true);
    expect(toggle.text()).toContain("查看最终用户许可协议");
    expect(wrapper.find(".privacy-terms").exists()).toBe(false);
    await toggle.trigger("click");
    const region = wrapper.find(".privacy-terms");
    expect(region.exists()).toBe(true);
    expect(region.text()).toContain("最终用户许可与免责协议");
    expect(region.text()).toContain("本协议正文示例");
    expect(toggle.text()).toContain("收起用户许可协议");
  });

  it("协议区硬换行不被折叠为同一段落（br 分段）", async () => {
    // 二十轮复核 P3：协议区直接输出文本，浏览器把换行当普通空白折叠，
    // 「更新日期 / 生效日期 / 适用范围」挤在同一段；政策区已正确分行。
    const { terms: _t, ...rest } = POLICY;
    const lines = [
      "# 协议",
      "",
      "更新日期：2026 年 10 月 28 日\\",
      "生效日期：2026 年 10 月 28 日\\",
      "适用范围：示例。",
      "",
    ];
    const wrapper = mountDialog({ ...rest, terms: lines.join("\n") });
    await wrapper.find(".privacy-terms-toggle").trigger("click");
    const paragraph = wrapper.findAll(".privacy-terms p").at(-1);
    expect(paragraph).toBeTruthy();
    expect(paragraph!.findAll("br").length).toBe(2);
  });

  it("协议内容只作为文本渲染：不产生可点击外链", async () => {
    const wrapper = mountDialog(POLICY);
    await wrapper.find(".privacy-terms-toggle").trigger("click");
    expect(wrapper.find(".privacy-terms a").exists()).toBe(false);
    expect(wrapper.html()).not.toContain("href=");
  });

  it("后端未提供协议全文时不显示入口（不伪造空协议）", () => {
    const { terms: _omit, ...withoutTerms } = POLICY;
    const wrapper = mountDialog(withoutTerms);
    expect(wrapper.find(".privacy-terms-toggle").exists()).toBe(false);
    expect(wrapper.find(".privacy-terms").exists()).toBe(false);
  });
});
