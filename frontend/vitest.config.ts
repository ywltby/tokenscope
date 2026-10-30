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
    coverage: {
      provider: "v8",
      // 统计 frontend/src 下全部产品源码（Vue + TS）。all: true 把未被
      // 任何测试导入的文件也按 0% 计入报告，而不是只统计已执行文件
      // （src/dev/ 与 src/views/ 等低频文件因此可见）。
      all: true,
      include: ["src/**/*.{ts,vue}"],
      // 排除测试本体；纯类型声明（*.d.ts，当前 src 下没有）无运行时
      // 行为，防御性排除。生成物（node_modules/dist）不匹配 include。
      exclude: ["src/**/*.test.ts", "src/**/*.d.ts"],
      reporter: ["text", "html", "lcovonly", "json-summary"],
      // 报告只落本地产物目录（qa-artifacts/ 已被 gitignore，不入库）。
      reportsDirectory: "../qa-artifacts/coverage/frontend-vitest",
    },
  },
});
