// P04/P05：隐私同意引导壳（App.vue）集成测试。
//
// 边界断言：
// - 一致前只允许引导 IPC（privacy_bootstrap / privacy_accept / privacy_exit_resolve）；
// - 业务主应用（MainApp.vue → Dashboard/Settings）只在后端 Ready 后动态挂载；
// - 未同意时不得访问 localStorage（持久化偏好），也不得发业务命令；
// - 拒绝/关闭路径与已记忆的关闭动作、托盘完全隔离。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from "@vue/test-utils";
import { defineComponent, h } from "vue";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const events = vi.hoisted(() => ({
  byName: new Map<string, ((ev?: unknown) => void)[]>(),
  unlisten: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, cb: (ev?: unknown) => void) => {
    const list = events.byName.get(event) ?? [];
    list.push(cb);
    events.byName.set(event, list);
    return Promise.resolve(events.unlisten);
  }),
}));

// 布局/浮层打桩：只验证引导壳的接线，不渲染 Naive UI 内部实现。
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
  const NModalStub = dc({
    name: "NModal",
    props: { show: { type: Boolean, default: false } },
    setup(props, { slots }) {
      return () => (props.show ? h("div", { class: "stub-NModal" }, slots.default?.()) : null);
    },
  });
  const NButtonStub = dc({
    name: "NButton",
    props: { disabled: { type: Boolean, default: false } },
    emits: ["click"],
    setup(props, { slots, emit }) {
      return () =>
        h(
          "button",
          {
            class: "stub-button",
            disabled: props.disabled,
            onClick: () => {
              if (!props.disabled) emit("click");
            },
          },
          slots.default?.(),
        );
    },
  });
  return {
    ...actual,
    NConfigProvider: passthrough("NConfigProvider"),
    NGlobalStyle: passthrough("NGlobalStyle"),
    NModal: NModalStub,
    NButton: NButtonStub,
  };
});

// 业务视图打桩（真实 MainApp.vue 会静态导入它们）：记录挂载时刻，
// 用于证明"偏好恢复发生在业务挂载之前"。
const order = vi.hoisted(() => ({ marks: [] as string[] }));
vi.mock("./views/Dashboard.vue", () => ({
  default: defineComponent({
    name: "Dashboard",
    props: { refreshKey: { type: Number, default: 0 } },
    setup() {
      order.marks.push("dashboard-mount");
      return () => h("div", { class: "stub-dashboard" });
    },
  }),
}));
vi.mock("./views/Settings.vue", () => ({
  default: defineComponent({
    name: "Settings",
    props: { refreshKey: { type: Number, default: 0 } },
    setup() {
      order.marks.push("settings-mount");
      return () => h("div", { class: "stub-settings" });
    },
  }),
}));

import App from "./App.vue";
import { renderPolicyMarkdown } from "./lib/policyMarkdown";

/// 打包进程序的政策全文（结构与 docs/privacy.md 同构：九章 + 列表 + 链接 +
/// 粗体）。与源文件逐字一致由 Rust 侧 include_str! 契约
/// （privacy_packaged_policy_matches_source）保证，前端只负责"完整渲染、
/// 不丢内容、不引入外部资源"。
const FULL_POLICY_MARKDOWN = [
  "# TokenScope 隐私政策",
  "",
  "更新日期：2026 年 10 月 9 日",
  "",
  "本政策说明应用会读取什么数据、保存在哪里，以及哪些功能会联网。",
  "",
  "## 一、适用范围",
  "",
  "本政策适用于 TokenScope 自身的日志读取、统计、缓存、设置和价格同步功能。",
  "",
  "## 二、读取的数据及用途",
  "",
  "### 1. 本地会话日志",
  "",
  "TokenScope 读取已启用来源中的会话日志，默认来源为 `.claude/projects` 与 `.codex/sessions`。",
  "",
  "- 使用时间、AI 工具和模型名称",
  "- 项目标识或工作目录路径",
  "",
  "## 三、本地保存位置",
  "",
  "TokenScope 自身的大部分数据保存在 `~/.tokenscope` 目录。",
  "",
  "## 四、网络请求与数据传输",
  "",
  "**价格自动同步默认开启。** 使用的公开接口为 [models.dev](https://models.dev/api.json)。",
  "",
  "## 五、第三方服务与运行时",
  "",
  "- **models.dev**：提供公开模型价格目录",
  "- **Microsoft WebView2**：用于显示界面",
  "",
  "## 六、用户可以如何控制数据",
  "",
  "可以在设置中关闭某个日志来源或关闭自动价格同步。",
  "",
  "## 七、保留、删除与卸载",
  "",
  "删除本地数据需要退出应用后清理数据目录。",
  "",
  "## 八、反馈与联系方式",
  "",
  "可通过项目 Issues 联系维护者。",
  "",
  "## 九、政策更新",
  "",
  "数据处理方式发生变化时，维护者会更新本文件的日期与内容。",
].join("\n");

const POLICY_SECTIONS = [
  "一、适用范围",
  "二、读取的数据及用途",
  "三、本地保存位置",
  "四、网络请求与数据传输",
  "五、第三方服务与运行时",
  "六、用户可以如何控制数据",
  "七、保留、删除与卸载",
  "八、反馈与联系方式",
  "九、政策更新",
];

function snapshot(phase: string, extra: Record<string, unknown> = {}) {
  return {
    phase,
    detail: null,
    exitPromptPending: false,
    diagnostics: [],
    policy: {
      title: "TokenScope 隐私政策",
      date: "2026 年 10 月 9 日",
      markdown: FULL_POLICY_MARKDOWN,
    },
    ...extra,
  };
}

const statusOk = {
  modelsdevAvailable: true,
  modelsdevCount: 1,
  modelsdevSyncedAt: "2026-10-09T00:00:00Z",
  openrouterAvailable: true,
  externalCount: 0,
  hasAnyPricing: true,
  needsSync: false,
  warnings: [],
};

/** 引导期 mock：只实现引导命令，其余命令一律记录但返回 null。 */
function mockGate(overrides: Record<string, () => Promise<unknown>> = {}): void {
  invokeMock.mockImplementation((cmd: string) => {
    const override = overrides[cmd];
    if (override) return override();
    if (cmd === "privacy_bootstrap") return Promise.resolve(snapshot("needs_consent"));
    return Promise.resolve(null);
  });
}

function commands(): string[] {
  return invokeMock.mock.calls.map((call) => String(call[0]));
}

function buttonByText(w: VueWrapper, text: string): DOMWrapper<Element> {
  const found = w.findAll("button.stub-button").find((b) => b.text().includes(text));
  if (!found) throw new Error(`未找到按钮：${text}`);
  return found;
}

function listenerOf(event: string): (ev?: unknown) => void {
  const list = events.byName.get(event);
  if (!list?.length) throw new Error(`未注册监听: ${event}`);
  return list.at(-1)!;
}

beforeEach(() => {
  invokeMock.mockReset();
  events.byName.clear();
  order.marks.length = 0;
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("P04 隐私同意引导壳", () => {
  it("bootstrap_does_not_import_or_mount_business_app：未 Ready 不挂载业务视图", async () => {
    mockGate();
    const w = mount(App);
    await flushPromises();
    expect(w.find(".stub-dashboard").exists()).toBe(false);
    expect(w.find(".stub-settings").exists()).toBe(false);
    expect(order.marks).toEqual([]);
    // 政策可见，且是唯一可交互的界面
    expect(w.text()).toContain("TokenScope 隐私政策");
    expect(w.text()).toContain("同意并继续");
    w.unmount();
  });

  it("consent_screen_makes_only_bootstrap_calls：首屏只发一条引导 IPC", async () => {
    mockGate();
    const w = mount(App);
    await flushPromises();
    expect(commands()).toEqual(["privacy_bootstrap"]);
    w.unmount();
  });

  it("consent_screen_does_not_access_local_storage：未同意不读持久化偏好", async () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem");
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    mockGate();
    const w = mount(App);
    await flushPromises();
    expect(getItem).not.toHaveBeenCalled();
    expect(setItem).not.toHaveBeenCalled();
    w.unmount();
  });

  it("policy_is_complete_and_available_offline：政策全文离线完整渲染无残留标记", async () => {
    // 渲染器是纯函数：不依赖网络、不依赖 DOM 资源（离线可用）
    const blocks = renderPolicyMarkdown(FULL_POLICY_MARKDOWN);
    const headings = FULL_POLICY_MARKDOWN.split(/\r?\n/).filter((line) => /^#{1,3}\s/.test(line));
    const renderedHeadings = blocks.filter((b) => b.type !== "ul" && b.type !== "p");
    expect(renderedHeadings.length).toBe(headings.length);
    const text = blocks.map((b) => (b.type === "ul" ? b.items.join("\n") : b.text)).join("\n");
    for (const section of POLICY_SECTIONS) expect(text).toContain(section);
    // 无未渲染的标记；链接降级为纯文本（没有可点击外链的原材料）
    expect(text).not.toContain("**");
    expect(text).not.toContain("](");
    expect(text).not.toContain("<a ");
    expect(text).not.toContain("<script");

    // 弹窗里同样完整可读（后端下发的那一份）
    mockGate();
    const w = mount(App);
    await flushPromises();
    const dialog = w.text();
    for (const section of POLICY_SECTIONS) expect(dialog).toContain(section);
    expect(dialog).toContain("更新日期：2026 年 10 月 9 日");
    w.unmount();
  });

  it("accept_waits_for_backend_ready：后端未 Ready 前不挂载业务", async () => {
    const resolvers: ((value: unknown) => void)[] = [];
    mockGate({
      privacy_accept: () =>
        new Promise((resolve) => {
          resolvers.push(resolve);
        }),
    });
    const w = mount(App);
    await flushPromises();
    await buttonByText(w, "同意并继续").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("正在保存");
    expect(w.find(".stub-dashboard").exists()).toBe(false);
    // 后端返回 starting（保存成功但初始化中）→ 仍然不挂载
    resolvers.at(-1)?.(snapshot("starting"));
    await flushPromises();
    expect(w.find(".stub-dashboard").exists()).toBe(false);
    expect(order.marks).toEqual([]);
    w.unmount();
  });

  it("failed_accept_keeps_policy_visible：保存失败保留政策与原因", async () => {
    mockGate({
      privacy_accept: () => Promise.reject("保存同意记录失败：磁盘满（合成）"),
    });
    const w = mount(App);
    await flushPromises();
    await buttonByText(w, "同意并继续").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("保存同意记录失败");
    expect(w.text()).toContain("磁盘满（合成）");
    // 弹窗未消失：政策与两个选择都还在，业务未挂载
    expect(w.text()).toContain("同意并继续");
    expect(w.text()).toContain("不同意");
    expect(w.find(".stub-dashboard").exists()).toBe(false);
    w.unmount();
  });

  it("accepted_start_restores_preferences_before_main_mount：先恢复偏好再挂业务", async () => {
    localStorage.setItem("tokenscope-theme", "dark");
    const marks = order.marks;
    const getItem = Storage.prototype.getItem;
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(function (
      this: Storage,
      key: string,
    ) {
      if (key === "tokenscope-theme") marks.push("preference-read");
      return getItem.call(this, key);
    });
    mockGate({
      privacy_accept: () => Promise.resolve(snapshot("ready")),
      pricing_status: () => Promise.resolve(statusOk),
      startup_diagnostics: () => Promise.resolve({ state: "ok", message: null }),
    });
    const w = mount(App);
    await flushPromises();
    // 引导阶段：不读偏好，首帧主题仍按系统（浅色）
    expect(marks).not.toContain("preference-read");
    await buttonByText(w, "同意并继续").trigger("click");
    await flushPromises();
    // Ready 后：业务视图挂载（动态 import 需要跨多个微任务周期）
    await vi.waitFor(() => expect(marks).toContain("dashboard-mount"));
    // 偏好先于业务视图被读取，且主题属性已按存储偏好恢复
    expect(marks.indexOf("preference-read")).toBeGreaterThanOrEqual(0);
    expect(marks.indexOf("preference-read")).toBeLessThan(marks.indexOf("dashboard-mount"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(w.find(".stub-dashboard").exists()).toBe(true);
    w.unmount();
  });

  it("reject_opens_exit_confirmation：不同意进入专属退出确认", async () => {
    mockGate();
    const w = mount(App);
    await flushPromises();
    await buttonByText(w, "不同意").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("退出 TokenScope？");
    expect(w.text()).toContain("返回隐私政策");
    expect(w.text()).toContain("尚未同意隐私政策");
    // 不提供记忆/托盘选项，且拒绝本身不发业务命令
    expect(w.text()).not.toContain("记住我的选择");
    expect(w.text()).not.toContain("最小化到托盘");
    expect(commands()).toEqual(["privacy_bootstrap"]);
    w.unmount();
  });

  it("cancel_exit_returns_to_blocked_policy：取消退出回到仍然阻断的政策", async () => {
    mockGate();
    const w = mount(App);
    await flushPromises();
    await buttonByText(w, "不同意").trigger("click");
    await flushPromises();
    await buttonByText(w, "返回隐私政策").trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("退出 TokenScope？");
    expect(w.text()).toContain("同意并继续");
    expect(w.find(".stub-dashboard").exists()).toBe(false);
    // 后端只收到"清除待处理标记"，没有任何业务命令
    expect(commands().at(-1)).toBe("privacy_exit_resolve");
    expect(invokeMock).toHaveBeenLastCalledWith("privacy_exit_resolve", { exit: false });
    w.unmount();
  });

  it("close_before_listener_is_recovered_by_bootstrap：监听前到达的关闭请求要恢复显示", async () => {
    mockGate({
      privacy_bootstrap: () =>
        Promise.resolve(snapshot("needs_consent", { exitPromptPending: true })),
    });
    const w = mount(App);
    await flushPromises();
    expect(w.text()).toContain("退出 TokenScope？");
    w.unmount();
  });

  it("关闭事件到达后只出现一个退出确认（重复 X 不堆叠）", async () => {
    mockGate();
    const w = mount(App);
    await flushPromises();
    listenerOf("privacy-exit-requested")();
    listenerOf("privacy-exit-requested")();
    await flushPromises();
    const dialogs = w.findAll(".stub-NModal").filter((n) => n.text().includes("退出 TokenScope？"));
    expect(dialogs.length).toBe(1);
    w.unmount();
  });
});
