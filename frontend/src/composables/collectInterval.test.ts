// U06（UI 调整计划 2026-10-13）：自动读取间隔——值映射与串行保存。
//
// 间隔的唯一来源是 `settings.toml` 的 `collect_interval_secs`；写入走 U03 的
// 串行保存队列（同一时刻只有一个保存在途、在途选择只留最新、失败回滚到最后
// 落盘值并暴露局部错误）。设置页与状态胶囊的接线分别由 Settings.test.ts /
// Dashboard.test.ts 覆盖。
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  COLLECT_INTERVAL_OPTIONS,
  DEFAULT_COLLECT_INTERVAL_SECS,
  adoptCollectInterval,
  collectIntervalText,
  resetCollectIntervalForTests,
  useCollectInterval,
} from "./collectInterval";

/** 可控完成时序的挂起保存。 */
function deferredSave(): { gates: ((v: number) => void)[] } {
  const gates: ((v: number) => void)[] = [];
  invokeMock.mockImplementation(() => new Promise<number>((r) => gates.push(r)));
  return { gates };
}

function savedArgs(): Record<string, unknown>[] {
  return invokeMock.mock.calls.map((c) => c[1] as Record<string, unknown>);
}

beforeEach(() => {
  invokeMock.mockReset();
  resetCollectIntervalForTests();
});

describe("自动读取间隔：白名单与文案（U06）", () => {
  it("interval_options_match_backend_whitelist：可选值与文案只有后端白名单那六种", () => {
    expect(COLLECT_INTERVAL_OPTIONS.map((o) => o.value)).toEqual([5, 10, 30, 60, 300, 600]);
    expect(COLLECT_INTERVAL_OPTIONS.map((o) => o.label)).toEqual([
      "5 秒",
      "10 秒",
      "30 秒",
      "1 分钟",
      "5 分钟",
      "10 分钟",
    ]);
    expect(collectIntervalText(5)).toBe("5 秒");
    expect(collectIntervalText(30)).toBe("30 秒");
    expect(collectIntervalText(60)).toBe("1 分钟");
    expect(collectIntervalText(600)).toBe("10 分钟");
    expect(DEFAULT_COLLECT_INTERVAL_SECS).toBe(300);
    expect(useCollectInterval().intervalSecs.value, "缺省与后端一致").toBe(300);
  });

  it("adopt_from_settings_takes_authoritative_value_and_falls_back：权威值来自 settings_get，非法值回缺省", () => {
    adoptCollectInterval(60);
    expect(useCollectInterval().intervalSecs.value).toBe(60);
    adoptCollectInterval(7); // 白名单外
    expect(useCollectInterval().intervalSecs.value).toBe(300);
    adoptCollectInterval(undefined); // 旧设置文件没有该字段
    expect(useCollectInterval().intervalSecs.value).toBe(300);
  });
});

describe("自动读取间隔：串行保存（U06）", () => {
  it("in_flight_selection_sends_first_and_latest_only：在途只留最新，不发中间值", async () => {
    const { gates } = deferredSave();
    const { intervalSecs } = useCollectInterval();

    intervalSecs.value = 5;
    intervalSecs.value = 10;
    intervalSecs.value = 30;
    expect(invokeMock, "同一时刻只有一个保存在途").toHaveBeenCalledTimes(1);
    expect(savedArgs()[0]).toEqual({ secs: 5 });
    expect(intervalSecs.value, "界面显示用户最新选择").toBe(30);

    gates[0](5);
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    expect(savedArgs()[1], "在途结束后补写最新值").toEqual({ secs: 30 });

    gates[1](30);
    await useCollectInterval().idle();
    expect(useCollectInterval().error.value).toBeNull();
    expect(useCollectInterval().intervalSecs.value).toBe(30);
  });

  it("failure_rolls_back_to_persisted_and_exposes_local_error：保存失败回滚到已落盘值并给出局部错误", async () => {
    adoptCollectInterval(300);
    invokeMock.mockRejectedValue(new Error("写入设置失败：磁盘不可写"));
    const { intervalSecs, error } = useCollectInterval();

    intervalSecs.value = 5;
    await useCollectInterval().idle();

    expect(intervalSecs.value, "回滚到最后一次成功落盘的值").toBe(300);
    expect(error.value).toBe("写入设置失败：磁盘不可写");
  });

  it("late_settings_read_does_not_override_user_selection：读取结果晚于用户修改时不覆盖选择", async () => {
    const { gates } = deferredSave();
    const { intervalSecs } = useCollectInterval();

    intervalSecs.value = 10; // 用户选择，保存在途
    adoptCollectInterval(300); // 晚到的读取结果
    expect(intervalSecs.value, "以用户选择为准").toBe(10);

    gates[0](10);
    await useCollectInterval().idle();
    expect(useCollectInterval().intervalSecs.value).toBe(10);
  });
});
