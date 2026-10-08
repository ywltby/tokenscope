// R03：时区偏好持久化校验——非法旧值回退默认，不抛异常导致首屏崩溃。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// useTimezone 是模块级单例：每用例重置模块并注入受控 localStorage。
async function setup(stored: string | null) {
  vi.resetModules();
  if (stored == null) localStorage.removeItem("tokenscope-tz");
  else localStorage.setItem("tokenscope-tz", stored);
  const mod = await import("./timezone");
  return mod.useTimezone();
}

beforeEach(() => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("timezone 偏好校验（R03）", () => {
  it("合法 IANA 时区原样采用", async () => {
    const { tz } = await setup("America/Los_Angeles");
    expect(tz.value).toBe("America/Los_Angeles");
  });

  it("local 特判放行（Dashboard 标题/快捷项依赖它）", async () => {
    const { tz } = await setup("local");
    expect(tz.value).toBe("local");
  });

  it("非法旧偏好回退默认 Asia/Shanghai 且不抛异常", async () => {
    const { tz } = await setup("Not/AZone");
    expect(tz.value).toBe("Asia/Shanghai");
    expect(() => tz.value).not.toThrow();
  });

  it("无存储值默认 Asia/Shanghai", async () => {
    const { tz } = await setup(null);
    expect(tz.value).toBe("Asia/Shanghai");
  });

  it("storage_unavailable_falls_back_and_never_throws：localStorage 抛错不阻断时区偏好", async () => {
    // RC09：隐私模式/策略禁用下访问 localStorage 直接抛错。读必须回落默认，
    // 写必须静默失败——否则 composable 初始化即抛，组件 setup 崩溃、窗口空白。
    vi.resetModules();
    const desc = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("storage blocked");
      },
    });
    try {
      const mod = await import("./timezone");
      const { tz } = mod.useTimezone();
      expect(tz.value, "读失败应回落默认时区").toBe("Asia/Shanghai");
      // 写入（watchEffect）不得抛——等一次微任务让副作用真正执行
      tz.value = "UTC";
      await new Promise((r) => setTimeout(r, 0));
      expect(tz.value).toBe("UTC");
    } finally {
      Object.defineProperty(window, "localStorage", desc ?? {});
    }
  });
});
