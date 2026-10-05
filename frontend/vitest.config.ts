import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

// 行为测试（计划 A1）：mock IPC，不依赖 Tauri 运行时；happy-dom 提供 DOM。
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.ts"],
  },
});
