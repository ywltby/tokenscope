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

  it("actual EXE 标记只存在于 acceptance 专用源码，普通构建有 stub 标记", () => {
    const shellAcceptance = read("src-tauri/src/acceptance.rs");
    expect(shellAcceptance).toContain("验收构建要求进程级");
    expect(shellAcceptance).toContain("验收模式：WebView 用户数据与 identifier 已指向隔离根");
    const shellLib = read("src-tauri/src/lib.rs");
    expect(shellLib).toContain("验收模式启动失败");
    expect(shellLib).toMatch(/#\[cfg\(feature = "acceptance"\)\]\s*mod acceptance;/);
    const coreStub = read("crates/tokenscope-core/src/acceptance.rs");
    expect(coreStub).toContain("当前构建未启用 acceptance feature，不接受验收根注入");
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

  it("托盘脚本按按钮索引取 command ID 再读文本，并拒绝不可观察/矛盾结果", () => {
    const source = read("scripts/read-tray-notification-area.ps1");
    expect(source).toContain("TB_GETBUTTON");
    expect(source).toContain("commandId");
    expect(source).toContain("TB_GETBUTTONTEXTW");
    expect(source).toContain("WriteProcessMemory");
    expect(source).toContain("$tokenMatches");
    expect(source).not.toMatch(/\$matches\b/i);
    expect(source).toContain("SelfTest");
    for (const scenario of [
      "match",
      "noncontiguous",
      "timeout",
      "count-mismatch",
      "overflow-unobservable",
      "truncated",
    ]) {
      expect(source).toContain(scenario);
    }
    expect(source).toContain("overflow area is unobservable");
    const test = read("scripts/test_read_tray_notification_area.ps1");
    expect(test).toContain("noncontiguous");
    expect(test).toContain("count-mismatch");
    expect(test).toContain("overflow-unobservable");
  });

  it("托盘读取是只读指针解析协议，且有受控文本/真实区域/受控窗口自测", () => {
    const source = read("scripts/read-tray-notification-area.ps1");
    // 旧的无界风险协议必须消失：不再向目标进程发送 TB_GETBUTTONTEXTW
    expect(source).not.toContain("TB_GETBUTTONTEXTW length query timeout");
    expect(source).not.toContain("text grew after length query");
    expect(source).not.toContain("WriteProcessMemory text buffer failed");
    // 只读协议：iString 指针 + 本地缓冲上界 + NUL 截断
    expect(source).toContain("ReadTextPointer");
    expect(source).toContain("MaxTextBytes");
    expect(source).toContain("iStringPtr");
    // TB_GETBUTTON 仍用固定 64 字节远程缓冲（写入与文本长度无关）
    expect(source).toContain("TB_GETBUTTON");
    // 受控文本（变长/超长/无效指针）+ 真实区域 + 受控窗口三个自测都在
    expect(source).toContain("SelfTestControlledText");
    expect(source).toContain("SelfTestReal");
    expect(source).toContain("SelfTestWindow");
    expect(source).toContain("CreateControlledToolbar");
    const test = read("scripts/test_read_tray_notification_area.ps1");
    expect(test).toContain("-SelfTestControlledText");
    expect(test).toContain("-SelfTestReal");
    expect(test).toContain("PENDING real notification area unobservable");
  });

  it("隔离/EXE 预检发生在目录准备和任何进程启动之前", () => {
    const driver = read("frontend/scripts/native-acceptance.mjs");
    const mainStart = driver.indexOf("async function main()");
    expect(mainStart).toBeGreaterThanOrEqual(0);
    const main = driver.slice(mainStart);
    const preflightAt = main.indexOf("evaluateAcceptanceLaunchPreflight");
    expect(preflightAt).toBeGreaterThanOrEqual(0);
    for (const marker of [
      "fs.mkdirSync(OUT",
      "startAcceptanceIo(",
      "inst = new Instance(",
      "await inst.prepareRoot(",
      "readTrayObservation()",
      'spawnSyncBounded("git"',
      'spawnSyncBounded("cmd"',
    ]) {
      const at = main.indexOf(marker);
      expect(at, marker).toBeGreaterThan(preflightAt);
    }
    const refusal = main.slice(preflightAt, main.indexOf("fs.mkdirSync(OUT"));
    expect(refusal).toContain("if (!launchPreflight.ok)");
    expect(refusal).toContain("process.exitCode = 2");
    expect(driver).toContain("startIfPreflightAllows(");
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
      "record.exitConfirmed",
      "record.terminateRequested",
      "record.terminateError",
      "record.hostSignal",
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
    expect(scenario).toContain("second_process_exit_confirmed");
    expect(scenario).toContain("second_process_exited_within_budget");
    expect(scenario).toContain("second_process_exit_code_zero");
    expect(scenario).toContain("first_window_restored_visible");
    expect(scenario).toContain("first_window_foreground");
    expect(scenario).toContain("hostProcessAlive");
    expect(scenario).toContain("classifyWindowRestore");
  });
});
