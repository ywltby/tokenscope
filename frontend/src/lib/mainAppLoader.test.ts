// BS02：主界面动态加载器的单元测试（导入失败可见、就绪丢失不复活、并发单飞）。
import { describe, expect, it, vi } from "vitest";
import { createMainAppLoader } from "./mainAppLoader";

describe("BS02 主界面加载器", () => {
  it("load_failure_records_error_and_stays_hidden：导入失败记录原因并保持隐藏", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const loader = createMainAppLoader({
      load: () => Promise.reject(new Error("Failed to fetch dynamically imported module")),
      isReady: () => true,
    });
    await loader.enter();
    expect(loader.error.value).toBe("Failed to fetch dynamically imported module");
    expect(loader.visible.value).toBe(false);
    expect(consoleError).toHaveBeenCalledTimes(1);
    expect(consoleError.mock.calls[0][0]).toContain("主界面加载失败");
  });

  it("readiness_lost_does_not_show：导入成功但就绪已丢失时不挂载", async () => {
    let ready = true;
    const loader = createMainAppLoader({
      load: async () => {
        ready = false; // 导入期间进入退出流程
      },
      isReady: () => ready,
    });
    await loader.enter();
    expect(loader.error.value).toBeNull();
    expect(loader.visible.value).toBe(false);
  });

  it("success_shows_main：导入成功且就绪保持时可见", async () => {
    const loader = createMainAppLoader({
      load: async () => undefined,
      isReady: () => true,
    });
    await loader.enter();
    expect(loader.visible.value).toBe(true);
    expect(loader.error.value).toBeNull();
  });

  it("concurrent_enter_is_single_flight：并发调用共用同一次加载", async () => {
    let calls = 0;
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const loader = createMainAppLoader({
      load: () => {
        calls++;
        return pending;
      },
      isReady: () => true,
    });
    const first = loader.enter();
    const second = loader.enter();
    release();
    await Promise.all([first, second]);
    expect(calls).toBe(1);
    expect(loader.visible.value).toBe(true);
  });
});
