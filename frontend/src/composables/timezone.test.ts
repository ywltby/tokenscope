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
});
