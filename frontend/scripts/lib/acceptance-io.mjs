// AC02：只提供回环价格桩；计数代理从不转发。
import fs from "node:fs";
import crypto from "node:crypto";
import http from "node:http";
import path from "node:path";

const ENDPOINTS = { "/modelsdev/api.json": "modelsdev", "/openrouter/api/v1/models": "openrouter" };
const EVENTS = new Set([
  "consent_saved",
  "business_ready",
  "collect_round_start",
  "source_root_read",
  "history_open",
  "ccs_open",
  "pricing_fetch",
  "opener",
  "settings_write",
  "autostart_read",
  "autostart_write",
]);
const PATH_EVENTS = new Set([
  "source_root_read",
  "history_open",
  "ccs_open",
  "pricing_fetch",
  "opener",
  "settings_write",
  "consent_saved",
  "autostart_read",
  "autostart_write",
]);

export function proxyEnvironment(proxyUrl, noProxy = "127.0.0.1,localhost") {
  return {
    HTTP_PROXY: proxyUrl,
    HTTPS_PROXY: proxyUrl,
    http_proxy: proxyUrl,
    https_proxy: proxyUrl,
    ALL_PROXY: proxyUrl,
    all_proxy: proxyUrl,
    NO_PROXY: noProxy,
    no_proxy: noProxy,
  };
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve(server.address().port);
    });
  });
}

function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
    server.closeAllConnections();
  });
}

export async function startAcceptanceIo({ onViolation = () => {} } = {}) {
  const modes = { modelsdev: "ok", openrouter: "ok" };
  const hits = { modelsdev: [], openrouter: [] };
  const proxyAttempts = [];
  const fixtures = Object.fromEntries(
    Object.values(ENDPOINTS).map((source) => [
      source,
      fs.readFileSync(new URL(`../fixtures/pricing/${source}.json`, import.meta.url)),
    ]),
  );
  const prices = http.createServer((request, response) => {
    const source = ENDPOINTS[request.url];
    if (!source) {
      response.writeHead(404).end();
      return;
    }
    hits[source].push({
      at: new Date().toISOString(),
      method: request.method,
      url: request.url,
      mode: modes[source],
    });
    if (modes[source] === "down") {
      request.socket.destroy();
      return;
    }
    if (modes[source] === "500") {
      response.writeHead(500).end("synthetic price source failure");
      return;
    }
    response.writeHead(200, { "Content-Type": "application/json" }).end(fixtures[source]);
  });
  function attempt(request) {
    const item = { at: new Date().toISOString(), method: request.method, url: request.url };
    proxyAttempts.push(item);
    onViolation(`计数代理记录到连接尝试：${request.method} ${request.url}`);
  }
  const proxy = http.createServer((request, response) => {
    attempt(request);
    response.writeHead(403, { Connection: "close" }).end("acceptance proxy never forwards");
  });
  proxy.on("connect", (request, socket) => {
    attempt(request);
    socket.end("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");
  });
  let pricePort;
  let proxyPort;
  try {
    pricePort = await listen(prices);
    proxyPort = await listen(proxy);
  } catch (error) {
    if (prices.listening) await close(prices);
    if (proxy.listening) await close(proxy);
    throw error;
  }
  const base = `http://127.0.0.1:${pricePort}`;
  const proxyUrl = `http://127.0.0.1:${proxyPort}`;
  return {
    base,
    env: {
      TOKENSCOPE_ACCEPTANCE_PRICING_BASE: base,
      TOKENSCOPE_ACCEPTANCE_OPENER: "record",
      ...proxyEnvironment(proxyUrl),
    },
    setMode(source, mode) {
      if (!(source in modes) || !["ok", "500", "down"].includes(mode))
        throw new Error(`无效价格桩模式：${source}/${mode}`);
      modes[source] = mode;
    },
    snapshot: () => structuredClone({ base, modes, hits, proxy_attempts: proxyAttempts }),
    close: () => Promise.all([close(prices), close(proxy)]),
  };
}

function canonicalPath(value) {
  let current = path.resolve(value);
  const suffix = [];
  while (!fs.existsSync(current)) {
    const parent = path.dirname(current);
    if (parent === current) throw new Error(`路径没有存在的父目录：${value}`);
    suffix.unshift(path.basename(current));
    current = parent;
  }
  return path.join(fs.realpathSync.native(current), ...suffix);
}

function inside(root, value) {
  const relative = path.relative(canonicalPath(root), canonicalPath(value));
  return (
    relative === "" ||
    (!path.isAbsolute(relative) && relative !== ".." && !relative.startsWith(`..${path.sep}`))
  );
}

export function validateAcceptanceTrace(root, text, { partial = false } = {}) {
  const records = [];
  const violations = [];
  const lines = text.split("\n");
  if (partial && !text.endsWith("\n")) lines.pop();
  let previousMs = 0;
  for (const line of lines.filter((value) => value.length > 0)) {
    try {
      const row = JSON.parse(line);
      if (row.seq !== records.length + 1) throw new Error(`seq 不连续：${row.seq}`);
      if (!Number.isSafeInteger(row.monotonic_ms) || row.monotonic_ms < previousMs)
        throw new Error("单调时间无效");
      if (!row.utc?.endsWith("Z") || !Number.isFinite(Date.parse(row.utc)))
        throw new Error("UTC 时间无效");
      if (!EVENTS.has(row.event)) throw new Error(`未知追踪事件：${row.event}`);
      if (
        PATH_EVENTS.has(row.event) &&
        (typeof row.path !== "string" || !row.path || !path.isAbsolute(row.path))
      ) {
        throw new Error(`路径类追踪缺少绝对 path：${row.event}`);
      }
      if (row.path && !inside(root, row.path)) throw new Error(`隔离根外追踪路径：${row.path}`);
      if (row.event === "pricing_fetch") {
        const url = new URL(row.url);
        if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || !ENDPOINTS[url.pathname])
          throw new Error(`非本地价格请求：${row.url}`);
      }
      previousMs = row.monotonic_ms;
      records.push(row);
    } catch (error) {
      violations.push(String(error.message ?? error));
    }
  }
  return { records, violations };
}

export function missingBusinessEvents(records) {
  const events = new Set(records.map((row) => row.event));
  return ["business_ready", "collect_round_start", "history_open", "source_root_read"].filter(
    (event) => !events.has(event),
  );
}

// 既有 stop 只允许在取消前进入；先进入安全收尾，再阻断后续业务和重启。
// 两个操作即使有一个失败，也必须等待另一个完成，不能留下悬空的退出确认。
export function beginIsolationCleanup({ terminate, stop, cancel }) {
  const run = (callback) => {
    try {
      return Promise.resolve(callback());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const terminating = run(terminate);
  const stopping = run(stop);
  cancel();
  return Promise.allSettled([terminating, stopping]).then((results) => {
    const errors = results
      .filter((result) => result.status === "rejected")
      .map((result) => result.reason);
    if (errors.length) throw new AggregateError(errors, "隔离违例收尾未完整成功");
    return results[1].value;
  });
}

export async function terminateAfterSnapshot({ refresh, terminate }) {
  let snapshot;
  try {
    snapshot = await refresh();
  } catch (error) {
    snapshot = { ok: false, error: String(error?.message ?? error) };
  }
  const terminated = await terminate();
  if (!snapshot?.ok) throw new Error(`隔离停止未能确认后代身份：${snapshot?.error ?? "探测失败"}`);
  return terminated;
}

export function readAcceptanceTrace(root, options) {
  const file = path.join(root, "acceptance-trace.jsonl");
  return validateAcceptanceTrace(
    root,
    fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "",
    options,
  );
}

export function settingsFingerprint(bytes) {
  let value = 0xcbf29ce484222325n;
  for (const byte of bytes) value = BigInt.asUintN(64, (value ^ BigInt(byte)) * 0x100000001b3n);
  return { algorithm: "fnv1a64", value: value.toString(16).padStart(16, "0") };
}

export function snapshotBusinessFiles(dataDir) {
  const files = {};
  function walk(directory, relative = "") {
    for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
      const name = relative ? `${relative}/${item.name}` : item.name;
      const absolute = path.join(directory, item.name);
      if (item.isDirectory()) walk(absolute, name);
      else if (name !== "settings.toml") {
        const stat = fs.statSync(absolute);
        files[name] = {
          bytes: stat.size,
          mtime_ms: stat.mtimeMs,
          sha256: crypto.createHash("sha256").update(fs.readFileSync(absolute)).digest("hex"),
        };
      }
    }
  }
  if (fs.existsSync(dataDir)) walk(dataDir);
  return files;
}

export function changedBusinessFiles(before, after) {
  return [...new Set([...Object.keys(before), ...Object.keys(after)])]
    .sort()
    .filter((name) => JSON.stringify(before[name]) !== JSON.stringify(after[name]));
}
