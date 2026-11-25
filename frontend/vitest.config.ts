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
    // 2026-11-05：并发上限 4。本机 Windows 的提交内存/句柄有限，默认按 CPU
    // 起 70+ 个 fork worker 时会 `spawn` 失败（errno -4094），表现为随机的
    // "Test Files 73/75"。测试集合与断言不变，只降低并行度；CI runner 资源
    // 充足，多花的时间在 frontend 作业预算内。
    maxWorkers: 4,
    minWorkers: 1,
    // 设计系统 Task 1：css: true 让 tokens.css 在测试中真实注入 DOM，
    // 断言语义 token 的两套主题与降级规则（默认空模块无法校验内容）。
    css: true,
    coverage: {
      // Vitest 5 已移除 coverage.all；include 声明的文件由 provider 补算
      // 未执行项。这里的 custom provider 只修正一处补算失效：契约测试以
      // ?raw 导入全部源码，空壳模块会占住源码路径并被误判为已加载。
      provider: "custom",
      customProviderModule: "./vitest.coverage-provider.ts",
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
