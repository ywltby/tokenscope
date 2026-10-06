// Task 1（设计系统）：主题偏好 light|dark|system 与视觉 token 断言。
// theme.ts 是模块级单例——每个用例重置模块并注入可控 matchMedia。
import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

const KEY = "tokenscope-theme";
type Listener = (e: { matches: boolean }) => void;

async function setup(opts: { stored?: string | null; systemDark?: boolean } = {}) {
  vi.resetModules();
  const listeners = new Set<Listener>();
  vi.stubGlobal("matchMedia", () => ({
    matches: opts.systemDark ?? false,
    addEventListener: (_t: string, cb: Listener) => listeners.add(cb),
    removeEventListener: (_t: string, cb: Listener) => listeners.delete(cb),
  }));
  if (opts.stored === undefined || opts.stored === null) localStorage.removeItem(KEY);
  else localStorage.setItem(KEY, opts.stored as string);
  const mod = await import("./theme");
  return {
    mod,
    setSystem(dark: boolean) {
      listeners.forEach((cb) => cb({ matches: dark }));
    },
  };
}

describe("主题偏好（设计系统 Task 1）", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    localStorage.clear();
  });

  it("默认跟随系统：无存储值时 preference=system，解析值来自 matchMedia", async () => {
    const { mod } = await setup({ systemDark: false });
    const { preference, mode } = mod.useTheme();
    expect(preference.value).toBe("system");
    expect(mode.value).toBe("light");
  });

  it("系统深色 + system 偏好解析为 dark", async () => {
    const { mod } = await setup({ systemDark: true });
    const { mode } = mod.useTheme();
    expect(mode.value).toBe("dark");
  });

  it("旧存储的解析值兼容读取；显式选择持久化", async () => {
    const { mod } = await setup({ stored: "dark", systemDark: false });
    const { preference, mode, setPreference } = mod.useTheme();
    expect(preference.value).toBe("dark");
    expect(mode.value).toBe("dark");
    setPreference("light");
    expect(mode.value).toBe("light");
    await nextTick();
    expect(localStorage.getItem(KEY)).toBe("light");
  });

  it("系统主题变化只影响 system 偏好", async () => {
    const { mod, setSystem } = await setup({ systemDark: false });
    const { mode, setPreference } = mod.useTheme();
    setSystem(true);
    expect(mode.value).toBe("dark");
    setPreference("dark");
    setSystem(false);
    // 显式偏好不受系统变化影响
    expect(mode.value).toBe("dark");
    setPreference("system");
    expect(mode.value).toBe("light");
  });

  it("解析后的主题值只可能是 light 或 dark", async () => {
    for (const p of ["light", "dark", "system"] as const) {
      const { mod } = await setup({ systemDark: false });
      const { mode, setPreference } = mod.useTheme();
      setPreference(p);
      expect(["light", "dark"]).toContain(mode.value);
    }
  });

  it("token 样式表可被应用入口加载且包含两套主题与降级", async () => {
    await import("../styles/tokens.css");
    const css = [...document.querySelectorAll("style")]
      .map((s) => s.textContent ?? "")
      .join(String.fromCharCode(10));
    expect(css).toContain("--ts-canvas");
    expect(css).toContain("--ts-accent");
    expect(css).toContain("--ts-surface");
    expect(css).toContain("--ts-fill");
    expect(css).toContain("--ts-separator");
    expect(css).toContain('data-theme="dark"');
    expect(css).toContain("@supports");
  });

  it("Naive UI / ECharts 适配器为两种主题产出完整覆盖", async () => {
    const { naiveThemeOverrides } = await import("../styles/naiveTheme");
    const { chartTokens } = await import("../styles/chartTheme");
    for (const m of ["light", "dark"] as const) {
      const o = naiveThemeOverrides(m);
      expect(o.common?.primaryColor).toBeTruthy();
      expect(o.common?.bodyColor).toBeTruthy();
      expect(o.common?.borderRadius).toBeTruthy();
      const ct = chartTokens(m);
      expect(ct.series).toHaveLength(4);
      expect(ct.series.map((s) => s.key)).toEqual(["input", "output", "cache_write", "cache_read"]);
      for (const s of ct.series) expect(s.color).toMatch(/^#/);
    }
    // 两套主题的系列色不得相同（浅色/深色是两套完整主题）。
    const light = chartTokens("light").series.map((s) => s.color);
    const dark = chartTokens("dark").series.map((s) => s.color);
    expect(light).not.toEqual(dark);
  });
});
