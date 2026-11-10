// @vitest-environment node
import http from "node:http";
import { EventEmitter } from "node:events";
import { describe, expect, it } from "vitest";
import {
  captureStartupDiagnostics,
  parseStartupMarkers,
  readRawPageState,
  recordPageObservation,
} from "./native-startup-diagnostics.mjs";

describe("启动取证", () => {
  it("原始页面也为空白时只描述观察，不把它认定为某个根因", async () => {
    const result = await captureStartupDiagnostics({
      port: 1,
      playwrightUrls: ["about:blank"],
      confirmPortOwner: async () => true,
      fetchImpl: async (url) => ({
        ok: true,
        json: async () =>
          url.endsWith("/list") ? [{ type: "page", url: "about:blank" }] : { Browser: "fixture" },
      }),
      readPageState: async () => ({ frame: { url: "about:blank" } }),
      captureDump: async () => {
        throw new Error("fixture dump denied");
      },
    });
    expect(result.observation).toBe("blank-targets");
    expect(result.dump.error).toBe("fixture dump denied");
    expect(result).not.toHaveProperty("root_cause");
  });

  it("取消原始 WebSocket 读取时关闭连接，不留下挂起诊断", async () => {
    let closed = false;
    class Socket extends EventEmitter {
      addEventListener(name, fn) {
        this.on(name, fn);
      }
      removeEventListener(name, fn) {
        this.off(name, fn);
      }
      close() {
        closed = true;
      }
    }
    const controller = new AbortController();
    const read = readRawPageState(
      { webSocketDebuggerUrl: "ws://127.0.0.1:9452/devtools/page/p1" },
      {
        port: 9452,
        signal: controller.signal,
        WebSocketImpl: Socket,
      },
    );
    controller.abort();
    await expect(read).rejects.toThrow("取消");
    expect(closed).toBe(true);
  });

  it("独立读取真实 HTTP 端点，识别 Playwright 与原始页面状态不一致", async () => {
    const server = http.createServer((req, res) => {
      res.setHeader("content-type", "application/json");
      res.end(
        JSON.stringify(
          req.url === "/json/version"
            ? { Browser: "Edg/fixture" }
            : [{ id: "p1", type: "page", url: "http://tauri.localhost/" }],
        ),
      );
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    try {
      const result = await captureStartupDiagnostics({
        port: server.address().port,
        confirmPortOwner: async () => true,
        playwrightUrls: ["about:blank"],
        readPageState: async () => ({
          location: { href: "http://tauri.localhost/", readyState: "complete" },
        }),
        captureDump: async () => ({ status: "unavailable", error: "fixture permission denied" }),
      });
      expect(result.runtime_version.value.Browser).toBe("Edg/fixture");
      expect(result.observation).toBe("observer-disagreement");
      expect(result.dump.value.error).toContain("permission denied");
    } finally {
      await new Promise((resolve) => server.close(resolve));
    }
  });

  it("诊断挂起仍在独立预算内结束，不把空白推断为已定位的初始化死锁", async () => {
    const started = performance.now();
    let aborted = false;
    const result = await captureStartupDiagnostics({
      port: 1,
      timeoutMs: 50,
      confirmPortOwner: async () => true,
      fetchImpl: (_url, { signal }) => {
        signal.addEventListener(
          "abort",
          () => {
            aborted = true;
          },
          { once: true },
        );
        return new Promise(() => {});
      },
      captureDump: () => new Promise(() => {}),
    });
    expect(performance.now() - started).toBeLessThan(700);
    expect(result.observation).toBe("unavailable");
    expect(result.dump.status).toBe("unavailable");
    expect(aborted).toBe(true);
  });

  it("端口无法证明归属时不请求任何 HTTP/CDP，但仍保留转储拒绝原因", async () => {
    let requested = false;
    const result = await captureStartupDiagnostics({
      port: 1,
      confirmPortOwner: async () => false,
      fetchImpl: async () => {
        requested = true;
        throw new Error("must not request");
      },
      captureDump: async () => ({ status: "unavailable", error: "host already exited" }),
    });
    expect(requested).toBe(false);
    expect(result.raw_targets.status).toBe("unavailable");
    expect(result.dump.value.error).toBe("host already exited");
  });

  it("只记录变化并限制页面观察数量", () => {
    const record = { startedAtMs: 1000 };
    for (let i = 0; i < 60; i++) recordPageObservation(record, [`about:blank#${i}`], 1000 + i);
    recordPageObservation(record, ["about:blank#59"], 1100);
    expect(record.pageObservations).toHaveLength(32);
    expect(record.pageObservations.at(-1).at_ms).toBe(59);
  });

  it("阶段标记正确解析，畸形行保留为无效标记而不伪造阶段", () => {
    expect(
      parseStartupMarkers(
        'noise\n[tokenscope-startup] {"phase":"window_build_started","pid":12}\n[tokenscope-startup] broken\n',
      ),
    ).toEqual({ events: [{ phase: "window_build_started", pid: 12 }], invalid: 1 });
  });

  it("独立 WebSocket 只发读取命令，不导航、重载或恢复调试器", async () => {
    const methods = [];
    class Socket extends EventEmitter {
      constructor() {
        super();
        queueMicrotask(() => this.emit("open"));
      }
      addEventListener(name, cb) {
        this.on(name, cb);
      }
      removeEventListener(name, cb) {
        this.off(name, cb);
      }
      send(raw) {
        const msg = JSON.parse(raw);
        methods.push(msg.method);
        queueMicrotask(() =>
          this.emit("message", {
            data: JSON.stringify({
              id: msg.id,
              result:
                msg.method === "Page.getFrameTree"
                  ? { frameTree: { frame: { url: "about:blank" } } }
                  : { result: { value: { href: "about:blank", readyState: "complete" } } },
            }),
          }),
        );
      }
      close() {
        this.closed = true;
      }
    }
    const result = await readRawPageState(
      { webSocketDebuggerUrl: "ws://127.0.0.1:9452/devtools/page/p1" },
      {
        port: 9452,
        signal: new AbortController().signal,
        WebSocketImpl: Socket,
      },
    );
    expect(result.location.href).toBe("about:blank");
    expect(methods).toEqual(["Page.getFrameTree", "Runtime.evaluate"]);
    await expect(
      readRawPageState(
        { webSocketDebuggerUrl: "ws://example.com:9452/p1" },
        {
          port: 9452,
          signal: new AbortController().signal,
          WebSocketImpl: Socket,
        },
      ),
    ).rejects.toThrow("端点");
  });
});
