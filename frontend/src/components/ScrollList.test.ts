import { mount, flushPromises } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { h } from "vue";
import ScrollList from "./ScrollList.vue";

function metrics(el: Element, top: number, scrollHeight = 8000, clientHeight = 400) {
  Object.defineProperties(el, {
    clientHeight: { configurable: true, value: clientHeight },
    scrollHeight: { configurable: true, value: scrollHeight },
    scrollTop: { configurable: true, writable: true, value: top },
  });
}

type KeyedRow = { id: string; height?: number };

/**
 * 为 happy-dom 中的表格行提供与真实浏览器同口径的 getBoundingClientRect：
 * 每行顶边 = viewport.top + headerHeight + 之前各行高度和 - viewport.scrollTop。
 */
function attachTableLayout(
  el: HTMLElement,
  rowsRef: () => KeyedRow[],
  { headerHeight = 32, defaultRowHeight = 40, clientHeight = 380 } = {},
) {
  const VP_TOP = 100;
  const totalHeight = () =>
    headerHeight + rowsRef().reduce((sum, r) => sum + (r.height ?? defaultRowHeight), 0);
  Object.defineProperties(el, {
    clientHeight: { configurable: true, value: clientHeight },
    scrollHeight: { configurable: true, get: totalHeight },
    scrollTop: { configurable: true, writable: true, value: 0 },
  });
  el.getBoundingClientRect = () =>
    ({
      top: VP_TOP,
      bottom: VP_TOP + clientHeight,
      left: 0,
      right: 800,
      width: 800,
      height: clientHeight,
      x: 0,
      y: VP_TOP,
      toJSON: () => ({}),
    }) as DOMRect;
  const syncRowRects = () => {
    const trs = Array.from(el.querySelectorAll<HTMLElement>("tbody > tr"));
    const rows = rowsRef();
    let y = headerHeight;
    trs.forEach((tr, i) => {
      const rowH = rows[i]?.height ?? defaultRowHeight;
      const startY = y;
      y += rowH;
      tr.getBoundingClientRect = () => {
        const top = VP_TOP + startY - el.scrollTop;
        return {
          top,
          bottom: top + rowH,
          left: 0,
          right: 800,
          width: 800,
          height: rowH,
          x: 0,
          y: top,
          toJSON: () => ({}),
        } as DOMRect;
      };
    });
  };
  syncRowRects();
  return {
    syncRowRects,
    rowOffset: (id: string) => {
      syncRowRects();
      const idx = rowsRef().findIndex((r) => r.id === id);
      const tr = el.querySelectorAll<HTMLElement>("tbody > tr")[idx];
      return tr ? tr.getBoundingClientRect().top - el.getBoundingClientRect().top : null;
    },
  };
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

  it("未提供 resetKey 时保持首行引用变化即重置（兼容未传键调用方）", async () => {
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

  it("同筛选刷新：上方新增请求时按锚点行对齐 scrollTop，把正在看的那一行留在原位置", async () => {
    let currentRows: KeyedRow[] = Array.from({ length: 60 }, (_, i) => ({
      id: `r-${i}`,
      height: i % 3 === 0 ? 52 : 40,
    }));
    const w = mount(ScrollList, {
      props: {
        rows: currentRows,
        resetKey: "filter-a",
        rowKey: (r: unknown) => (r as KeyedRow).id,
        label: "请求明细列表",
      },
      slots: {
        default: ({ rows }: { rows: unknown[] }) =>
          h("table", [
            h(
              "tbody",
              (rows as KeyedRow[]).map((r) => h("tr", { key: r.id }, [h("td", r.id)])),
            ),
          ]),
      },
    });
    const el = w.element as HTMLElement;
    const layout = attachTableLayout(el, () => currentRows);
    el.scrollTop = 515;
    await w.trigger("scroll");
    // 找到此时视口内第 12 行（r-12）相对视口顶边的位置
    const beforeOffset = layout.rowOffset("r-12");
    expect(beforeOffset).not.toBeNull();

    // 1) 同内容、全新行对象：不归零，原行留在原位
    currentRows = currentRows.map((r) => ({ ...r }));
    await w.setProps({ rows: currentRows, resetKey: "filter-a" });
    layout.syncRowRects();
    await flushPromises();
    expect(el.scrollTop).toBe(515);
    expect(layout.rowOffset("r-12")).toBeCloseTo(beforeOffset!, 1);

    // 2) 上方新增 5 条请求（含不同行高）：scrollTop 顺延，原行保持在视口原像素位置
    const prepended: KeyedRow[] = [
      { id: "new-0", height: 40 },
      { id: "new-1", height: 56 },
      { id: "new-2", height: 40 },
      { id: "new-3", height: 40 },
      { id: "new-4", height: 44 },
    ];
    const addedHeight = prepended.reduce((s, r) => s + (r.height ?? 40), 0); // 220px
    // 在同步 prop 变更前保持旧 DOM rect，DOM 更新后切换为新 rect
    const oldRows = currentRows;
    currentRows = [...prepended, ...oldRows.map((r) => ({ ...r }))];
    // 先把新 DOM 的 rect 安装逻辑延后到 patch 后执行：
    // 借用 Vue 的微任务队列：setProps 内部同步触发 sync watcher 读取旧 rect，再渲染新 DOM。
    let activeRows = oldRows;
    const dynamicLayout = attachTableLayout(el, () => activeRows);
    el.scrollTop = 515;
    const beforeOffset2 = dynamicLayout.rowOffset("r-12");
    const origGetRect = el.getBoundingClientRect.bind(el);
    el.getBoundingClientRect = () => {
      if (el.querySelectorAll("tbody > tr").length === currentRows.length) {
        activeRows = currentRows;
        dynamicLayout.syncRowRects();
      }
      return origGetRect();
    };
    await w.setProps({ rows: currentRows, resetKey: "filter-a" });
    await flushPromises();
    expect(el.scrollTop).toBeCloseTo(515 + addedHeight, 1);
    expect(dynamicLayout.rowOffset("r-12")).toBeCloseTo(beforeOffset2!, 1);

    // 3) 主动切换筛选（resetKey 变化）：回到顶部 0px
    const otherRows: KeyedRow[] = Array.from({ length: 40 }, (_, i) => ({ id: `other-${i}` }));
    activeRows = otherRows;
    currentRows = otherRows;
    await w.setProps({ rows: otherRows, resetKey: "filter-b" });
    await flushPromises();
    expect(el.scrollTop).toBe(0);
  });

  it("停在顶部（scrollTop=0）时上方新增行保持在 0px，立即看到最新请求", async () => {
    let rows: KeyedRow[] = Array.from({ length: 40 }, (_, i) => ({ id: `r-${i}` }));
    const w = mount(ScrollList, {
      props: {
        rows,
        resetKey: "filter-a",
        rowKey: (r: unknown) => (r as KeyedRow).id,
        label: "请求明细列表",
      },
      slots: {
        default: ({ rows }: { rows: unknown[] }) =>
          h("table", [
            h(
              "tbody",
              (rows as KeyedRow[]).map((r) => h("tr", { key: r.id }, [h("td", r.id)])),
            ),
          ]),
      },
    });
    const el = w.element as HTMLElement;
    attachTableLayout(el, () => rows);
    el.scrollTop = 0;
    rows = [{ id: "new-0" }, { id: "new-1" }, ...rows.map((r) => ({ ...r }))];
    await w.setProps({ rows, resetKey: "filter-a" });
    await flushPromises();
    expect(el.scrollTop).toBe(0);
  });

  it("同筛选刷新后不自发触发 load-more，且刷新后的新列表仍可再次分页", async () => {
    const page1 = Array.from({ length: 200 }, (_, i) => ({ id: `r-${i}` }));
    const w = mount(ScrollList, {
      props: {
        rows: page1,
        total: 500,
        more: true,
        resetKey: "filter-a",
        rowKey: (r: unknown) => (r as KeyedRow).id,
        label: "请求明细列表",
      },
    });
    metrics(w.element, 7500, 8000, 380);
    await w.trigger("scroll");
    expect(w.emitted("load-more")).toHaveLength(1);
    // 模拟刷新返回新一页 200 行（非追加），不自发连环请求 load-more
    const refreshed = Array.from({ length: 200 }, (_, i) => ({ id: `fresh-${i}` }));
    await w.setProps({ rows: refreshed, resetKey: "filter-a" });
    await flushPromises();
    expect(w.emitted("load-more")).toHaveLength(1);
    // 用户再次滚动到底部时，新批次恢复分页资格
    await w.trigger("scroll");
    expect(w.emitted("load-more")).toHaveLength(2);
  });
});
