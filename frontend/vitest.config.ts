import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

// 行为测试（计划 A1）：mock IPC，不依赖 Tauri 运行时；happy-dom 提供 DOM。
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.ts"],
    // 设计系统 Task 1：css: true 让 tokens.css 在测试中真实注入 DOM，
    // 断言语义 token 的两套主题与降级规则（默认空模块无法校验内容）。
    css: true,
  },
});
