// R03：时区偏好校验——非法值回退默认，不抛异常导致首屏崩溃。
// U03：时区的**唯一权威来源是 settings.toml**（启动经 settings_get 读取、
// 修改经 settings_set_timezone 串行保存）；旧的 localStorage 键
// `tokenscope-tz` 只用于一次性迁移。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const LEGACY_KEY = "tokenscope-tz";

/** 按命令名分派的 IPC mock；未声明的命令返回 null。 */
function mockInvoke(handlers: Record<string, (args?: unknown) => unknown>): void {
  invokeMock.mockImplementation((cmd: string, args?: unknown) => {
    const h = handlers[cmd];
    if (!h) return Promise.resolve(null);
    try {
      return Promise.resolve(h(args));
    } catch (e) {
      return Promise.reject(e);
    }
  });
}

/** useTimezone 是模块级单例：每用例重置模块。 */
async function freshTimezone(): Promise<typeof import("./timezone")> {
  vi.resetModules();
  return await import("./timezone");
}

function savedCalls(): unknown[] {
  return invokeMock.mock.calls.filter((c) => c[0] === "settings_set_timezone");
}

beforeEach(() => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  invokeMock.mockReset();
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("timezone 唯一来源 settings.toml（U03）", () => {
  it("toml_value_wins：TOML 已有 timezone 时以 TOML 为准（含 local），删除旧键且不触发保存", async () => {
    mockInvoke({ settings_get: () => ({ timezone: "America/Los_Angeles" }) });
    localStorage.setItem(LEGACY_KEY, "UTC");
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value, "TOML 值被采用").toBe("America/Los_Angeles");
    expect(localStorage.getItem(LEGACY_KEY), "旧键不等保存即删除").toBeNull();
    expect(savedCalls(), "TOML 已有时区 → 不需要迁移保存").toEqual([]);
    expect(notice.value).toBeNull();
  });

  it("toml_local_value_accepted：TOML 里写 local 时原样放行（本机时区）", async () => {
    mockInvoke({ settings_get: () => ({ timezone: "local" }) });
    const mod = await freshTimezone();
    await mod.initTimezone();
    expect(mod.useTimezone().tz.value).toBe("local");
  });

  it("legacy_key_migrated：字段缺失且旧键合法 → 保存到 TOML，成功后删除旧键", async () => {
    mockInvoke({
      settings_get: () => ({ price_auto_sync: true }),
      settings_set_timezone: (args) => (args as { timezone: string }).timezone,
    });
    localStorage.setItem(LEGACY_KEY, "UTC");
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value, "本次会话使用旧值").toBe("UTC");
    expect(invokeMock).toHaveBeenCalledWith("settings_set_timezone", { timezone: "UTC" });
    expect(localStorage.getItem(LEGACY_KEY), "迁移成功后删除旧键").toBeNull();
    expect(notice.value).toBeNull();
  });

  it("migration_failure_keeps_legacy：保存失败则保留旧键、本次会话用旧值、显示非阻断提示", async () => {
    mockInvoke({
      settings_get: () => ({}),
      settings_set_timezone: () => Promise.reject(new Error("磁盘只读")),
    });
    localStorage.setItem(LEGACY_KEY, "UTC");
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value, "保存失败仍沿用旧值").toBe("UTC");
    expect(localStorage.getItem(LEGACY_KEY), "失败时保留旧键（下次启动可重试）").toBe("UTC");
    expect(notice.value, "必须给出非阻断提示").toBeTruthy();
    expect(notice.value).toContain("时区");
  });

  it("read_failure_uses_legacy_without_migrating：读取设置失败 → 用合法旧键，不迁移、不删除，提示", async () => {
    mockInvoke({ settings_get: () => Promise.reject(new Error("设置解析失败")) });
    localStorage.setItem(LEGACY_KEY, "UTC");
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value, "读取失败时用旧值").toBe("UTC");
    expect(savedCalls(), "读取失败不迁移").toEqual([]);
    expect(localStorage.getItem(LEGACY_KEY), "读取失败不删除旧键").toBe("UTC");
    expect(notice.value).toBeTruthy();
  });

  it("read_failure_without_legacy_uses_default：读取设置失败且无合法旧键 → 默认 Asia/Shanghai", async () => {
    mockInvoke({ settings_get: () => Promise.reject(new Error("设置解析失败")) });
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value).toBe("Asia/Shanghai");
    expect(notice.value).toBeTruthy();
  });

  it("missing_field_without_legacy_uses_default：字段缺失且无旧键 → 默认值，不保存、无提示", async () => {
    mockInvoke({ settings_get: () => ({ price_auto_sync: true }) });
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value).toBe("Asia/Shanghai");
    expect(savedCalls(), "没有旧键就没有可迁移的东西").toEqual([]);
    expect(notice.value).toBeNull();
  });

  it("illegal_stored_values_fall_back：TOML 非法值回退默认并提示；旧键非法同样回退默认", async () => {
    mockInvoke({ settings_get: () => ({ timezone: "Not/AZone" }) });
    localStorage.setItem(LEGACY_KEY, "UTC");
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, notice } = mod.useTimezone();
    expect(tz.value, "非法 TOML 值 → 默认").toBe("Asia/Shanghai");
    expect(localStorage.getItem(LEGACY_KEY), "TOML 为准 → 旧键删除").toBeNull();
    expect(notice.value).toBeTruthy();

    // 旧键非法：回退默认，不迁移、不报错（删除无意义的旧键）
    vi.resetModules();
    invokeMock.mockReset();
    mockInvoke({ settings_get: () => ({}) });
    localStorage.setItem(LEGACY_KEY, "Not/AZone");
    const mod2 = await freshTimezone();
    await mod2.initTimezone();
    expect(mod2.useTimezone().tz.value).toBe("Asia/Shanghai");
    expect(savedCalls()).toEqual([]);
  });

  it("init_is_idempotent：重复调用只读取一次设置", async () => {
    mockInvoke({ settings_get: () => ({ timezone: "UTC" }) });
    const mod = await freshTimezone();
    await Promise.all([mod.initTimezone(), mod.initTimezone()]);
    await mod.initTimezone();
    expect(invokeMock.mock.calls.filter((c) => c[0] === "settings_get")).toHaveLength(1);
  });

  it("save_serial_and_rollback：修改走串行保存，失败回滚到已落盘值并暴露局部错误", async () => {
    let fail = true;
    mockInvoke({
      settings_get: () => ({ timezone: "UTC" }),
      settings_set_timezone: (args) => {
        if (fail) return Promise.reject(new Error("写设置失败"));
        return (args as { timezone: string }).timezone;
      },
    });
    const mod = await freshTimezone();
    await mod.initTimezone();
    const { tz, saving, error } = mod.useTimezone();
    expect(tz.value).toBe("UTC");
    tz.value = "local";
    expect(tz.value, "界面立即显示最新选择").toBe("local");
    await flushPromises();
    expect(saving.value).toBe(false);
    expect(tz.value, "失败且无待保存值 → 回滚").toBe("UTC");
    expect(error.value).toContain("写设置失败");
    // 重试成功：值落盘，错误清空
    fail = false;
    tz.value = "local";
    await flushPromises();
    expect(tz.value).toBe("local");
    expect(error.value).toBeNull();
  });

  it("late_boot_result_does_not_overwrite_user_choice：启动读取晚于用户修改 → 不覆盖", async () => {
    let releaseSettings!: (v: unknown) => void;
    mockInvoke({
      settings_get: () => new Promise((r) => (releaseSettings = r)),
      settings_set_timezone: (args) => (args as { timezone: string }).timezone,
    });
    const mod = await freshTimezone();
    const booting = mod.initTimezone();
    const { tz } = mod.useTimezone();
    tz.value = "UTC"; // 用户在启动读取返回前就改了
    await flushPromises();
    expect(tz.value).toBe("UTC");
    releaseSettings({ timezone: "Asia/Shanghai" });
    await booting;
    expect(tz.value, "晚到的启动读取不得覆盖用户修改").toBe("UTC");
  });

  it("storage_unavailable_falls_back_and_never_throws：localStorage 抛错不阻断时区偏好（RC09）", async () => {
    // 隐私模式/策略禁用下访问 localStorage 直接抛错。读取只作迁移用途，读不到
    // 也不能抛——否则 composable 初始化即抛，组件 setup 崩溃、窗口空白。
    mockInvoke({
      settings_get: () => ({ timezone: "UTC" }),
      settings_set_timezone: (args) => (args as { timezone: string }).timezone,
    });
    const desc = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("storage blocked");
      },
    });
    try {
      const mod = await freshTimezone();
      await mod.initTimezone();
      const { tz } = mod.useTimezone();
      expect(tz.value, "存储不可用不影响 settings.toml 读取").toBe("UTC");
      tz.value = "local";
      await flushPromises();
      expect(tz.value).toBe("local");
    } finally {
      Object.defineProperty(window, "localStorage", desc ?? {});
    }
  });
});
