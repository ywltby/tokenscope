import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

// 行为测试（计划 A1）：mock IPC，不依赖 Tauri 运行时；happy-dom 提供 DOM。
// UP09（升级矩阵 §4.8 第 6 条）：frontend/scripts/lib/*.test.mjs 是原生验收生命周期
// 的纯逻辑契约，跑在 node 环境（由文件顶部 @vitest-environment 指定），不受 happy-dom 影响。
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.ts", "scripts/lib/**/*.test.mjs"],
    // 设计系统 Task 1：css: true 让 tokens.css 在测试中真实注入 DOM，
    // 断言语义 token 的两套主题与降级规则（默认空模块无法校验内容）。
    css: true,
  },
});
