import { describe, expect, it, vi } from "vitest";
import { createSettingsPreload, SETTINGS_READ_COMMANDS } from "./settingsPreload";

describe("设置后台预读", () => {
  it("启动时读完所有区块，首次消费复用结果，重试重新读取", async () => {
    const read = vi.fn(async (command: string) => ({ command }));
    const preload = createSettingsPreload(read);
    preload.start();
    expect(read.mock.calls.map(([command]) => command)).toEqual([...SETTINGS_READ_COMMANDS]);
    await expect(preload.read("settings_get")).resolves.toEqual({ command: "settings_get" });
    expect(read).toHaveBeenCalledTimes(5);
    await preload.read("settings_get");
    expect(read).toHaveBeenCalledTimes(6);
  });

  it("进入页面复用仍在途的读取，失败可见且下次读取可恢复", async () => {
    let reject!: (reason: Error) => void;
    const pending = new Promise((_, fail) => {
      reject = fail;
    });
    const read = vi.fn(async (command: string) => (command === "settings_get" ? pending : true));
    const preload = createSettingsPreload(read);
    preload.start();
    reject(new Error("配置暂不可读"));
    await Promise.resolve();
    await expect(preload.read("settings_get")).rejects.toThrow("配置暂不可读");
    expect(read).toHaveBeenCalledTimes(5);
    read.mockResolvedValueOnce(true);
    await expect(preload.read("settings_get")).resolves.toBe(true);
  });
});
