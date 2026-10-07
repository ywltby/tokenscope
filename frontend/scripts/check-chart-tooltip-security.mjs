// SF01（安全审查 Task 1）：真实 ECharts tooltip 注入检查（HTML 输出边界）。
//
// 依赖：Node 18+、playwright-core；浏览器解析同 check-app-scroll.mjs
//（ms-playwright 缓存 → 系统 Chrome → 系统 Edge）。
//
// 需要先启动本地 Vite dev server（不参与生产构建的检查页）：
//   pnpm --dir frontend exec vite --host 127.0.0.1 --port 1437
// 然后运行：
//   node frontend/scripts/check-chart-tooltip-security.mjs --url http://127.0.0.1:1437
//
// 检查页 frontend/security-tooltip.html **无 CSP**——通过即证明安全来自
// TrendChart/chartTooltip 的 DOM textContent 输出边界，而非策略掩盖。
// 具名检查：real_echarts_does_not_interpret_untrusted_html（浅/深 ×
// 模型/项目四场景）。--vuln 为负向对照：走漏洞页，断言本脚本**能**
// 检出注入（产品修复回退时此处会变红）。
//
// 产出：docs/plans/qa-artifacts/chart-tooltip-security/{measurements.json,*.png}
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

function parseArgs(argv) {
  const args = { url: "http://127.0.0.1:1437", vuln: false };
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === "--url") args.url = argv[++i];
    else if (argv[i] === "--vuln") args.vuln = true;
    else if (argv[i] === "--output") args.output = argv[++i];
    else throw new Error(`未知参数 ${argv[i]}`);
  }
  return args;
}
const args = parseArgs(process.argv);

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const outDir = join(root, "docs", "plans", "qa-artifacts", "chart-tooltip-security");
mkdirSync(outDir, { recursive: true });

function resolveExecutable() {
  const candidates = [];
  const cache = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, "ms-playwright") : null;
  if (cache && existsSync(cache)) {
    for (const d of readdirSafe(cache).filter((d) => d.startsWith("chromium-"))) {
      for (const rel of ["chrome-win64/chrome.exe", "chrome-win/chrome.exe"]) {
        candidates.push(join(cache, d, rel));
      }
    }
  }
  candidates.push("C:/Program Files/Google/Chrome/Application/chrome.exe");
  candidates.push("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe");
  const hit = candidates.find((p) => existsSync(p));
  if (!hit) throw new Error("未找到可用的 Chromium/Chrome/Edge，请安装后再运行本脚本");
  return hit;
}
function readdirSafe(p) {
  try {
    return readdirSync(p);
  } catch {
    return [];
  }
}

const base = args.url.replace(/\/$/, "");
const pageUrl = `${base}/security-tooltip.html`;

const browser = await chromium.launch({
  executablePath: resolveExecutable(),
  headless: true,
});

const results = [];
let violationsTotal = 0;

async function runScenario(label, query, theme) {
  const context = await browser.newContext({ viewport: { width: 980, height: 620 } });
  const violations = [];

  await context.addInitScript(() => {
    window.__TS_ATTACKED = 0;
    window.__TS_IPC_CALLS = [];
    window.__TS_PAGE_ERRORS = [];
    const calls = window.__TS_IPC_CALLS;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {
        invoke: (cmd, args) => {
          calls.push({ cmd, args });
          return Promise.reject(new Error("security-check: unexpected IPC"));
        },
        postMessage: (msg) => calls.push({ postMessage: msg }),
        transformCallback: (cb) => cb,
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { label: "main" },
        },
      },
      writable: false,
      configurable: false,
    });
    window.addEventListener("error", (e) => window.__TS_PAGE_ERRORS.push(String(e.message)));
    window.addEventListener("unhandledrejection", (e) =>
      window.__TS_PAGE_ERRORS.push(String(e.reason)),
    );
  });

  // 主题偏好先于页面模块写入（theme.ts 在模块加载时读取）
  await context.addInitScript(([t]) => window.localStorage.setItem("tokenscope-theme", t), [theme]);

  const externalRequests = [];
  await context.route("**/*", (route) => {
    const url = route.request().url();
    if (!url.startsWith(base) && !url.startsWith("data:") && !url.startsWith("blob:")) {
      externalRequests.push(url);
      violations.push(`external_request: ${url}`);
      return route.abort();
    }
    return route.continue();
  });

  const page = await context.newPage();
  const consoleErrors = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

  await page.goto(`${pageUrl}?${query}`, { waitUntil: "networkidle" });
  await page.waitForFunction(() => window.__TS_PAGE_READY === true);

  const ctx = await page.evaluate(() => window.__TS_GET_TOOLTIP_CONTEXT());
  if (!ctx?.ok) throw new Error(`检查页未就绪：${ctx?.reason ?? "unknown"}`);

  // 等 tooltip 过渡结束再量测（ECharts 默认 transitionDuration 0.4s）
  await page.waitForSelector("#app .chart-tooltip, .chart-canvas div", { timeout: 5000 });
  await page.waitForTimeout(700);

  const m = await page.evaluate(
    ({ payloads, texts }) => {
      const chartDom =
        document.querySelector("#app .chart-canvas") ?? document.querySelector(".chart-canvas");
      // ECharts tooltip 容器：图表容器内 position:absolute 的 HTML tooltip
      const root = document.querySelector(".chart-tooltip");
      const dangerSel =
        "img,svg,iframe,script,a,object,embed,video,audio,form,button,input,link,style,math";
      const rootText = root ? root.textContent : "";
      const allText = [...texts, rootText].join("\n");
      const injectedInRoot = root ? root.querySelectorAll(dangerSel).length : -1;
      const injectedInChart = chartDom ? chartDom.querySelectorAll(dangerSel).length : -1;
      const rootChildren = root
        ? Array.from(root.childNodes).map((n) =>
            n.nodeType === Node.TEXT_NODE
              ? "text"
              : n.nodeType === Node.ELEMENT_NODE
                ? n.tagName
                : `node${n.nodeType}`,
          )
        : [];
      return {
        attacked: window.__TS_ATTACKED,
        ipcCalls: window.__TS_IPC_CALLS,
        pageErrors: window.__TS_PAGE_ERRORS,
        hasRoot: !!root,
        rootText,
        payloadsIntact: Object.values(payloads).every((p) => allText.includes(p)),
        injectedInRoot,
        injectedInChart,
        rootChildren,
        rootTag: root?.tagName ?? null,
      };
    },
    { payloads: ctx.payloads, texts: ctx.texts },
  );

  const attackRequests = externalRequests.filter((u) => u.includes("ts-attack-probe-src"));

  // 漏洞对照模式断言"能检出"；正常模式断言"无注入"
  const checks = args.vuln
    ? [["vuln 页确实出现注入元素", m.injectedInRoot > 0 || m.attacked > 0]]
    : [
        ["formatter 走 DOM 分支（.chart-tooltip 根元素）", m.hasRoot && m.rootTag === "DIV"],
        ["payload 完整作为文本可读", m.payloadsIntact],
        ["tooltip 内无注入元素", m.injectedInRoot === 0],
        ["图表容器内无注入元素", m.injectedInChart === 0],
        ["攻击哨兵未被触发", m.attacked === 0],
        ["无攻击诱发的网络请求", attackRequests.length === 0],
        ["未发生任何 IPC 调用", m.ipcCalls.length === 0],
        ["无页面错误/未处理拒绝", m.pageErrors.length === 0 && consoleErrors.length === 0],
      ];
  const ok = checks.every(([, pass]) => pass);
  if (!ok) violationsTotal++;

  results.push({
    label,
    mode: args.vuln ? "vuln-control" : "verify",
    ok,
    theme,
    query,
    checks,
    attacked: m.attacked,
    ipcCalls: m.ipcCalls.length,
    pageErrors: [...m.pageErrors, ...consoleErrors],
    externalRequests,
    attackRequests,
    injectedInRoot: m.injectedInRoot,
    injectedInChart: m.injectedInChart,
    payloadsIntact: m.payloadsIntact,
  });
  console.log(`${ok ? "PASS" : "FAIL"} ${label}`);
  for (const [name, pass] of checks) console.log(`  ${pass ? "✓" : "✗"} ${name}`);

  await page.screenshot({ path: join(outDir, `${label}.png`) });
  await context.close();
}

try {
  if (args.vuln) {
    await runScenario("vuln-control-light-model", "theme=light&dim=model&vuln=1", "light");
  } else {
    for (const theme of ["light", "dark"]) {
      for (const dim of ["model", "project"]) {
        await runScenario(`tooltip-security-${theme}-${dim}`, `theme=${theme}&dim=${dim}`, theme);
      }
    }
  }
} finally {
  await browser.close();
}

writeFileSync(
  join(outDir, "measurements.json"),
  JSON.stringify({ mode: args.vuln ? "vuln-control" : "verify", results }, null, 2),
);

if (args.vuln) {
  const detected = results.every((r) => r.ok);
  if (!detected) {
    console.error("负向对照失败：注入未被检出，检查脚本或检查页已失效");
    process.exit(1);
  }
  console.log("\n负向对照通过：漏洞模式被正确检出（检查脚本具备检测能力）");
  process.exit(0);
}

const checkName = "real_echarts_does_not_interpret_untrusted_html";
if (violationsTotal > 0) {
  console.error(`\nFAIL ${checkName}：${violationsTotal} 个场景存在注入或异常`);
  process.exit(1);
}
console.log(`\nPASS ${checkName}（四场景 × 无 CSP 环境，证据见 ${outDir}）`);
