// 自包含浏览器 E2E：只测试本次构建，端口冲突必须失败，不能借用未知服务。
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { preview } from "vite";
const repo = fileURLToPath(new URL("../../", import.meta.url));
const port = Number(process.env.TOKENSCOPE_E2E_PORT ?? "1459");
const url = `http://127.0.0.1:${port}`;
const server = await preview({
  root: fileURLToPath(new URL("../", import.meta.url)),
  preview: { host: "127.0.0.1", port, strictPort: true, open: false },
});
try {
  const child = spawn(
    process.execPath,
    [
      "frontend/scripts/check-ui-contracts.mjs",
      "--url",
      url,
      "--phase",
      "verify",
      "--fixture",
      "normal",
      "--theme",
      "light,dark",
      "--viewport",
      "1280x820,980x620",
      "--contract",
      "ui_refresh_lifecycle,ui_refresh_clock,ui_adjustment_shapes,ui_date_keyboard_matrix,ui_control_material,source_paths_management_contract",
      "--output",
      "qa-artifacts/ui-adjustment/ci-e2e",
    ],
    { cwd: repo, stdio: "inherit", windowsHide: true },
  );
  process.exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code) => resolve(code ?? 1));
  });
} finally {
  await new Promise((resolve, reject) =>
    server.httpServer.close((error) => (error ? reject(error) : resolve())),
  );
}
