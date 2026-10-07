// UX00（UI/UX 审查修复计划）：真实组件量测入口。
//
// 与 check-app-scroll.mjs（合成 CSS 长页）不同，本脚本通过 URL 加载**真实
// 前端**（main.ts → App.vue），用 addInitScript 注入合成 __TAURI_INTERNALS__
// 并显式 mock 所有 IPC：未知 command 直接失败，避免"静默返回 null"把协议
// 漂移伪装成通过。所有数据均为 fixture 合成，不含真实用户数据。
//
// 依赖：Node 18+、playwright-core；浏览器解析同 check-app-scroll.mjs
//（ms-playwright 缓存 → 系统 Chrome → 系统 Edge）。
//
// 运行：
//   pnpm --dir frontend exec vite --host 127.0.0.1 --port 1437
//   node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 \
//     --phase baseline --output docs/plans/qa-artifacts/ui-ux-remediation/before
//   node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 \
//     --phase verify --output docs/plans/qa-artifacts/ui-ux-remediation/after
//
// phase=baseline 只记录当前违例（用于"修复前证据"）；phase=verify 按本计划
// 断言具名契约。两种模式都要求：无未知 command、无未处理拒绝、无页面错误、
// 无外部网络请求（fixture 主动返回的 IPC reject 属预期输入，不判脚本失败）。
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { FIXTURE_NAMES, buildFixture, isReject } from "./fixtures/ui-contracts.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:1437",
    phase: "verify",
    output: join(root, "docs", "plans", "qa-artifacts", "ui-ux-remediation", "after"),
    fixtures: ["normal", "empty", "unknown-price", "partial-price", "long-text", "multi-category"],
    themes: ["light", "dark"],
    viewports: [
      { width: 1280, height: 820, label: "1280x820" },
      { width: 980, height: 620, label: "980x620" },
    ],
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--url") args.url = argv[++i];
    else if (a === "--phase") args.phase = argv[++i];
    else if (a === "--output") args.output = argv[++i];
    else if (a === "--fixture") args.fixtures = argv[++i].split(",");
    else if (a === "--theme") args.themes = argv[++i].split(",");
    else if (a === "--viewport") {
      args.viewports = argv[++i].split(",").map((s) => {
        const [w, h] = s.split("x").map(Number);
        return { width: w, height: h, label: s };
      });
    } else throw new Error(`未知参数 ${a}`);
  }
  if (args.phase !== "baseline" && args.phase !== "verify")
    throw new Error(`--phase 必须是 baseline 或 verify（收到 ${args.phase}）`);
  for (const f of args.fixtures)
    if (!FIXTURE_NAMES.includes(f)) throw new Error(`未知 fixture ${f}（可选：${FIXTURE_NAMES.join(", ")}）`);
  return args;
}

function resolveExecutable() {
  const candidates = [];
  const cache = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, "ms-playwright") : null;
  if (cache && existsSync(cache)) {
    for (const d of readdirSafe(cache).filter((x) => x.startsWith("chromium-"))) {
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

const args = parseArgs(process.argv);
const base = args.url.replace(/\/$/, "");
mkdirSync(args.output, { recursive: true });

const browser = await chromium.launch({ executablePath: resolveExecutable(), headless: true });
const browserVersion = browser.version();

const results = [];
let hardFailures = 0;
let contractFailures = 0;

/** 页面内注入：合成 Tauri 运行时 + 收集钩子。 */
const INIT_SCRIPT = () => {
  window.__TS_IPC_CALLS = [];
  window.__TS_PAGE_ERRORS = [];
  window.__TS_CB_ID = 0;
  window.__TS_CB = {};
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, payload) => {
      window.__TS_IPC_CALLS.push({ cmd, args: payload ?? {} });
      return window.__TS_FIXTURE_IPC(cmd, payload ?? {});
    },
    transformCallback: (cb) => {
      const id = ++window.__TS_CB_ID;
      window.__TS_CB[id] = cb;
      return id;
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.addEventListener("error", (e) => {
    // ResizeObserver 的 "loop completed with undelivered notifications" 是
    // 浏览器对布局抖动的良性提示（ECharts ResizeObserver + 量测引起），
    // 非应用错误；其余错误照常记录。
    if (/ResizeObserver loop/.test(String(e.message))) return;
    window.__TS_PAGE_ERRORS.push(String(e.message));
  });
  window.addEventListener("unhandledrejection", (e) =>
    window.__TS_PAGE_ERRORS.push(`unhandledrejection: ${String(e.reason?.message ?? e.reason)}`),
  );
};

/** 页面内量测：返回结构化数值（不做断言）。 */
const MEASURE = () => {
  const px = (v) => Math.round(Number.parseFloat(v) * 100) / 100;
  const rect = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { width: px(r.width), height: px(r.height), top: px(r.top), left: px(r.left) };
  };
  const findSurface = (el) => {
    let n = el;
    while (n && n !== document.body) {
      const bg = getComputedStyle(n).backgroundColor;
      if (bg && bg !== "transparent" && bg !== "rgba(0, 0, 0, 0)") return n;
      n = n.parentElement;
    }
    return el;
  };
  const textOverflow = (el) => {
    if (!el) return null;
    const range = document.createRange();
    range.selectNodeContents(el);
    const textW = px(range.getBoundingClientRect().width);
    return {
      scrollWidth: el.scrollWidth,
      clientWidth: el.clientWidth,
      textWidth: textW,
      clipped: el.scrollWidth > el.clientWidth + 1 || textW > el.clientWidth + 1,
      text: el.textContent ?? "",
    };
  };

  const segmented = [...document.querySelectorAll(".ts-segmented")].map((s) => {
    const items = [...s.querySelectorAll(".ts-segmented-item")];
    const checked = items.find((i) => i.getAttribute("aria-checked") === "true") ?? null;
    const thumb = s.querySelector(".ts-segmented-thumb");
    return {
      outer: rect(s),
      computedHeight: getComputedStyle(s).height,
      boxSizing: getComputedStyle(s).boxSizing,
      itemHeights: items.map((i) => rect(i)?.height ?? null),
      checkedRect: rect(checked),
      thumbRect: rect(thumb),
      thumbOverflow: thumb ? textOverflow(thumb) : null,
    };
  });

  const filterRow = document.querySelector(".filter-row");
  const filterControls = filterRow
    ? [...filterRow.children].map((c) => ({
        tag: c.tagName.toLowerCase(),
        cls: c.className,
        rect: rect(c),
      }))
    : [];

  const tableTypography = (sel) => {
    const root = document.querySelector(sel);
    if (!root) return null;
    const th = root.querySelector("th");
    const td = root.querySelector("tbody td");
    const g = (el) => (el ? { fontSize: getComputedStyle(el).fontSize, fontWeight: getComputedStyle(el).fontWeight, lineHeight: getComputedStyle(el).lineHeight, color: getComputedStyle(el).color } : null);
    return { th: g(th), td: g(td) };
  };

  const totalRow = document.querySelector("tr.total-row");
  const totalCells = totalRow ? [...totalRow.querySelectorAll("td")].map((td) => getComputedStyle(td).fontWeight) : null;

  return {
    segmented,
    filterControls,
    aggregateTable: tableTypography(".usage-card"),
    eventsTable: tableTypography(".events-card"),
    totalRowWeights: totalCells,
    costTooltip: window.__TS_COST_TOOLTIP ?? null,
    datePanel: window.__TS_DATE_PANEL ?? null,
    dateShortcuts: window.__TS_DATE_SHORTCUTS ?? null,
    ipcCalls: window.__TS_IPC_CALLS,
    pageErrors: window.__TS_PAGE_ERRORS,
    docScrollWidth: document.scrollingElement?.scrollWidth ?? 0,
    viewportWidth: window.innerWidth,
  };
};

/** 打开费用浮层并量测（在页面内执行）。 */
const OPEN_COST_TOOLTIP = () => {
  const trigger = document.querySelector('span[role="button"][aria-label="费用计算明细"]');
  if (!trigger) return { available: false, reason: "no-cost-trigger" };
  trigger.dispatchEvent(new MouseEvent("mouseenter", { bubbles: true }));
  return { available: true };
};

const READ_COST_TOOLTIP = () => {
  const content = document.querySelector(".cost-tooltip");
  if (!content) return { available: false, reason: "tooltip-not-open" };
  const px = (v) => Math.round(Number.parseFloat(v) * 100) / 100;
  let surface = content;
  while (surface && surface !== document.body) {
    const bg = getComputedStyle(surface).backgroundColor;
    if (bg && bg !== "transparent" && bg !== "rgba(0, 0, 0, 0)") break;
    surface = surface.parentElement;
  }
  const cs = getComputedStyle(surface);
  const r = surface.getBoundingClientRect();
  return {
    available: true,
    outerWidth: px(r.width),
    backgroundColor: cs.backgroundColor,
    backdropFilter: cs.backdropFilter || cs.webkitBackdropFilter,
    borderRadius: cs.borderRadius,
    boxSizing: cs.boxSizing,
    overflowX: content.scrollWidth > content.clientWidth + 1,
  };
};

/** 打开日期弹层并量测快捷项与面板材质。 */
const OPEN_DATE_PANEL = () => {
  const trigger = document.querySelector(".range-trigger");
  if (!trigger) return { available: false };
  trigger.click();
  return { available: true };
};

const READ_DATE_PANEL = () => {
  const panel = document.querySelector(".range-panel");
  if (!panel) return { available: false };
  const px = (v) => Math.round(Number.parseFloat(v) * 100) / 100;
  const cs = getComputedStyle(panel);
  const shortcuts = [...document.querySelectorAll(".shortcut-btn")].map((b) => {
    const label = b.querySelector(".n-button__content") ?? b;
    const range = document.createRange();
    range.selectNodeContents(label);
    const textW = px(range.getBoundingClientRect().width);
    const r = b.getBoundingClientRect();
    return {
      text: b.textContent.trim(),
      rect: { width: px(r.width), height: px(r.height) },
      scrollWidth: b.scrollWidth,
      clientWidth: b.clientWidth,
      textWidth: textW,
      clipped: b.scrollWidth > b.clientWidth + 1 || textW > b.clientWidth + 1,
    };
  });
  const r = panel.getBoundingClientRect();
  return {
    available: true,
    backgroundColor: cs.backgroundColor,
    backdropFilter: cs.backdropFilter || cs.webkitBackdropFilter,
    borderRadius: cs.borderRadius,
    width: px(r.width),
    shortcuts,
  };
};

/** 切到设置页（用于三表字号与目录输入验收）。 */
const GOTO_SETTINGS = () => {
  const radios = [...document.querySelectorAll(".ts-segmented-item")];
  const settings = radios.find((r) => r.textContent.trim() === "设置");
  if (!settings) return { ok: false };
  settings.click();
  return { ok: true };
};

// ── 具名契约检查（对 measurements 求值，verify 阶段断言） ──────────
function buildChecks(m, ctx) {
  const checks = [];
  const near = (a, b, tol = 1) => a != null && b != null && Math.abs(a - b) <= tol;
  const parseColorAlpha = (c) => {
    const mm = /rgba?\(([^)]+)\)/.exec(c ?? "");
    if (!mm) return null;
    const parts = mm[1].split(",").map((s) => Number.parseFloat(s.trim()));
    return parts.length === 4 ? parts[3] : 1;
  };

  // UX02
  if (ctx.fixture === "normal") {
    checks.push([
      "segmented_outer_height_is_32",
      m.segmented.length > 0 && m.segmented.every((s) => near(s.outer?.height, 32)),
      m.segmented.map((s) => s.outer?.height).join(","),
    ]);
    checks.push([
      "thumb_tracks_option_bounds",
      m.segmented.every(
        (s) =>
          s.checkedRect &&
          s.thumbRect &&
          near(s.thumbRect.width, s.checkedRect.width, 1) &&
          near(s.thumbRect.left, s.checkedRect.left, 1),
      ),
      m.segmented.map((s) => `${s.thumbRect?.width}/${s.checkedRect?.width}`).join(","),
    ]);
  }

  // UX04
  if (ctx.fixture === "normal") {
    const heights = m.filterControls.map((c) => c.rect?.height).filter((h) => h != null);
    checks.push([
      "filter_controls_have_equal_outer_height",
      heights.length >= 4 && heights.every((h) => near(h, 32)) && heights.every((h) => near(h, heights[0])),
      heights.join(","),
    ]);
    const sc = m.dateShortcuts;
    checks.push([
      "date_shortcuts_do_not_clip_text",
      Array.isArray(sc) && sc.length >= 5 && sc.every((s) => !s.clipped),
      Array.isArray(sc) ? sc.map((s) => `${s.text}:${s.clipped ? "CLIP" : "ok"}`).join(",") : "no-panel",
    ]);
    checks.push([
      "date_panel_matches_elevated_contract",
      m.datePanel?.available === true &&
        parseColorAlpha(m.datePanel.backgroundColor) != null &&
        Math.abs(parseColorAlpha(m.datePanel.backgroundColor) - 0.85) <= 0.06 &&
        /blur\(16px\)/.test(m.datePanel.backdropFilter ?? "") &&
        /12px/.test(m.datePanel.borderRadius ?? ""),
      m.datePanel ? `${m.datePanel.backgroundColor} ${m.datePanel.backdropFilter}` : "no-panel",
    ]);
  }

  // UX01
  if (ctx.fixture === "normal") {
    const typo = m.aggregateTable;
    checks.push([
      "three_tables_use_contract_typography",
      typo?.th != null &&
        typo?.td != null &&
        near(Number.parseFloat(typo.th.fontSize), 12, 0.5) &&
        typo.th.fontWeight === "500" &&
        near(Number.parseFloat(typo.td.fontSize), 13, 0.5),
      typo ? `th ${typo.th.fontSize}/${typo.th.fontWeight} td ${typo.td.fontSize}` : "no-table",
    ]);
    checks.push([
      "total_row_is_semibold",
      Array.isArray(m.totalRowWeights) &&
        m.totalRowWeights.length > 0 &&
        m.totalRowWeights.every((w) => w === "600"),
      Array.isArray(m.totalRowWeights) ? m.totalRowWeights.join(",") : "no-total-row",
    ]);
    checks.push([
      "floating_material_matches_elevated_contract",
      m.costTooltip?.available === true &&
        Math.abs(parseColorAlpha(m.costTooltip.backgroundColor) - 0.85) <= 0.06 &&
        /blur\(16px\)/.test(m.costTooltip.backdropFilter ?? "") &&
        /12px/.test(m.costTooltip.borderRadius ?? ""),
      m.costTooltip ? `${m.costTooltip.backgroundColor} ${m.costTooltip.backdropFilter}` : "no-tooltip",
    ]);
    checks.push([
      "cost_popover_outer_width_is_bounded",
      m.costTooltip?.available === true &&
        m.costTooltip.boxSizing === "border-box" &&
        m.costTooltip.outerWidth <= 481,
      m.costTooltip ? `w=${m.costTooltip.outerWidth} box=${m.costTooltip.boxSizing}` : "no-tooltip",
    ]);
  }

  // 横向不滚动（长文本/多类别场景）
  if (ctx.fixture === "long-text" || ctx.fixture === "multi-category") {
    checks.push([
      "page_has_no_horizontal_overflow",
      m.docScrollWidth <= m.viewportWidth + 1,
      `${m.docScrollWidth} vs ${m.viewportWidth}`,
    ]);
  }

  return checks;
}

for (const fixtureName of args.fixtures) {
  for (const theme of args.themes) {
    for (const vp of args.viewports) {
      const label = `${fixtureName}-${theme}-${vp.label}`;
      const fixture = buildFixture(fixtureName);
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const externalRequests = [];
      const ipcCalls = [];
      const unknownCommands = [];

      // Node 侧桥接：未知 command 显式记录并由断言判定；reject 标记翻成拒绝。
      await context.exposeFunction("__TS_FIXTURE_IPC", (cmd, payload) => {
        ipcCalls.push({ cmd, args: payload });
        const res = fixture.ipc(cmd, payload);
        if (res === undefined) {
          unknownCommands.push(cmd);
          return { __unknown: true, cmd };
        }
        if (isReject(res)) throw new Error(res.message);
        return res;
      });
      await context.addInitScript(INIT_SCRIPT);
      await context.addInitScript(([t]) => {
        try {
          window.localStorage.setItem("tokenscope-theme", t);
        } catch {
          /* ignore */
        }
      }, [theme]);
      await context.route("**/*", (route) => {
        const url = route.request().url();
        if (!url.startsWith(base) && !url.startsWith("data:") && !url.startsWith("blob:")) {
          externalRequests.push(url);
          return route.abort();
        }
        return route.continue();
      });

      const page = await context.newPage();
      const consoleErrors = [];
      page.on("console", (msg) => {
        if (msg.type() === "error" && !/ResizeObserver loop/.test(msg.text()))
          consoleErrors.push(msg.text());
      });
      page.on("pageerror", (e) => {
        if (!/ResizeObserver loop/.test(e.message)) consoleErrors.push(`pageerror: ${e.message}`);
      });

      const record = { label, fixture: fixtureName, theme, viewport: vp.label, checks: [], ok: false };

      try {
        await page.goto(base, { waitUntil: "networkidle" });
        await page.waitForSelector("#app .app-shell", { timeout: 10000 });
        await page.waitForSelector(".page-head", { timeout: 10000 });
        // 等字体与过渡结束（禁止把动画中间值当最终尺寸）
        await page.evaluate(() => document.fonts?.ready);
        await page.waitForTimeout(500);

        // 依次采集：费用浮层 → 日期弹层 → 设置页
        const costOpen = await page.evaluate(OPEN_COST_TOOLTIP);
        if (costOpen.available) {
          await page.waitForTimeout(350);
          const tip = await page.evaluate(READ_COST_TOOLTIP);
          await page.evaluate((t) => {
            window.__TS_COST_TOOLTIP = t;
          }, tip);
          await page.evaluate(() => {
            const trigger = document.querySelector('span[role="button"][aria-label="费用计算明细"]');
            trigger?.dispatchEvent(new MouseEvent("mouseleave", { bubbles: true }));
          });
          await page.waitForTimeout(150);
        }
        const dateOpen = await page.evaluate(OPEN_DATE_PANEL);
        if (dateOpen.available) {
          await page.waitForTimeout(350);
          const panel = await page.evaluate(READ_DATE_PANEL);
          await page.evaluate((p) => {
            window.__TS_DATE_PANEL = p;
            window.__TS_DATE_SHORTCUTS = p.shortcuts ?? null;
          }, panel);
          await page.keyboard.press("Escape");
          await page.waitForTimeout(150);
        }

        const m = await page.evaluate(MEASURE);
        const checks = buildChecks(m, { fixture: fixtureName });

        // 硬约束：IPC 无未知命令、无未处理拒绝、无外部请求、无页面错误
        const bootChecks = [
          ["real_app_fixture_boots_without_ipc_leak", unknownCommands.length === 0, unknownCommands.join(",")],
          ["no_unhandled_rejection_or_page_error", m.pageErrors.length === 0, m.pageErrors.join(" | ")],
          ["no_console_error", consoleErrors.length === 0, consoleErrors.join(" | ")],
          ["no_external_request", externalRequests.length === 0, externalRequests.join(",")],
        ];

        const allChecks = [...bootChecks, ...checks];
        record.checks = allChecks.map(([name, pass, detail]) => ({ name, pass, detail }));
        record.ok = allChecks.every(([, pass]) => pass);
        record.pageErrors = m.pageErrors;
        record.consoleErrors = consoleErrors;
        record.externalRequests = externalRequests;
        record.ipcCommands = [...new Set(ipcCalls.map((c) => c.cmd))];
        record.measurements = m;

        const bootOk = bootChecks.every(([, pass]) => pass);
        if (!bootOk) hardFailures++;
        const contractOk = checks.every(([, pass]) => pass);
        if (!contractOk) contractFailures++;

        await page.screenshot({ path: join(args.output, `${label}.png`), fullPage: false });

        console.log(`${record.ok ? "PASS" : "FAIL"} ${label}`);
        for (const [name, pass, detail] of allChecks)
          console.log(`  ${pass ? "✓" : "✗"} ${name}${detail ? `  (${detail})` : ""}`);
      } catch (e) {
        record.error = e instanceof Error ? e.message : String(e);
        hardFailures++;
        console.log(`FAIL ${label} — ${record.error}`);
      } finally {
        results.push(record);
        await context.close();
      }
    }
  }
}

await browser.close();

writeFileSync(
  join(args.output, "measurements.json"),
  JSON.stringify(
    {
      phase: args.phase,
      generated_at: new Date().toISOString(),
      url: base,
      browserVersion,
      results,
    },
    null,
    2,
  ),
);

const baselineCheck = {
  name: "baseline_records_contract_violations",
  pass: results.length > 0 && results.every((r) => Array.isArray(r.checks)),
  detail: `${results.length} 个场景，${contractFailures} 个契约违例`,
};

console.log(`\n${baselineCheck.pass ? "PASS" : "FAIL"} ${baselineCheck.name} (${baselineCheck.detail})`);

// 硬约束在两种模式下都必须通过（boot 失败说明 fixture/协议漂移，不是"待修契约"）。
if (hardFailures > 0) {
  console.error(`\n${hardFailures} 个场景存在硬失败（IPC 泄漏/页面错误/外部请求），不得作为 baseline 通过`);
  process.exit(1);
}
if (args.phase === "verify" && contractFailures > 0) {
  console.error(`\nverify 阶段仍有 ${contractFailures} 个场景未满足具名契约`);
  process.exit(1);
}
console.log(`\n${args.phase} 完成：证据写入 ${args.output}`);
