// @vitest-environment node
// AC10（N12 单实例）合成契约：应用标识、隔离根、第二次启动退出、
// 窗口恢复/前台、托盘唯一性。所有外部观察都是注入的合成输入；本文件
// 不启动原生实例，也不读取真实用户数据。
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  ACCEPTANCE_IDENTIFIER_SUFFIX,
  ACCEPTANCE_EXE_MARKERS,
  NORMAL_ACCEPTANCE_STUB_MARKER,
  SINGLE_INSTANCE_EXIT_BUDGET_MS,
  acceptanceIdentifier,
  classifySecondaryExit,
  classifyTrayObservation,
  classifyWindowRestore,
  countTokenScopeMatches,
  evaluateAcceptanceLaunchPreflight,
  hostProcessAlive,
  isIsolatedAcceptanceRoot,
  parseForeground,
  parseTrayObservation,
  parseWindowState,
  secondaryLaunchEnvironment,
  startIfPreflightAllows,
  validateAcceptanceRoot,
  verifyAcceptanceExecutable,
} from "./native-single-instance.mjs";
import { buildLaunchesEvidence, createLaunchRecord } from "./native-instance.mjs";

const REAL_DATA = path.join(os.homedir(), ".tokenscope");
const ISOLATED_ROOT = path.join(os.tmpdir(), "tokenscope-ac10-single-instance");

describe("验收 identifier 与数据根预检", () => {
  it("在基础 identifier 后追加 .acceptance，与正式实例不同", () => {
    const base = "io.github.ywltby.tokenscope";
    const acceptance = acceptanceIdentifier(base);
    expect(acceptance).toBe(`${base}${ACCEPTANCE_IDENTIFIER_SUFFIX}`);
    expect(acceptance).not.toBe(base);
    expect(acceptance.endsWith(".acceptance")).toBe(true);
  });

  it("空 identifier 直接拒绝，不生成只剩后缀的互斥名", () => {
    expect(() => acceptanceIdentifier("")).toThrow(/非空/);
    expect(() => acceptanceIdentifier(null)).toThrow(/非空/);
  });

  it("临时隔离根通过预检", () => {
    expect(isIsolatedAcceptanceRoot(ISOLATED_ROOT, REAL_DATA)).toMatchObject({ ok: true });
  });

  it("真实数据目录内、真实目录祖先、盘符根和相对路径都被拒绝", () => {
    expect(isIsolatedAcceptanceRoot(path.join(REAL_DATA, "nested"), REAL_DATA).ok).toBe(false);
    expect(isIsolatedAcceptanceRoot(path.dirname(REAL_DATA), REAL_DATA).ok).toBe(false);
    expect(isIsolatedAcceptanceRoot(path.parse(REAL_DATA).root, REAL_DATA).ok).toBe(false);
    expect(isIsolatedAcceptanceRoot("relative/root", REAL_DATA).ok).toBe(false);
    expect(isIsolatedAcceptanceRoot("", REAL_DATA).ok).toBe(false);
  });
});

describe("受保护根别名预检与实际 EXE 验收构建验证", () => {
  const aliases = new Map([
    [path.resolve("C:/alias-protected"), path.resolve(REAL_DATA)],
    [path.resolve("C:/alias-child"), path.resolve(path.join(REAL_DATA, "nested"))],
    [path.resolve("C:/alias-home"), path.dirname(path.resolve(REAL_DATA))],
  ]);
  const canonicalize = (target) => {
    const resolved = path.resolve(target);
    return { ok: true, path: aliases.get(resolved) ?? resolved };
  };

  it("拒绝受保护目录本身、子目录、祖先及指向它们的别名", () => {
    for (const root of [
      REAL_DATA,
      path.join(REAL_DATA, "nested"),
      path.dirname(REAL_DATA),
      "C:/alias-protected",
      "C:/alias-child",
      "C:/alias-home",
    ]) {
      expect(validateAcceptanceRoot({ root, protectedDirs: [REAL_DATA], canonicalize }).ok).toBe(
        false,
      );
    }
    expect(
      validateAcceptanceRoot({ root: "C:/safe-root", protectedDirs: [REAL_DATA], canonicalize }).ok,
    ).toBe(true);
  });

  it("真实路径无法解析时 fail closed", () => {
    const result = validateAcceptanceRoot({
      root: ISOLATED_ROOT,
      protectedDirs: [REAL_DATA],
      canonicalize: () => ({ ok: false, reason: "broken junction" }),
    });
    expect(result.ok).toBe(false);
    expect(result.reason).toMatch(/无法解析/);
  });

  it("实际 EXE 必须是 PE 且含 acceptance 专用标记；普通/不可读一律拒绝", () => {
    const pe = (text) => {
      const header = Buffer.alloc(0x100, 0);
      header[0] = 0x4d;
      header[1] = 0x5a;
      header.writeUInt32LE(0x80, 0x3c);
      header.write("PE\u0000\u0000", 0x80, "latin1");
      return Buffer.concat([header, Buffer.from(text, "utf8")]);
    };
    expect(
      verifyAcceptanceExecutable("acceptance.exe", {
        readFile: () => pe("prefix " + ACCEPTANCE_EXE_MARKERS[0] + " suffix"),
      }),
    ).toMatchObject({ ok: true });
    expect(
      verifyAcceptanceExecutable("normal.exe", {
        readFile: () => pe(NORMAL_ACCEPTANCE_STUB_MARKER),
      }).ok,
    ).toBe(false);
    expect(verifyAcceptanceExecutable("plain.exe", { readFile: () => pe("no marker") }).ok).toBe(
      false,
    );
    expect(
      verifyAcceptanceExecutable("not-pe.exe", {
        readFile: () => Buffer.from(ACCEPTANCE_EXE_MARKERS[0]),
      }).ok,
    ).toBe(false);
    expect(
      verifyAcceptanceExecutable("missing.exe", {
        readFile: () => {
          throw new Error("ENOENT");
        },
      }).ok,
    ).toBe(false);
  });

  it("预检拒绝受保护根/普通 EXE 时 prepare 与 launch 零调用", async () => {
    let prepared = 0;
    let launched = 0;
    const refused = await startIfPreflightAllows({
      preflight: () => ({ ok: false, reason: "protected root" }),
      prepare: async () => {
        prepared += 1;
      },
      launch: async () => {
        launched += 1;
      },
    });
    expect(refused.started).toBe(false);
    expect(prepared).toBe(0);
    expect(launched).toBe(0);

    const normalExe = evaluateAcceptanceLaunchPreflight({
      root: ISOLATED_ROOT,
      exePath: "normal.exe",
      realDataDir: REAL_DATA,
      canonicalize,
      readFile: () => {
        const header = Buffer.alloc(0x100, 0);
        header[0] = 0x4d;
        header[1] = 0x5a;
        header.writeUInt32LE(0x80, 0x3c);
        header.write("PE\u0000\u0000", 0x80, "latin1");
        return Buffer.concat([header, Buffer.from(NORMAL_ACCEPTANCE_STUB_MARKER, "utf8")]);
      },
    });
    expect(normalExe.ok).toBe(false);
    expect(normalExe.root.ok).toBe(true);
    expect(normalExe.exe.ok).toBe(false);
  });

  it("attach 模式只校验根，不要求启动 EXE", () => {
    const result = evaluateAcceptanceLaunchPreflight({
      root: ISOLATED_ROOT,
      exePath: "missing.exe",
      realDataDir: REAL_DATA,
      attachExisting: true,
      canonicalize,
      readFile: () => {
        throw new Error("must not read in attach mode");
      },
    });
    expect(result.ok).toBe(true);
    expect(result.exe.skipped).toBe(true);
  });

  it("main 存活同时看 exitCode 与 signalCode", () => {
    expect(hostProcessAlive({ exitCode: null, signalCode: null })).toBe(true);
    expect(hostProcessAlive({ exitCode: 0, signalCode: null })).toBe(false);
    expect(hostProcessAlive({ exitCode: null, signalCode: "SIGTERM" })).toBe(false);
  });
});

describe("第二次启动的退出判定", () => {
  it("第二个实例强制同一隔离根并清除远程调试参数，保留固定验收通道", () => {
    const env = secondaryLaunchEnvironment({
      inherited: {
        KEEP: "kept",
        TOKENSCOPE_ACCEPTANCE_ROOT: "C:/real-tokenscope",
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9999",
      },
      controlled: {
        TOKENSCOPE_ACCEPTANCE_PRICING_BASE: "http://127.0.0.1:12345",
        HTTPS_PROXY: "http://127.0.0.1:12346",
      },
      scenario: { TOKENSCOPE_ACCEPTANCE_COLLECT_DELAY_MS: "5" },
      root: ISOLATED_ROOT,
    });
    expect(env).toMatchObject({
      KEEP: "kept",
      TOKENSCOPE_ACCEPTANCE_ROOT: ISOLATED_ROOT,
      TOKENSCOPE_ACCEPTANCE_PRICING_BASE: "http://127.0.0.1:12345",
      HTTPS_PROXY: "http://127.0.0.1:12346",
      TOKENSCOPE_ACCEPTANCE_COLLECT_DELAY_MS: "5",
    });
    expect(env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS).toBeUndefined();
    expect(() => secondaryLaunchEnvironment({ root: "" })).toThrow(/隔离根/);
  });

  it("限定时间内退出码 0 通过", () => {
    expect(classifySecondaryExit({ exitCode: 0, elapsedMs: 320, budgetMs: 15000 }).status).toBe(
      "passed",
    );
  });

  it("超时即使进程后来被终止也是失败，并说明已终止该 PID", () => {
    const verdict = classifySecondaryExit({
      exitCode: null,
      elapsedMs: SINGLE_INSTANCE_EXIT_BUDGET_MS + 1,
      timedOut: true,
    });
    expect(verdict.status).toBe("failed");
    expect(verdict.reason).toMatch(/终止/);
  });

  it("非零退出码失败；退出码不可读是 pending，不猜成 0", () => {
    expect(classifySecondaryExit({ exitCode: 1, elapsedMs: 100 }).status).toBe("failed");
    expect(classifySecondaryExit({ exitCode: null, elapsedMs: 100 }).status).toBe("pending");
  });

  it("信号终止不是正常移交；超出预算的 0 退出也失败", () => {
    expect(
      classifySecondaryExit({ exitCode: null, signal: "SIGTERM", elapsedMs: 100 }).status,
    ).toBe("failed");
    expect(classifySecondaryExit({ exitCode: 0, elapsedMs: 15001, budgetMs: 15000 }).status).toBe(
      "failed",
    );
  });

  it("未确认退出但未超时是 pending；带终止失败/未确认的失败原因不伪报已终止", () => {
    expect(classifySecondaryExit({ exitConfirmed: false, elapsedMs: 100 }).status).toBe("pending");
    const failed = classifySecondaryExit({
      exitConfirmed: false,
      timedOut: true,
      terminateRequested: true,
      terminateError: "access denied",
      terminateUnconfirmed: true,
      elapsedMs: 15100,
    });
    expect(failed.status).toBe("failed");
    expect(failed.reason).toMatch(/终止请求失败/);
    expect(failed.reason).toMatch(/退出未确认/);
  });
});

describe("第一个窗口恢复可见与前台", () => {
  const state = (overrides = {}) => ({
    ok: true,
    found: true,
    hwnd: "101",
    visible: true,
    minimized: false,
    rect: "0,0,900,700",
    ...overrides,
  });
  const foreground = (overrides = {}) => ({
    ok: true,
    match: true,
    pid: "101",
    hwnd: "101",
    foreground_pid: "101",
    foreground_hwnd: "101",
    ...overrides,
  });

  it("可见、未最小化且前台属于第一个实例时通过", () => {
    const verdict = classifyWindowRestore({ state: state(), foreground: foreground() });
    expect(verdict.status).toBe("passed");
    expect(verdict.visibility.status).toBe("passed");
    expect(verdict.foreground.status).toBe("passed");
  });

  it("仍最小化、不可见或前台属于别的窗口都失败，且两项分开报告", () => {
    expect(
      classifyWindowRestore({ state: state({ minimized: true }), foreground: foreground() }).status,
    ).toBe("failed");
    expect(
      classifyWindowRestore({ state: state({ visible: false }), foreground: foreground() }).status,
    ).toBe("failed");
    const wrongForeground = classifyWindowRestore({
      state: state(),
      foreground: foreground({ match: false, foreground_pid: "999" }),
    });
    expect(wrongForeground.visibility.status).toBe("passed");
    expect(wrongForeground.foreground.status).toBe("failed");
    expect(wrongForeground.foreground.reason).toMatch(/999/);
  });

  it("窗口状态或前台观察不可用时是 pending，不用进程存活替代", () => {
    expect(
      classifyWindowRestore({
        state: { ok: false, error: "state probe failed" },
        foreground: foreground(),
      }).visibility.status,
    ).toBe("pending");
    const pending = classifyWindowRestore({
      state: state(),
      foreground: { ok: false, error: "foreground probe failed" },
    });
    expect(pending.status).toBe("pending");
    expect(pending.foreground.status).toBe("pending");
    expect(pending.reason).not.toMatch(/进程.*存活/);
  });

  it("找不到主窗口是失败，而不是观察失败", () => {
    const verdict = classifyWindowRestore({
      state: state({ found: false }),
      foreground: foreground(),
    });
    expect(verdict.visibility.status).toBe("failed");
  });
});

describe("托盘唯一性判定", () => {
  const tray = (overrides = {}) => ({
    ok: true,
    visible_count: 3,
    overflow_count: 5,
    total_count: 8,
    token_scope_count: 1,
    matches: [{ area: "visible", text: "TokenScope" }],
    ...overrides,
  });

  it("实际观察到 1 个 TokenScope 图标才通过", () => {
    expect(classifyTrayObservation({ observation: tray() }).status).toBe("passed");
  });

  it("0 个或 2 个都失败，并保留实际计数", () => {
    expect(
      classifyTrayObservation({
        observation: tray({ token_scope_count: 0, matches: [] }),
      }).status,
    ).toBe("failed");
    expect(
      classifyTrayObservation({
        observation: tray({
          token_scope_count: 2,
          matches: [
            { area: "visible", text: "TokenScope" },
            { area: "overflow", text: "TokenScope hidden" },
          ],
        }),
      }).status,
    ).toBe("failed");
  });

  it("计数总和、TokenScope 计数与 matches 明细矛盾时 pending，不能通过", () => {
    expect(
      classifyTrayObservation({
        observation: tray({ visible_count: 2, overflow_count: 5, total_count: 8 }),
      }).status,
    ).toBe("pending");
    expect(
      classifyTrayObservation({
        observation: tray({ token_scope_count: 2, matches: [{ text: "TokenScope" }] }),
      }).status,
    ).toBe("pending");
    expect(
      classifyTrayObservation({
        observation: tray({
          token_scope_count: 1,
          matches: [{ text: "TokenScope" }, { text: "Other" }],
        }),
      }).status,
    ).toBe("pending");
    expect(
      classifyTrayObservation({
        observation: tray({ token_scope_count: 1, matches: [{ text: "Other" }] }),
      }).status,
    ).toBe("pending");
  });

  it("观察不可用是 pending；即使 processCount=1 也不能充当托盘证据", () => {
    const verdict = classifyTrayObservation({
      observation: { ok: false, error: "tray probe failed" },
      processCount: 1,
    });
    expect(verdict.status).toBe("pending");
    expect(verdict.reason).toMatch(/tray probe failed/);
    expect(classifyTrayObservation({ observation: null, processCount: 1 }).status).toBe("pending");
  });
});

describe("外部观察输出解析", () => {
  it("解析窗口状态与前台窗口，允许 PowerShell 告警行在前", () => {
    expect(
      parseWindowState(
        "WARNING: something\nSTATE found true hwnd 101 visible true minimized false rect 0,0,900,700\n",
      ),
    ).toMatchObject({ ok: true, found: true, visible: true, minimized: false, hwnd: "101" });
    expect(
      parseForeground("FOREGROUND match true pid 101 hwnd 101 fg_pid 101 fg_hwnd 101\n"),
    ).toMatchObject({ ok: true, match: true, pid: "101", foreground_pid: "101" });
  });

  it("畸形输出不会被当成观察通过", () => {
    expect(parseWindowState("nothing").ok).toBe(false);
    expect(parseWindowState("STATE found true hwnd 1").ok).toBe(false);
    expect(parseForeground("nothing").ok).toBe(false);
    expect(parseTrayObservation("nothing").ok).toBe(false);
    expect(parseTrayObservation('{"ok":true}').ok).toBe(false);
    expect(
      parseTrayObservation(
        '{"ok":true,"visible_count":2,"overflow_count":1,"total_count":4,"token_scope_count":1,"matches":[{"text":"TokenScope"}]}',
      ).ok,
    ).toBe(false);
    expect(
      parseTrayObservation(
        '{"ok":true,"visible_count":3,"overflow_count":1,"total_count":4,"token_scope_count":2,"matches":[{"text":"TokenScope"}]}',
      ).ok,
    ).toBe(false);
  });

  it("解析托盘 JSON 并用 matches 校准计数", () => {
    const observation = parseTrayObservation(
      'noise\n{"ok":true,"visible_count":3,"overflow_count":5,"total_count":8,"token_scope_count":1,"matches":[{"area":"visible","text":"TokenScope"}]}',
    );
    expect(observation).toMatchObject({
      ok: true,
      token_scope_count: 1,
      visible_count: 3,
      overflow_count: 5,
      total_count: 8,
    });
    expect(countTokenScopeMatches(["TokenScope", "Clash", { text: "tokenscope - 1 window" }])).toBe(
      2,
    );
  });
});

describe("launches[] 记录两个进程的归属与退出状态", () => {
  it("primary 与 secondary 的角色、同一隔离根、PID 和退出码都保留", () => {
    const primary = createLaunchRecord({ index: 1, port: 9471, root: ISOLATED_ROOT });
    primary.hostPid = 1001;
    primary.hostExitCode = 0;
    primary.role = "primary";
    const secondary = createLaunchRecord({
      index: 2,
      port: null,
      root: ISOLATED_ROOT,
      browserArgs: "",
    });
    secondary.role = "secondary";
    secondary.owner = "acceptance-driver";
    secondary.parentPid = 4321;
    secondary.sameRoot = true;
    secondary.acceptanceIdentifier = acceptanceIdentifier("io.github.ywltby.tokenscope");
    secondary.hostPid = 1002;
    secondary.hostExitCode = 0;
    secondary.exitElapsedMs = 240;
    secondary.exitBudgetMs = SINGLE_INSTANCE_EXIT_BUDGET_MS;
    secondary.exitWithinBudget = true;
    const rows = buildLaunchesEvidence([primary, secondary]);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ role: "primary", host_pid: 1001, host_exit_code: 0 });
    expect(rows[1]).toMatchObject({
      role: "secondary",
      host_pid: 1002,
      host_exit_code: 0,
      owner: "acceptance-driver",
      parent_pid: 4321,
      same_root: true,
      acceptance_identifier: "io.github.ywltby.tokenscope.acceptance",
      exit_elapsed_ms: 240,
      exit_budget_ms: SINGLE_INSTANCE_EXIT_BUDGET_MS,
      exit_within_budget: true,
    });
  });
});
