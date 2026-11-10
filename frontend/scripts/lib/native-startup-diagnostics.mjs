// 启动已判失败后的只读取证。结果只描述观察，不推断 WebView 挂起的根因。
export const STARTUP_DIAGNOSTIC_TIMEOUT_MS = 8000;
const shortError = (e) => String(e?.message ?? e).slice(0, 1000);
const isApp = (url) =>
  /^(?:tauri:\/\/localhost|https?:\/\/tauri\.localhost)(?:[/:]|$)/i.test(url ?? "");

/** task 必须使用传入 signal 释放网络/子进程；不合作的 Promise 也不能拖住收尾。 */
async function bounded(task, parent, timeoutMs) {
  if (parent.aborted) return { status: "unavailable", error: "启动诊断已取消" };
  const controller = new AbortController();
  let timer;
  let rejectAbort;
  const abort = () => {
    controller.abort();
    rejectAbort?.(new Error("启动诊断超时或已取消"));
  };
  const canceled = new Promise((_, reject) => {
    rejectAbort = reject;
  });
  parent.addEventListener("abort", abort, { once: true });
  if (parent.aborted) abort();
  else timer = setTimeout(abort, timeoutMs);
  try {
    const value = await Promise.race([
      Promise.resolve().then(() => task(controller.signal)),
      canceled,
    ]);
    return { status: "captured", value };
  } catch (error) {
    return { status: "unavailable", error: shortError(error) };
  } finally {
    clearTimeout(timer);
    parent.removeEventListener("abort", abort);
    controller.abort();
  }
}

export function recordPageObservation(record, urls, atMs = Date.now()) {
  const observations = (record.pageObservations ??= []);
  const clean = urls.slice(0, 16).map((u) => String(u).slice(0, 1000));
  if (JSON.stringify(observations.at(-1)?.urls) === JSON.stringify(clean)) return;
  observations.push({ at_ms: Math.max(0, atMs - record.startedAtMs), urls: clean });
  if (observations.length > 32) observations.shift();
}

export function parseStartupMarkers(text) {
  const result = { events: [], invalid: 0 };
  for (const line of String(text).split(/\r?\n/)) {
    const offset = line.indexOf("[tokenscope-startup] ");
    if (offset < 0) continue;
    try {
      const event = JSON.parse(line.slice(offset + "[tokenscope-startup] ".length));
      if (typeof event.phase !== "string" || !Number.isInteger(event.pid))
        throw new Error("invalid marker");
      result.events.push(event);
      if (result.events.length > 64) result.events.shift();
    } catch {
      result.invalid++;
    }
  }
  return result;
}

/** 直连页面 CDP，绕过 Playwright 的 Frame/URL 缓存；严格限定到本次本地端口。 */
export async function readRawPageState(target, { port, signal, WebSocketImpl = WebSocket }) {
  const url = new URL(target.webSocketDebuggerUrl);
  if (
    url.protocol !== "ws:" ||
    !["127.0.0.1", "localhost"].includes(url.hostname) ||
    Number(url.port) !== port ||
    url.username ||
    url.password
  )
    throw new Error("诊断 CDP 端点不属于本次本地端口");
  return new Promise((resolve, reject) => {
    const socket = new WebSocketImpl(url.href);
    const replies = new Map();
    let finished = false;
    const finish = (error) => {
      if (finished) return;
      finished = true;
      signal.removeEventListener("abort", onAbort);
      for (const [name, handler] of listeners) socket.removeEventListener(name, handler);
      try {
        socket.close();
      } catch {}
      if (error) reject(error);
      else
        resolve({
          frame: replies.get(1)?.frameTree?.frame ?? null,
          frame_error: replies.get(1)?.error ?? null,
          location: replies.get(2)?.result?.value ?? null,
          location_error: replies.get(2)?.error ?? null,
          exception: replies.get(2)?.exceptionDetails?.text ?? null,
        });
    };
    const onAbort = () => finish(new Error("原始页面诊断已取消"));
    const listeners = [
      [
        "open",
        () => {
          socket.send(JSON.stringify({ id: 1, method: "Page.getFrameTree" }));
          socket.send(
            JSON.stringify({
              id: 2,
              method: "Runtime.evaluate",
              params: {
                expression:
                  "({href:location.href,readyState:document.readyState,tauri:!!window.__TAURI_INTERNALS__})",
                returnByValue: true,
              },
            }),
          );
        },
      ],
      [
        "message",
        (event) => {
          try {
            const reply = JSON.parse(String(event.data));
            if (![1, 2].includes(reply.id)) return;
            replies.set(reply.id, reply.error ? { error: reply.error } : reply.result);
            if (replies.size === 2) finish();
          } catch (error) {
            finish(error);
          }
        },
      ],
      ["error", () => finish(new Error("原始页面 CDP 连接失败"))],
      ["close", () => finish(new Error("原始页面 CDP 连接提前关闭"))],
    ];
    for (const [name, handler] of listeners) socket.addEventListener(name, handler);
    signal.addEventListener("abort", onAbort, { once: true });
    if (signal.aborted) onAbort();
  });
}

export async function captureStartupDiagnostics({
  port,
  confirmPortOwner,
  captureDump,
  playwrightUrls = [],
  fetchImpl = fetch,
  readPageState = readRawPageState,
  timeoutMs = STARTUP_DIAGNOSTIC_TIMEOUT_MS,
}) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const result = { observation: "unavailable", playwright_urls: playwrightUrls.slice(0, 16) };
  try {
    const browser = async () => {
      result.port_owner = await bounded(confirmPortOwner, controller.signal, 3200);
      if (result.port_owner.value !== true) {
        result.raw_targets = { status: "unavailable", error: "端口归属未确认，不请求 HTTP/CDP" };
        return;
      }
      const read = (name) =>
        bounded(
          async (signal) => {
            const response = await fetchImpl(`http://127.0.0.1:${port}/json/${name}`, { signal });
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            return response.json();
          },
          controller.signal,
          1200,
        );
      [result.runtime_version, result.raw_targets] = await Promise.all([
        read("version"),
        read("list"),
      ]);
      const targets = result.raw_targets.value;
      if (!Array.isArray(targets)) return;
      result.raw_targets.value = targets.slice(0, 16);
      result.page_states = await Promise.all(
        targets
          .filter((t) => t.type === "page")
          .slice(0, 2)
          .map((target) =>
            bounded((signal) => readPageState(target, { port, signal }), controller.signal, 1500),
          ),
      );
      const app =
        result.page_states.some(
          (s) => isApp(s.value?.frame?.url) || isApp(s.value?.location?.href),
        ) || targets.some((t) => t.type === "page" && isApp(t.url));
      result.observation = app
        ? playwrightUrls.some(isApp)
          ? "app-page-observed"
          : "observer-disagreement"
        : targets.some((t) => t.type === "page" && t.url === "about:blank")
          ? "blank-targets"
          : "unavailable";
    };
    await Promise.all([
      browser().catch((e) => {
        result.browser_error = shortError(e);
      }),
      bounded(captureDump, controller.signal, Math.min(timeoutMs, 6000)).then((dump) => {
        result.dump =
          dump.status === "captured" && dump.value?.status !== "captured"
            ? { status: "unavailable", error: dump.value?.error ?? "转储未生成" }
            : dump;
      }),
    ]);
    return result;
  } finally {
    clearTimeout(timer);
    controller.abort();
  }
}
