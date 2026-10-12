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

  it("token 层提供状态胶囊与 reduced-motion 降级（计划任务 1 Step 4）", async () => {
    await import("../styles/tokens.css");
    const css = [...document.querySelectorAll("style")]
      .map((s) => s.textContent ?? "")
      .join(String.fromCharCode(10));
    // 标题行状态胶囊（已更新/刷新中/缓存数据）的公共基类
    expect(css).toContain(".ts-status-pill");
    // 尊重系统减少动效偏好：动效 token 归零
    expect(css).toContain("prefers-reduced-motion");
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

  it("分段控件底槽必须 relative：thumb 是 absolute 子元素，锚点丢失会错位到吸顶导航", async () => {
    await import("../styles/tokens.css");
    const css = [...document.querySelectorAll("style")]
      .map((s) => s.textContent ?? "")
      .join(String.fromCharCode(10));
    const start = css.indexOf(".ts-segmented {");
    expect(start, "必须存在 .ts-segmented 规则").toBeGreaterThanOrEqual(0);
    const body = css.slice(start, css.indexOf("}", start));
    // 回归：缺 position: relative 时，absolute 滑块以 sticky 导航栏为包含块，
    // 页面/主题两个滑块全部叠到窗口左上角（盖住品牌图标）。
    expect(body).toContain("position: relative");
    expect(css).toContain(".ts-segmented-thumb");
  });
});

// UX09：首帧主题取证与共享解析——boot 脚本与运行时必须消费同一存储键与
// 同一组取值/回落规则，否则"先按默认画一帧再切换"。
describe("UX09 首帧主题解析（prepaint）", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    localStorage.clear();
  });

  it("preference_resolution_is_shared：boot 首帧只跟随系统，偏好恢复走显式函数", async () => {
    const { applyResolvedThemeAttribute } = await import("../lib/themePreference");
    const boot = (await import("../../public/theme-boot.js?raw")).default;
    // P04：未同意隐私政策前不得读取持久化偏好——boot 不碰存储键
    expect(boot).not.toContain("localStorage");
    expect(boot).not.toContain("tokenscope-theme");
    // 首帧只跟随系统明暗并写入 data-theme
    expect(boot).toContain("prefers-color-scheme: dark");
    expect(boot).toContain("data-theme");
    // 偏好恢复收敛到显式函数（同意后、业务挂载前调用一次）
    localStorage.setItem("tokenscope-theme", "dark");
    vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener: () => {} }));
    expect(applyResolvedThemeAttribute()).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    // 系统明暗仍决定 system 偏好下的解析结果
    const { parsePreference, resolveMode } = await import("../lib/themePreference");
    expect(parsePreference("dark")).toBe("dark");
    expect(resolveMode("system", true)).toBe("dark");
    expect(resolveMode("system", false)).toBe("light");
  });

  it("invalid_or_unavailable_storage_falls_back_to_system", async () => {
    const { parsePreference, readStoredPreference } = await import("../lib/themePreference");
    // 缺失 / 非法值一律回落 system（不猜 light/dark）
    expect(parsePreference(null)).toBe("system");
    expect(parsePreference(undefined)).toBe("system");
    expect(parsePreference("")).toBe("system");
    expect(parsePreference("bogus")).toBe("system");
    expect(parsePreference("Dark")).toBe("system"); // 大小写不敏感不作兼容
    expect(parsePreference("dark")).toBe("dark");
    // localStorage 抛错（隐私模式/被禁用）也不得抛出
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(readStoredPreference()).toBe("system");
  });

  it("system_changes_only_affect_system_preference", async () => {
    const { resolveMode } = await import("../lib/themePreference");
    expect(resolveMode("system", true)).toBe("dark");
    expect(resolveMode("system", false)).toBe("light");
    // 显式偏好不受系统明暗影响
    expect(resolveMode("light", true)).toBe("light");
    expect(resolveMode("dark", false)).toBe("dark");
  });

  it("storage_unavailable_still_resolves_theme_and_never_throws：存储不可用时主题仍可用", async () => {
    // RC09：localStorage 访问抛错时，theme composable 必须回落 system 偏好、
    // 写入静默降级；否则 setup 抛错 → 首帧后再也渲染不出 .app-shell。
    vi.resetModules();
    vi.stubGlobal("matchMedia", () => ({
      matches: true,
      addEventListener: () => {},
      removeEventListener: () => {},
    }));
    const desc = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("storage blocked");
      },
    });
    try {
      const mod = await import("./theme");
      const { useTheme } = mod;
      const { mode, setPreference } = useTheme();
      // 读失败 → 回落 system；system + 系统深色 → 实际主题必须是 dark。
      // （若 composable 在存储抛错时中断初始化，这里根本取不到解析结果。）
      expect(mode.value, "存储不可用时仍应按 system 偏好解析出主题").toBe("dark");
      // 写入（watchEffect）不得抛，且偏好仍在本次会话内生效
      setPreference("light");
      await new Promise((r) => setTimeout(r, 0));
      expect(mode.value, "显式偏好切换仍生效（持久化被静默降级）").toBe("light");
    } finally {
      Object.defineProperty(window, "localStorage", desc ?? {});
      vi.unstubAllGlobals();
    }
  });
});
