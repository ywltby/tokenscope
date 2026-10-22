// @vitest-environment node
import { describe, expect, it } from "vitest";
import { EventEmitter } from "node:events";
import { correlateNativeQuery, matchEventDom, nextQueryDimension, observeNativeInvokes } from "./native-unified.mjs";

it("重启后没有已捕获 DTO 也必须切离真实选中的按时间", () => {
  expect(nextQueryDimension("按时间")).toBe("project");
  expect(nextQueryDimension("按项目")).toBe("day");
  expect(() => nextQueryDimension(null)).toThrow();
});

it("当前页面的开始参数必须按查询身份关联，不能取最后发起的另一个查询", () => {
  const old = { command: "query_begin", status: "fulfilled", args: { agent: "all" }, value: { queryId: "shown" } };
  const next = { command: "query_begin", status: "fulfilled", args: { agent: "claude" }, value: { queryId: "pending-ui" } };
  const snapshot = correlateNativeQuery({ summaryQuery: "shown", eventsQuery: "shown", records: [old, next] });
  expect(snapshot.beginCall).toEqual(old);
});

describe("真实 IPC 旁路观察", () => {
  it("明细DOM行、总条数、已加载条数与真实DTO前八列逐项对应", () => {
    const events = {
      total: 1,
      rows: [
        {
          ts: "2026-10-06T01:00:00Z",
          agent: "claude-code",
          model: "claude-opus-5-5",
          project: "C:/workspace/tokenscope",
          input: 1000,
          output: 20,
          cache_write: 3,
          cache_read: 10,
        },
      ],
    };
    const dom = {
      total: 1,
      loaded: 1,
      rows: [
        [
          "2026-10-06T01:00:00Z",
          "Claude Code",
          "claude-opus-5-5",
          "tokenscope",
          "1,013",
          "20",
          "3",
          "10",
        ],
      ],
    };
    expect(matchEventDom(events, dom)).toBe(true);
    for (const mutate of [
      (d) => d.rows.push([...d.rows[0]]),
      (d) => (d.total = 10),
      (d) => (d.loaded = 10),
      (d) => (d.rows[0][2] = "old-model"),
      (d) => (d.rows[0][4] = "1,000"),
    ]) {
      const wrong = structuredClone(dom);
      mutate(wrong);
      expect(matchEventDom(events, wrong)).toBe(false);
    }
    expect(matchEventDom(events, null)).toBe(false);
  });
  it("Tauri 真实只读描述符不阻碍旁路观察，保护属性不被修改", () => {
    const page = new EventEmitter();
    const invoke = () => Promise.resolve("real");
    Object.defineProperty(page, "invoke", { value: invoke });
    const descriptor = Object.getOwnPropertyDescriptor(page, "invoke");
    let probe;
    expect(() => {
      probe = observeNativeInvokes(page, () => 1);
    }).not.toThrow();
    probe.dispose();
    expect(Object.getOwnPropertyDescriptor(page, "invoke")).toEqual(descriptor);
  });
  it("保留原 Promise、this、参数与结果，只旁路观察传输并记录可控时钟", async () => {
    let clock = 12;
    let resolve;
    const promise = new Promise((done) => (resolve = done));
    const page = new EventEmitter();
    const internals = {
      invoke(command, args, options) {
        expect(this).toBe(internals);
        expect([command, args, options]).toEqual(["query_summary", { queryId: "q1" }, { x: 1 }]);
        return promise;
      },
    };
    const original = internals.invoke;
    const probe = observeNativeInvokes(page, () => clock);
    expect(internals.invoke("query_summary", { queryId: "q1" }, { x: 1 })).toBe(promise);
    const request = {
      url: () => "http://ipc.localhost/query_summary",
      postDataJSON: () => ({ queryId: "q1" }),
    };
    page.emit("request", request);
    clock = 19;
    resolve({ query_id: "q1", groups: [] });
    await promise;
    page.emit("response", {
      request: () => request,
      json: () => promise,
      status: () => 200,
      ok: () => true,
      headers: () => ({ "tauri-response": "ok" }),
    });
    await new Promise(setImmediate);
    expect(probe.read()).toEqual([
      {
        command: "query_summary",
        args: { queryId: "q1" },
        status: "fulfilled",
        started_ms: 12,
        settled_ms: 19,
        value: { query_id: "q1", groups: [] },
        error: null,
        http_status: 200,
      },
    ]);
    probe.dispose();
    expect(internals.invoke).toBe(original);
  });

  it("真实后端拒绝和传输失败均留证据；dispose 只移除本探针监听", async () => {
    const failure = new Error("backend rejected");
    const page = new EventEmitter();
    const internals = { invoke: () => Promise.reject(failure) };
    const probe = observeNativeInvokes(page, () => 20);
    await expect(internals.invoke("ccs_import_commit", { planId: "p" })).rejects.toBe(failure);
    const request = {
      url: () => "http://ipc.localhost/ccs_import_commit",
      postDataJSON: () => ({ planId: "p" }),
    };
    page.emit("request", request);
    page.emit("response", {
      request: () => request,
      json: async () => "backend rejected",
      status: () => 400,
      ok: () => false,
      headers: () => ({ "tauri-response": "error" }),
    });
    await new Promise(setImmediate);
    expect(probe.read()[0]).toMatchObject({ status: "rejected", error: "backend rejected" });
    const failed = {
      url: () => "ipc://localhost/query_events",
      postDataJSON: () => ({ queryId: "q" }),
      failure: () => ({ errorText: "net::ERR_FAILED" }),
    };
    page.emit("request", failed);
    page.emit("requestfailed", failed);
    expect(probe.read()[1]).toMatchObject({
      status: "rejected",
      error: "net::ERR_FAILED",
      settled_ms: 20,
    });
    const other = () => {};
    page.on("request", other);
    probe.dispose();
    expect(page.listeners("request")).toEqual([other]);
    expect(page.listeners("response")).toEqual([]);
    expect(page.listeners("requestfailed")).toEqual([]);
  });

  it("只关联 DOM 当前 query_id，且不会把不同筛选或旧查询的响应算作当前结果", () => {
    const rows = [{ record_id: "one" }];
    const records = [
      {
        command: "query_summary",
        args: { queryId: "old" },
        status: "fulfilled",
        value: { query_id: "old" },
      },
      {
        command: "query_summary",
        args: { queryId: "new" },
        status: "fulfilled",
        value: { query_id: "new" },
      },
      {
        command: "query_events",
        args: { queryId: "new", model: null },
        status: "fulfilled",
        value: { query_id: "new", rows: [] },
      },
      {
        command: "query_events",
        args: { queryId: "new", model: "opus" },
        status: "fulfilled",
        value: { query_id: "new", rows },
      },
      {
        command: "query_events",
        args: { queryId: "other" },
        status: "fulfilled",
        value: { query_id: "other" },
      },
    ];
    const snapshot = correlateNativeQuery({ summaryQuery: "new", eventsQuery: "new", records });
    expect(snapshot).toMatchObject({ summary: { query_id: "new" } });
    expect(snapshot.summary.query_id).toBe("new");
    expect(snapshot.events.rows).toEqual(rows);
    expect(snapshot.eventArgs.model).toBe("opus");
    expect(snapshot.sameQuery).toBe(true);
    expect(
      correlateNativeQuery({ summaryQuery: "new", eventsQuery: "other", records }).sameQuery,
    ).toBe(false);
    expect(
      correlateNativeQuery({ summaryQuery: "absent", eventsQuery: "absent", records }).sameQuery,
    ).toBe(false);
  });
});
