// @vitest-environment node
// AC10 静态契约：验收 identifier/数据根隔离、单实例回调恢复窗口、唯一托盘，
// 以及驱动器确实接入了新场景与外部观察脚本。只读源码/配置，不启动应用。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { acceptanceIdentifier } from "./native-single-instance.mjs";

const REPO = fileURLToPath(new URL("../../../", import.meta.url));
const read = (relative) => fs.readFileSync(path.join(REPO, relative), "utf8");

describe("acceptance 应用标识与数据根隔离", () => {
  it("acceptance::configure 追加 .acceptance 并把应用目录/WebView 指向隔离根", () => {
    const source = read("src-tauri/src/acceptance.rs");
    expect(source).toMatch(
      /config\.identifier\s*=\s*format!\("\{\}\.acceptance",\s*config\.identifier\);/,
    );
    expect(source).toMatch(/AppDirectoriesOverride::Root\(webview_data_dir\(root\)\)/);
    expect(source).toMatch(/root\.join\("webview"\)/);
  });

  it("验收根必须由 TOKENSCOPE_ACCEPTANCE_ROOT 显式提供，缺失拒绝启动", () => {
    const source = read("src-tauri/src/acceptance.rs");
    expect(source).toContain("ENV_ROOT");
    expect(source).toMatch(/要求进程级 \{ENV_ROOT\} 指向已准备好的隔离根/);
    expect(source).toContain("bootstrap_with(raw)");
  });

  it("验收数据目录由隔离根派生，不落回真实 ~/.tokenscope", () => {
    const report = read("crates/tokenscope-core/src/report.rs");
    expect(report).toMatch(/if let Some\(dir\) = crate::acceptance::data_dir_override\(\)/);
    const acceptance = read("crates/tokenscope-core/src/acceptance.rs");
    expect(acceptance).toMatch(/root\(\)\.map\(\|r\| r\.join\("tokenscope"\)\)/);
  });

  it("产品 identifier 与验收 identifier 不同名", () => {
    const config = JSON.parse(read("src-tauri/tauri.conf.json"));
    expect(config.identifier).toBeTruthy();
    const acceptance = acceptanceIdentifier(config.identifier);
    expect(acceptance.endsWith(".acceptance")).toBe(true);
    expect(acceptance).not.toBe(config.identifier);
    expect(config.app.windows[0]).toMatchObject({ label: "main", title: "TokenScope" });
  });
});

describe("单实例回调恢复第一个窗口", () => {
  it("插件回调调用 show_main，且 show/unminimize/set_focus 都在 show_main 内", () => {
    const source = read("src-tauri/src/lib.rs");
    expect(source).toMatch(
      /tauri_plugin_single_instance::init\([\s\S]*?show_main\(app\);[\s\S]*?\}\)/,
    );
    const showMain = source.match(/fn show_main\(app: &tauri::AppHandle\) \{[\s\S]*?\n\}/);
    expect(showMain).not.toBeNull();
    expect(showMain[0]).toMatch(/\.show\(\)/);
    expect(showMain[0]).toMatch(/\.unminimize\(\)/);
    expect(showMain[0]).toMatch(/\.set_focus\(\)/);
  });

  it("单实例插件在窗口 setup 之前注册", () => {
    const source = read("src-tauri/src/lib.rs");
    const runStart = source.indexOf("pub fn run()");
    expect(runStart).toBeGreaterThanOrEqual(0);
    const run = source.slice(runStart);
    const pluginAt = run.indexOf("tauri_plugin_single_instance::init");
    const setupAt = run.indexOf(".setup(");
    expect(pluginAt).toBeGreaterThanOrEqual(0);
    expect(setupAt).toBeGreaterThan(pluginAt);
  });

  it("acceptance identifier 在单实例插件注册前改写", () => {
    const source = read("src-tauri/src/lib.rs");
    const runStart = source.indexOf("pub fn run()");
    const configureAt = source.indexOf(
      "acceptance::configure(context.config_mut(), &acceptance_root);",
      runStart,
    );
    const pluginAt = source.indexOf("tauri_plugin_single_instance::init", runStart);
    expect(configureAt).toBeGreaterThanOrEqual(0);
    expect(pluginAt).toBeGreaterThan(configureAt);
  });

  it("应用只有一个托盘构建点（id=main-tray）", () => {
    const source = read("src-tauri/src/lib.rs");
    expect(source.match(/TrayIconBuilder::with_id/g) ?? []).toHaveLength(1);
    expect(source).toContain('TrayIconBuilder::with_id("main-tray")');
  });
});

describe("外部观察脚本与场景接线", () => {
  it("窗口脚本提供 state/foreground 且读取真实前台窗口", () => {
    const source = read("scripts/native-window-action.ps1");
    expect(source).toMatch(/'state',\s*'foreground'/);
    expect(source).toContain("GetForegroundWindow");
    expect(source).toContain("IsIconic");
    expect(source).toContain("FindAny");
  });

  it("托盘脚本读取可见+溢出工具栏文本并输出 TokenScope 计数", () => {
    const source = read("scripts/read-tray-notification-area.ps1");
    expect(source).toContain("TB_GETBUTTONTEXTW");
    expect(source).toContain("TB_BUTTONCOUNT");
    expect(source).toContain("Shell_TrayWnd");
    expect(source).toContain("NotifyIconOverflowWindow");
    expect(source).toContain("token_scope_count");
  });

  it("驱动器注册 ui-single-instance 并使用 secondary/托盘/窗口观察", () => {
    const driver = read("frontend/scripts/native-acceptance.mjs");
    expect(driver).toContain('"ui-single-instance": nativeSingleInstance');
    expect(driver).toContain("async launchSecondary(");
    expect(driver).toContain("pendingRows");
    expect(driver).toContain("process.exitCode = 3");
    expect(driver).toContain("ev.tray_baseline_before_primary = readTrayObservation();");
    const launchSecondary = driver.match(/async launchSecondary\([\s\S]*?\n  \}/);
    expect(launchSecondary).not.toBeNull();
    for (const marker of [
      'role: "secondary"',
      'record.owner = "acceptance-driver";',
      "record.parentPid = process.pid;",
      "record.sameRoot = true;",
      "record.acceptanceIdentifier = this.opts.acceptanceIdentifier",
      "secondaryLaunchEnvironment(",
      "waitForSecondaryExit(",
      "record.hostExitCode",
      "record.exitElapsedMs",
      "record.exitWithinBudget",
      "record.hostExitDetected",
    ]) {
      expect(launchSecondary[0]).toContain(marker);
    }
    const scenario = read("frontend/scripts/contracts/native-single-instance.mjs");
    expect(scenario).toContain("inst.launchSecondary()");
    expect(scenario).toContain("ck.pending(");
    expect(scenario).toContain("trayBaseline.token_scope_count + 1");
    expect(scenario).toContain("readTray()");
    expect(scenario).toContain("single_tray_icon_before_second");
    expect(scenario).toContain("second_process_exited_within_budget");
    expect(scenario).toContain("second_process_exit_code_zero");
    expect(scenario).toContain("first_window_restored_visible");
    expect(scenario).toContain("first_window_foreground");
    expect(scenario).toContain("classifyWindowRestore");
  });
});
