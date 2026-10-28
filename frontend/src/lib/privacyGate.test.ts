// P04/P05：隐私引导状态模块的单元测试（不含组件，直接验证门的行为）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { BOOTSTRAP_SLOW_MS, createPrivacyGate, type PrivacySnapshot } from "./privacyGate";

const POLICY = {
  title: "TokenScope 隐私政策",
  date: "2026 年 10 月 9 日",
  markdown: "# TokenScope 隐私政策\n\n正文示例。\n",
};

function snapshot(
  phase: PrivacySnapshot["phase"],
  extra: Partial<PrivacySnapshot> = {},
): PrivacySnapshot {
  return {
    phase,
    detail: null,
    exitPromptPending: false,
    diagnostics: [],
    policy: POLICY,
    ...extra,
  };
}

describe("P04/P05 隐私引导门（状态模块）", () => {
  it("repeated_accept_is_single_flight：并发/重复同意只发一次写事务", async () => {
    const resolvers: ((value: unknown) => void)[] = [];
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "privacy_bootstrap") return Promise.resolve(snapshot("needs_consent"));
      if (cmd === "privacy_accept") return new Promise((resolve) => resolvers.push(resolve));
      return Promise.resolve(null);
    });
    const gate = createPrivacyGate(invoke);
    await gate.bootstrap();

    const first = gate.accept();
    await gate.accept(); // 第二次直接返回（单飞）
    const acceptCalls = () => invoke.mock.calls.filter((c) => c[0] === "privacy_accept").length;
    expect(acceptCalls()).toBe(1);
    expect(gate.phase.value).toBe("saving_consent");

    resolvers[0](snapshot("ready"));
    await first;
    expect(gate.phase.value).toBe("ready");

    // Ready 之后重复点击不再写事务
    await gate.accept();
    expect(acceptCalls()).toBe(1);
  });

  it("close_while_saving_does_not_race_accept：保存中不穿插关闭动作", async () => {
    const resolvers: ((value: unknown) => void)[] = [];
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "privacy_bootstrap") return Promise.resolve(snapshot("needs_consent"));
      if (cmd === "privacy_accept") return new Promise((resolve) => resolvers.push(resolve));
      return Promise.resolve(null);
    });
    const gate = createPrivacyGate(invoke);
    await gate.bootstrap();

    const pending = gate.accept();
    expect(gate.phase.value).toBe("saving_consent");
    gate.onExitRequested(); // 保存期间的窗口 X
    expect(gate.exitPromptOpen.value).toBe(false);

    resolvers[0](snapshot("blocked_error", { detail: "保存失败（合成）" }));
    await pending;
    // 保存落定（仍未解锁）后可继续关闭：确认框此时才出现
    expect(gate.exitPromptOpen.value).toBe(true);
    expect(gate.exitError.value).toBeNull();
  });

  it("accept_failure_keeps_policy_and_gate_closed：保存失败保持封闭且可重试", async () => {
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "privacy_bootstrap") return Promise.resolve(snapshot("needs_consent"));
      if (cmd === "privacy_accept")
        return Promise.reject("privacy_consent_required：磁盘满（合成）");
      return Promise.resolve(null);
    });
    const gate = createPrivacyGate(invoke);
    await gate.bootstrap();
    await gate.accept();
    expect(gate.phase.value).toBe("blocked_error");
    expect(gate.error.value).toContain("保存同意记录失败");
    expect(gate.blocked.value).toBe(true);
    expect(gate.consentVisible.value).toBe(true);
    // 失败后可继续重试：再点一次仍会发 IPC（不是"成功过就永久放行"）
    await gate.accept();
    expect(invoke.mock.calls.filter((c) => c[0] === "privacy_accept")).toHaveLength(2);
  });

  it("bootstrap_failure_keeps_gate_closed_and_retryable：检查失败不放行", async () => {
    const invoke = vi.fn((_cmd: string) => Promise.reject("ipc 不可用（合成）"));
    const gate = createPrivacyGate(invoke);
    await gate.bootstrap();
    expect(gate.phase.value).toBe("checking");
    expect(gate.error.value).toContain("无法检查隐私设置");
    expect(gate.consentVisible.value).toBe(false);
    expect(gate.blocked.value).toBe(true);
    expect(gate.policy.value.markdown).toBe("");
    // 重试走同一入口（幂等，不产生业务调用）
    gate.retry();
    expect(invoke.mock.calls.filter((c) => c[0] === "privacy_bootstrap")).toHaveLength(2);
  });

  it("exit_confirmation_clears_backend_flag_and_resends_business_state", async () => {
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "privacy_bootstrap") {
        return Promise.resolve(snapshot("needs_consent", { exitPromptPending: true }));
      }
      return Promise.resolve(null);
    });
    const gate = createPrivacyGate(invoke);
    await gate.bootstrap();
    // 监听前到达的关闭请求由 bootstrap 恢复
    expect(gate.exitPromptOpen.value).toBe(true);

    gate.cancelExit();
    expect(gate.exitPromptOpen.value).toBe(false);
    await Promise.resolve();
    expect(invoke).toHaveBeenLastCalledWith("privacy_exit_resolve", { exit: false });

    // 确认退出：单飞，重复点击只发一次
    await gate.confirmExit();
    expect(invoke).toHaveBeenLastCalledWith("privacy_exit_resolve", { exit: true });
    expect(gate.exitBusy.value).toBe(false);
  });

  it("ready_snapshot_closes_pending_exit_prompt：同意成功后不残留遮挡", async () => {
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "privacy_bootstrap") {
        return Promise.resolve(snapshot("needs_consent", { exitPromptPending: true }));
      }
      if (cmd === "privacy_accept") return Promise.resolve(snapshot("ready"));
      return Promise.resolve(null);
    });
    const gate = createPrivacyGate(invoke);
    await gate.bootstrap();
    expect(gate.exitPromptOpen.value).toBe(true);
    await gate.accept();
    expect(gate.phase.value).toBe("ready");
    expect(gate.exitPromptOpen.value).toBe(false);
    expect(gate.consentVisible.value).toBe(false);
  });
});

describe("BS01 检查耗时提示与代次保护", () => {
  /** 假计时器下排空微任务：响应链上的 await 都走完。 */
  async function flush(): Promise<void> {
    for (let i = 0; i < 6; i++) await Promise.resolve();
  }

  afterEach(() => {
    vi.useRealTimers();
  });

  it("slow_hint_is_not_error：挂起超过阈值只提示不报错", async () => {
    vi.useFakeTimers();
    const invoke = vi.fn((_cmd: string) => new Promise<unknown>(() => {}));
    const gate = createPrivacyGate(invoke);
    void gate.bootstrap();
    await vi.advanceTimersByTimeAsync(BOOTSTRAP_SLOW_MS);
    expect(gate.slow.value).toBe(true);
    expect(gate.error.value).toBeNull();
    expect(gate.phase.value).toBe("checking");
  });

  it("late_ready_after_slow_clears_hint：超时提示后原请求返回 ready，提示清除", async () => {
    vi.useFakeTimers();
    let resolveBoot!: (value: unknown) => void;
    const invoke = vi.fn((_cmd: string) => new Promise((r) => (resolveBoot = r)));
    const gate = createPrivacyGate(invoke);
    void gate.bootstrap();
    await vi.advanceTimersByTimeAsync(BOOTSTRAP_SLOW_MS);
    expect(gate.slow.value).toBe(true);
    resolveBoot(snapshot("ready"));
    await flush();
    expect(gate.slow.value).toBe(false);
    expect(gate.phase.value).toBe("ready");
  });

  it("stale_success_ignored：迟到的 needs_consent 不把 ready 退回去", async () => {
    vi.useFakeTimers();
    const resolvers: ((value: unknown) => void)[] = [];
    const invoke = vi.fn((_cmd: string) => new Promise((r) => resolvers.push(r)));
    const gate = createPrivacyGate(invoke);
    void gate.bootstrap(); // 第一次：挂起
    void gate.retry(); // 第二次：返回 ready
    resolvers[1](snapshot("ready"));
    await flush();
    expect(gate.phase.value).toBe("ready");
    resolvers[0](snapshot("needs_consent")); // 第一次迟到
    await flush();
    expect(gate.phase.value).toBe("ready");
  });

  it("stale_failure_ignored：迟到的失败不写入错误", async () => {
    vi.useFakeTimers();
    const settles: { resolve: (value: unknown) => void; reject: (e: unknown) => void }[] = [];
    const invoke = vi.fn(
      (_cmd: string) =>
        new Promise((resolve, reject) => {
          settles.push({ resolve, reject });
        }),
    );
    const gate = createPrivacyGate(invoke);
    void gate.bootstrap(); // 第一次：挂起
    void gate.retry(); // 第二次：ready
    settles[1].resolve(snapshot("ready"));
    await flush();
    expect(gate.phase.value).toBe("ready");
    settles[0].reject("第一次检查迟到失败（合成）");
    await flush();
    expect(gate.error.value).toBeNull();
    expect(gate.phase.value).toBe("ready");
  });

  it("accept_invalidates_pending_bootstrap：同意作废在途检查，迟到响应不回退", async () => {
    vi.useFakeTimers();
    const settles = new Map<
      string,
      { resolve: (value: unknown) => void; reject: (e: unknown) => void }[]
    >();
    const invoke = vi.fn(
      (cmd: string) =>
        new Promise((resolve, reject) => {
          const list = settles.get(cmd) ?? [];
          list.push({ resolve, reject });
          settles.set(cmd, list);
        }),
    );
    const gate = createPrivacyGate(invoke);
    void gate.bootstrap();
    settles.get("privacy_bootstrap")![0].resolve(snapshot("blocked_error"));
    await flush();
    expect(gate.phase.value).toBe("blocked_error");
    void gate.retry(); // 挂起
    expect(settles.get("privacy_bootstrap")).toHaveLength(2);
    const saving = gate.accept(); // 作废在途 bootstrap
    settles.get("privacy_accept")![0].resolve(snapshot("ready"));
    await saving;
    await flush();
    expect(gate.phase.value).toBe("ready");
    settles.get("privacy_bootstrap")![1].resolve(snapshot("needs_consent")); // 迟到
    await flush();
    expect(gate.phase.value).toBe("ready");
  });
});

describe("十八轮复核 P3：政策已更新提示不重复显示为错误", () => {
  it("needs_consent 的 detail 只作为 updateNotice，不进入 message", async () => {
    const invoke = vi.fn((cmd: string) => {
      if (cmd === "privacy_bootstrap")
        return Promise.resolve(
          snapshot("needs_consent", { detail: "政策已更新，请重新阅读并同意" }),
        );
      if (cmd === "privacy_load_policy") return Promise.resolve(POLICY);
      return Promise.resolve(snapshot("needs_consent"));
    });
    const gate = createPrivacyGate(invoke as never);
    await gate.bootstrap();
    expect(gate.policyUpdated.value).toBe("政策已更新，请重新阅读并同意");
    expect(gate.error.value).toBeNull();
    expect(gate.blockedDetail.value).toBeNull();
  });
});
