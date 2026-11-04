// i18n（2026-11-04）：语言状态、隐私边界与持久化的定向回归。
//
// 断言重点（用户要求 1/3）：默认简体中文；同意前只在内存切换、绝不读写
// 持久化；同意成功后才把选择写进 settings.toml；旧设置缺字段时保持中文且
// 不碰其他字段；保存失败回滚到已落盘值并暴露原因；语言与统计时区分离。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

type I18nModule = typeof import("./i18n");

async function freshI18n(): Promise<I18nModule> {
  vi.resetModules();
  return import("./i18n");
}

/** 按命令名分派的 invoke 替身；未声明命令一律视为不该被调用。 */
function mockInvoke(
  handlers: Record<string, (args?: Record<string, unknown>) => unknown>,
): string[] {
  const calls: string[] = [];
  invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    calls.push(cmd);
    const handler = handlers[cmd];
    if (!handler) return Promise.reject(new Error(`未预期的命令：${cmd}`));
    return Promise.resolve(handler(args));
  });
  return calls;
}

beforeEach(() => {
  invokeMock.mockReset();
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("i18n 语言状态（默认与切换）", () => {
  // 首次 import 会加载 vue-i18n + naive-ui（语言包映射）——并行跑全套时
  // 超过默认 5 秒并不是断言问题，这里给模块加载留出余量。
  it("defaults_to_zh_cn_and_syncs_html_lang：默认简体中文，首帧即同步 html[lang]", async () => {
    const mod = await freshI18n();
    expect(mod.normalizeLocale(undefined)).toBe("zh-CN");
    expect(mod.t("app.pageSummary")).toBe("汇总");
    expect(document.documentElement.lang).toBe("zh-CN");
  }, 20_000);

  it("switching_locale_updates_text_and_naive_locale：切换后文案、Naive 语言与 html[lang] 同源", async () => {
    const mod = await freshI18n();
    mod.setLocale("en-US");
    expect(mod.t("app.pageSummary")).toBe("Summary");
    expect(document.documentElement.lang).toBe("en-US");
    // Naive UI 组件语言跟随（英文包与中文包是不同对象）
    expect(mod.naiveLocale("en-US")).not.toBe(mod.naiveLocale("zh-CN"));
    expect(mod.naiveDateLocale("en-US")).not.toBe(mod.naiveDateLocale("zh-CN"));
  });

  it("unknown_locale_falls_back_to_zh_cn：非法语言值归一为简体中文，不抛错", async () => {
    const mod = await freshI18n();
    expect(mod.normalizeLocale("fr-FR")).toBe("zh-CN");
    expect(mod.normalizeLocale(null)).toBe("zh-CN");
    expect(mod.normalizeLocale(42)).toBe("zh-CN");
    expect(mod.normalizeLocale("en-US")).toBe("en-US");
  });
});

describe("同意前的持久化边界（用户要求 3）", () => {
  it("pre_consent_switch_touches_no_storage_and_no_ipc：只在内存切换，零持久化", async () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem");
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    const messages = mockInvoke({});
    const mod = await freshI18n();
    mod.setLocale("en-US");
    expect(mod.t("app.pageSummary")).toBe("Summary");
    // 内存选择成立，但没有任何 IO：不读设置、不写设置、不碰 localStorage
    expect(messages).toEqual([]);
    expect(getItem).not.toHaveBeenCalled();
    expect(setItem).not.toHaveBeenCalled();
  });

  it("consent_flush_saves_selection_then_reads_authoritative：同意后才保存并读回权威值", async () => {
    const calls = mockInvoke({
      settings_set_language: (args) => args?.language,
      settings_get: () => ({ language: "en-US" }),
    });
    const mod = await freshI18n();
    mod.setLocale("en-US");
    expect(calls).toEqual([]);
    await mod.flushPendingOnConsent();
    expect(calls).toEqual(["settings_set_language", "settings_get"]);
    expect(mod.t("app.pageSummary")).toBe("Summary");
  });

  it("consent_without_touch_does_not_write_language：没动过语言时不会凭空写设置", async () => {
    const calls = mockInvoke({ settings_get: () => ({ timezone: "UTC" }) });
    const mod = await freshI18n();
    await mod.flushPendingOnConsent();
    expect(calls).toEqual(["settings_get"]);
    expect(mod.t("app.pageSummary")).toBe("汇总");
  });
});

describe("旧设置兼容与重启恢复", () => {
  it("missing_language_field_keeps_zh_cn_without_rewriting_settings：缺字段按默认解释，不回写设置", async () => {
    const calls = mockInvoke({ settings_get: () => ({ timezone: "UTC", price_auto_sync: false }) });
    const mod = await freshI18n();
    await mod.adoptPersisted();
    expect(mod.t("app.pageSummary")).toBe("汇总");
    // 只有读取，没有任何写入——旧设置文件不被语言功能改写
    expect(calls).toEqual(["settings_get"]);
  });

  it("restart_restores_persisted_language：重启（新模块实例）从设置恢复英文", async () => {
    const calls = mockInvoke({ settings_get: () => ({ language: "en-US" }) });
    const mod = await freshI18n();
    await mod.adoptPersisted();
    expect(calls).toEqual(["settings_get"]);
    expect(mod.t("app.pageSummary")).toBe("Summary");
    expect(document.documentElement.lang).toBe("en-US");

    // 再启动一次（新实例、同一份设置）：仍然恢复英文
    const again = await freshI18n();
    await again.adoptPersisted();
    expect(again.t("app.pageSummary")).toBe("Summary");
  });

  it("settings_read_failure_keeps_current_language：读取失败不阻断、不改写设置", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const calls = mockInvoke({
      settings_get: () => {
        throw new Error("设置解析失败");
      },
    });
    const mod = await freshI18n();
    await mod.adoptPersisted();
    expect(calls).toEqual(["settings_get"]);
    expect(mod.t("app.pageSummary")).toBe("汇总");
    expect(warn).toHaveBeenCalled();
  });

  it("late_startup_read_does_not_override_user_change：启动读取晚到不覆盖用户已选项", async () => {
    let release: ((value: unknown) => void) | undefined;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "settings_get") return new Promise((r) => (release = r));
      if (cmd === "settings_set_language") return Promise.resolve("en-US");
      return Promise.reject(new Error(`未预期的命令：${cmd}`));
    });
    const mod = await freshI18n();
    const pending = mod.adoptPersisted();
    mod.changeLocale("en-US");
    release?.({ language: "zh-CN" });
    await pending;
    expect(mod.t("app.pageSummary")).toBe("Summary");
  });
});

describe("保存失败与回滚", () => {
  it("save_failure_rolls_back_and_exposes_reason：写设置失败回滚到已落盘值并给出原因", async () => {
    mockInvoke({
      settings_set_language: () => {
        throw new Error("磁盘只读");
      },
    });
    const mod = await freshI18n();
    const api = mod.useAppLocale();
    mod.changeLocale("en-US");
    await vi.waitFor(() => expect(api.persistError.value).toContain("磁盘只读"));
    // 回滚：界面不保留一个并未写入的选择
    expect(api.locale.value).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
  });

  it("change_locale_persists_immediately_and_skips_noop：主界面切换立即保存，重复同值不重复写", async () => {
    const calls = mockInvoke({
      settings_set_language: (args) => args?.language,
    });
    const mod = await freshI18n();
    mod.changeLocale("en-US");
    await vi.waitFor(() => expect(calls).toEqual(["settings_set_language"]));
    mod.changeLocale("en-US");
    expect(calls).toEqual(["settings_set_language"]);
  });
});
