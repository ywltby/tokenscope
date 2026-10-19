// 浏览器契约失败取证（main 599c89e：ui_control_material 等待 `#app .app-shell`
// 超时，现场没有截图、DOM 与页面错误，无法说明原因）。
//
// 契约在自己的 finally 里关闭会话，异常传到运行器时页面已经不在，事后补截图
// 拿不到现场。因此：
//   - 会话关闭前先留一份有界 DOM 摘要（读取超时或失败只记原因，不阻断关闭）；
//   - 契约抛错时汇总本契约打开过的全部会话：Node 侧的页面错误、IPC 与外联
//     记录关闭后仍在；仍打开的会话现读 DOM 并截图；
//   - 取证写入产物目录 `<契约>--failure.json`，随浏览器证据一起上传。

import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { withTimeout } from "./native-scenario.mjs";

/** 单次 DOM 摘要读取上限：页面卡住时不拖住关闭与收尾。 */
export const SNAPSHOT_TIMEOUT_MS = 2000;
/** 失败截图上限。 */
export const FAILURE_SHOT_TIMEOUT_MS = 5000;

/**
 * 在页面内执行的只读 DOM 摘要（经 page.evaluate 序列化，不得引用外部变量）。
 * 只取定位"页面停在哪一步"所需的结构信息；fixture 均为合成数据。
 */
export function domSnapshotInPage() {
  // 前三层元素轮廓（标签 + 前三个类名）：比整段 outerHTML 便宜。#app 下前两层是
  // 配置与隐私闸门包装（n-config-provider → privacy-shell），第三层才看得出停在
  // 闸门检查、同意弹窗还是主应用。
  const outline = (el, depth) =>
    [...el.children].slice(0, 8).map((child) => {
      const classes =
        typeof child.className === "string" ? child.className.trim().split(/\s+/).slice(0, 3) : [];
      const name = [child.tagName.toLowerCase(), ...classes.filter(Boolean)].join(".");
      return depth > 0 && child.children.length > 0 ? { [name]: outline(child, depth - 1) } : name;
    });
  const app = document.querySelector("#app");
  const shell = document.querySelector("#app .app-shell");
  let shellVisible = false;
  if (shell) {
    const rect = shell.getBoundingClientRect();
    shellVisible =
      rect.width > 0 && rect.height > 0 && getComputedStyle(shell).visibility !== "hidden";
  }
  const root = document.documentElement;
  const shellEl = document.querySelector("#app .privacy-shell");
  const pageIpc = Array.isArray(window.__TS_IPC_CALLS) ? window.__TS_IPC_CALLS : [];
  return {
    url: location.href,
    ready_state: document.readyState,
    visibility: document.visibilityState,
    html_class: root.className || "",
    html_data: { ...root.dataset },
    app_exists: !!app,
    app_children: app ? app.childElementCount : null,
    app_shell: !!shell,
    app_shell_visible: shellVisible,
    // 引导阶段标记（App.vue 的 data-boot-state，只用于诊断）与页面侧 IPC 尾部。
    boot_state: shellEl ? shellEl.getAttribute("data-boot-state") : null,
    page_ipc: pageIpc.slice(-15).map((call) => call.cmd),
    page_ipc_count: pageIpc.length,
    app_outline: app ? outline(app, 2) : null,
    body_text_head: (document.body?.innerText ?? "").slice(0, 400),
    page_errors: Array.isArray(window.__TS_PAGE_ERRORS) ? window.__TS_PAGE_ERRORS.slice(-20) : [],
  };
}

/**
 * 引导卡死的自动判断（只读、纯函数）：按固定顺序给出最可能的卡点。
 *
 * 判定顺序：
 *   1. DOM 摘要不可读 → "DOM 不可读"；
 *   2. boot_state=main-load-failed，或页面错误匹配动态导入/预加载 CSS →
 *      "主界面资源加载失败"，附失败的同源请求；
 *   3. boot_state=checking / checking-slow：按 IPC 两侧对照细分——页面侧
 *      已发出 privacy_bootstrap 而 Node 侧无记录（"IPC 未到达 Node"）、
 *      Node 侧挂起（"fixture 未返回"）、Node 侧已返回（"响应未回到页
 *      面"）、页面侧也没有（"页面未发出检查"）；
 *   4. boot_state=loading-main → "主界面导入未结束"；
 *   5. boot_state=main 但壳层缺失或不可见 → "主界面挂载异常"；
 *   6. 其余 → "未分类"。
 *
 * @param {object} item captureContractFailure 的会话取证项（dom、
 *   page_errors、ipc_tail、ipc_pending、requests_tail）。
 * @returns {string} 一行判断，直接进摘要与取证文件。
 */
export function classifyBootStall(item) {
  const dom = item.dom ?? {};
  if (dom.unavailable !== undefined) return "DOM 不可读";
  const pageErrors = item.page_errors ?? [];
  const resourceError = pageErrors.find((e) =>
    /dynamically imported module|preload CSS/i.test(String(e)),
  );
  if (dom.boot_state === "main-load-failed" || resourceError) {
    const failed = (item.requests_tail ?? [])
      .filter((r) => r.outcome === "error")
      .map((r) => r.path)
      .join(", ");
    return `主界面资源加载失败（失败请求：${failed || "未见失败请求记录"}）`;
  }
  const boot = dom.boot_state;
  if (boot === "checking" || boot === "checking-slow") {
    const pageIpc = dom.page_ipc ?? [];
    const nodeTail = item.ipc_tail ?? [];
    const nodePending = item.ipc_pending ?? [];
    if (!pageIpc.includes("privacy_bootstrap")) return "页面未发出检查（页面侧未见 privacy_bootstrap）";
    if (nodePending.includes("privacy_bootstrap")) return "fixture 未返回（privacy_bootstrap 挂起）";
    const settled = nodeTail.filter((c) => c.cmd === "privacy_bootstrap" && c.ms !== undefined);
    if (settled.length > 0) {
      const last = settled.at(-1);
      return `响应未回到页面（Node 侧 ${last.outcome ?? "已返回"}，at=${last.at ?? "?"}ms，${last.ms}ms 后落定）`;
    }
    return "IPC 未到达 Node（页面侧已发出 privacy_bootstrap，Node 侧无记录）";
  }
  if (boot === "loading-main") return "主界面导入未结束";
  if (boot === "main" && (!dom.app_shell || !dom.app_shell_visible)) return "主界面挂载异常";
  return "未分类";
}

const message = (error) => String(error instanceof Error ? error.message : error);

async function readSnapshot(page, timeoutMs) {
  try {
    return await withTimeout(() => page.evaluate(domSnapshotInPage), timeoutMs, "DOM 摘要读取");
  } catch (error) {
    return { unavailable: message(error) };
  }
}

/**
 * 跟踪契约打开的会话：包一层 close，关闭前留 DOM 摘要。返回同一个会话对象。
 * @param {object} session openSession 返回的会话
 * @param {object[]} registry 本契约的会话登记表
 */
export function trackSession(session, registry, { timeoutMs = SNAPSHOT_TIMEOUT_MS } = {}) {
  const entry = { session, label: session.label, closed: false, snapshot: null };
  registry.push(entry);
  const close = session.close.bind(session);
  session.close = async () => {
    if (!entry.closed) {
      entry.snapshot = await readSnapshot(session.page, timeoutMs);
      entry.closed = true;
    }
    return close();
  };
  return session;
}

/** 一行摘要（打印进 CI 日志，完整内容在取证文件里）。 */
export function summarizeFailure(sessions) {
  if (sessions.length === 0) return "本契约没有打开会话";
  return sessions
    .slice(-3)
    .map((item) => {
      const dom = item.dom ?? {};
      const shell =
        dom.unavailable !== undefined
          ? `DOM 不可读(${dom.unavailable})`
          : `shell=${dom.app_shell ? (dom.app_shell_visible ? "可见" : "不可见") : "无"} app子元素=${dom.app_children ?? "无"}`;
      return (
        `${item.label}[${item.closed_before_capture ? "已关闭" : "打开"}] ${shell} ` +
        `页面错误=${item.page_errors.length} 失败请求=${item.failed_requests.length} ` +
        `IPC=${item.ipc_calls} 最后IPC=${item.last_ipc.at(-1) ?? "无"}` +
        (item.suspected ? ` 判断=${item.suspected}` : "")
      );
    })
    .join("；");
}

/**
 * 契约抛错后汇总取证并写入 `<契约>--failure.json`。
 * @returns {Promise<{file: string, summary: string, payload: object}>}
 */
export async function captureContractFailure({
  contract,
  error,
  registry,
  output,
  writeFile = writeFileSync,
  timeoutMs = SNAPSHOT_TIMEOUT_MS,
  shotTimeoutMs = FAILURE_SHOT_TIMEOUT_MS,
}) {
  const sessions = [];
  for (const [index, entry] of registry.entries()) {
    const session = entry.session;
    const ipcCalls = session.ipcCalls ?? [];
    const sameOriginRequests = session.requests ?? [];
    const item = {
      label: entry.label,
      created_at:
        session.createdAt !== undefined ? new Date(session.createdAt).toISOString() : null,
      closed_before_capture: entry.closed,
      page_errors: [...(session.pageErrors ?? [])].slice(-20),
      ipc_calls: ipcCalls.length,
      last_ipc: ipcCalls.slice(-15).map((call) => call.cmd),
      // IPC 时间线尾与挂起：at 为相对会话创建的毫秒数，挂起的调用没有 ms。
      ipc_tail: ipcCalls.slice(-15).map((call) => ({
        cmd: call.cmd,
        at: call.at,
        ms: call.ms,
        outcome: call.outcome,
      })),
      ipc_pending: ipcCalls.filter((call) => call.ms === undefined).map((call) => call.cmd),
      // 同源请求时间线尾（保留最近 50 条，这里只取尾 15）与挂起。
      requests_tail: sameOriginRequests.slice(-15).map((r) => ({
        path: r.path,
        at: r.at,
        ms: r.ms,
        outcome: r.outcome,
      })),
      requests_pending: sameOriginRequests
        .filter((r) => r.ms === undefined)
        .map((r) => r.path),
      unknown_commands: [...(session.unknownCommands ?? [])],
      external_requests: [...(session.externalRequests ?? [])].slice(-10),
      failed_requests: [...(session.failedRequests ?? [])].slice(-10),
      dom: entry.snapshot,
      screenshot: null,
    };
    if (!entry.closed) {
      item.dom = await readSnapshot(session.page, timeoutMs);
      const file = `${contract}--failure--${index + 1}.png`;
      try {
        await withTimeout(
          () => session.page.screenshot({ path: join(output, file) }),
          shotTimeoutMs,
          "失败截图",
        );
        item.screenshot = file;
      } catch (shotError) {
        item.screenshot = `截图失败：${message(shotError)}`;
      }
    }
    // 壳层不可见（或缺失）才给自动判断：主界面正常可见的失败与引导期无关。
    if (item.dom && item.dom.unavailable === undefined && !(item.dom.app_shell && item.dom.app_shell_visible)) {
      item.suspected = classifyBootStall(item);
    }
    sessions.push(item);
  }
  const payload = {
    contract,
    error: message(error),
    stack: error instanceof Error ? String(error.stack ?? "").split("\n").slice(0, 12) : [],
    captured_at: new Date().toISOString(),
    sessions,
  };
  const file = `${contract}--failure.json`;
  writeFile(join(output, file), JSON.stringify(payload, null, 2));
  return { file, summary: summarizeFailure(sessions), payload };
}
