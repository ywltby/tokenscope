// WorkBuddy 图标源码契约（WB 图标修正 2026-11-04）：图标文件取自官方
// workbuddy-title SVG 的 560×560 图标组，不含右侧文字。用 ?raw 源文本
// 钉住不变量，防止回退成遮挡式方案或引入文字/画布裁剪残留。
import { describe, expect, it } from "vitest";
import workbuddySvg from "../assets/icons/workbuddy.svg?raw";

describe("WorkBuddy 图标源码契约", () => {
  it("画布为 560×560 方形，不是 1620 标题横幅", () => {
    expect(workbuddySvg).toContain('viewBox="0 0 560 560"');
    expect(workbuddySvg).not.toContain("1620");
  });

  it("不含官方标题图的文字路径与 1620 画布裁剪", () => {
    // 原图文字组各路径的起始坐标（WorkBuddy 品牌字与 AI 后缀图形）。
    for (const marker of ["M880.654", "M891.596", "M1498.2", "clip0_10480_3"]) {
      expect(workbuddySvg).not.toContain(marker);
    }
  });

  it("圆角方形图标组完整：背景、模糊光斑、主图形与白色圆角条", () => {
    // 圆角方形背景（rx=120.842，非圆形蒙版）。
    expect(workbuddySvg).toContain('rx="120.842"');
    // 背景品牌渐变与光斑滤镜。
    expect(workbuddySvg).toContain("#0EC8A9");
    expect(workbuddySvg).toContain("#01C886");
    expect(workbuddySvg).toContain("#FFE355");
    expect(workbuddySvg).toContain("feGaussianBlur");
    // 主图形路径与两个白色圆角条（与官方图标组逐字节一致）。
    expect(workbuddySvg).toContain("M428.301 43.7869");
    expect((workbuddySvg.match(/rx="28\.0633"/g) ?? []).length).toBe(2);
  });
});
