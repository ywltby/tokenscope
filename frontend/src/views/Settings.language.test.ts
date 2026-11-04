// 设置页与主应用壳的语言接线（不只是 lib 层）：
// 切换语言后真实组件渲染的文案、无障碍名称与保存调用一起更新。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const passthrough = (name: string) =>
    dc({
      name,
      setup(_, { slots }) {
        return () => h("div", { class: `stub-${name}` }, slots.default?.());
      },
    });
  return {
    ...actual,
    NMessageProvider: passthrough("NMessageProvider"),
    NConfigProvider: passthrough("NConfigProvider"),
    NModal: passthrough("NModal"),
    // Settings 用 useMessage 弹保存结果；测试不需要真实 provider。
    useMessage: () => ({ success: vi.fn(), error: vi.fn(), info: vi.fn(), warning: vi.fn() }),
  };
});

vi.mock("../components/AgentIcon.vue", () => ({
  default: defineComponent({ name: "AgentIcon", setup: () => () => h("i") }),
}));

import Settings from "./Settings.vue";
import { DEFAULT_LOCALE, adoptPersisted, changeLocale, t } from "../lib/i18n";

function mockInvoke(handlers: Record<string, () => unknown> = {}): string[] {
  const calls: string[] = [];
  invokeMock.mockImplementation((cmd: string) => {
    calls.push(cmd);
    const handler = handlers[cmd];
    if (handler) return Promise.resolve(handler());
    switch (cmd) {
      case "settings_get":
        return Promise.resolve({ language: "zh-CN", price_auto_sync: true, sources: {} });
      case "source_paths_list":
        return Promise.resolve([]);
      case "cache_stats":
      case "autostart_status":
      case "pricing_entries":
      case "startup_diagnostics":
      case "pricing_status":
        return Promise.resolve(null);
      case "ccs_import_defaults":
        return Promise.resolve({ path: "", exists: false, timezone: "local" });
      default:
        return Promise.resolve(null);
    }
  });
  return calls;
}

beforeEach(() => {
  invokeMock.mockReset();
});

afterEach(async () => {
  // 语言与已落盘值是模块级状态：先复位并等保存落地，避免用例互相污染。
  changeLocale(DEFAULT_LOCALE);
  await flushPromises();
  vi.restoreAllMocks();
});

describe("设置页语言行（i18n）", () => {
  it("language_row_renders_and_switch_updates_whole_page：语言行存在，切换后整页文案变英文", async () => {
    mockInvoke();
    const w = mount(Settings, { props: { refreshKey: 0 } });
    await flushPromises();
    // 中文（默认）：通用分区标题与语言行标签
    expect(w.text()).toContain(t("settings.sectionGeneral"));
    expect(w.text()).toContain(t("settings.language"));

    changeLocale("en-US");
    await flushPromises();
    expect(w.text()).toContain("General");
    expect(w.text()).toContain("Interface language");
    // 分区导航同样跟随（同一资源键）
    expect(w.text()).toContain("Appearance");
    expect(w.text()).not.toContain("外观");
  });

  it("switch_persists_through_backend_command：切换调用 settings_set_language 且只调一次", async () => {
    const calls = mockInvoke({ settings_set_language: () => "en-US" });
    const w = mount(Settings, { props: { refreshKey: 0 } });
    await flushPromises();
    changeLocale("en-US");
    await flushPromises();
    expect(calls.filter((c) => c === "settings_set_language")).toHaveLength(1);
    expect(invokeMock).toHaveBeenCalledWith("settings_set_language", { language: "en-US" });
    w.unmount();
  });

  it("save_failure_shows_reason_in_language_row：保存失败在语言行显示原因并回滚", async () => {
    mockInvoke({
      settings_set_language: () => {
        throw new Error("设置文件只读");
      },
    });
    // 先建立确定的已落盘基線（zh-CN），再验证失败回滚。
    await adoptPersisted();
    const w = mount(Settings, { props: { refreshKey: 0 } });
    await flushPromises();
    changeLocale("en-US");
    await flushPromises();
    // 回滚到中文，并在语言行给出原因（不是把失败藏在控制台）
    expect(w.text()).toContain("设置文件只读");
    expect(w.text()).toContain("界面语言");
    w.unmount();
  });
});
