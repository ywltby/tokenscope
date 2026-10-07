import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  enqueueSnapshotSave,
  snapshotQueueIdle,
  resetSnapshotQueueForTests,
  lastSnapshotError,
  SNAPSHOT_VERSION,
} from "./viewSnapshot";

beforeEach(() => {
  resetSnapshotQueueForTests();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("视图快照保存队列（R04）", () => {
  it("latest_snapshot_wins_out_of_order_saves：串行派发，磁盘最终 = 最新入队", async () => {
    const dispatched: unknown[] = [];
    let resolveA!: () => void;
    const dispatch = vi.fn((p: unknown) => {
      dispatched.push(p);
      if (dispatched.length === 1) {
        return new Promise<void>((r) => {
          resolveA = r;
        });
      }
      return Promise.resolve();
    });
    const a = { v: SNAPSHOT_VERSION, tag: "A" };
    const b = { v: SNAPSHOT_VERSION, tag: "B" };
    enqueueSnapshotSave(a, dispatch);
    // 让 drain 微任务跑起来：A 真正进入在途（gate 未释放）
    await Promise.resolve();
    enqueueSnapshotSave(b, dispatch); // A 在途时入队 B → 排下一轮
    expect(dispatched.map((x) => (x as { tag: string }).tag)).toEqual(["A"]);
    resolveA();
    await snapshotQueueIdle();
    // A（旧）先完成，B 后派发——最终磁盘状态 = B（晚完成不能倒序覆盖）
    expect(dispatched.map((x) => (x as { tag: string }).tag)).toEqual(["A", "B"]);
  });

  it("在途时重复入队只保留最新 payload（合并）", async () => {
    const dispatched: unknown[] = [];
    let release!: () => void;
    const gate = new Promise<void>((r) => {
      release = r;
    });
    const dispatch = vi.fn((p: unknown) => {
      dispatched.push(p);
      if (dispatched.length === 1) return gate;
      return Promise.resolve();
    });
    enqueueSnapshotSave({ n: 1 }, dispatch);
    await Promise.resolve(); // 第一笔进入在途
    // 在途时连入三笔 → 合并为一笔（最新）
    enqueueSnapshotSave({ n: 2 }, dispatch);
    enqueueSnapshotSave({ n: 3 }, dispatch);
    enqueueSnapshotSave({ n: 4 }, dispatch);
    release();
    await snapshotQueueIdle();
    expect(dispatched.map((x) => (x as { n: number }).n)).toEqual([1, 4]);
  });

  it("IPC 失败不产生未处理 rejection，不阻止后续保存", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    let fail = true;
    const dispatch = vi.fn(() =>
      fail ? Promise.reject(new Error("disk full")) : Promise.resolve(),
    );
    enqueueSnapshotSave({ n: 1 }, dispatch);
    await snapshotQueueIdle();
    expect(lastSnapshotError()).toBeInstanceOf(Error);
    fail = false;
    enqueueSnapshotSave({ n: 2 }, dispatch);
    await snapshotQueueIdle();
    expect(lastSnapshotError()).toBeNull();
    expect(dispatch).toHaveBeenCalledTimes(2);
    expect(warn).toHaveBeenCalled();
  });

  it("SNAPSHOT_VERSION 为 5（v4 及更旧快照读取时忽略）", () => {
    expect(SNAPSHOT_VERSION).toBe(5);
  });
});
