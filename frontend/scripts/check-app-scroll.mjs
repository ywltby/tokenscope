// F06（修复后复核 Task 6）：主滚动容器真实浏览器布局验收。
//
// 依赖：Node 18+、`playwright-core`（devDependency）；浏览器按优先级
// 解析 ms-playwright 缓存 → 系统 Chrome → 系统 Edge，全部缺失时报错退出
//（不回退 happy-dom——布局断言必须经过真实 Chromium）。
//
// 脚本从当前主应用组件 MainApp.vue（P04 起 App.vue 是隐私引导壳，业务
// 布局迁到 MainApp.vue）读取**实际** scoped 样式（.app-shell/.app-nav/
// .scroll-container/.app-content/.banner-slot）与 styles/tokens.css，
// 注入一个不含任何用户数据的 2200px 合成长页面，在 1280×820 与
// 980×620、浅色/深色下断言：
//   1. 滚动容器高度受视口约束（不随内容生长）；
//   2. scrollHeight > clientHeight（长内容确实需要内部滚动）；
//   3. 设置 container.scrollTop 后真实内部滚动（document 不产生第二条
//      主滚动条）；
//   4. 吸顶导航 top 始终等于容器顶边（±1px），内容从其后滚过；
//   5. 通知条可见。
// 通过后把量测输出与脱敏截图写入仓库根 qa-artifacts/app-scroll/（已被 .gitignore
// 忽略：属可再生的生成物，不作为源码提交）。
//
// 运行：node frontend/scripts/check-app-scroll.mjs
import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const frontend = join(root, "frontend");
const appVue = readFileSync(join(frontend, "src", "MainApp.vue"), "utf8");
const tokensCss = readFileSync(join(frontend, "src", "styles", "tokens.css"), "utf8");

// ── 从 MainApp.vue 提取实际 scoped 样式（不复制修正后的 CSS 给测试通过）──
function extractStyle(cls) {
  const re = new RegExp(`\\.${cls}\\s*\\{([^}]*)\\}`, "m");
  const m = appVue.match(re);
  if (!m) throw new Error(`MainApp.vue 中未找到 .${cls} 样式（脚本依赖实际样式，禁止内嵌副本）`);
  return m[1].trim();
}
const styleShell = extractStyle("app-shell");
const styleNav = extractStyle("app-nav");
const styleScroll = extractStyle("scroll-container");
const styleBanner = extractStyle("banner-slot");
const styleContent = extractStyle("app-content");

// ── 浏览器解析：ms-playwright 缓存 → 系统 Chrome → 系统 Edge ──
function resolveExecutable() {
  const candidates = [];
  const cache = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, "ms-playwright") : null;
  if (cache && existsSync(cache)) {
    const dirs = readdirSafe(cache).filter((d) => d.startsWith("chromium-"));
    for (const d of dirs) {
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

const page = `<!doctype html>
<html><head><meta charset="utf-8"><style>${tokensCss}</style>
<style>
* { margin: 0; box-sizing: border-box; }
html, body { height: 100%; }
.app-shell { ${styleShell} }
.app-nav { ${styleNav} background: var(--ts-surface-solid); }
.scroll-container { ${styleScroll} }
.banner-slot { ${styleBanner} }
.app-content { ${styleContent} }
.ts-notice { padding: 8px 12px; border: 1px solid var(--ts-separator-strong); border-radius: 8px; }
</style></head>
<body>
<div class="app-shell" id="shell">
  <div class="scroll-container" id="scroller">
    <nav class="app-nav" id="nav"><span class="brand">TokenScope（合成页面）</span></nav>
    <div class="banner-slot"><div class="ts-notice" id="notice">合成通知：布局验收用，不含真实数据。</div></div>
    <div class="app-content" id="content" style="height: 2200px; background: linear-gradient(var(--ts-surface-solid), var(--ts-surface-solid));">
      合成长页内容（2200px，无用户数据）
    </div>
  </div>
</div>
</body></html>`;

const VIEWPORTS = [
  { width: 1280, height: 820 },
  { width: 980, height: 620 },
];
const THEMES = ["light", "dark"];

const results = [];
let failed = 0;

const browser = await chromium.launch({
  executablePath: resolveExecutable(),
  headless: true,
});
try {
  const outDir = join(root, "qa-artifacts", "app-scroll");
  mkdirSync(outDir, { recursive: true });
  for (const vp of VIEWPORTS) {
    for (const theme of THEMES) {
      const context = await browser.newContext({ viewport: vp });
      const p = await context.newPage();
      await p.setContent(page);
      await p.evaluate((t) => document.documentElement.setAttribute("data-theme", t), theme);
      await p.waitForTimeout(50);
      const label = `${vp.width}x${vp.height}-${theme}`;
      const m = await p.evaluate(() => {
        const sc = document.getElementById("scroller");
        const nav = document.getElementById("nav");
        const content = document.getElementById("content");
        const notice = document.getElementById("notice");
        const doc = document.scrollingElement;
        const navTop = () => nav.getBoundingClientRect().top;
        const navTopBefore = navTop();
        sc.scrollTop = 400;
        const scrollTopAfterSet = sc.scrollTop;
        const navTopAfter = navTop();
        const containerTop = sc.getBoundingClientRect().top;
        const contentTop = content.getBoundingClientRect().top;
        const noticeRect = notice.getBoundingClientRect();
        return {
          clientHeight: sc.clientHeight,
          scrollHeight: sc.scrollHeight,
          scrollTopAfterSet,
          docScrollTop: doc.scrollTop,
          docScrollHeight: doc.scrollHeight,
          viewportH: window.innerHeight,
          navTopBefore,
          navTopAfter,
          containerTop,
          contentTop,
          noticeVisible: noticeRect.height > 0 && noticeRect.width > 0,
        };
      });
      const checks = [
        ["容器高度受视口约束", m.clientHeight <= m.viewportH + 1],
        ["容器存在内部可滚动内容", m.scrollHeight > m.clientHeight + 1],
        ["scrollTop=400 真实内部滚动", Math.abs(m.scrollTopAfterSet - 400) <= 1],
        ["document 无第二条主滚动", m.docScrollTop === 0 && m.docScrollHeight <= m.viewportH + 1],
        ["导航吸顶（top=容器顶边±1）", Math.abs(m.navTopAfter - m.containerTop) <= 1],
        ["内容随滚动位移（不被导航遮挡后消失）", m.contentTop < m.containerTop],
        ["通知条可见", m.noticeVisible],
      ];
      const ok = checks.every(([, pass]) => pass);
      if (!ok) failed++;
      results.push({ label, ok, ...m, checks });
      await p.screenshot({ path: join(outDir, `${label}.png`), fullPage: false });
      console.log(`${ok ? "PASS" : "FAIL"} ${label}`);
      for (const [name, pass] of checks) {
        console.log(`  ${pass ? "✓" : "✗"} ${name}`);
      }
      console.log(
        `  clientHeight=${m.clientHeight} scrollHeight=${m.scrollHeight} docScrollHeight=${m.docScrollHeight} navTop=${m.navTopAfter} containerTop=${m.containerTop}`,
      );
      await context.close();
    }
  }
} finally {
  await browser.close();
}

writeFileSync(
  join(root, "qa-artifacts", "app-scroll", "measurements.json"),
  JSON.stringify(results, null, 2),
);
if (failed > 0) {
  console.error(`\n${failed} 个场景未通过布局验收`);
  process.exit(1);
}
console.log("\n全部场景通过布局验收（截图与量测见 qa-artifacts/app-scroll/）");
