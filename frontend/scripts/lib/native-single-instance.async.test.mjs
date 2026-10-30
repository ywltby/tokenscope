// @vitest-environment node
// AC10 第二个实例退出等待的真实异步契约：真实短命 Node 子进程 + 真实计时器。
// 不启动 Native；只验证 waitForSecondaryExit 的 exit/timeout/cancel/error 分支。
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { afterAll, describe, expect, it } from "vitest";
import { classifySecondaryExit, waitForSecondaryExit } from "./native-single-instance.mjs";

const children = [];
afterAll(() => {
  for (const child of children) {
    try {
      child.kill();
    } catch {}
  }
});

function spawnNode(code) {
  const child = spawn(process.execPath, ["-e", code], { stdio: "ignore" });
  children.push(child);
  return child;
}

describe("waitForSecondaryExit（真实子进程）", () => {
  it("子进程在限定时间内退出：记录真实退出码与耗时", async () => {
    const child = spawnNode("setTimeout(() => process.exit(0), 120)");
    const wait = await waitForSecondaryExit({ child, timeoutMs: 3000, intervalMs: 20 });
    expect(wait).toMatchObject({ kind: "exit", exitCode: 0, timedOut: false, canceled: false });
    expect(wait.elapsedMs).toBeGreaterThanOrEqual(100);
    expect(classifySecondaryExit(wait).status).toBe("passed");
  });

  it("非零退出码原样保留，分类为失败", async () => {
    const child = spawnNode("setTimeout(() => process.exit(7), 60)");
    const wait = await waitForSecondaryExit({ child, timeoutMs: 3000, intervalMs: 20 });
    expect(wait.exitCode).toBe(7);
    expect(classifySecondaryExit(wait).status).toBe("failed");
  });

  it("超时后终止本次子进程并返回 timedOut", async () => {
    const child = spawnNode("setInterval(() => {}, 1000)");
    const wait = await waitForSecondaryExit({
      child,
      timeoutMs: 150,
      intervalMs: 20,
      killGraceMs: 2000,
    });
    expect(wait.timedOut).toBe(true);
    expect(wait.killed).toBe(true);
    expect(classifySecondaryExit(wait).status).toBe("failed");
    expect(wait.elapsedMs).toBeGreaterThanOrEqual(140);
  });

  it("取消标志生效时终止子进程，分类为失败", async () => {
    const started = Date.now();
    const child = spawnNode("setInterval(() => {}, 1000)");
    const wait = await waitForSecondaryExit({
      child,
      timeoutMs: 3000,
      intervalMs: 20,
      isCanceled: () => Date.now() - started >= 40,
      killGraceMs: 2000,
    });
    expect(wait.canceled).toBe(true);
    expect(wait.killed).toBe(true);
    expect(classifySecondaryExit(wait).status).toBe("failed");
  });
});

describe("waitForSecondaryExit（假子进程错误/事件）", () => {
  it("spawn error 不会被当成超时或正常退出", async () => {
    const child = new EventEmitter();
    child.kill = () => true;
    setTimeout(() => child.emit("error", new Error("spawn failed")), 10);
    const wait = await waitForSecondaryExit({ child, timeoutMs: 500, intervalMs: 10 });
    expect(wait.kind).toBe("error");
    expect(wait.spawnError).toMatch(/spawn failed/);
    expect(wait.timedOut).toBe(false);
  });
});
