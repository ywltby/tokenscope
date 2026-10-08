import { beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});

describe("token 显示颜色", () => {
  it("自定义颜色跨重载保留，恢复默认清除覆盖", async () => {
    const { useTokenColors } = await import("./tokenColors");
    const state = useTokenColors();
    expect(state.setColor("cache_read", "#123abc")).toBe(true);
    expect(state.overrides.value.cache_read).toBe("#123ABC");
    vi.resetModules();
    const restored = (await import("./tokenColors")).useTokenColors();
    expect(restored.overrides.value.cache_read).toBe("#123ABC");
    restored.resetColors();
    expect(restored.overrides.value).toEqual({});
  });

  it("损坏偏好与非法颜色不会进入图表或 CSS", async () => {
    localStorage.setItem(
      "tokenscope-token-colors",
      '{"input":"red;display:none","cache_read":"#AB1234","unknown":"#123456"}',
    );
    const state = (await import("./tokenColors")).useTokenColors();
    expect(state.overrides.value).toEqual({ cache_read: "#AB1234" });
    expect(state.setColor("input", "bad")).toBe(false);
    expect(state.overrides.value.input).toBeUndefined();
  });
});
