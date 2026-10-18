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
    app_outline: app ? outline(app, 2) : null,
    body_text_head: (document.body?.innerText ?? "").slice(0, 400),
    page_errors: Array.isArray(window.__TS_PAGE_ERRORS) ? window.__TS_PAGE_ERRORS.slice(-20) : [],
  };
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
        `IPC=${item.ipc_calls} 最后IPC=${item.last_ipc.at(-1) ?? "无"}`
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
    const item = {
      label: entry.label,
      closed_before_capture: entry.closed,
      page_errors: [...(session.pageErrors ?? [])].slice(-20),
      ipc_calls: ipcCalls.length,
      last_ipc: ipcCalls.slice(-15).map((call) => call.cmd),
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
