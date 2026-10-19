// BS04：引导期卡死的真实浏览器契约——两种卡死都必须可见、可操作。
//
// 与本目录其他契约的差异：
// - 两个场景都从引导壳出发（业务界面只作为恢复结果出现），不能复用
//   openDashboard（它等 .app-shell，与卡死前提矛盾）；
// - 「检查耗时较长」用 page.clock 在 goto 前安装并精确推进 10 秒阈值，
//   不靠真实等待；clock 安装后 page.waitForTimeout 也被模拟，一律用
//   waitForFunction / locator.waitFor 等真实信号。
//
// 阈值说明：脚本无法 import TS 常量，BOOTSTRAP_SLOW_MS 在此对齐
// frontend/src/lib/privacyGate.ts（10_000）；改动该值时必须同步这里。

import { waitForIpcSettled } from "../lib/boot-stall-wait.mjs";

const BOOTSTRAP_SLOW_MS = 10_000;

/** Node 侧真实等待（不受 page.clock 影响，给 CDP 往返留时间）。 */
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function bootStallRecovery(ctx) {
  const name = "boot_stall_recovery";

  // ── 场景 1：检查挂起（耗时提示 + 重新检查 + 代次保护）────────────
  {
    const s = await ctx.open({
      fixture: "privacy-bootstrap-deferred",
      theme: "light",
      viewport: { width: 1280, height: 820, label: "1280x820" },
    });
    const { page } = s;
    try {
      await page.clock.install();
      await page.goto(s.url);
      const shell = page.locator(".privacy-shell");
      await shell.waitFor({ state: "visible", timeout: 10000 });
      const early = await shell.getAttribute("data-boot-state");
      ctx.record(
        name,
        "检查挂起时引导壳阶段为 checking",
        early === "checking",
        `data-boot-state=${early}`,
      );

      // 推进超过慢阈值：提示出现但不是错误，原请求仍挂起。
      await page.clock.runFor(BOOTSTRAP_SLOW_MS + 500);
      await page.waitForFunction(
        () =>
          document.querySelector(".privacy-shell")?.getAttribute("data-boot-state") ===
          "checking-slow",
        undefined,
        { timeout: 5000 },
      );
      const slowText = (await page.locator(".privacy-slow-text").innerText())
        .replace(/\s+/g, " ")
        .trim();
      const retry = page
        .locator(".privacy-checking-wrap")
        .getByRole("button", { name: "重新检查" });
      await retry.waitFor({ state: "visible", timeout: 5000 });
      const slowState = await shell.getAttribute("data-boot-state");
      ctx.record(
        name,
        "超阈值后进入 checking-slow，提示与重新检查可见",
        slowState === "checking-slow" && slowText.includes("检查耗时较长") && (await retry.isVisible()),
        `state=${slowState} text="${slowText}" retry=${await retry.isVisible()}`,
      );

      // 重新检查 = 再发一次 privacy_bootstrap（新代次），返回 ready 后主界面挂载。
      const firstBootstrap = () => s.ipcCalls.filter((c) => c.cmd === "privacy_bootstrap");
      const beforeRetry = firstBootstrap().length;
      await retry.click();
      await page.locator("#app .app-shell").waitFor({ state: "visible", timeout: 30000 });
      const mainState = await shell.getAttribute("data-boot-state");
      ctx.record(
        name,
        "重新检查发出第二次检查并挂载主界面",
        firstBootstrap().length === beforeRetry + 1 && mainState === "main",
        `calls=${firstBootstrap().length}/${beforeRetry} state=${mainState}`,
      );

      // 迟到的第 1 次响应（needs_consent）必须被代次保护丢弃：状态不退回、
      // 不弹政策弹窗。断言前提是旧响应真实送达 Node 侧——等待落定，超时
      // 即契约失败（响应没送达时界面当然不会退回，继续断言是误通过）。
      const firstCall = firstBootstrap()[0];
      s.fixture.settleDeferred("privacy_bootstrap#1", s.fixture.staleBootstrap());
      const lateOutcome = await waitForIpcSettled(firstCall, { timeoutMs: 5000 });
      await sleep(400);
      await page.clock.runFor(1500);
      const lateState = await shell.getAttribute("data-boot-state");
      const dialogs = await page.locator(".privacy-dialog").count();
      ctx.record(
        name,
        "迟到的响应已在 Node 侧落定（outcome=ok）",
        lateOutcome === "ok",
        `outcome=${lateOutcome}`,
      );
      ctx.record(
        name,
        "迟到的 needs_consent 不改变已就绪状态",
        lateState === "main",
        `state=${lateState} outcome=${lateOutcome}`,
      );
      ctx.record(
        name,
        "迟到的响应不弹政策弹窗",
        dialogs === 0,
        `dialogs=${dialogs}`,
      );
      await ctx.shot(name, s, "late-bootstrap-discarded");
      await ctx.finishSession(s);
    } finally {
      await s.close();
    }
  }

  // ── 场景 2：主界面加载失败（失败面板 + 整页重新加载恢复）──────────
  {
    const s = await ctx.open({
      fixture: "normal",
      theme: "dark",
      viewport: { width: 980, height: 620, label: "980x620" },
    });
    const { page } = s;
    try {
      await s.context.route(/MainApp-[\w-]+\.js/, (route) => route.abort());
      await page.goto(s.url);
      const shell = page.locator(".privacy-shell");
      await page.waitForFunction(
        () =>
          document.querySelector(".privacy-shell")?.getAttribute("data-boot-state") ===
          "main-load-failed",
        undefined,
        { timeout: 30000 },
      );
      const alertText = (await page.locator(".privacy-blocked").innerText())
        .replace(/\s+/g, " ")
        .trim();
      const reload = page.getByRole("button", { name: "重新加载" });
      ctx.record(
        name,
        "主界面代码块注入失败后出现失败面板",
        (await shell.getAttribute("data-boot-state")) === "main-load-failed" &&
          alertText.includes("主界面加载失败"),
        `state=${await shell.getAttribute("data-boot-state")} text="${alertText.slice(0, 140)}"`,
      );
      ctx.record(
        name,
        "失败面板提供可见的重新加载按钮",
        await reload.isVisible(),
        `reload=${await reload.isVisible()}`,
      );

      // 预期内会出现与注入失败相关的资源错误记录：断言页面错误只来自注入
      // 的失败资源、且没有未处理拒绝。本会话不调用 finishSession——它的
      // 「无未处理拒绝/页面错误」检查会被预期内的资源错误触发；以下显式
      // 记录替代其检查项。
      const pageErrors = await s.readPageErrors();
      const unrelated = pageErrors.filter(
        (e) => !/MainApp|Failed to fetch|dynamically imported|preload/i.test(e),
      );
      const unhandled = pageErrors.filter((e) => e.startsWith("unhandledrejection"));
      ctx.record(
        name,
        "页面错误只有注入失败的资源记录",
        unrelated.length === 0,
        pageErrors.join(" | ") || "无页面错误",
      );
      ctx.record(
        name,
        "无未处理拒绝",
        unhandled.length === 0,
        unhandled.join(" | ") || "无",
      );
      ctx.record(
        name,
        "无未知 IPC 命令",
        s.unknownCommands.length === 0,
        s.unknownCommands.join(",") || "无",
      );
      ctx.record(
        name,
        "无外联请求",
        s.externalRequests.length === 0,
        s.externalRequests.join(",") || "无",
      );
      await ctx.shot(name, s, "main-load-failed");

      // 恢复只能整页重新加载（失败模块被映射缓存，再次 import() 不会恢复）。
      await s.context.unroute(/MainApp-[\w-]+\.js/);
      await reload.click();
      await page.locator("#app .app-shell").waitFor({ state: "visible", timeout: 30000 });
      const recovered = await shell.getAttribute("data-boot-state");
      ctx.record(
        name,
        "整页重新加载后主界面恢复且阶段为 main",
        recovered === "main",
        `state=${recovered}`,
      );
      await ctx.shot(name, s, "reloaded");
    } finally {
      await s.close();
    }
  }
}
