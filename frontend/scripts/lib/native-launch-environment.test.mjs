// @vitest-environment node
// 执行真实 launch 的环境对象表达式，防止重启替换场景变量时丢失固定隔离通道。
import fs from "node:fs";
import { describe, expect, it } from "vitest";
import { proxyEnvironment } from "./acceptance-io.mjs";

const source = fs.readFileSync(new URL("../native-acceptance.mjs", import.meta.url), "utf8");
const expression = source.match(/    const env = \{[\s\S]*?\n    \};\r?\n    const out = fs\.openSync/);
if (!expression) throw new Error("找不到真实 launch 环境装配，必须同步复核此契约");
const assemble = new Function("process", "browserArgs", expression[0].replace(/\r?\n    const out = fs\.openSync$/, "\nreturn env;"));
const controlled = {
  TOKENSCOPE_ACCEPTANCE_PRICING_BASE: "http://127.0.0.1:12345",
  TOKENSCOPE_ACCEPTANCE_OPENER: "record",
  ...proxyEnvironment("http://127.0.0.1:12346"),
};
function launchEnvironment(scene) {
  return assemble.call({ root: "C:/isolated/root", opts: { env: scene, controlledIo: { env: controlled } } },
    { env: { ALL_PROXY: "http://inherited.invalid", all_proxy: "http://inherited.invalid", KEEP: "kept" } },
    "--remote-debugging-port=9471");
}

describe("固定验收环境跨原生重启保留", () => {
  it.each([
    { TOKENSCOPE_ACCEPTANCE_COLLECT_DELAY_MS: "2500", TOKENSCOPE_ACCEPTANCE_COLLECT_FAIL_ON_ROUND: "2" },
    {},
  ])("场景替换 env 后价格桩、opener 和所有代理覆盖仍存在：%j", scene => {
    expect(launchEnvironment(scene)).toMatchObject(controlled);
  });
  it("sync-failure 的末级代理覆盖保留，且固定价格桩和根仍存在", () => {
    const overrides = proxyEnvironment("http://127.0.0.1:9", "");
    expect(launchEnvironment(overrides)).toMatchObject({ ...controlled, ...overrides,
      KEEP: "kept", TOKENSCOPE_ACCEPTANCE_ROOT: "C:/isolated/root",
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9471" });
  });
});
