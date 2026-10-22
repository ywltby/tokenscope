// @vitest-environment node
import { expect, it } from "vitest";
import { observeCollectionCallbacks } from "./collection-publish-window.mjs";

it("原样转发回调，记录注册时刻与终态处理结束，恢复仍存活的回调", () => {
  let time = 0;
  const callbacks = new Map();
  const originalSet = callbacks.set;
  let received;
  const query = (value) => {
    received = value;
    callbacks.delete(2);
  };
  const status = () => {
    time = 50;
    callbacks.set(2, query);
    time = 51;
    return "return-value";
  };
  callbacks.set(1, status);
  const probe = observeCollectionCallbacks({ callbacks }, () => time);
  const event = {
    event: "collection-status",
    payload: { phase: "succeeded", runId: 4, latestGeneration: 2 },
  };
  expect(callbacks.get(1)(event)).toBe("return-value");
  time = 200;
  const result = { queryId: "new", generation: 2 };
  callbacks.get(2)(result);
  expect(received).toBe(result);
  expect(callbacks.has(2)).toBe(false);
  expect(probe.read()).toEqual([
    { kind: "status-enter", at_ms: 0, ...event.payload },
    { kind: "status-exit", at_ms: 51, ...event.payload },
    { kind: "query", at_ms: 50, resolved_ms: 200, queryId: "new", generation: 2 },
  ]);
  probe.dispose();
  expect(callbacks.set).toBe(originalSet);
  expect(callbacks.get(1)).toBe(status);
  expect(callbacks.has(2)).toBe(false);
});
it("回调抛错时保留原错误，不能记录成功的处理结束", () => {
  const error = new Error("failed handler");
  const callbacks = new Map([
    [
      1,
      () => {
        throw error;
      },
    ],
  ]);
  const probe = observeCollectionCallbacks({ callbacks }, () => 1);
  expect(() =>
    callbacks.get(1)({ event: "collection-status", payload: { phase: "succeeded" } }),
  ).toThrow(error);
  expect(probe.read().map((x) => x.kind)).toEqual(["status-enter"]);
  probe.dispose();
});
it("不支持的 Tauri 调试接口必须报错，不回退到轮询猜测", () => {
  expect(() => observeCollectionCallbacks({}, () => 1)).toThrow();
});
it("同一页面时钟可取得本轮 running 与 failed 的真实回调边界", () => {
  let time = 100;
  const callbacks = new Map([
    [
      1,
      () => {
        time += 0.25;
      },
    ],
  ]);
  const probe = observeCollectionCallbacks({ callbacks }, () => time);
  callbacks.get(1)({ event: "collection-status", payload: { phase: "running", runId: 2 } });
  time = 2600;
  callbacks.get(1)({ event: "collection-status", payload: { phase: "failed", runId: 2 } });
  expect(probe.read()).toEqual([
    { kind: "status-enter", at_ms: 100, phase: "running", runId: 2 },
    { kind: "status-exit", at_ms: 100.25, phase: "running", runId: 2 },
    { kind: "status-enter", at_ms: 2600, phase: "failed", runId: 2 },
    { kind: "status-exit", at_ms: 2600.25, phase: "failed", runId: 2 },
  ]);
  probe.dispose();
});
