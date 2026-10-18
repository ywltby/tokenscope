// 浏览器契约失败取证（main 599c89e 的 ui_control_material 超时无现场）：关闭前留
// DOM 摘要、读取卡住不阻断关闭、抛错时汇总已关闭与仍打开的会话并写出取证文件。
// 默认 happy-dom 环境：DOM 摘要函数直接对构造的文档执行。
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  captureContractFailure,
  domSnapshotInPage,
  summarizeFailure,
  trackSession,
} from "./contract-failure.mjs";

const never = () => new Promise(() => {});

function fakeSession(label, { evaluate, screenshot, order = [] } = {}) {
  return {
    label,
    pageErrors: [],
    ipcCalls: [],
    unknownCommands: [],
    externalRequests: [],
    failedRequests: [],
    page: {
      evaluate: evaluate ?? (async () => ({ app_shell: false, app_children: 0 })),
      screenshot: screenshot ?? (async () => {}),
    },
    async close() {
      order.push(`close:${label}`);
    },
  };
}

afterEach(() => {
  document.body.innerHTML = "";
  delete window.__TS_PAGE_ERRORS;
});

describe("domSnapshotInPage", () => {
  it("经 toString 重建后仍可执行（page.evaluate 的序列化方式），并给出壳层与轮廓", () => {
    document.body.innerHTML =
      '<div id="app"><div class="n-config-provider"><div class="privacy-shell">' +
      '<div class="app-shell layout"><header class="page-head"></header></div></div></div></div>';
    window.__TS_PAGE_ERRORS = ["boom"];
    const rebuilt = new Function(`return (${domSnapshotInPage.toString()})`)();
    const snapshot = rebuilt();
    expect(snapshot.app_exists).toBe(true);
    expect(snapshot.app_shell).toBe(true);
    expect(snapshot.app_children).toBe(1);
    // 前两层是包装，第三层才是主应用；更深的层级不展开。
    expect(snapshot.app_outline).toEqual([
      { "div.n-config-provider": [{ "div.privacy-shell": ["div.app-shell.layout"] }] },
    ]);
    expect(snapshot.page_errors).toEqual(["boom"]);
  });

  it("壳层缺失时如实记录页面停留的结构", () => {
    document.body.innerHTML = '<div id="app"><section class="consent-page"></section></div>';
    const snapshot = domSnapshotInPage();
    expect(snapshot.app_shell).toBe(false);
    expect(snapshot.app_shell_visible).toBe(false);
    expect(snapshot.app_outline).toEqual(["section.consent-page"]);
  });
});

describe("trackSession", () => {
  it("关闭前先留 DOM 摘要，重复关闭不再读取", async () => {
    const order = [];
    const registry = [];
    const session = fakeSession("s1", {
      order,
      evaluate: async () => {
        order.push("snapshot");
        return { app_shell: true };
      },
    });
    trackSession(session, registry);
    await session.close();
    await session.close();
    expect(order).toEqual(["snapshot", "close:s1", "close:s1"]);
    expect(registry[0]).toMatchObject({ label: "s1", closed: true, snapshot: { app_shell: true } });
  });

  it("页面卡住时摘要按上限放弃，关闭照常完成", async () => {
    const order = [];
    const registry = [];
    const session = fakeSession("stuck", { order, evaluate: never });
    trackSession(session, registry, { timeoutMs: 20 });
    await session.close();
    expect(order).toEqual(["close:stuck"]);
    expect(registry[0].snapshot.unavailable).toContain("DOM 摘要读取");
  });
});

describe("captureContractFailure", () => {
  it("汇总已关闭与仍打开的会话：前者用关闭前摘要，后者现读并截图", async () => {
    const registry = [];
    const closed = fakeSession("light", {
      evaluate: async () => ({ app_shell: true, app_shell_visible: true, app_children: 1 }),
    });
    closed.ipcCalls.push({ cmd: "query_begin" });
    const shots = [];
    const open = fakeSession("dark", {
      evaluate: async () => ({ app_shell: false, app_children: 2 }),
      screenshot: async ({ path }) => shots.push(path),
    });
    open.pageErrors.push("pageerror: x");
    open.ipcCalls.push({ cmd: "privacy_status" }, { cmd: "settings_get" });
    open.failedRequests.push("http://127.0.0.1/assets/MainApp.js net::ERR_FAILED");
    trackSession(closed, registry);
    trackSession(open, registry);
    await closed.close();

    const writes = [];
    const result = await captureContractFailure({
      contract: "c",
      error: new Error("waitForSelector timeout"),
      registry,
      output: "out",
      writeFile: (path, text) => writes.push({ path, payload: JSON.parse(text) }),
    });

    expect(result.file).toBe("c--failure.json");
    expect(writes).toHaveLength(1);
    expect(writes[0].path).toBe(join("out", "c--failure.json"));
    const [first, second] = writes[0].payload.sessions;
    expect(writes[0].payload.error).toBe("waitForSelector timeout");
    expect(first).toMatchObject({
      label: "light",
      closed_before_capture: true,
      screenshot: null,
      dom: { app_shell: true },
      last_ipc: ["query_begin"],
    });
    expect(second).toMatchObject({
      label: "dark",
      closed_before_capture: false,
      screenshot: "c--failure--2.png",
      dom: { app_shell: false, app_children: 2 },
      page_errors: ["pageerror: x"],
      failed_requests: ["http://127.0.0.1/assets/MainApp.js net::ERR_FAILED"],
      last_ipc: ["privacy_status", "settings_get"],
    });
    expect(shots).toEqual([join("out", "c--failure--2.png")]);
    expect(result.summary).toContain("light[已关闭] shell=可见");
    expect(result.summary).toContain(
      "dark[打开] shell=无 app子元素=2 页面错误=1 失败请求=1 IPC=2 最后IPC=settings_get",
    );
  });

  it("截图卡住按上限放弃并记原因，取证照常写出", async () => {
    const registry = [];
    trackSession(fakeSession("hang", { screenshot: never }), registry);
    const writes = [];
    await captureContractFailure({
      contract: "c",
      error: "plain",
      registry,
      output: "out",
      writeFile: (path, text) => writes.push(JSON.parse(text)),
      shotTimeoutMs: 20,
    });
    expect(writes[0].sessions[0].screenshot).toMatch(/^截图失败：/);
    expect(writes[0].error).toBe("plain");
  });

  it("没有会话时摘要明确说明", () => {
    expect(summarizeFailure([])).toBe("本契约没有打开会话");
  });
});
