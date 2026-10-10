// 业务主应用集成测试（原 App.test.ts；P04 起主内容迁至 MainApp.vue）：
// 真实组件接线 + 结构化 needsSync 可见性（Tauri invoke 全 mock，重子组件
// 打桩，不复制 Naive UI 内部实现）。隐私引导壳自身的测试见 App.test.ts。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

// close-requested / close-action-failed 事件监听打桩：保留回调引用供测试手动触发。
const closeEvent = vi.hoisted(() => ({
  fns: [] as ((ev?: unknown) => void)[],
  byName: new Map<string, ((ev?: unknown) => void)[]>(),
  unlisten: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, cb: (ev?: unknown) => void) => {
    closeEvent.fns.push(cb);
    const list = closeEvent.byName.get(event) ?? [];
    list.push(cb);
    closeEvent.byName.set(event, list);
    return Promise.resolve(closeEvent.unlisten);
  }),
}));

// 布局类组件打桩：只验证 App 的接线（横幅 + 页面切换），不渲染 Naive UI 内部。
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
  // NModal 受 show 控制渲染（真实组件 teleport，测试不可达内容）。
  const NModalStub = dc({
    name: "NModal",
    props: { show: { type: Boolean, default: false } },
    setup(props, { slots }) {
      return () => (props.show ? h("div", { class: "stub-NModal" }, slots.default?.()) : null);
    },
  });
  return {
    ...actual,
    NLayout: passthrough("NLayout"),
    NLayoutHeader: passthrough("NLayoutHeader"),
    NLayoutContent: passthrough("NLayoutContent"),
    NMessageProvider: passthrough("NMessageProvider"),
    NConfigProvider: passthrough("NConfigProvider"),
    NModal: NModalStub,
  };
});

vi.mock("./views/Dashboard.vue", () => ({
  default: defineComponent({
    name: "Dashboard",
    props: { refreshKey: { type: Number, default: 0 } },
    setup: () => () => h("div", { class: "stub-dashboard" }),
  }),
}));
vi.mock("./views/Settings.vue", () => ({
  default: defineComponent({
    name: "Settings",
    props: { refreshKey: { type: Number, default: 0 } },
    setup: () => () => h("div", { class: "stub-settings" }),
  }),
}));

import MainApp from "./MainApp.vue";
import PricingStatusBanner from "./components/PricingStatusBanner.vue";
import { useTheme } from "./composables/theme";

const statusNeedsSync = {
  modelsdevAvailable: false,
  modelsdevCount: 0,
  modelsdevSyncedAt: null,
  openrouterAvailable: false,
  externalCount: 0,
  hasAnyPricing: false,
  needsSync: true,
  warnings: [],
};
const statusOk = { ...statusNeedsSync, needsSync: false, hasAnyPricing: true };

function mockApp(pricingStatus: object): void {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "pricing_status") return Promise.resolve(pricingStatus);
    if (cmd === "view_cache_load") return Promise.resolve(null);
    if (cmd === "summarize") return Promise.resolve({ groups: [], totals: {}, sources: [] });
    if (cmd === "list_events") return Promise.resolve({ rows: [], total: 0, warnings: [] });
    if (cmd === "source_status") return Promise.resolve([]);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  closeEvent.fns.length = 0;
  closeEvent.byName.clear();
});

/// AP07：App 现在注册两个监听（close-requested / close-action-failed），
/// 按事件名取回调，避免"最后一个注册的"歧义。
function listenerOf(event: string): (ev?: unknown) => void {
  const list = closeEvent.byName.get(event);
  if (!list?.length) throw new Error(`未注册监听: ${event}`);
  return list.at(-1)!;
}

function triggerCloseRequested(): void {
  listenerOf("close-requested")();
}

describe("App 集成（Task 4）", () => {
  it("汇总首屏已预读设置数据，切换回来复用设置实例", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    expect(w.findComponent({ name: "Settings" }).exists()).toBe(false);
    for (const command of [
      "source_status",
      "cache_stats",
      "pricing_entries",
      "settings_get",
      "autostart_status",
    ]) {
      expect(invokeMock).toHaveBeenCalledWith(command);
    }
    await w.get('[aria-label="页面切换"] [aria-label="设置"]').trigger("click");
    await flushPromises();
    const instance = w.findComponent({ name: "Settings" }).vm;
    await w.get('[aria-label="页面切换"] [aria-label="汇总"]').trigger("click");
    await w.get('[aria-label="页面切换"] [aria-label="设置"]').trigger("click");
    await flushPromises();
    expect(w.findComponent({ name: "Settings" }).vm).toBe(instance);
    w.unmount();
  });
  it("needsSync=true 时全局横幅可见", async () => {
    mockApp(statusNeedsSync);
    const w = mount(MainApp);
    await flushPromises();
    expect(w.text()).toContain("尚未获取定价");
    expect(w.findComponent(PricingStatusBanner).exists()).toBe(true);
  });

  it("needsSync=false 时不显示横幅", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    expect(w.text()).not.toContain("尚未获取定价");
    expect(w.findComponent(PricingStatusBanner).exists()).toBe(true);
  });
});

describe("App 应用壳（设计系统 Task 2，苹果风格分段控件）", () => {
  beforeEach(() => {
    localStorage.removeItem("tokenscope-theme");
  });

  it("汇总/设置分段控件可识别且选中态可见", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    const items = w.findAll('[role="radio"]');
    // 第一组是页面切换（汇总/设置），第二组是主题切换（浅色/深色/跟随系统）
    const pageItems = items.slice(0, 2);
    expect(pageItems.map((t) => t.text())).toEqual(["汇总", "设置"]);
    expect(pageItems[0].attributes("aria-checked")).toBe("true");
    await pageItems[1].trigger("click");
    await flushPromises();
    expect(pageItems[1].attributes("aria-checked")).toBe("true");
    expect(w.find(".stub-settings").exists()).toBe(true);
  });

  it("主题分段控件可读出当前偏好并驱动 data-theme", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    const groups = w.findAll('[role="radiogroup"]');
    expect(groups.length).toBeGreaterThanOrEqual(2);
    const themeGroup = groups[1]; // 第二组是主题
    const themeItems = themeGroup.findAll('[role="radio"]');
    // UX02：语义化名称 + 装饰 SVG 图标（不再是 ☀/☾ 字符）
    expect(themeItems.map((t) => t.text())).toEqual(["", "", ""]);
    expect(themeItems.map((t) => t.attributes("title"))).toEqual([
      "浅色模式",
      "深色模式",
      "跟随系统",
    ]);
    expect(themeItems.map((t) => t.attributes("aria-label"))).toEqual([
      "浅色模式",
      "深色模式",
      "跟随系统",
    ]);
    expect(themeItems.every((t) => t.find(".seg-icon").exists())).toBe(true);
    // 默认 system（自动），解析为 light
    expect(themeItems[2].attributes("aria-checked")).toBe("true");
    expect(document.documentElement.dataset.theme).toBe("light");
    await themeItems[1].trigger("click");
    await flushPromises();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("tokenscope-theme")).toBe("dark");
  });

  it("横幅渲染在内容之前（不遮挡主体）", async () => {
    mockApp(statusNeedsSync);
    const w = mount(MainApp);
    await flushPromises();
    const banner = w.find(".banner-slot").element;
    const content = w.find(".app-content").element;
    expect(
      banner.compareDocumentPosition(content) & Node.DOCUMENT_POSITION_FOLLOWING,
      "横幅必须在内容之前",
    ).toBeTruthy();
  });

  it("导航为吸顶玻璃层，品牌是线性图标 + 文字（DESIGN.md §5）", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    const nav = w.find("header.app-nav");
    expect(nav.exists()).toBe(true);
    expect(nav.classes()).toContain("ts-glass");
    const brand = nav.find(".brand");
    expect(brand.text()).toContain("TokenScope");
    // 品牌标记必须是 SVG 图标，不允许字符/emoji 充当图标
    expect(brand.find("svg").exists()).toBe(true);
  });

  it("分段组内左右方向键可切换选中项", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    // theme.ts 是模块级单例：前一用例可能把偏好改成 dark，先复位 system
    useTheme().setPreference("system");
    await flushPromises();
    const themeItems = w.findAll('[role="radiogroup"]')[1].findAll('[role="radio"]');
    expect(themeItems[2].attributes("aria-checked")).toBe("true");
    await themeItems[2].trigger("keydown", { key: "ArrowLeft" });
    await flushPromises();
    expect(themeItems[1].attributes("aria-checked")).toBe("true");
    await themeItems[1].trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    expect(themeItems[2].attributes("aria-checked")).toBe("true");
  });
});

describe("App 启动诊断（SF06：日志初始化降级非阻断通知）", () => {
  function mockWithStartup(startup: object | Promise<never>): void {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "startup_diagnostics") return startup as Promise<unknown>;
      if (cmd === "pricing_status") return Promise.resolve(statusOk);
      if (cmd === "view_cache_load") return Promise.resolve(null);
      if (cmd === "source_status") return Promise.resolve([]);
      return Promise.resolve(null);
    });
  }

  it("startup_log_degraded_shows_notice：state != ok → 横幅通知可见且不阻断页面", async () => {
    mockWithStartup(
      Promise.resolve({
        state: "stderr",
        dir: null,
        message: "日志目录创建失败: C:/x/logs（拒绝访问）",
      }),
    );
    const w = mount(MainApp);
    await flushPromises();
    expect(w.text()).toContain("文件日志不可用");
    expect(w.text()).toContain("日志目录创建失败");
    // 非阻断：汇总页照常渲染
    expect(w.find(".stub-dashboard").exists()).toBe(true);
  });

  it("startup_log_ok_shows_no_notice：state = ok 无通知", async () => {
    mockWithStartup(Promise.resolve({ state: "ok", dir: "C:/x/logs", message: null }));
    const w = mount(MainApp);
    await flushPromises();
    expect(w.text()).not.toContain("文件日志不可用");
    expect(w.find(".stub-dashboard").exists()).toBe(true);
  });

  it("startup_diagnostics 失败静默跳过（非 Tauri 环境不递归报错）", async () => {
    mockWithStartup(Promise.reject(new Error("no tauri")));
    const w = mount(MainApp);
    await flushPromises();
    expect(w.text()).not.toContain("文件日志不可用");
    expect(w.find(".stub-dashboard").exists()).toBe(true);
  });
});

describe("App 关闭确认（关闭确认与配置文件计划 Task 3）", () => {
  it("close-requested 事件打开弹窗；resolve 调 close_resolve 并关闭", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    // 初始（未收到关闭请求）不渲染弹窗内容
    expect(w.find(".stub-NModal").exists()).toBe(false);
    triggerCloseRequested();
    await flushPromises();
    const dialog = w.find(".stub-NModal");
    expect(dialog.exists()).toBe(true);
    expect(dialog.text()).toContain("关闭 TokenScope");
    // 点「最小化到托盘」→ close_resolve 参数正确，弹窗关闭
    invokeMock.mockClear();
    const minimize = dialog.findAll("button").find((b) => b.text().includes("最小化到托盘"));
    await minimize!.trigger("click");
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("close_resolve", {
      minimize: true,
      remember: false,
    });
    expect(w.find(".stub-NModal").exists()).toBe(false);
  });

  it("弹窗打开期间忽略重复 close-requested；取消不调 close_resolve", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    triggerCloseRequested();
    triggerCloseRequested();
    await flushPromises();
    expect(w.find(".stub-NModal").exists()).toBe(true);
    invokeMock.mockClear();
    const cancel = w
      .find(".stub-NModal")
      .findAll("button")
      .find((b) => b.text().includes("取消"));
    await cancel!.trigger("click");
    await flushPromises();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(w.find(".stub-NModal").exists()).toBe(false);
  });

  it("R07：close_resolve 失败 → 弹窗保持打开显示原因，重试成功后关闭", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    triggerCloseRequested();
    await flushPromises();
    invokeMock.mockClear();
    // 第一次提交失败
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "close_resolve") return Promise.reject(new Error("写配置失败"));
      return Promise.resolve(null);
    });
    const dialog = w.find(".stub-NModal");
    const minimize = dialog.findAll("button").find((b) => b.text().includes("最小化到托盘"));
    await minimize!.trigger("click");
    await flushPromises();
    // 弹窗保持打开，原因可见
    expect(w.find(".stub-NModal").exists()).toBe(true);
    expect(w.text()).toContain("关闭操作失败");
    expect(w.text()).toContain("写配置失败");
    // 重试成功 → 关闭
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "close_resolve") return Promise.resolve(null);
      return Promise.resolve(null);
    });
    const minimize2 = w
      .find(".stub-NModal")
      .findAll("button")
      .find((b) => b.text().includes("最小化到托盘"));
    await minimize2!.trigger("click");
    await flushPromises();
    expect(w.find(".stub-NModal").exists()).toBe(false);
  });

  it("F08 close_hide_failure_remains_retryable：hide 失败弹窗保持可重试", async () => {
    // 后端 hide 失败透传（“隐藏窗口失败: …（可重试）”）：弹窗保持打开、
    // 原因可见；重试成功后关闭。不把“无错误”当隐藏成功。
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    triggerCloseRequested();
    await flushPromises();
    invokeMock.mockClear();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "close_resolve")
        return Promise.reject(new Error("隐藏窗口失败: webview busy（可重试）"));
      return Promise.resolve(null);
    });
    const dialog = w.find(".stub-NModal");
    const minimize = dialog.findAll("button").find((b) => b.text().includes("最小化到托盘"));
    await minimize!.trigger("click");
    await flushPromises();
    expect(w.find(".stub-NModal").exists()).toBe(true);
    expect(w.text()).toContain("隐藏窗口失败");
    expect(w.text()).toContain("webview busy");
    // 重试成功 → 关闭
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "close_resolve") return Promise.resolve(null);
      return Promise.resolve(null);
    });
    const minimize2 = w
      .find(".stub-NModal")
      .findAll("button")
      .find((b) => b.text().includes("最小化到托盘"));
    await minimize2!.trigger("click");
    await flushPromises();
    expect(w.find(".stub-NModal").exists()).toBe(false);
  });

  it("R07：提交进行中不重复 invoke（防重复提交）", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    triggerCloseRequested();
    await flushPromises();
    let release!: () => void;
    const gate = new Promise<void>((r) => {
      release = r;
    });
    invokeMock.mockClear();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "close_resolve") return gate;
      return Promise.resolve(null);
    });
    const dialog = w.find(".stub-NModal");
    const minimize = dialog.findAll("button").find((b) => b.text().includes("最小化到托盘"));
    // pending 期间按钮禁用：再次点击不再派发
    await minimize!.trigger("click");
    await minimize!.trigger("click");
    release();
    await flushPromises();
    expect(invokeMock.mock.calls.filter((c) => c[0] === "close_resolve").length).toBe(1);
  });

  it("unmount 时取消 close-requested 监听", async () => {
    mockApp(statusOk);
    closeEvent.unlisten.mockClear();
    const w = mount(MainApp);
    await flushPromises();
    w.unmount();
    expect(closeEvent.unlisten).toHaveBeenCalled();
  });

  it("AP07 close_action_failure_is_reported_and_retryable：已记忆动作失败也有原因与重试", async () => {
    // 后端按记忆的默认动作执行失败（例如隐藏窗口失败）时窗口仍然可见，
    // 必须把原因送到界面并提供重试/取消——不能静默留在"点了关闭没反应"。
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    expect(w.find(".stub-NModal").exists()).toBe(false);

    listenerOf("close-action-failed")({
      payload: { action: "minimize", reason: "隐藏窗口失败: webview busy（可重试）" },
    });
    await flushPromises();
    expect(w.find(".stub-NModal").exists(), "失败必须弹出可操作的对话框").toBe(true);
    expect(w.text()).toContain("关闭操作失败");
    expect(w.text()).toContain("webview busy");

    // 与未记忆路径共用结果处理：重试走 close_resolve
    invokeMock.mockClear();
    const minimize = w
      .find(".stub-NModal")
      .findAll("button")
      .find((b) => b.text().includes("最小化到托盘"));
    await minimize!.trigger("click");
    await flushPromises();
    expect(invokeMock).toHaveBeenCalledWith("close_resolve", {
      minimize: true,
      remember: false,
    });
    expect(w.find(".stub-NModal").exists()).toBe(false);
  });

  it("AP07 close_action_failure_can_be_cancelled：取消不触发任何关闭动作", async () => {
    mockApp(statusOk);
    const w = mount(MainApp);
    await flushPromises();
    listenerOf("close-action-failed")({ payload: { reason: "隐藏窗口失败" } });
    await flushPromises();
    invokeMock.mockClear();
    const cancel = w
      .find(".stub-NModal")
      .findAll("button")
      .find((b) => b.text().includes("取消"));
    await cancel!.trigger("click");
    await flushPromises();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(w.find(".stub-NModal").exists()).toBe(false);
  });
});
