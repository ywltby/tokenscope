// 设置 → 关于面板的行为测试：加载、失败原因、重试、并发抑制、卸载后迟到结果、
// 语言切换与只读副作用边界。版本模拟值与仓库版本不同（9.8.7），防止写死。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

const getVersionMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/app", () => ({ getVersion: getVersionMock }));
// 关于只允许调用只读 app API；任何 core invoke（设置写入/采集/导入）都算越界。
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import AboutPanel from "./AboutPanel.vue";
import aboutSource from "./AboutPanel.vue?raw";
import { changeLocale, DEFAULT_LOCALE } from "../lib/i18n";

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (v: T) => void;
  reject: (e: unknown) => void;
} {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  getVersionMock.mockReset();
  invokeMock.mockReset();
});

afterEach(async () => {
  changeLocale(DEFAULT_LOCALE);
  await flushPromises();
  vi.restoreAllMocks();
});

describe("AboutPanel 版本读取", () => {
  it("延迟响应先显示加载状态，成功后显示运行时版本", async () => {
    const pending = deferred<string>();
    getVersionMock.mockReturnValue(pending.promise);
    const w = mount(AboutPanel);
    await flushPromises();
    expect(w.find("[role='status']").text()).toContain("正在读取版本");
    expect(w.find("[data-about-version]").exists()).toBe(false);

    pending.resolve("9.8.7");
    await flushPromises();
    expect(w.text()).toContain("TokenScope");
    expect(w.find("[data-about-version]").text()).toBe("9.8.7");
    expect(w.text()).not.toContain("正在读取版本");
    expect(getVersionMock).toHaveBeenCalledTimes(1);
    expect(invokeMock, "不得调用任何设置/采集/导入命令").not.toHaveBeenCalled();
    w.unmount();
  });

  it("失败显示本地化原因与重试；重试成功后恢复，且不并发第二条读取", async () => {
    const retryGate = deferred<string>();
    getVersionMock
      .mockRejectedValueOnce(new Error("plugin:app|version unavailable"))
      .mockReturnValueOnce(retryGate.promise);
    const w = mount(AboutPanel);
    await flushPromises();
    const alert = w.find("[role='alert']");
    expect(alert.exists()).toBe(true);
    expect(alert.text()).toContain("版本读取失败");
    expect(alert.text()).toContain("plugin:app|version unavailable");
    expect(w.find("[data-about-version]").exists()).toBe(false);

    const retry = w.findAll("button").find((b) => b.text() === "重试")!;
    await retry.trigger("click");
    await retry.trigger("click"); // 在途期间的重复触发不得并发第二条
    await flushPromises();
    expect(getVersionMock).toHaveBeenCalledTimes(2);

    retryGate.resolve("9.8.7");
    await flushPromises();
    expect(w.find("[data-about-version]").text()).toBe("9.8.7");
    expect(w.find("[role='alert']").exists()).toBe(false);
    w.unmount();
  });

  it("空白返回值按不可用处理，不展示空白版本", async () => {
    getVersionMock.mockResolvedValue("   ");
    const w = mount(AboutPanel);
    await flushPromises();
    expect(w.find("[data-about-version]").exists()).toBe(false);
    expect(w.find("[role='alert']").text()).toContain("版本不可用");
    w.unmount();
  });

  it("切换语言只更新标签，保留版本且不重复读取", async () => {
    getVersionMock.mockResolvedValue("9.8.7");
    const w = mount(AboutPanel);
    await flushPromises();
    expect(getVersionMock).toHaveBeenCalledTimes(1);

    changeLocale("en-US");
    await flushPromises();
    expect(w.text()).toContain("Installed version");
    expect(w.find("[data-about-version]").text()).toBe("9.8.7");
    expect(getVersionMock).toHaveBeenCalledTimes(1);
    w.unmount();
  });

  it("失败后切换语言，原因随界面翻译，不残留读取时的中文", async () => {
    getVersionMock.mockResolvedValue("   ");
    const w = mount(AboutPanel);
    await flushPromises();
    expect(w.find("[role='alert']").text()).toContain("版本不可用");

    changeLocale("en-US");
    await flushPromises();
    const alert = w.find("[role='alert']").text();
    expect(alert).toContain("Failed to read version");
    expect(alert).toContain("Version unavailable");
    expect(alert).not.toContain("版本不可用");
    w.unmount();
  });

  it("关于行复用设置行的局部样式（对齐、间距与辅助文字层级）", () => {
    // Settings.vue 的 scoped 样式不会作用到子组件内部：AboutPanel 必须自带
    // 同一组行样式，否则版本行缺少 flex 对齐/间距与 12px 辅助文字。
    expect(aboutSource).toMatch(/\.setting-row\s*\{[^}]*display:\s*flex/);
    expect(aboutSource).toMatch(/\.setting-row\s*\{[^}]*justify-content:\s*space-between/);
    expect(aboutSource).toMatch(/\.setting-row\s*\{[^}]*padding:\s*var\(--ts-space-3\)\s+0/);
    expect(aboutSource).toMatch(/\.setting-main\s*\{[^}]*min-width:\s*0/);
    expect(aboutSource).toMatch(/\.setting-label\s*\{[^}]*font-size:\s*14px/);
    expect(aboutSource).toMatch(/\.setting-help\s*\{[^}]*font-size:\s*12px/);
  });

  it("卸载后的迟到结果不写状态，也不产生未处理拒绝", async () => {
    const pending = deferred<string>();
    getVersionMock.mockReturnValue(pending.promise);
    const reasons: unknown[] = [];
    const proc = (
      globalThis as unknown as {
        process?: {
          on: (ev: string, cb: (r: unknown) => void) => void;
          off: (ev: string, cb: (r: unknown) => void) => void;
        };
      }
    ).process;
    const onNode = (r: unknown): void => {
      reasons.push(r);
    };
    proc?.on("unhandledRejection", onNode);

    const w = mount(AboutPanel);
    await flushPromises();
    w.unmount();
    pending.resolve("9.8.7");
    await flushPromises();
    await new Promise((r) => setTimeout(r, 0));
    await flushPromises();
    proc?.off("unhandledRejection", onNode);
    expect(reasons, "迟到成功结果只被丢弃，不得成为未处理拒绝").toEqual([]);
  });
});
