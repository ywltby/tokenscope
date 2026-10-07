// SF01（安全审查 Task 1）：真实 ECharts + 真实 TrendChart 的 tooltip 安全
// 检查页。仅用于本地 Vite dev server（frontend/security-tooltip.html 入口，
// 不参与生产构建）。本页**不设置 CSP**——安全必须来自组件的输出边界
//（chartTooltip 的 textContent 节点），而不是内容安全策略的掩盖。
//
// 页面职责：
//  1. 挂载真实的 TrendChart.vue（真实 ECharts init/setOption/tooltip），
//     类别键为含哨兵的 `<img onerror>` / `<svg onload>` 类合成 payload；
//  2. 暴露 dispatchAction(showTip) 的触发点与 DOM 检查入口；
//  3. vuln=1 查询参数切换为"漏洞对照模式"（formatter 返回 HTML 字符串，
//     走 ECharts innerHTML 分支）——仅用于证明检查脚本真的能抓到注入，
//     产品代码永远不使用该路径。
//
// 合成数据：所有标签/数值均为固定字符串，不含任何真实用户数据。
import "../styles/tokens.css";
import { createApp, h } from "vue";
import * as echarts from "echarts";
import TrendChart from "../components/TrendChart.vue";
import type { Group } from "../types";

// ── 哨兵与 payload（固定字符串，驱动脚本据此断言文本完整可读）──
const SENTINEL = "ts-sentinel-6b21";
const IMG_PAYLOAD = `<img src=ts-attack-probe-src onerror="window.__TS_ATTACKED=(window.__TS_ATTACKED||0)+1">${SENTINEL}`;
const SVG_PAYLOAD = `<svg onload="window.__TS_ATTACKED=(window.__TS_ATTACKED||0)+1"></svg>${SENTINEL}-svg`;
const QUOTE_PAYLOAD = `&<>"' 中文模型 ${SENTINEL}-q`;

function group(key: string, input: number): Group {
  return {
    key,
    label: key.split("/").pop() ?? key,
    requests: 1,
    tokens: { input, output: input * 2, cache_write: 0, cache_read: 0 },
    cost_usd: 0.0126,
    unknown_pricing: false,
    unknown_tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
  };
}

// 参数：theme=light|dark（localStorage 在驱动脚本的 addInitScript 里已设置）、
// dim=model|project、vuln=1（漏洞对照，仅限本检查页）。
const params = new URLSearchParams(window.location.search);
const dim = params.get("dim") === "project" ? "project" : "model";
const vuln = params.get("vuln") === "1";

document.documentElement.setAttribute(
  "data-theme",
  params.get("theme") === "dark" ? "dark" : "light",
);

const groups: Group[] = [
  group(`${IMG_PAYLOAD}`, 120),
  group(`${SVG_PAYLOAD}`, 60),
  group(`${QUOTE_PAYLOAD}`, 30),
  group("normal-model-a/opus", 10),
];

// 漏洞对照模式：把组件 formatter 换成 HTML 字符串拼接（与修复前等价），
// 走 ECharts 的 innerHTML 分支——用于验证检查脚本的检测能力。
if (vuln) {
  const el = document.getElementById("app");
  const root = document.createElement("div");
  root.className = "chart-canvas";
  root.style.width = "800px";
  root.style.height = "320px";
  el?.appendChild(root);
  const chart = echarts.init(root, null);
  const fmt = (v: number): string => String(v);
  const html = (params: unknown): string => {
    const arr = (Array.isArray(params) ? params : [params]) as {
      name?: string;
      seriesName: string;
      value: unknown;
    }[];
    return [
      arr[0]?.name ?? "",
      ...arr.map((p) => `${p.seriesName} ${fmt(Number(p.value ?? 0))}`),
    ].join("<br/>");
  };
  chart.setOption({
    backgroundColor: "transparent",
    tooltip: { trigger: "axis", formatter: html },
    xAxis: { type: "category", data: groups.map((g) => g.key) },
    yAxis: { type: "value" },
    series: [
      { name: "输入", type: "bar", stack: "tokens", data: groups.map((g) => g.tokens.input) },
    ],
  });
} else {
  const app = createApp({
    render: () =>
      h(TrendChart, {
        groups,
        by: dim,
        style: { width: "800px" },
      }),
  });
  app.mount("#app");
}

// 驱动脚本入口：取真实图表实例（挂载后的 TrendChart 画布），逐个 payload
// 类别触发 showTip 并收集 tooltip 文本（ECharts HTML tooltip 每次显示一个
// 类别；内容同步更新，过渡仅是动画）。
(window as unknown as Record<string, unknown>).__TS_GET_TOOLTIP_CONTEXT = () => {
  const canvas =
    document.querySelector<HTMLElement>("#app .chart-canvas") ??
    document.querySelector<HTMLElement>(".chart-canvas");
  const chart = canvas ? echarts.getInstanceByDom(canvas) : undefined;
  if (!chart) return { ok: false, reason: "chart-not-found" };
  const texts: string[] = [];
  for (let i = 0; i < 3; i++) {
    chart.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: i });
    texts.push(document.querySelector(".chart-tooltip")?.textContent ?? "");
  }
  chart.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: 0 });
  return {
    ok: true,
    sentinel: SENTINEL,
    texts,
    payloads: { img: IMG_PAYLOAD, svg: SVG_PAYLOAD, quote: QUOTE_PAYLOAD },
  };
};

(window as unknown as Record<string, unknown>).__TS_PAGE_READY = true;
