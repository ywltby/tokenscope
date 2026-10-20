// 迟到 bootstrap 响应的落定等待：落定才放行，超时必须抛错——
// 「旧响应不落定时契约必须失败」的反例在这里固化（复核意见 P2-2）。
import { describe, expect, it } from "vitest";
import { waitForIpcSettled } from "./boot-stall-wait.mjs";

describe("waitForIpcSettled", () => {
  it("已落定的调用立即返回 outcome，不再等待", async () => {
    const sleeps = [];
    const outcome = await waitForIpcSettled(
      { outcome: "ok" },
      { sleep: async (ms) => sleeps.push(ms), now: () => 1000 },
    );
    expect(outcome).toBe("ok");
    expect(sleeps).toEqual([]);
  });

  it("挂起的调用在期限内落定后返回 outcome", async () => {
    const call = { outcome: undefined };
    let clock = 1000;
    const outcome = await waitForIpcSettled(call, {
      timeoutMs: 5000,
      intervalMs: 50,
      sleep: async () => {
        clock += 50;
        if (clock >= 1200) call.outcome = "ok";
      },
      now: () => clock,
    });
    expect(outcome).toBe("ok");
  });

  it("反例：响应始终不落定时必须抛错（契约失败，不得继续断言界面状态）", async () => {
    const call = { outcome: undefined };
    let clock = 1000;
    await expect(
      waitForIpcSettled(call, {
        timeoutMs: 5000,
        intervalMs: 50,
        sleep: async () => {
          clock += 50;
        },
        now: () => clock,
      }),
    ).rejects.toThrow(/未在 Node 侧落定.*无法证明代次保护/s);
  });

  it("抛错信息包含超时预算，便于失败取证定位", async () => {
    let clock = 1000;
    await expect(
      waitForIpcSettled({ outcome: undefined }, { timeoutMs: 1234, sleep: async () => { clock += 50; }, now: () => clock }),
    ).rejects.toThrow("1234ms");
  });
});
