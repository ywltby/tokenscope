// D1/C5 打磨回归：EventTable——项目列末段展示（C2）与「加载更多」
// 分页交互（D1 游标的前端入口）。
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import EventTable from "./EventTable.vue";
import { projectLabel, type EventList, type EventRow } from "../types";

function row(over: Partial<EventRow> = {}): EventRow {
  return {
    ts: "2026-10-05 10:00:00",
    record_id: "m1",
    cursor: "2026-10-05T10:00:00Z|m1",
    agent: "codex",
    model: "gpt-x",
    session_id: "s",
    project: "C:/work/alpha",
    input: 1,
    output: 1,
    cache_write: 0,
    cache_read: 0,
    cost_usd: 0,
    ...over,
  };
}

function list(rows: EventRow[], total: number): EventList {
  return { rows, total, warnings: [] };
}

describe("EventTable（D1 分页 / C2 项目列）", () => {
  it("project_label_visible：项目列取路径末段（NDataTable 虚拟滚动在测试环境不渲染行，纯函数验证）", () => {
    // 正斜杠 / 反斜杠 / 尾部斜杠 / 无分隔符（slug、占位名）
    expect(projectLabel("C:/work/alpha")).toBe("alpha");
    expect(projectLabel("C:\\work\\beta")).toBe("beta");
    expect(projectLabel("D:/other/alpha/")).toBe("alpha");
    expect(projectLabel("(根目录)")).toBe("(根目录)");
    expect(projectLabel("my-slug")).toBe("my-slug");
  });

  it("加载更多按钮在还有余量时出现，显示剩余数并 emit load-more", async () => {
    const w = mount(EventTable, {
      props: { list: list([row()], 3), filterLabel: "", more: true },
    });
    expect(w.text()).toContain("加载更多");
    expect(w.text()).toContain("还剩 2 条");
    const btn = w.findAll("button").find((b) => b.text().includes("加载更多"));
    expect(btn).toBeDefined();
    await btn!.trigger("click");
    expect(w.emitted("load-more")).toHaveLength(1);
  });

  it("全部加载后不显示按钮", () => {
    const w = mount(EventTable, {
      props: { list: list([row()], 1), filterLabel: "" },
    });
    expect(w.text()).not.toContain("加载更多");
  });
});
