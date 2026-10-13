// U03：设置项串行保存队列（时区与读取间隔共用同一机制）。
// 语义：同一时刻最多一个保存请求在途；在途期间的新选择只保留最新（待保存
// 值）；在途请求结束后若待保存值与已落盘值不同再发一次；界面显示用户最新
// 选择；全部保存失败且没有待保存值时回滚到最后一次成功落盘的值并暴露错误。
import { describe, expect, it } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { createSettingQueue, type SettingSaver } from "./settingQueue";

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (v: T) => void;
  reject: (e: unknown) => void;
} {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** 记录每次保存调用，并给出可逐个放行/拒绝的闸门（控制完成时序）。 */
function gatedSaver<T extends string>() {
  const calls: T[] = [];
  const gates: { resolve: (v: T) => void; reject: (e: unknown) => void }[] = [];
  const save: SettingSaver<T> = (value) => {
    calls.push(value);
    const d = deferred<T>();
    gates.push({ resolve: d.resolve, reject: d.reject });
    return d.promise;
  };
  return { calls, gates, save };
}

describe("settingQueue 串行保存（U03）", () => {
  it("burst_keeps_latest_only：依次选 A、B、C 且第一次保存延迟 → 只发 A 与 C，最终发送 C", async () => {
    const { calls, gates, save } = gatedSaver<string>();
    const queue = createSettingQueue<string>({ save, persisted: "A0" });
    queue.set("A");
    queue.set("B");
    queue.set("C");
    expect(calls, "在途期间不得并发第二个请求").toEqual(["A"]);
    expect(queue.value.value, "界面显示用户最新选择（不回退到在途值）").toBe("C");
    expect(queue.saving.value).toBe(true);

    gates[0].resolve("A");
    await flushPromises();
    expect(calls, "B 被 C 覆盖：只发 A 与 C").toEqual(["A", "C"]);
    expect(queue.value.value).toBe("C");

    gates[1].resolve("C");
    await queue.idle();
    expect(queue.saving.value).toBe(false);
    expect(queue.error.value).toBeNull();
  });

  it("save_persists_last_value：连续两次选择都落盘，最终值 = 最后一次选择", async () => {
    const { calls, gates, save } = gatedSaver<string>();
    const queue = createSettingQueue<string>({ save, persisted: "Asia/Shanghai" });
    queue.set("UTC");
    await flushPromises();
    gates[0].resolve("UTC");
    await queue.idle();
    queue.set("local");
    await flushPromises();
    gates[1].resolve("local");
    await queue.idle();
    expect(calls).toEqual(["UTC", "local"]);
    expect(queue.value.value).toBe("local");
  });

  it("failure_without_pending_rolls_back：保存失败且无待保存值 → 回滚到最后成功落盘的值并暴露错误", async () => {
    const { gates, save } = gatedSaver<string>();
    const queue = createSettingQueue<string>({ save, persisted: "Asia/Shanghai" });
    queue.set("UTC");
    expect(queue.value.value).toBe("UTC");
    gates[0].reject(new Error("写设置失败"));
    await queue.idle();
    expect(queue.value.value, "回滚到已落盘值").toBe("Asia/Shanghai");
    expect(queue.error.value).toContain("写设置失败");
    expect(queue.saving.value).toBe(false);
  });

  it("failure_with_pending_keeps_user_choice：失败但仍有更新的待保存值 → 不回滚，继续保存最新值", async () => {
    const { calls, gates, save } = gatedSaver<string>();
    const queue = createSettingQueue<string>({ save, persisted: "Asia/Shanghai" });
    queue.set("UTC");
    queue.set("local");
    gates[0].reject(new Error("写设置失败"));
    await flushPromises();
    expect(queue.value.value, "仍在重试最新选择：不回滚").toBe("local");
    expect(calls).toEqual(["UTC", "local"]);
    gates[1].resolve("local");
    await queue.idle();
    expect(queue.error.value, "后续保存成功 → 错误清空").toBeNull();
    expect(queue.value.value).toBe("local");
  });

  it("same_value_skips_save：与已落盘值相同不重复发请求；adopt 只设基线不发保存", async () => {
    const { calls, gates, save } = gatedSaver<string>();
    const queue = createSettingQueue<string>({ save, persisted: "Asia/Shanghai" });
    queue.set("Asia/Shanghai");
    await queue.idle();
    expect(calls, "与已落盘值相同 → 不发请求").toEqual([]);
    queue.adopt("UTC");
    expect(queue.value.value).toBe("UTC");
    await queue.idle();
    expect(calls).toEqual([]);
    queue.set("local");
    gates[0].resolve("local");
    await queue.idle();
    expect(calls).toEqual(["local"]);
  });
});
