// @vitest-environment node
// AC08 日期浮层材质契约（PR52 复盘缺陷 5）：量测边界必须从 .range-panel 到
// 实际浮层外壳、排除 body 等页面背景——旧实现把页面背景算进实色层并要求
// solidLayers >= 1，双层浮层/全透明浮层靠实色页面背景也能通过（反例
// ac08-material-review.mjs）。这里以内存量测驱动真实 runContracts。
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "vitest";
import { runContracts } from "../contracts/interaction-contracts.mjs";

const transparent = "rgba(0, 0, 0, 0)";
const white = "rgb(255, 255, 255)";
const surface = (cls, bg) => ({ cls, bg, blur: "none", radius: "12px" });

// 正例链取自真实取证（ac08-matrix-final4）：range-panel → n-popover__content
// → n-popover(ts-popover-solid) → v-binder-follower-content/container，其后
// 是 BODY（页面背景，不属浮层本体）。
const scenarios = [
  {
    name: "single_solid_shell_inside_floating_boundary",
    expectedPass: true,
    hitPageBackground: true,
    chain: [
      surface("range-panel", transparent),
      surface("n-popover__content", transparent),
      surface("n-popover n-popover-shared ts-popover-solid", white),
      surface("v-binder-follower-content", transparent),
      surface("v-binder-follower-container", transparent),
    ],
  },
  {
    name: "double_solid_surfaces_plus_solid_page_background",
    expectedPass: false,
    hitPageBackground: true,
    chain: [
      surface("range-panel", transparent),
      surface("n-popover__content", white),
      surface("n-popover n-popover-shared ts-popover-solid", white),
      surface("v-binder-follower-container", transparent),
    ],
  },
  {
    name: "transparent_shell_only_page_background_solid",
    expectedPass: false,
    hitPageBackground: true,
    chain: [
      surface("range-panel", transparent),
      surface("n-popover__content", transparent),
      surface("n-popover n-popover-shared ts-popover-solid", transparent),
      surface("v-binder-follower-container", transparent),
    ],
  },
  {
    name: "boundary_never_reaches_page_background",
    expectedPass: false,
    hitPageBackground: false,
    chain: [
      surface("range-panel", transparent),
      surface("n-popover__content", transparent),
      surface("n-popover n-popover-shared ts-popover-solid", white),
      surface("v-binder-follower-container", transparent),
    ],
  },
];

test("date 材质契约按浮层本体边界判定：单实色外壳过、双层/透明/无边界失败", async () => {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "tokenscope-date-contract-"));
  try {
    const results = [];
    for (const scenario of scenarios) {
      const measured = {
        panel: scenario.chain[0],
        ancestors: scenario.chain.slice(1),
        hitPageBackground: scenario.hitPageBackground,
      };
      const picker = { first() { return this; }, count: async () => 1, click: async () => {} };
      const open = async () => ({
        label: scenario.name,
        url: "about:blank",
        unknownCommands: [],
        externalRequests: [],
        pageErrors: [],
        close: async () => {},
        page: {
          goto: async () => {},
          waitForSelector: async () => {},
          waitForTimeout: async () => {},
          locator: () => picker,
          keyboard: { press: async () => {} },
          screenshot: async () => {},
          evaluate: async (fn) =>
            fn.toString().includes("const panel = document.querySelector") ? measured : {},
        },
      });
      const result = await runContracts({
        open,
        output,
        meta: {},
        only: ["date_outer_surface_matches_contract"],
      });
      const actualPass =
        result.failures.length === 0 && result.results.every((row) => row.pass);
      results.push({ name: scenario.name, expectedPass: scenario.expectedPass, actualPass });
    }
    assert.deepEqual(
      results.map((row) => ({ name: row.name, pass: row.actualPass === row.expectedPass })),
      results.map((row) => ({ name: row.name, pass: true })),
      JSON.stringify(results, null, 2),
    );
  } finally {
    fs.rmSync(output, { recursive: true });
  }
});
