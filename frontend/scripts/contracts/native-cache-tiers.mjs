// @vitest-environment node
import fs from "node:fs";
import path from "node:path";
import { createUnifiedContext } from "../lib/native-unified.mjs";

/**
 * N18 真实界面验收（PR #53 合并前收口，2026-11-03）。
 *
 * 复用统一验收根与真实 Tauri 验收程序：合成 Claude 日志 + 隔离价格表，
 * 检查**事件表费用浮层**这一真实界面上分档两档与「分档未知/估算」的呈现。
 *
 * 两条请求：
 * - `m-tiers-known`：`cache_creation.ephemeral_5m/1h` 各 1,000,000（和=写入总量
 *   2,000,000），单价 $3.75/$6 → 界面必须出现「缓存写（5 分钟档）」「缓存写
 *   （1 小时档）」两行，金额 $3.75 / $6.00，且**不得**出现「写入分档未知」。
 * - `m-tiers-unknown`：只有写入总量 2,000,000、没有分档字段 → 界面必须出现
 *   「写入分档未知」与「按写入总量单价估算」，且写费已计价（不是未计价/未知）。
 *
 * 其余价格项（输入/输出/缓存命中）两条请求都完整给出，避免未计价干扰判断。
 */

const pricingFile = (root) => path.join(root, "tokenscope", "pricing.toml");
const sessionDir = (root) =>
  path.join(root, "sources", "claude", "projects", "-acceptance-workspace-tokenscope");

// 隔离价格表：完整四类价格 + 分档价（$3.75 / $6）。无峰谷、无分段，
// 避免其它规则参与，让界面上的写费只由分档决定。
const PRICING_TOML = `# N18 界面验收专用合成价格表（USD / 百万 token）
[[model]]
prefix = "external/tiers-probe"
input = 3.0
output = 15.0
cache_write = 3.75
cache_read = 0.3
cache_write_5m = 3.75
cache_write_1h = 6.0
`;

function event({ id, cacheCreation }) {
  return {
    type: "assistant",
    isSidechain: false,
    sessionId: "cache-tiers-session",
    timestamp: "2026-10-06T01:00:00.000Z",
    cwd: "C:\\acceptance\\workspace\\tiers",
    message: {
      id,
      role: "assistant",
      model: "external/tiers-probe",
      content: [{ type: "text", text: "acceptance synthetic cache tier reply" }],
      usage: {
        input_tokens: 1000,
        output_tokens: 100,
        cache_read_input_tokens: 50000,
        cache_creation_input_tokens: 2000000,
        ...(cacheCreation ? { cache_creation: cacheCreation } : {}),
      },
    },
  };
}

export function prepareCacheTiersScenario(root) {
  fs.mkdirSync(path.join(root, "tokenscope"), { recursive: true });
  fs.writeFileSync(pricingFile(root), PRICING_TOML, "utf8");
  fs.mkdirSync(sessionDir(root), { recursive: true });
  const rows = [
    event({
      id: "m-tiers-known",
      cacheCreation: {
        ephemeral_5m_input_tokens: 1000000,
        ephemeral_1h_input_tokens: 1000000,
      },
    }),
    event({ id: "m-tiers-unknown" }),
  ];
  fs.writeFileSync(
    path.join(sessionDir(root), "session-cache-tiers.jsonl"),
    rows.map((row) => JSON.stringify(row)).join("\n") + "\n",
    "utf8",
  );
}

export async function nativeCacheTiers(context) {
  const { inst, ck, ev } = context;
  const u = createUnifiedContext(context);
  ev.cache_tiers_ui = { shots: [] };

  // 打开浮层但**不**关闭：截图必须在浮层展开、内容已渲染且过渡结束之后，
  // 关闭动作交给调用方（先截图再 Escape），避免截到关闭动画或被当成本身缺省。
  const openTooltip = async (trigger) => {
    await u.click(trigger);
    let id;
    await u.waitUntil(
      async () => {
        id = await trigger.getAttribute("aria-describedby");
        return id && (await u.page().locator(`[id="${id}"]`).isVisible());
      },
      20000,
      "真实说明浮层展开",
    );
    const body = u.page().locator(`[id="${id}"]`);
    const text = await body.innerText();
    const rows = await body.locator(".bd-row").evaluateAll((items) =>
      items.map((row) => ({
        label: row.querySelector(".bd-label")?.textContent?.trim() ?? null,
        value: row.querySelector(".bd-value")?.textContent?.trim() ?? null,
      })),
    );
    return {
      text,
      rows,
      body,
      isVisible: () => body.isVisible(),
      close: () => trigger.press("Escape"),
    };
  };

  // 浮层是过渡动画：内容读完后仍要等一帧稳定再截图（并确认此刻确实可见）。
  const captureOpen = async (tip, fileName) => {
    await u.page().waitForTimeout(300);
    const visible = await tip.isVisible();
    await inst.page.screenshot({ path: path.join(inst.opts.shots, fileName) });
    return { file: fileName, overlay_visible_at_capture: visible };
  };

  // 必须在 Node 侧装上 IPC 旁路探针并重载页面：查询快照与 DOM 一致性
  // 都来自探针记录的真实 query_summary/query_events（与 ui-pricing 同法）。
  await u.installProbe();
  await u.page().reload({ waitUntil: "domcontentloaded" });
  await inst.waitForData();
  let snapshot;
  const diag = { polls: [] };
  ev.cache_tiers_ui.diag = diag;
  try {
    await u.waitUntil(
      async () => {
        snapshot = await u.querySnapshot();
        const ids = (snapshot.events?.rows ?? []).map((row) => row.record_id);
        const ok =
          snapshot.sameQuery &&
          snapshot.domMatches &&
          ids.includes("m-tiers-known") &&
          ids.includes("m-tiers-unknown");
        if (diag.polls.length < 6 || ok) {
          diag.polls.push({
            sameQuery: snapshot.sameQuery,
            domMatches: snapshot.domMatches,
            total: snapshot.events?.total ?? null,
            rows: ids.length,
            known: ids.includes("m-tiers-known"),
            unknown: ids.includes("m-tiers-unknown"),
            sample: ids.slice(0, 6),
          });
        }
        return ok;
      },
      30000,
      "分档两条请求进入真实明细",
    );
  } catch (error) {
    diag.failed = String(error?.message ?? error);
    throw error;
  }

  const rowIndex = (recordId) => {
    const index = snapshot.events.rows.findIndex((row) => row.record_id === recordId);
    if (index < 0) throw new Error(`请求 ${recordId} 不在当前真实查询内`);
    return index;
  };
  const rowTrigger = (recordId) =>
    u.page().locator(".events-card tbody tr").nth(rowIndex(recordId)).locator(".cost-trigger");

  // ── 场景 1：已知分档（5 分钟/1 小时各 100 万，$3.75/$6）
  const knownTip = await openTooltip(rowTrigger("m-tiers-known"));
  const knownShot = await captureOpen(knownTip, "n18-known-tiers.png");
  await knownTip.close();
  const known = { text: knownTip.text, rows: knownTip.rows };
  const knownRow = snapshot.events.rows[rowIndex("m-tiers-known")];
  const knownLabels = known.rows.map((row) => row.label);
  const knownWriteRows = known.rows.filter((row) => (row.label ?? "").includes("缓存写"));
  ev.cache_tiers_ui.shots.push(knownShot.file);

  const knownChecks = {
    two_lines:
      knownLabels.includes("缓存写（5 分钟档）") && knownLabels.includes("缓存写（1 小时档）"),
    no_merged_row: !known.rows.some((row) => row.label === "缓存写"),
    five_tokens: knownWriteRows.some(
      (row) => row.label === "缓存写（5 分钟档）" && /1,000,000|\b1000000\b/.test(row.value ?? ""),
    ),
    one_hour_tokens: knownWriteRows.some(
      (row) => row.label === "缓存写（1 小时档）" && /1,000,000|\b1000000\b/.test(row.value ?? ""),
    ),
    five_amount: knownWriteRows.some(
      (row) => row.label === "缓存写（5 分钟档）" && (row.value ?? "").includes("$3.75"),
    ),
    one_hour_amount: knownWriteRows.some(
      (row) => row.label === "缓存写（1 小时档）" && (row.value ?? "").includes("$6.00"),
    ),
    // 两档之和 = 写入总量 200 万，且没有重复计入的第三行写费
    write_rows_only_two: knownWriteRows.length === 2,
    total_write_tokens:
      knownRow.cache_write === 2000000 &&
      knownRow.cache_write_5m === 1000000 &&
      knownRow.cache_write_1h === 1000000,
    not_estimated_marker: !known.text.includes("写入分档未知"),
  };
  ev.cache_tiers_ui.known = {
    tooltip: known.text,
    rows: known.rows,
    row: {
      cache_write: knownRow.cache_write,
      cache_write_5m: knownRow.cache_write_5m,
      cache_write_1h: knownRow.cache_write_1h,
      cost_usd: knownRow.cost_usd,
    },
    checks: knownChecks,
    screenshot: knownShot,
  };
  ck.add(
    "ui_known_tiers_two_lines",
    knownChecks.two_lines && knownChecks.no_merged_row && knownChecks.write_rows_only_two,
    JSON.stringify(knownWriteRows),
  );
  ck.add(
    "ui_known_tiers_amounts",
    knownChecks.five_amount && knownChecks.one_hour_amount,
    JSON.stringify(knownWriteRows.map((row) => [row.label, row.value])),
  );
  ck.add(
    "ui_known_tiers_total_write_2000000",
    knownChecks.total_write_tokens && knownChecks.five_tokens && knownChecks.one_hour_tokens,
    JSON.stringify(ev.cache_tiers_ui.known.row),
  );
  ck.add(
    "ui_known_tiers_not_marked_estimated",
    knownChecks.not_estimated_marker,
    known.text.includes("写入分档未知") ? "界面误标分档未知" : "未标分档未知",
  );

  // ── 场景 2：分档未知（只有写入总量）
  const unknownTip = await openTooltip(rowTrigger("m-tiers-unknown"));
  const unknownShot = await captureOpen(unknownTip, "n18-unknown-tiers.png");
  await unknownTip.close();
  const unknown = { text: unknownTip.text, rows: unknownTip.rows };
  const unknownRow = snapshot.events.rows[rowIndex("m-tiers-unknown")];

  const unknownChecks = {
    unknown_marker: unknown.text.includes("写入分档未知"),
    estimate_note: unknown.text.includes("按写入总量单价估算"),
    merged_write_row: unknown.rows.some(
      (row) => (row.label ?? "").includes("缓存写") && !String(row.label).includes("档"),
    ),
    // 已计价：写费行带美元金额（不是"未知"/"无法估算"）
    write_priced: unknown.rows.some(
      (row) => (row.label ?? "").includes("缓存写") && /\$[0-9]/.test(row.value ?? ""),
    ),
    not_tier_rows: !unknown.rows.some((row) => String(row.label ?? "").includes("分钟档")),
    total_write_tokens:
      unknownRow.cache_write === 2000000 &&
      unknownRow.cache_write_5m == null &&
      unknownRow.cache_write_1h == null,
  };
  ev.cache_tiers_ui.unknown = {
    tooltip: unknown.text,
    rows: unknown.rows,
    row: {
      cache_write: unknownRow.cache_write,
      cache_write_5m: unknownRow.cache_write_5m,
      cache_write_1h: unknownRow.cache_write_1h,
      cost_usd: unknownRow.cost_usd,
    },
    checks: unknownChecks,
    screenshot: unknownShot,
  };
  ev.cache_tiers_ui.shots.push(unknownShot.file);
  ck.add(
    "ui_unknown_tiers_marked_estimated",
    unknownChecks.unknown_marker && unknownChecks.estimate_note,
    JSON.stringify({ marker: unknownChecks.unknown_marker, note: unknownChecks.estimate_note }),
  );
  ck.add(
    "ui_unknown_tiers_priced_not_unknown",
    unknownChecks.merged_write_row && unknownChecks.write_priced && unknownChecks.not_tier_rows,
    JSON.stringify(unknown.rows.filter((row) => (row.label ?? "").includes("缓存写"))),
  );
  ck.add(
    "ui_unknown_tiers_total_write_2000000",
    unknownChecks.total_write_tokens,
    JSON.stringify(ev.cache_tiers_ui.unknown.row),
  );
  ck.add(
    "ui_screenshots_with_overlay_open",
    knownShot.overlay_visible_at_capture === true &&
      unknownShot.overlay_visible_at_capture === true,
    JSON.stringify([knownShot, unknownShot]),
  );
}
