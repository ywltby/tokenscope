import { mount, flushPromises } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import ScrollList from "./ScrollList.vue";

function metrics(el: Element, top: number) {
  Object.defineProperties(el, {
    clientHeight: { configurable: true, value: 400 },
    scrollHeight: { configurable: true, value: 8000 },
    scrollTop: { configurable: true, writable: true, value: top },
  });
}
describe("统一滚动加载", () => {
  it("200 行以内全量，超出后只在接近底部时追加 200 行", async () => {
    const rows = Array.from({ length: 601 }, (_, id) => ({ id }));
    const w = mount(ScrollList, {
      props: { rows: rows.slice(0, 200), label: "列表" },
      slots: { default: ({ rows }: { rows: unknown[] }) => String(rows.length) },
    });
    expect(w.text()).toBe("200");
    await w.setProps({ rows });
    expect(w.text()).toContain("200");
    metrics(w.element, 0);
    await w.trigger("scroll");
    expect(w.text()).toContain("200");
    metrics(w.element, 7500);
    await w.trigger("scroll");
    expect(w.text()).toContain("400");
    await w.setProps({ rows: [{ id: 999 }] });
    expect(w.text()).toBe("1");
  });
  it("远端加载防重复，阻断和失败不自动循环请求", async () => {
    const w = mount(ScrollList, {
      props: { rows: [{ id: 1 }], total: 400, more: true, label: "明细", blocked: "请刷新" },
    });
    metrics(w.element, 7600);
    await w.trigger("scroll");
    expect(w.emitted("load-more")).toBeUndefined();
    await w.setProps({ blocked: undefined });
    await flushPromises();
    await w.trigger("scroll");
    await w.trigger("scroll");
    expect(w.emitted("load-more")).toHaveLength(1);
  });

  // T4（2026-11-13）：相同内容刷新（对象引用替换）保留滚动锚点与
  // 已展开批次；只有内容身份变化才重置。
  it("resetKey 相同：刷新替换行对象不归零、不收回已展开批次", async () => {
    const rows = Array.from({ length: 601 }, (_, id) => ({ id }));
    const w = mount(ScrollList, {
      props: { rows, resetKey: "same", label: "价格列表" },
      slots: { default: ({ rows }: { rows: unknown[] }) => String(rows.length) },
    });
    metrics(w.element, 7500);
    await w.trigger("scroll");
    expect(w.text()).toContain("400");
    metrics(w.element, 1234);
    // 模拟「重新读取价格表」：同内容、新对象引用。
    await w.setProps({ rows: rows.map((r) => ({ ...r })), resetKey: "same" });
    expect(w.text()).toContain("400");
    expect(w.element.scrollTop).toBe(1234);
  });

  it("resetKey 变化：真实内容变化恢复首批并回到顶部", async () => {
    const rows = Array.from({ length: 601 }, (_, id) => ({ id }));
    const w = mount(ScrollList, {
      props: { rows, resetKey: "a", label: "价格列表" },
      slots: { default: ({ rows }: { rows: unknown[] }) => String(rows.length) },
    });
    metrics(w.element, 7500);
    await w.trigger("scroll");
    expect(w.text()).toContain("400");
    metrics(w.element, 1234);
    await w.setProps({ resetKey: "b" });
    expect(w.text()).toContain("200");
    expect(w.text()).not.toContain("400");
    expect(w.element.scrollTop).toBe(0);
  });

  it("未提供 resetKey 时保持首行引用变化即重置（明细/聚合表语义）", async () => {
    const rows = Array.from({ length: 601 }, (_, id) => ({ id }));
    const w = mount(ScrollList, {
      props: { rows, label: "明细" },
      slots: { default: ({ rows }: { rows: unknown[] }) => String(rows.length) },
    });
    metrics(w.element, 7500);
    await w.trigger("scroll");
    expect(w.text()).toContain("400");
    await w.setProps({ rows: rows.map((r) => ({ ...r })) });
    expect(w.text()).toContain("200");
    expect(w.text()).not.toContain("400");
  });
});
