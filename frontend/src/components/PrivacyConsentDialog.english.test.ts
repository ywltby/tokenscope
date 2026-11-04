// 英文界面下的隐私政策/协议可达性（用户要求 4）：
//
// - 政策与协议正文是简体中文原文（准据语言），英文界面**不冒充**已翻译：
//   必须显式标注实际语言与法律效力；
// - 正文仍然离线完整可读（无外链、无网络资源）；
// - 弹窗内的语言切换只改内存，不触发任何持久化（隐私门禁边界）。
import { afterEach, describe, expect, it, vi } from "vitest";
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
  // NSelect 用原生 select 替身，便于断言"切换语言"的交互路径。
  const NSelectStub = dc({
    name: "NSelect",
    props: { value: { type: String, default: "" }, options: { type: Array, default: () => [] } },
    emits: ["update:value"],
    setup(props, { emit }) {
      return () =>
        h(
          "select",
          {
            class: "select-stub",
            value: props.value,
            onChange: (e: Event) => emit("update:value", (e.target as HTMLSelectElement).value),
          },
          (props.options as { label: string; value: string }[]).map((o) =>
            h("option", { value: o.value }, o.label),
          ),
        );
    },
  });
  return { ...actual, NModal: NModalStub, NButton: NButtonStub, NSelect: NSelectStub };
});

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import PrivacyConsentDialog from "./PrivacyConsentDialog.vue";
import { DEFAULT_LOCALE, normalizeLocale, setLocale, t } from "../lib/i18n";

const POLICY = {
  title: "TokenScope 隐私政策",
  date: "2026 年 10 月 28 日",
  markdown:
    "# TokenScope 隐私政策\n\n更新日期：2026 年 10 月 28 日\\\n\n## 一、适用范围\n\n本政策正文示例。\n",
  terms: "# TokenScope 最终用户许可与免责协议\n\n## 一、许可\n\n本协议正文示例。\n",
};

function mountDialog() {
  return mount(PrivacyConsentDialog, {
    props: {
      open: true,
      policy: POLICY,
      saving: false,
      blocked: false,
      message: null,
      updateNotice: null,
    },
  });
}

afterEach(() => {
  setLocale(DEFAULT_LOCALE);
  vi.restoreAllMocks();
});

describe("英文界面的隐私政策与协议（用户要求 4）", () => {
  it("english_ui_keeps_governing_chinese_text_and_labels_language：正文保持中文原文并显式标注", () => {
    setLocale("en-US");
    const w = mountDialog();
    // 界面文案已切换
    expect(w.text()).toContain("Agree and continue");
    // 正文仍是准据语言原文（不冒充英文翻译）
    expect(w.find(".privacy-scroll").text()).toContain("本政策正文示例");
    // 显式语言与法律效力标注
    const note = w.find(".privacy-lang-note");
    expect(note.exists()).toBe(true);
    expect(note.text()).toContain("Simplified Chinese");
    expect(note.text()).toContain("governing");
  });

  it("chinese_ui_has_no_language_note：中文界面不出现多余的语言标注", () => {
    setLocale("zh-CN");
    const w = mountDialog();
    expect(w.find(".privacy-lang-note").exists()).toBe(false);
  });

  it("policy_and_terms_stay_readable_offline_in_both_locales：两语言下都能离线读全文且无外链", async () => {
    for (const locale of ["zh-CN", "en-US"] as const) {
      setLocale(locale);
      const w = mountDialog();
      expect(w.find(".privacy-scroll").text()).toContain("一、适用范围");
      await w.find(".privacy-terms-toggle").trigger("click");
      expect(w.find(".privacy-terms").text()).toContain("本协议正文示例");
      expect(w.html()).not.toContain("href=");
      expect(w.html()).not.toContain("http://");
      w.unmount();
    }
  });

  it("dialog_locale_switch_does_not_persist：弹窗内切换语言不触发任何 IPC", async () => {
    const calls: string[] = [];
    invokeMock.mockImplementation((cmd: string) => {
      calls.push(cmd);
      return Promise.resolve(null);
    });
    setLocale("zh-CN");
    const w = mountDialog();
    const select = w.find("select.select-stub");
    await select.setValue("en-US");
    expect(normalizeLocale("en-US")).toBe("en-US");
    // 界面切到英文，但没有任何持久化调用（保存只发生在同意成功后）
    expect(w.text()).toContain("Agree and continue");
    expect(calls).toEqual([]);
  });

  it("accept_button_labels_come_from_catalog：按钮与提示文案来自资源而不是硬编码", () => {
    setLocale("en-US");
    const w = mountDialog();
    expect(w.text()).toContain(t("consent.reject"));
    expect(w.text()).toContain(t("consent.termsShow"));
    expect(w.text()).toContain(t("app.languageAria"));
  });
});
