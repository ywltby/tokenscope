// i18n 定向回归（2026-11-04 第二轮）：
//   1) 重新同意时必须保留弹窗内**最后一次明确选择**的语言；
//   2) 语言保存走设置串行队列——较早请求不得覆盖较晚选择，失败回滚到最后
//      成功落盘值，界面/设置/托盘最终与最后一次选择一致。
//
// 与 `i18n.test.ts` 分开：这里只放这两条行为，失败时定位更直接。
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

type I18nModule = typeof import("./i18n");

async function freshI18n(): Promise<I18nModule> {
  vi.resetModules();
  return import("./i18n");
}

/**
 * 语言写入的后端替身：**写入发生在 resolve 时**（模拟真实 IPC 的完成顺序），
 * 因此"较早请求晚完成"会像真实系统一样把旧值写到磁盘。
 */
function mockLanguageBackend(initial: string | null) {
  const state = { stored: initial, calls: [] as string[] };
  const pending: {
    language: string;
    resolve: (value: string) => void;
    reject: (reason: unknown) => void;
  }[] = [];
  invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "settings_get") return Promise.resolve({ language: state.stored });
    if (cmd !== "settings_set_language") {
      return Promise.reject(new Error(`未预期的命令：${cmd}`));
    }
    const language = String(args?.language);
    state.calls.push(language);
    return new Promise<string>((resolve, reject) => {
      pending.push({
        language,
        resolve: (committed) => {
          state.stored = committed;
          resolve(committed);
        },
        reject: (reason) => reject(reason),
      });
    });
  });
  return { state, pending };
}

/** 让已 resolve 的微任务跑完（不依赖真实计时器）。 */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

// 每个用例都要 resetModules 重新 import（含 vue-i18n + naive 语言包）：并行跑
// 全套时切换会超过默认 5 秒，因此本文件的用例统一放宽超时（不是放宽断言）。
const itSlow = (name: string, fn: () => Promise<void> | void): void => {
  it(name, fn, 20_000);
};

beforeEach(() => {
  invokeMock.mockReset();
});

describe("重新同意：保留弹窗内最后一次选择（修复 1）", () => {
  itSlow(
    "last_choice_wins_over_old_setting：旧设置为英文，弹窗里英文→中文，同意后保存并显示中文",
    async () => {
      const { state } = await (async () => {
        const backend = mockLanguageBackend("en-US");
        // 设置写入立即成功（这条用例只验证"选择被保留并保存"）。
        invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
          if (cmd === "settings_get") return Promise.resolve({ language: backend.state.stored });
          if (cmd === "settings_set_language") {
            const language = String(args?.language);
            backend.state.calls.push(language);
            backend.state.stored = language;
            return Promise.resolve(language);
          }
          return Promise.reject(new Error(`未预期的命令：${cmd}`));
        });
        return backend;
      })();
      const mod = await freshI18n();
      // 同意前：默认中文 → 用户先切英文（弹窗界面变英文）→ 再切回中文。
      mod.setLocale("en-US");
      mod.setLocale("zh-CN");
      expect(mod.t("app.pageSummary")).toBe("汇总");
      await mod.flushPendingOnConsent();
      // 必须真的把中文写进设置（不能因为"等于默认值"就跳过），并显示中文。
      expect(state.calls).toEqual(["zh-CN"]);
      expect(state.stored).toBe("zh-CN");
      expect(mod.t("app.pageSummary")).toBe("汇总");
    },
  );

  itSlow(
    "single_explicit_switch_to_english_is_persisted：只切过一次英文时同样保存英文",
    async () => {
      mockLanguageBackend("zh-CN");
      const mod = await freshI18n();
      const calls: string[] = [];
      invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
        if (cmd === "settings_get") return Promise.resolve({ language: "zh-CN" });
        if (cmd === "settings_set_language") {
          const language = String(args?.language);
          calls.push(language);
          return Promise.resolve(language);
        }
        return Promise.reject(new Error(`未预期的命令：${cmd}`));
      });
      mod.setLocale("en-US");
      await mod.flushPendingOnConsent();
      expect(calls).toEqual(["en-US"]);
      expect(mod.t("app.pageSummary")).toBe("Summary");
    },
  );

  itSlow("untouched_choice_does_not_rewrite_settings：没动过语言时不写设置", async () => {
    const calls: string[] = [];
    invokeMock.mockImplementation((cmd: string) => {
      calls.push(cmd);
      if (cmd === "settings_get") return Promise.resolve({ language: "en-US" });
      return Promise.reject(new Error(`未预期的命令：${cmd}`));
    });
    const mod = await freshI18n();
    await mod.flushPendingOnConsent();
    expect(calls).toEqual(["settings_get"]);
    // 没动过 → 采用设置里的权威值
    expect(mod.t("app.pageSummary")).toBe("Summary");
  });

  itSlow(
    "save_failure_keeps_the_explicit_choice_for_this_session：保存失败仍保留本次会话的选择",
    async () => {
      invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
        if (cmd === "settings_get") return Promise.resolve({ language: "en-US" });
        if (cmd === "settings_set_language") {
          void args;
          return Promise.reject(new Error("设置文件只读"));
        }
        return Promise.reject(new Error(`未预期的命令：${cmd}`));
      });
      const mod = await freshI18n();
      mod.setLocale("en-US");
      mod.setLocale("zh-CN");
      await mod.flushPendingOnConsent();
      // 明确选过中文：即便写入失败，也不能被旧设置（en-US）顶回去。
      expect(mod.t("app.pageSummary")).toBe("汇总");
      expect(mod.useAppLocale().persistError.value).toContain("设置文件只读");
    },
  );

  itSlow("pre_consent_never_touches_persistence：同意前不读不写（含多次切换）", async () => {
    const calls: string[] = [];
    invokeMock.mockImplementation((cmd: string) => {
      calls.push(cmd);
      return Promise.resolve(null);
    });
    const mod = await freshI18n();
    mod.setLocale("en-US");
    mod.setLocale("zh-CN");
    mod.setLocale("en-US");
    expect(calls).toEqual([]);
    expect(mod.t("app.pageSummary")).toBe("Summary");
  });
});

describe("保存并发顺序（修复 2）", () => {
  itSlow(
    "only_one_save_in_flight_and_last_choice_wins：串行保存，后到的选择不被较早请求覆盖",
    async () => {
      const { state, pending } = mockLanguageBackend("zh-CN");
      const mod = await freshI18n();
      await mod.adoptPersisted();

      mod.changeLocale("en-US");
      mod.changeLocale("zh-CN");
      await settle();
      // 同一时刻只有一个设置在途（复用设置串行队列）
      expect(pending.map((p) => p.language)).toEqual(["en-US"]);

      // 较早请求先完成：界面与设置此刻仍是英文，但用户最后选的是中文
      pending[0].resolve("en-US");
      await settle();
      expect(pending.map((p) => p.language)).toEqual(["en-US", "zh-CN"]);
      expect(mod.useAppLocale().locale.value).toBe("zh-CN");

      pending[1].resolve("zh-CN");
      await vi.waitFor(() => expect(mod.useAppLocale().persisting.value).toBe(false));
      // 最终：界面、设置（磁盘）与最后一次选择一致
      expect(state.stored).toBe("zh-CN");
      expect(mod.useAppLocale().locale.value).toBe("zh-CN");
      expect(mod.t("app.pageSummary")).toBe("汇总");
    },
  );

  itSlow(
    "out_of_order_completion_cannot_revert_to_earlier_choice：即使较晚请求先完成，也不回退到较早值",
    async () => {
      const { state, pending } = mockLanguageBackend("zh-CN");
      const mod = await freshI18n();
      await mod.adoptPersisted();

      mod.changeLocale("en-US");
      mod.changeLocale("zh-CN");
      await settle();
      // 队列串行：第二个请求尚未发出，不存在两个并发响应
      expect(pending).toHaveLength(1);

      pending[0].resolve("en-US");
      await settle();
      pending[1].resolve("zh-CN");
      await vi.waitFor(() => expect(mod.useAppLocale().persisting.value).toBe(false));
      expect(state.stored).toBe("zh-CN");
      expect(mod.useAppLocale().locale.value).toBe("zh-CN");
    },
  );

  itSlow(
    "failed_save_rolls_back_to_last_committed_value：最终保存失败回滚到最后成功落盘值",
    async () => {
      const { state, pending } = mockLanguageBackend("zh-CN");
      const mod = await freshI18n();
      await mod.adoptPersisted();

      mod.changeLocale("en-US");
      await settle();
      pending[0].resolve("en-US");
      await settle();
      expect(state.stored).toBe("en-US");

      mod.changeLocale("zh-CN");
      await settle();
      pending[1].reject(new Error("磁盘只读"));
      await vi.waitFor(() => expect(mod.useAppLocale().persisting.value).toBe(false));
      // 回滚到上一次成功落盘值（英文），并把原因暴露出来
      expect(mod.useAppLocale().locale.value).toBe("en-US");
      expect(mod.useAppLocale().persistError.value).toContain("磁盘只读");
      expect(state.stored).toBe("en-US");
    },
  );

  itSlow(
    "failed_old_save_does_not_cancel_pending_choice：较早请求失败不吞掉在途的较晚选择",
    async () => {
      const { state, pending } = mockLanguageBackend("zh-CN");
      const mod = await freshI18n();
      await mod.adoptPersisted();

      mod.changeLocale("en-US");
      mod.changeLocale("zh-CN");
      await settle();
      expect(pending.map((p) => p.language)).toEqual(["en-US"]);
      pending[0].reject(new Error("第一次写入失败"));
      await vi.waitFor(() => expect(mod.useAppLocale().persisting.value).toBe(false));
      // 最后一次选择等于已知落盘值（中文）→ 队列不再重复写，但绝不回滚成英文，
      // 也不把较早请求的失败留在界面上。
      expect(pending).toHaveLength(1);
      expect(mod.useAppLocale().locale.value).toBe("zh-CN");
      expect(state.stored).toBe("zh-CN");
      expect(mod.useAppLocale().persistError.value).toBeNull();
    },
  );
});
