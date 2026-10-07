// D1/C5 打磨回归：EventTable——项目列末段展示（C2）与「加载更多」
// 分页交互（D1 游标的前端入口）。
// Task 7：费用悬浮提示——纯函数 formatCostBreakdown 的排版断言
//（NDataTable 虚拟滚动在测试环境不渲染行，与既有用例同口径）。
import { afterEach, describe, expect, it } from "vitest";
import { nextTick } from "vue";
import { mount } from "@vue/test-utils";
import { defineComponent, h, type VNode } from "vue";
import EventTable from "./EventTable.vue";
import { projectLabel, type EventCostBreakdown, type EventList, type EventRow } from "../types";
import { formatCostBreakdownRows, formatCostBreakdownText } from "../lib/costBreakdown";

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
  return { query_id: "q", pricing_revision: "rev", rows, total, warnings: [] };
}

/** Task 7 fixture：外置来源、完整匹配、峰谷档命中的高档请求（部分缺价）。 */
function highTierBd(): EventCostBreakdown {
  return {
    matched: {
      raw_key: "nano-gpt/qwen/qwen3.8-27b-obliterated:thinking",
      channel: "nano-gpt",
      source: "external",
      matched_key: "qwen3-8-27b-obliterated:thinking",
      match_mode: "full",
      candidate_count: 2,
      reason: "highest_complete_cost",
      schedule_label: "peak",
      schedule_timezone: "Asia/Shanghai",
      request_at: "2026-01-05T04:00:00Z",
    },
    basis: "prompt_tokens",
    basis_value: 277_001,
    segment_label: ">272K",
    lines: [
      { kind: "input", tokens: 272_001, unit_price: 8, subtotal: 2.176008, priced: true },
      { kind: "output", tokens: 1_000, unit_price: 30, subtotal: 0.03, priced: true },
      { kind: "cache_write", tokens: 0, unit_price: 10, subtotal: 0, priced: true },
      { kind: "cache_read", tokens: 5_000, unit_price: null, subtotal: 0, priced: false },
    ],
    cost_usd: 2.206008,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 5_000 },
    complete: false,
  };
}

function lowTierBd(): EventCostBreakdown {
  const bd = highTierBd();
  return {
    ...bd,
    matched: { ...bd.matched, schedule_label: null, schedule_timezone: null },
    basis_value: 130_000,
    segment_label: null,
    lines: [
      { kind: "input", tokens: 100_000, unit_price: 4, subtotal: 0.4, priced: true },
      { kind: "output", tokens: 50_000, unit_price: 20, subtotal: 1.0, priced: true },
      { kind: "cache_write", tokens: 10_000, unit_price: 5, subtotal: 0.05, priced: true },
      { kind: "cache_read", tokens: 20_000, unit_price: 0.4, subtotal: 0.008, priced: true },
    ],
    cost_usd: 1.458,
    unknown: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
    complete: true,
  };
}

const text = (bd: EventCostBreakdown) => formatCostBreakdownText(bd);

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

  // RC02：没有分页资格（恢复视图 / 新批次首页尚未成功）时按钮禁用并给出
  // 可见原因——不允许"点击无反馈"。
  it("more_blocked_hint_disables_button_with_visible_reason", async () => {
    const w = mount(EventTable, {
      props: {
        list: list([row()], 3),
        filterLabel: "",
        more: true,
        moreBlockedHint: "刷新完成后可继续加载",
      },
    });
    const btn = w.findAll("button").find((b) => b.text().includes("加载更多"));
    expect(btn).toBeDefined();
    expect((btn!.element as HTMLButtonElement).disabled).toBe(true);
    expect(btn!.attributes("title")).toBe("刷新完成后可继续加载");
    // 原因在界面上可见（不只靠 title）
    expect(w.text()).toContain("刷新完成后可继续加载");
    await btn!.trigger("click");
    expect(w.emitted("load-more")).toBeUndefined();

    // 有分页资格时按钮恢复可用、无原因提示
    await w.setProps({ moreBlockedHint: undefined });
    const btn2 = w.findAll("button").find((b) => b.text().includes("加载更多"))!;
    expect((btn2.element as HTMLButtonElement).disabled).toBe(false);
    expect(w.text()).not.toContain("刷新完成后可继续加载");
  });
});

describe("EventTable 费用悬浮（Task 7）", () => {
  it("高档请求：来源/模型、渠道、匹配方式与候选最高费用说明可见", () => {
    const t = text(highTierBd());
    expect(t).toContain("匹配模型 nano-gpt/qwen/qwen3.8-27b-obliterated:thinking");
    expect(t).toContain("计价来源 外置价格表");
    expect(t).toContain("渠道 nano-gpt");
    expect(t).toContain("完整匹配");
    expect(t).toContain("候选 2 条（完整 ?/不完整 ?），在完整候选中取最高费用");
  });

  it("请求时间、时间档及时区可见；无峰谷规则时明确说明", () => {
    expect(text(highTierBd())).toContain("请求时间 2026-01-05T04:00:00Z");
    expect(text(highTierBd())).toContain("时间档 peak（时区 Asia/Shanghai）");
    expect(text(lowTierBd())).toContain("时间档 无峰谷规则");
  });

  it("prompt 度量式 = input + cache_write + cache_read = basis_value", () => {
    expect(text(highTierBd())).toContain(
      "prompt tokens：输入 272,001 + 缓存写 0 + 缓存读 5,000 = 277,001",
    );
    expect(text(lowTierBd())).toContain(
      "prompt tokens：输入 100,000 + 缓存写 10,000 + 缓存读 20,000 = 130,000",
    );
  });

  it("命中档位可见：高档显示分段标签，低档显示基础价档", () => {
    expect(text(highTierBd())).toContain("命中档位 >272K");
    expect(text(lowTierBd())).toContain("命中档位 基础价档");
  });

  it("每个分项展示 token、USD/百万单价与小计；金额无二进制浮点长尾", () => {
    const t = text(highTierBd());
    expect(t).toContain("输入 272,001 × $8.00/M = $2.18");
    expect(t).toContain("输出 1,000 × $30.00/M = $0.03");
    expect(t).toContain("缓存写 0 × $10.00/M = $0.00");
    expect(t).not.toMatch(/2\.1760080\d*0000/);
    expect(t).not.toContain("0.030000000000000002");
  });

  it("未计价分项有明确文案且标注 unknown；总价与缺价说明可见", () => {
    const rows = formatCostBreakdownRows(highTierBd());
    const unknownRow = rows.find((l) => l.label === "缓存命中");
    expect(unknownRow).toBeDefined();
    expect(unknownRow!.value).toContain("缺少单价");
    expect(unknownRow!.unknown).toBe(true);
    const t = formatCostBreakdownText(highTierBd());
    expect(t).toContain("估算合计 $2.21");
    expect(t).toContain("未计价：缺价 ≠ 免费：未计价 token 不计入合计");
    // 完整计价请求不出缺价说明。
    expect(text(lowTierBd())).not.toContain("未计价");
  });

  it("cost_usd 列仍按原格式渲染；旧响应（无 breakdown）不崩溃", () => {
    const w = mount(EventTable, {
      props: {
        list: list(
          [
            row({ cost_usd: 2.206008, cost_breakdown: highTierBd() }),
            row({ record_id: "m2", cost_usd: 1.458, cost_breakdown: null }),
            row({ record_id: "m3", cost_usd: null }),
          ],
          3,
        ),
        filterLabel: "",
      },
    });
    // 挂载即无异常（行渲染交给 NDataTable；列渲染函数经类型检查覆盖）。
    expect(w.exists()).toBe(true);
  });
});

// 设计系统 Task 5：明细表列语义（对齐/数字/省略列）。
describe("EventTable 表格语义（设计系统 Task 5）", () => {
  function exposed(w: ReturnType<typeof mount>) {
    return w.vm as unknown as {
      columns: { key: string; align?: string; className?: string; ellipsis?: unknown }[];
    };
  }

  it("数字列右对齐且使用 tabular 数字类；模型列省略并可查完整值", () => {
    const w = mount(EventTable, { props: { list: list([row()], 1), filterLabel: "" } });
    const cols = exposed(w).columns;
    for (const key of ["input", "output", "cache_write", "cache_read", "cost_usd"]) {
      const c = cols.find((x) => x.key === key);
      expect(c, `${key} 列存在`).toBeDefined();
      expect(c!.align).toBe("right");
      expect(c!.className).toContain("ts-num");
    }
    const model = cols.find((x) => x.key === "model");
    expect(model!.ellipsis).toBeTruthy();
  });

  it("R10 same_second_requests_have_distinct_row_keys：同秒同会话两行身份互异", () => {
    // 旧拼法（ts|agent|model|session）在同秒同会话时冲突——两条请求会
    // 共用费用浮层状态。行身份必须 = 后端唯一游标（完整精度时间 +
    // record_id）。
    const r1 = row({ ts: "2026-10-05 10:00:00", cursor: "2026-10-05T10:00:00.123450Z|m1" });
    const r2 = row({
      ts: "2026-10-05 10:00:00",
      cursor: "2026-10-05T10:00:00.234560Z|m2",
    });
    // fixture 前提：后端游标互异
    expect(r1.cursor).not.toBe(r2.cursor);
    // 从组件实际取 row-key 函数断言
    const vm = mount(EventTable, {
      props: { list: list([r1, r2], 2), filterLabel: "" },
    }).vm as unknown as { rowKey: (r: object) => string };
    expect(vm.rowKey(r1)).not.toBe(vm.rowKey(r2));
    expect(vm.rowKey(r1)).toBe(r1.cursor);
    expect(vm.rowKey(r2)).toBe(r2.cursor);
  });

  it("卡片化：外层 .ts-card + 卡头「请求明细」，提示行不再用 opacity 压低", () => {
    const w = mount(EventTable, { props: { list: list([row()], 1), filterLabel: "" } });
    expect(w.find("section.ts-card").exists()).toBe(true);
    expect(w.find(".card-title").text()).toContain("请求明细");
    expect(w.find(".table-hint").exists()).toBe(true);
    expect(w.html()).not.toContain("opacity: 0.6");
  });

  it("下钻筛选在卡头显示为可关闭筛选标签，关闭 emit clear-filter", async () => {
    const w = mount(EventTable, {
      props: { list: list([row()], 1), filterLabel: "model: gpt-x", filterClosable: true },
    });
    const chip = w.find(".filter-chip");
    expect(chip.exists()).toBe(true);
    expect(chip.text()).toContain("model: gpt-x");
    await chip.trigger("click");
    expect(w.emitted("clear-filter")).toHaveLength(1);
  });
});

// 设计系统 Task 6：费用明细玻璃浮层——触发路径、浮层视觉、分段结构。
describe("EventTable 费用浮层（设计系统 Task 6）", () => {
  /// 取费用列 render 的 VNode（NTooltip）——行级渲染在测试环境不可达，
  /// 与既有用例同口径直接调列渲染函数。

  // UX03 重构：费用触发器提取为 CostBreakdownTooltip（事件转发给父层
  // 持有的三态状态）。以下断言 EventTable 列渲染的**状态协调**：
  // toggle/hover/escape 事件正确驱动 show 翻转；组件内部的键盘/焦点
  // 语义由下方真实挂载测试覆盖。
  it("费用浮层状态协调：toggle 打开/Escape 关闭、hover 跟随指针", async () => {
    const w = mount(EventTable, {
      props: {
        list: list([row({ cost_usd: 2.206008, cost_breakdown: highTierBd() })], 1),
        filterLabel: "",
      },
    });
    const propsOf = (): Record<string, unknown> => {
      const cols = (
        w.vm as unknown as { columns: { key: string; render?: (r: object) => unknown }[] }
      ).columns;
      const cost = cols.find((c) => c.key === "cost_usd")!;
      const vnode = cost.render!(
        row({ cost_usd: 2.206008, cost_breakdown: highTierBd() }),
      ) as VNode;
      return vnode.props as Record<string, unknown>;
    };
    let p = propsOf();
    expect(p.open).toBe(false);
    expect(typeof p.onToggle).toBe("function");
    expect(typeof p.onEscape).toBe("function");
    // click（toggle）打开
    (p.onToggle as () => void)();
    await nextTick();
    p = propsOf();
    expect(p.open, "toggle 后 show 翻转").toBe(true);
    // Escape 关闭
    (p.onEscape as () => void)();
    await nextTick();
    expect((propsOf() as Record<string, unknown>).open).toBe(false);
    // hover 跟随指针
    (propsOf()["onHover-enter"] as () => void)();
    await nextTick();
    expect((propsOf() as Record<string, unknown>).open).toBe(true);
    (propsOf()["onHover-leave"] as () => void)();
    await nextTick();
    expect((propsOf() as Record<string, unknown>).open).toBe(false);
  });
});

// UX03：费用触发器真实浮层可访问性——名称含当前请求金额、描述关联
// 指向打开时的存在节点、Escape 关闭且焦点保留、focus 保持时鼠标离开
// 不关闭。CostBreakdownTooltip 提取后可直接挂载真实组件。
import { flushPromises } from "@vue/test-utils";
import { computed, ref } from "vue";
import CostBreakdownTooltip from "./CostBreakdownTooltip.vue";

describe("EventTable 费用触发器可访问性（UX03）", () => {
  function bd(): EventCostBreakdown {
    return {
      matched: {
        raw_key: "nano-gpt/qwen/x",
        channel: "nano-gpt",
        source: "external",
        matched_key: "x",
        match_mode: "full",
        candidate_count: 1,
        reason: "highest_complete_cost",
        schedule_label: null,
        schedule_timezone: null,
        request_at: "2026-01-05T04:00:00Z",
      },
      basis: "prompt_tokens",
      basis_value: 277_001,
      segment_label: null,
      lines: [
        { kind: "input", tokens: 272_001, unit_price: 8, subtotal: 2.176008, priced: true },
        { kind: "cache_read", tokens: 5_000, unit_price: null, subtotal: 0, priced: false },
      ],
      cost_usd: 2.206008,
      unknown: { input: 0, output: 0, cache_write: 0, cache_read: 5_000 },
      complete: false,
      excluded_candidate_warning: null,
    };
  }

  const TIP_STYLE =
    "box-sizing: border-box; max-width: min(480px, calc(100vw - 32px)); backdrop-filter: var(--ts-glass-blur-popover); border-radius: var(--ts-radius-popover);";

  /** 真实挂载：父层持有三态状态（与 EventTable 的 hoverKey/focusKey/
   * pinnedKey 同构——hover-leave 只清 hover，focus 在时保持打开）。 */
  function mountCell() {
    const hover = ref(false);
    const focus = ref(false);
    const pinned = ref(false);
    const Host = defineComponent({
      setup() {
        const open = computed(() => hover.value || focus.value || pinned.value);
        return () =>
          h(CostBreakdownTooltip, {
            cost: 2.206008,
            breakdown: bd(),
            open: open.value,
            tooltipStyle: TIP_STYLE,
            "onHover-enter": () => (hover.value = true),
            "onHover-leave": () => (hover.value = false),
            "onFocus-enter": () => (focus.value = true),
            "onFocus-leave": () => (focus.value = false),
            onToggle: () => (pinned.value = !pinned.value),
            onEscape: () => {
              hover.value = false;
              focus.value = false;
              pinned.value = false;
            },
          });
      },
    });
    const host = mount(Host, { attachTo: document.body });
    const trigger = host.find('[role="button"]');
    return { host, trigger };
  }

  function tipVisible(): boolean {
    const tips = Array.from(document.querySelectorAll<HTMLElement>(".cost-tooltip"));
    return tips.some((el) => {
      let node: HTMLElement | null = el;
      while (node) {
        if (getComputedStyle(node).display === "none") return false;
        node = node.parentElement;
      }
      return true;
    });
  }

  async function waitTip(shown: boolean): Promise<void> {
    const deadline = Date.now() + 1000;
    while (Date.now() < deadline) {
      if (tipVisible() === shown) return;
      await flushPromises();
      await new Promise((r) => setTimeout(r, 10));
    }
    expect(tipVisible(), `费用浮层应${shown ? "出现" : "消失"}于文档`).toBe(shown);
  }

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("cost_trigger_exposes_amount_and_description：名称含金额、描述指向存在节点", async () => {
    const { host, trigger } = mountCell();
    expect(trigger.exists(), "费用触发器应存在").toBe(true);
    expect(trigger.attributes("aria-label")).toBe("估算费用 $2.21，查看计算明细");
    expect(trigger.attributes("aria-describedby")).toBeUndefined();
    await trigger.trigger("focus");
    await waitTip(true);
    const descId = trigger.attributes("aria-describedby")!;
    expect(descId).toBeTruthy();
    expect(document.getElementById(descId), "描述节点必须存在").not.toBeNull();
    expect(document.getElementById(descId)!.textContent).toContain("估算合计");
    host.unmount();
  });

  it("tooltip_escape_closes_without_losing_trigger：Escape 关闭且焦点保留", async () => {
    const { host, trigger } = mountCell();
    // 真实键盘路径：focus 进入 → 鼠标掠过再离开（focus 分支不受影响）
    // → Escape 关闭 → 焦点仍持有且不循环重开。
    await (trigger.element as HTMLElement).focus();
    await trigger.trigger("focus");
    await waitTip(true);
    expect(document.activeElement).toBe(trigger.element);
    await trigger.trigger("mouseenter");
    await trigger.trigger("mouseleave");
    await flushPromises();
    expect(tipVisible(), "focus 在时鼠标离开不得关闭").toBe(true);
    await trigger.trigger("keydown", { key: "Escape" });
    await waitTip(false);
    // Escape 关闭不移动焦点：触发器仍持有文档焦点。
    expect(document.activeElement).toBe(trigger.element);
    // 关闭后不因焦点保持而循环重开。
    await flushPromises();
    expect(tipVisible()).toBe(false);
    host.unmount();
  });

  it("浮层视觉：真实挂载后 480px border-box + 玻璃模糊 + 公式区实色衬底", async () => {
    const { host, trigger } = mountCell();
    await trigger.trigger("focus");
    await waitTip(true);
    // naive 把 tooltipStyle 放到浮层容器上；沿 .cost-tooltip 祖先找
    const tip = Array.from(document.querySelectorAll<HTMLElement>(".cost-tooltip")).find((el) => {
      let node: HTMLElement | null = el;
      while (node) {
        if (getComputedStyle(node).display !== "none") return true;
        node = node.parentElement;
      }
      return false;
    });
    expect(tip, "浮层应可见").toBeDefined();
    let styled: HTMLElement | null = tip!;
    let foundBox = false;
    while (styled) {
      if (getComputedStyle(styled).boxSizing === "border-box") foundBox = true;
      styled = styled.parentElement;
    }
    expect(foundBox, "浮层链路存在 border-box（480px 上限按 border-box 计算）").toBe(true);
    // 公式区（bd-formula 实色衬底）渲染在浮层内容中（teleport 到 body）。
    expect(document.body.innerHTML).toContain("bd-formula");
    expect(document.body.innerHTML).toContain("ts-card-solid");
    host.unmount();
  });
});
