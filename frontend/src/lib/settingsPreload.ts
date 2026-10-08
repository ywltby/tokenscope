import type { InjectionKey } from "vue";

export const SETTINGS_READ_COMMANDS = [
  "source_status",
  "cache_stats",
  "pricing_entries",
  "settings_get",
  "autostart_status",
] as const;
type ReadCommand = (typeof SETTINGS_READ_COMMANDS)[number];
type Result = { ok: true; value: unknown } | { ok: false; error: unknown };

/** 每个应用实例持有一次性预读；写操作与后续刷新始终绕过旧结果。 */
export function createSettingsPreload(invoke: (command: string) => Promise<unknown>) {
  const pending = new Map<ReadCommand, Promise<Result>>();
  let started = false;
  const startedAt = Date.now();
  return {
    start() {
      if (started) return;
      started = true;
      for (const command of SETTINGS_READ_COMMANDS) {
        // 即刻处理拒绝：用户尚未打开设置时也不能产生未处理 rejection。
        pending.set(
          command,
          invoke(command).then(
            (value) => ({ ok: true, value }),
            (error) => ({ ok: false, error }),
          ),
        );
      }
    },
    age: () => Date.now() - startedAt,
    async read<T>(command: ReadCommand): Promise<T> {
      const warm = pending.get(command);
      pending.delete(command);
      if (!warm) return (await invoke(command)) as T;
      const result = await warm;
      if (!result.ok) throw result.error;
      return result.value as T;
    },
  };
}

export const SETTINGS_PRELOAD: InjectionKey<ReturnType<typeof createSettingsPreload>> =
  Symbol("settings-preload");
