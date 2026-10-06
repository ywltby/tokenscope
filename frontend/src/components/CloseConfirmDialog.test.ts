// 关闭确认弹窗（关闭确认与配置文件计划 Task 3）：打开/关闭态、
// resolve 载荷（minimize/remember）、取消路径。
import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";

// NModal 打桩：受 show 控制渲染（真实组件 teleport 到 body，测试不可达）。
vi.mock("naive-ui", async (importOriginal) => {
  const { defineComponent: dc, h } = await import("vue");
  const actual = await importOriginal<typeof import("naive-ui")>();
  const NModalStub = dc({
    name: "NModal",
    props: { show: { type: Boolean, default: false } },
    setup(props, { slots }) {
      return () => (props.show ? h("div", { class: "modal-stub" }, slots.default?.()) : null);
    },
  });
  return { ...actual, NModal: NModalStub };
});

import CloseConfirmDialog from "./CloseConfirmDialog.vue";

const findBtn = (w: ReturnType<typeof mount>, text: string) =>
  w.findAll("button").find((b) => b.text().includes(text));

describe("CloseConfirmDialog（关闭确认与配置文件计划 Task 3）", () => {
  it("关闭态不渲染任何内容", () => {
    const w = mount(CloseConfirmDialog, { props: { open: false } });
    expect(w.find(".modal-stub").exists()).toBe(false);
  });

  it("打开态：标题/说明/记忆勾选（默认不勾）与三个动作按钮", () => {
    const w = mount(CloseConfirmDialog, { props: { open: true } });
    const body = w.find(".modal-stub");
    expect(body.exists()).toBe(true);
    expect(body.text()).toContain("关闭 TokenScope");
    expect(body.text()).toContain("最小化到托盘");
    expect(body.text()).toContain("直接退出");
    expect(body.text()).toContain("记住我的选择");
    expect(body.find(".n-checkbox").exists()).toBe(true);
  });

  it("最小化到托盘 → resolve {minimize:true, remember:false}", async () => {
    const w = mount(CloseConfirmDialog, { props: { open: true } });
    await findBtn(w, "最小化到托盘")!.trigger("click");
    expect(w.emitted("resolve")!.at(-1)![0]).toEqual({ minimize: true, remember: false });
    expect(w.emitted("cancel")).toBeUndefined();
  });

  it("勾选记忆后直接退出 → resolve {minimize:false, remember:true}", async () => {
    const w = mount(CloseConfirmDialog, { props: { open: true } });
    await w.find(".n-checkbox").trigger("click");
    await findBtn(w, "直接退出")!.trigger("click");
    expect(w.emitted("resolve")!.at(-1)![0]).toEqual({ minimize: false, remember: true });
  });

  it("取消 → cancel 且不带 resolve", async () => {
    const w = mount(CloseConfirmDialog, { props: { open: true } });
    await findBtn(w, "取消")!.trigger("click");
    expect(w.emitted("cancel")).toHaveLength(1);
    expect(w.emitted("resolve")).toBeUndefined();
  });
});
