// H05/H08：设置页的 CCS 手动导入面板——进入页面**不读取**来源库，只有点击
// 才预览；预览是一次性计划，确认后提交一个批次，取消则丢弃计划。
// IPC 全 mock；不依赖真实 CCS 库。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const msgSpy = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }));
vi.mock("naive-ui", async (importOriginal) => {
  const actual = await importOriginal<typeof import("naive-ui")>();
  return { ...actual, useMessage: () => msgSpy };
});

import Settings from "./Settings.vue";

const pricingView = {
  path: "C:/pricing.toml",
  modelsdev_path: "C:/md.json",
  modelsdev_synced_at: null,
  modelsdev_count: 0,
  openrouter_path: "C:/or.json",
  openrouter_synced_at: null,
  openrouter_count: 0,
  external_count: 0,
  entries: [],
  warnings: [],
};
const sourceStatuses = [
  { agent: "claude-code", dir: "C:/claude", enabled: true, exists: true, files: 3, state: "ready" },
  { agent: "codex", dir: "C:/codex", enabled: true, exists: true, files: 2, state: "ready" },
];

const preview = {
  plan_id: "plan-1",
  logical_source: "ccs",
  source_path: "C:/Users/x/.cc-switch/cc-switch.db",
  source_schema: "ccs-user-version-20",
  source_day_timezone: "Asia/Shanghai",
  history_generation: 3,
  generated_at: "2026-10-10T00:00:00Z",
  expires_in_seconds: 600,
  requests_total: 10,
  requests_importable: 8,
  requests_skipped_other_app: 1,
  requests_skipped_duplicate_of_proxy: 1,
  requests_rejected: 0,
  rejected_reasons: [],
  would_insert: 6,
  would_update: 0,
  would_unchanged: 2,
  would_conflict: 0,
  would_stale: 0,
  would_overlap: 0,
  overlap_unresolved: false,
  overlap_examples: [] as string[],
  net_new_tokens: { input: 100, output: 10, cache_write: 0, cache_read: 0 },
  rollups_total: 2,
  rollups_new: 2,
  rollups_unchanged: 0,
  rollups_conflicting: 0,
  unsupported_apps: [["gemini", 1]] as [string, number][],
  records_without_project: 8,
};

const report = {
  run_id: 7,
  logical_source: "ccs",
  requests_inserted: 6,
  requests_updated: 0,
  requests_unchanged: 2,
  requests_conflicted: 0,
  requests_stale: 0,
  requests_overlap: 0,
  rollups_snapshotted: 2,
  rollups_conflicted: 0,
  net_new_tokens: { input: 100, output: 10, cache_write: 0, cache_read: 0 },
  generation_after: 9,
};

function mountSettings() {
  return mount(Settings, { props: { refreshKey: 0 } });
}

beforeEach(() => {
  invokeMock.mockReset();
  msgSpy.success.mockReset();
  msgSpy.error.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "source_status":
        return Promise.resolve(sourceStatuses);
      case "pricing_entries":
        return Promise.resolve(pricingView);
      case "cache_stats":
        return Promise.resolve({ path: "C:/data/history.db", files: 4, events: 8 });
      case "settings_get":
        return Promise.resolve({ price_auto_sync: true, sources: {} });
      case "autostart_status":
        return Promise.resolve(false);
      case "ccs_import_defaults":
        return Promise.resolve({
          path: preview.source_path,
          exists: true,
          timezone: "Asia/Shanghai",
        });
      case "ccs_import_preview":
        return Promise.resolve(preview);
      case "ccs_import_commit":
        return Promise.resolve(report);
      case "ccs_import_discard":
        return Promise.resolve(true);
      default:
        return Promise.resolve(null);
    }
  });
});

describe("Settings CCS 导入（H05/H08）", () => {
  it("进入设置页只解析默认路径，不打开来源库、不导入用量", async () => {
    const w = mountSettings();
    await flushPromises();
    const cmds = invokeMock.mock.calls.map((c) => c[0] as string);
    expect(cmds).toContain("ccs_import_defaults");
    // 计划不变量 11：未点击不读取、也不导入 CCS 用量
    expect(cmds).not.toContain("ccs_import_preview");
    expect(cmds).not.toContain("ccs_import_commit");
    // 默认路径只填进输入框，不读取库
    const input = w.find('input[aria-label="来源库路径"]');
    expect(input.exists()).toBe(true);
    expect((input.element as HTMLInputElement).value).toBe(preview.source_path);
    w.unmount();
  });

  it("改选其它副本时把该路径传给预览（仍只在点击后读取）", async () => {
    const w = mountSettings();
    await flushPromises();
    const input = w.find('input[aria-label="来源库路径"]');
    await input.setValue("D:/backup/cc-switch.db");
    expect(invokeMock.mock.calls.some((c) => c[0] === "ccs_import_preview")).toBe(false);
    await w
      .findAll("button")
      .find((b) => b.text().includes("从 CCS 导入用量"))!
      .trigger("click");
    await flushPromises();
    const call = invokeMock.mock.calls.find((c) => c[0] === "ccs_import_preview")!;
    expect((call[1] as { sourcePath: string }).sourcePath).toBe("D:/backup/cc-switch.db");
    w.unmount();
  });

  it("点击后预览展示可导入/重复/拒绝与净新增，确认提交一个批次", async () => {
    const w = mountSettings();
    await flushPromises();
    const importBtn = w.findAll("button").find((b) => b.text().includes("从 CCS 导入用量"))!;
    expect(importBtn).toBeDefined();
    await importBtn.trigger("click");
    await flushPromises();

    const previewCall = invokeMock.mock.calls.find((c) => c[0] === "ccs_import_preview");
    expect(previewCall).toBeDefined();
    // 预览文本必须如实给出范围与预计变化
    expect(w.text()).toContain("预览（尚未写入）");
    expect(w.text()).toContain("可导入");
    expect(w.text()).toContain("与代理日志重复");
    expect(w.text()).toContain("日汇总");
    expect(w.text()).toContain("无项目归属记录");

    const confirm = w.findAll("button").find((b) => b.text() === "导入")!;
    await confirm.trigger("click");
    await flushPromises();
    const commitCall = invokeMock.mock.calls.find((c) => c[0] === "ccs_import_commit");
    expect(commitCall).toBeDefined();
    expect((commitCall![1] as { planId: string }).planId).toBe("plan-1");
    expect(msgSpy.success).toHaveBeenCalled();
    // 提交后预览面板关闭，结果摘要可见
    expect(w.text()).not.toContain("预览（尚未写入）");
    expect(w.text()).toContain("最近一次导入");
    w.unmount();
  });

  it("取消丢弃计划，且不会提交任何用量", async () => {
    const w = mountSettings();
    await flushPromises();
    const importBtn = w.findAll("button").find((b) => b.text().includes("从 CCS 导入用量"))!;
    await importBtn.trigger("click");
    await flushPromises();
    const cancel = w.findAll("button").find((b) => b.text() === "取消")!;
    await cancel.trigger("click");
    await flushPromises();
    const discardCall = invokeMock.mock.calls.find((c) => c[0] === "ccs_import_discard");
    expect(discardCall).toBeDefined();
    expect((discardCall![1] as { planId: string }).planId).toBe("plan-1");
    expect(invokeMock.mock.calls.some((c) => c[0] === "ccs_import_commit")).toBe(false);
    expect(w.text()).not.toContain("预览（尚未写入）");
    w.unmount();
  });

  it("重叠候选默认拒绝提交，勾选后才按新增导入", async () => {
    const overlapPreview = {
      ...preview,
      would_overlap: 2,
      overlap_unresolved: true,
      overlap_examples: ["claude / gpt-5.6-sol / 2026-07-17T15:00:00Z / 130"],
    };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats")
        return Promise.resolve({ path: "C:/data/history.db", files: 4, events: 8 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "ccs_import_defaults")
        return Promise.resolve({ path: preview.source_path, exists: true, timezone: "UTC" });
      if (cmd === "ccs_import_preview") return Promise.resolve(overlapPreview);
      if (cmd === "ccs_import_commit") return Promise.resolve(report);
      return Promise.resolve(null);
    });
    const w = mountSettings();
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text().includes("从 CCS 导入用量"))!
      .trigger("click");
    await flushPromises();
    expect(w.text()).toContain("重叠候选");
    expect(w.text()).toContain("默认不导入");
    expect(w.text()).toContain("claude / gpt-5.6-sol");

    // 未勾选 → 不提交，给出明确提示
    await w
      .findAll("button")
      .find((b) => b.text() === "导入")!
      .trigger("click");
    await flushPromises();
    expect(invokeMock.mock.calls.some((c) => c[0] === "ccs_import_commit")).toBe(false);
    expect(w.text()).toContain("CCS 导入未完成");

    // 勾选「重叠候选」后提交，allowOverlap 必须传下去
    const overlapRow = w.findAll(".setting-row").find((r) => r.text().includes("重叠候选"));
    expect(overlapRow, "重叠候选行必须存在").toBeDefined();
    await overlapRow!.find(".n-switch").trigger("click");
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text() === "导入")!
      .trigger("click");
    await flushPromises();
    const commit = invokeMock.mock.calls.find((c) => c[0] === "ccs_import_commit");
    expect(commit, "勾选后必须发起提交").toBeDefined();
    expect((commit![1] as { allowOverlap: boolean }).allowOverlap).toBe(true);
    w.unmount();
  });

  it("提交失败保留错误提示且不自动重试", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "source_status") return Promise.resolve(sourceStatuses);
      if (cmd === "pricing_entries") return Promise.resolve(pricingView);
      if (cmd === "cache_stats")
        return Promise.resolve({ path: "C:/data/history.db", files: 4, events: 8 });
      if (cmd === "settings_get") return Promise.resolve({ price_auto_sync: true, sources: {} });
      if (cmd === "ccs_import_defaults")
        return Promise.resolve({ path: preview.source_path, exists: true, timezone: "UTC" });
      if (cmd === "ccs_import_preview") return Promise.resolve(preview);
      if (cmd === "ccs_import_commit")
        return Promise.reject("历史库在本预览之后发生了变化，请重新预览");
      return Promise.resolve(null);
    });
    const w = mountSettings();
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text().includes("从 CCS 导入用量"))!
      .trigger("click");
    await flushPromises();
    await w
      .findAll("button")
      .find((b) => b.text() === "导入")!
      .trigger("click");
    await flushPromises();
    const commits = invokeMock.mock.calls.filter((c) => c[0] === "ccs_import_commit");
    expect(commits.length).toBe(1);
    expect(w.text()).toContain("CCS 导入未完成");
    expect(w.text()).toContain("重新预览");
    w.unmount();
  });
});
