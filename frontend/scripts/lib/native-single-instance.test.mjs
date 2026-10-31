// @vitest-environment node
// AC10（N12 单实例）合成契约：应用标识、隔离根、第二次启动退出、
// 窗口恢复/前台、托盘唯一性。所有外部观察都是注入的合成输入；本文件
// 只用 mkdtemp 独占沙箱，绝不读写真实用户目录（受保护目录仅做元数据解析）。
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { afterAll, afterEach, describe, expect, it } from "vitest";
import {
  ACCEPTANCE_BUILD_COMMAND,
  ACCEPTANCE_BUILD_RECORD_KIND,
  ACCEPTANCE_IDENTIFIER_SUFFIX,
  ACCEPTANCE_EXE_MARKERS,
  NORMAL_ACCEPTANCE_STUB_MARKER,
  SINGLE_INSTANCE_EXIT_BUDGET_MS,
  acceptanceIdentifier,
  canonicalizeAcceptancePath,
  classifySecondaryExit,
  classifyTrayObservation,
  classifyWindowRestore,
  collectAcceptanceTreePaths,
  collectManifestAcceptancePaths,
  countTokenScopeMatches,
  evaluateAcceptanceLaunchPreflight,
  getAcceptancePathLayout,
  getAcceptanceWritePaths,
  hostProcessAlive,
  isIsolatedAcceptanceRoot,
  parseAcceptanceBuildRecord,
  parseForeground,
  parseTrayObservation,
  parseWindowState,
  readGitHeadCommit,
  secondaryLaunchEnvironment,
  startIfPreflightAllows,
  validateAcceptancePaths,
  validateAcceptanceRoot,
  verifyAcceptanceBuildIdentity,
  verifyAcceptanceExecutable,
  verifyBuildOriginBinding,
  readZipEntryBytes,
} from "./native-single-instance.mjs";
import { buildLaunchesEvidence, createLaunchRecord } from "./native-instance.mjs";

const REAL_DATA = path.join(os.homedir(), ".tokenscope");
const PROJECT_SLUG = "-acceptance-workspace-tokenscope";

/**
 * 本次运行独占的沙箱：mkdtempSync 保证目录唯一且由本进程创建，因此清理只
 * 可能落在自己拥有的路径上。登记本次创建的链接，清理时先逐个 unlink 链接
 * （绝不跟随进目标侧），再对沙箱根做递归删除——递归删除的前提是根由本次
 * mkdtemp 新建、位于系统临时目录之下且不等于临时目录本身。
 * 2026-10-31 收紧：清理前解析真实路径与链接祖先——sandbox.root 本身或任一
 * 已登记链接若经 junction 指到沙箱之外（例如被注入的 junction 把 root 的
 * 某段重定向到外部目录），拒绝递归删除，宁可留下现场也不删除外部数据。
 */
function createRunSandbox(label) {
  const realTmp = fs.realpathSync(os.tmpdir());
  const root = fs.realpathSync(fs.mkdtempSync(path.join(realTmp, `ac10-${label}-`)));
  const links = [];
  const sandbox = {
    root,
    dir: (...parts) => {
      const target = path.join(root, ...parts);
      fs.mkdirSync(target, { recursive: true });
      return target;
    },
    file: (...args) => {
      const content = args.pop();
      const target = path.join(root, ...args);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, content, "utf8");
      return target;
    },
    link: (linkPath, targetPath) => {
      fs.mkdirSync(path.dirname(linkPath), { recursive: true });
      fs.symlinkSync(targetPath, linkPath, process.platform === "win32" ? "junction" : "dir");
      links.push(linkPath);
      return linkPath;
    },
    owned: (target) => {
      const resolved = path.resolve(target);
      return resolved === root || resolved.startsWith(root + path.sep);
    },
    /**
     * 目标（含其全部祖先与**整棵子树**）在真实文件系统里仍解析回沙箱内才算
     * 自有。子树里的 junction 会把 rmSync 的递归删除引到链接另一侧——逐条目
     * lstat，链接/硬链接解析出界即拒绝。遍历只发生在沙箱内部路径上。
     */
    ownedReal: (target) => {
      const resolved = path.resolve(target);
      if (!sandbox.owned(resolved)) return false;
      // 自身与全部祖先：realpath 逐级上溯（防父级被换成 junction）
      let current = resolved;
      while (true) {
        let real;
        try {
          real = fs.realpathSync(current);
        } catch {
          return false;
        }
        if (!sandbox.owned(real)) return false;
        if (current === root) break;
        const parent = path.dirname(current);
        if (parent === current) return false;
        current = parent;
      }
      // 子树走查：每个条目若是链接，解析目标必须仍在沙箱内
      const stack = [resolved];
      while (stack.length) {
        const node = stack.pop();
        let stat;
        try {
          stat = fs.lstatSync(node);
        } catch {
          return false;
        }
        if (stat.isSymbolicLink()) {
          let linkTarget;
          try {
            linkTarget = fs.realpathSync(node);
          } catch {
            return false;
          }
          if (!sandbox.owned(linkTarget)) return false;
          continue;
        }
        if (stat.isDirectory()) {
          // 目录无论 nlink 数值为何都继续遍历（nlink>1 不是链接证据）
          let names;
          try {
            names = fs.readdirSync(node);
          } catch {
            return false;
          }
          for (const name of names) {
            const child = path.join(node, name);
            if (!sandbox.owned(child)) return false;
            stack.push(child);
          }
        }
      }
      return true;
    },
    remove: (target) => {
      if (!sandbox.owned(target)) throw new Error(`拒绝删除非本次运行创建的路径：${target}`);
      // 递归删除前校验目标及全部祖先的真实归属：目标之下若混入指向沙箱外
      // 的 junction，字符串前缀检查发现不了，rmSync 会跟随进外部目录。
      if (!sandbox.ownedReal(target)) {
        throw new Error(`目标或其链接祖先越出沙箱，拒绝递归删除：${target}`);
      }
      fs.rmSync(target, { recursive: true, force: true });
    },
    cleanup: () => {
      // 1) 沙箱根本身不得经链接祖先越出本次临时前缀
      if (!sandbox.ownedReal(root) || root === realTmp || !root.startsWith(realTmp + path.sep)) {
        throw new Error(`沙箱根经链接祖先越出本次沙箱，拒绝递归删除：${root}`);
      }
      // 2) 每个登记的链接必须登记时在沙箱内、现在也真实解析在沙箱内
      for (const link of links) {
        if (!sandbox.owned(link)) throw new Error(`登记的链接不在沙箱内：${link}`);
        // 链接自身允许已不存在（此前清理失败重入时）；存在但断链或指向外部
        // 都拒绝，宁可留下现场。
        let stat = null;
        try {
          stat = fs.lstatSync(link);
        } catch {
          continue;
        }
        if (!stat.isSymbolicLink()) continue;
        let linkTarget;
        try {
          linkTarget = fs.realpathSync(link);
        } catch {
          throw new Error(`登记的链接无法解析真实路径（断链/权限），拒绝清理：${link}`);
        }
        if (!sandbox.owned(linkTarget)) {
          throw new Error(`登记的链接经 junction 越出沙箱（指向 ${linkTarget}），拒绝清理：${link}`);
        }
        fs.unlinkSync(link);
      }
      // 3) 最后一次复核根的归属（unlink 链接之后状态可能变化）
      if (!sandbox.ownedReal(root)) {
        throw new Error(`沙箱根在链接处理后越出沙箱，拒绝递归删除：${root}`);
      }
      fs.rmSync(root, { recursive: true, force: true });
    },
  };
  return sandbox;
}

// 只读路径逻辑共用一个沙箱根（目录本身不创建，避免走查/清单分支干扰）。
const shared = createRunSandbox("shared");
const ISOLATED_ROOT = path.join(shared.root, "isolated-root");
const OUT_DIR = path.join(shared.root, "evidence-out");
afterAll(() => shared.cleanup());

/** 一个"什么都没写过"的 readFile：只在测试里代替不存在的路径。 */
function missingFile(target) {
  const error = new Error(`ENOENT: no such file or directory, open '${target}'`);
  error.code = "ENOENT";
  throw error;
}

function peWith(text) {
  const header = Buffer.alloc(0x100, 0);
  header[0] = 0x4d;
  header[1] = 0x5a;
  header.writeUInt32LE(0x80, 0x3c);
  header.write("PE\u0000\u0000", 0x80, "latin1");
  return Buffer.concat([header, Buffer.from(text, "utf8")]);
}

function sha256(bytes) {
  return cryptoModule.createHash("sha256").update(bytes).digest("hex");
}

/**
 * 合成 stored ZIP（八轮/十一轮复核问题 2）：真实 ZIP 字节布局——local
 * header(30B+name) + 数据 + central directory(46B+name) + EOCD(22B)。
 * 无压缩（method 0），符合 readZipEntryBytes 的解析契约。
 */
function makeStoredZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, content] of entries) {
    const nameBuf = Buffer.from(name, "utf8");
    const crc = crc32(content);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0, 6); // flags
    local.writeUInt16LE(0, 8); // method: stored
    local.writeUInt16LE(0, 10); // time
    local.writeUInt16LE(0x21, 12); // date (1980-01-01)
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(content.length, 18);
    local.writeUInt32LE(content.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, nameBuf, content);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0x21, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(content.length, 20);
    central.writeUInt32LE(content.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + content.length;
  }
  const cdStart = offset;
  const centralBuf = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralBuf.length, 12);
  eocd.writeUInt32LE(cdStart, 16);
  eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...locals, centralBuf, eocd]);
}

// CRC32（ZIP 需要；Node 无内置，用查表实现）
const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();
function crc32(buf) {
  let crc = -1;
  for (let i = 0; i < buf.length; i++) crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ buf[i]) & 0xff];
  return (crc ^ -1) >>> 0;
}

import cryptoModule from "node:crypto";

/** 与 scripts/prepare-native-acceptance.ps1 同形状的清单（绝对路径 + files）。 */
function acceptanceManifest(root, { files = {}, overrides = {} } = {}) {
  return {
    kind: "tokenscope-native-acceptance",
    root,
    data_dir: path.join(root, "tokenscope"),
    claude_source_dir: path.join(root, "sources", "claude"),
    claude_projects_dir: path.join(root, "sources", "claude", "projects"),
    codex_source_dir: path.join(root, "sources", "codex"),
    codex_sessions_dir: path.join(root, "sources", "codex", "sessions"),
    webview_data_dir: path.join(root, "webview"),
    env_var: { name: "TOKENSCOPE_ACCEPTANCE_ROOT", value: root },
    price_auto_sync: false,
    files,
    ...overrides,
  };
}

function writeManifest(sandbox, root, manifest) {
  const target = path.join(root, "manifest.json");
  sandbox.file(
    ...path.relative(sandbox.root, target).split(path.sep),
    JSON.stringify(manifest, null, 2),
  );
  return target;
}

describe("沙箱清理只触碰本次创建的路径", () => {
  it("预先存在的目录不在沙箱内时拒绝删除，内容保持不变", () => {
    const preExisting = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "ac10-owned-check-"));
    const marker = path.join(preExisting, "keep.txt");
    fs.writeFileSync(marker, "must survive", "utf8");
    const sandbox = createRunSandbox("cleanup");
    try {
      expect(sandbox.owned(preExisting)).toBe(false);
      expect(() => sandbox.remove(preExisting)).toThrow(/非本次运行创建/);
      expect(fs.readFileSync(marker, "utf8")).toBe("must survive");
      const own = sandbox.file("home", "workspace", "tmp.txt");
      sandbox.remove(path.dirname(own));
      expect(fs.existsSync(own)).toBe(false);
    } finally {
      sandbox.cleanup();
      fs.rmSync(preExisting, { recursive: true, force: true });
    }
    expect(fs.existsSync(sandbox.root)).toBe(false);
    expect(fs.existsSync(preExisting)).toBe(false);
  });

  it("每次运行拿到唯一目录，清理后不留残留", () => {
    const a = createRunSandbox("unique");
    const b = createRunSandbox("unique");
    expect(a.root).not.toBe(b.root);
    a.cleanup();
    b.cleanup();
    expect(fs.existsSync(a.root)).toBe(false);
    expect(fs.existsSync(b.root)).toBe(false);
  });

  it("remove 遇到沙箱内 junction 指向沙箱外时拒绝递归删除，外部目录不受影响", () => {
    // 独占外部目录：本测试创建并自己清理（证明沙箱 remove 不会动它）。
    const outside = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "ac10-remove-out-"));
    const outsideFile = path.join(outside, "precious.txt");
    fs.writeFileSync(outsideFile, "must survive", "utf8");
    const sandbox = createRunSandbox("remove-junction");
    // 目标目录本身在沙箱内，但里面藏了一个指向外部的 junction
    const target = sandbox.dir("victim");
    sandbox.link(path.join(target, "escape"), outside);
    try {
      expect(sandbox.owned(target)).toBe(true); // 字符串前缀检查放行
      expect(() => sandbox.remove(target)).toThrow(/越出沙箱|拒绝递归删除/);
      // 外部文件原样保留，junction 与目标目录也不被删除（留下现场）
      expect(fs.readFileSync(outsideFile, "utf8")).toBe("must survive");
      expect(fs.existsSync(path.join(target, "escape"))).toBe(true);
    } finally {
      fs.rmSync(path.join(target, "escape"), { force: true });
      fs.rmSync(outside, { recursive: true, force: true });
      fs.rmSync(sandbox.root, { recursive: true, force: true });
    }
  });

  it("remove 对无链接的沙箱内目标正常删除（正例对照）", () => {
    const sandbox = createRunSandbox("remove-ok");
    const target = sandbox.dir("plain", "nested");
    sandbox.file("plain", "nested", "data.txt", "x");
    sandbox.remove(target);
    expect(fs.existsSync(target)).toBe(false);
    sandbox.cleanup();
    expect(fs.existsSync(sandbox.root)).toBe(false);
  });

  it("登记的链接指向沙箱外时清理拒绝，外部目录与链接都不被删除", () => {
    // 独占外部目录：由本测试创建并自己清理（证明沙箱清理不会动它）。
    const outside = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "ac10-outside-"));
    const outsideFile = path.join(outside, "precious.txt");
    fs.writeFileSync(outsideFile, "must survive", "utf8");
    const sandbox = createRunSandbox("junction-escape");
    // 沙箱内登记一个 junction 指向外部目录（模拟注入/测试装配错误）
    sandbox.link(path.join(sandbox.root, "escape"), outside);
    try {
      expect(() => sandbox.cleanup()).toThrow(/越出/);
      // 外部目录原样保留
      expect(fs.readFileSync(outsideFile, "utf8")).toBe("must survive");
      // 拒绝清理时链接本身也不删（宁可留下现场）
      expect(fs.existsSync(path.join(sandbox.root, "escape"))).toBe(true);
    } finally {
      // 本测试自己负责拆除现场：先删链接再删外部与沙箱
      fs.rmSync(path.join(sandbox.root, "escape"), { force: true });
      fs.rmSync(outside, { recursive: true, force: true });
      fs.rmSync(sandbox.root, { recursive: true, force: true });
    }
  });

  it("链接目标在沙箱内时正常清理；断链链接拒绝清理并保留现场", () => {
    // 正例：沙箱内链接（link -> 沙箱内目录）清理成功
    const okSandbox = createRunSandbox("junction-inside");
    const innerTarget = okSandbox.dir("inner-target");
    okSandbox.link(path.join(okSandbox.root, "inner-link"), innerTarget);
    okSandbox.cleanup();
    expect(fs.existsSync(okSandbox.root)).toBe(false);

    // 断链：链接指向已删除的目录 → 无法证明解析在沙箱内 → 拒绝清理
    const broken = createRunSandbox("junction-broken");
    const gone = broken.dir("gone");
    broken.link(path.join(broken.root, "broken-link"), gone);
    fs.rmSync(gone, { recursive: true, force: true });
    try {
      // ownedReal 子树走查在根检查即拦截（断链无法证明解析在沙箱内）
      expect(() => broken.cleanup()).toThrow(/越出|无法解析真实路径|断链/);
      expect(fs.existsSync(broken.root)).toBe(true);
    } finally {
      fs.rmSync(broken.root, { recursive: true, force: true });
    }
  });
});

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

describe("路径布局与固定判定", () => {
  it("布局覆盖 acceptance.rs 的三类覆盖点与 prepare 脚本创建的每个目录层级", () => {
    const rootSample = path.resolve("C:/acc-root");
    const outSample = path.resolve("C:/acc-out");
    const layoutPaths = getAcceptanceWritePaths(rootSample, outSample);
    for (const expected of [
      rootSample,
      path.join(rootSample, "tokenscope"),
      path.join(rootSample, "tokenscope", "logs"),
      path.join(rootSample, "sources", "claude", "projects"),
      path.join(rootSample, "sources", "codex", "sessions"),
      path.join(rootSample, "sources", "workbuddy", "projects"),
      path.join(rootSample, "sources", "workbuddy-ai", "projects"),
      path.join(rootSample, "sources", "cc-switch"),
      path.join(rootSample, "webview"),
      path.join(rootSample, "manifest.json"),
      outSample,
    ]) {
      expect(layoutPaths).toContain(expected);
    }
    expect(getAcceptancePathLayout("C:/acc-root").find((e) => e.path.endsWith("manifest.json")).kind).toBe(
      "file",
    );
  });

  it("path.relative 对 .. 开头但非父目录的分量不误判（..fixture 是子目录）", () => {
    const sandbox = createRunSandbox("dotdot");
    try {
      const fakeHome = sandbox.dir("home");
      sandbox.dir("home", ".tokenscope");
      const root = path.join(fakeHome, ".tokenscope", "..fixture");
      fs.mkdirSync(root, { recursive: true });
      const result = validateAcceptancePaths({ root, homeDir: fakeHome });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/受保护目录/);
    } finally {
      sandbox.cleanup();
    }
  });

  it("--out 指向受保护目录时拒绝", () => {
    const sandbox = createRunSandbox("out-protected");
    try {
      const fakeHome = sandbox.dir("home");
      const protectedDir = sandbox.dir("home", ".tokenscope");
      sandbox.file("home", ".tokenscope", "history.db", "real user data");
      const result = validateAcceptancePaths({
        root: path.join(fakeHome, "workspace"),
        outDir: protectedDir,
        homeDir: fakeHome,
      });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/受保护目录/);
      expect(fs.readFileSync(path.join(protectedDir, "history.db"), "utf8")).toBe("real user data");
    } finally {
      sandbox.cleanup();
    }
  });

  it("根等于用户主目录、或主目录的祖先时拒绝（与 acceptance.rs 同口径）", () => {
    const sandbox = createRunSandbox("home-parity");
    try {
      const fakeHome = sandbox.dir("home");
      sandbox.dir("home", ".tokenscope");
      // 受保护目录整体别名到别处时，只剩"主目录本身/其祖先"这一条能拦住
      const relocate = (target) => {
        const resolved = path.resolve(target);
        const base = path.basename(resolved);
        if (base.startsWith(".")) return { ok: true, path: path.resolve("D:/elsewhere", base) };
        return { ok: true, path: resolved };
      };
      expect(
        validateAcceptancePaths({ root: fakeHome, homeDir: fakeHome, canonicalize: relocate }).reason,
      ).toMatch(/用户主目录本身/);
      expect(
        validateAcceptancePaths({
          root: path.dirname(fakeHome),
          homeDir: fakeHome,
          canonicalize: relocate,
        }).reason,
      ).toMatch(/用户主目录 .*的祖先/);
      // 主目录之下但不含受保护目录仍然允许（真实运行就在 tmpdir 里）
      expect(
        validateAcceptancePaths({ root: path.join(fakeHome, "workspace"), homeDir: fakeHome }).ok,
      ).toBe(true);
    } finally {
      sandbox.cleanup();
    }
  });

  it("全部验收子路径均通过预检（真实现实解析，不注入）", () => {
    const sandbox = createRunSandbox("layout-ok");
    try {
      const fakeHome = sandbox.dir("home");
      const root = sandbox.dir("home", "workspace");
      for (const entry of getAcceptancePathLayout(root)) {
        if (entry.kind === "dir") fs.mkdirSync(entry.path, { recursive: true });
      }
      const result = validateAcceptancePaths({
        root,
        outDir: sandbox.dir("evidence"),
        homeDir: fakeHome,
      });
      expect(result.ok).toBe(true);
      expect(result.write_paths.length).toBeGreaterThanOrEqual(
        getAcceptanceWritePaths(root).length,
      );
      expect(result.tree_directories).toBeGreaterThan(5);
      expect(result.manifest_present).toBe(false);
    } finally {
      sandbox.cleanup();
    }
  });

describe("深层 junction 真实反例：准备前拒绝且哨兵不变", () => {
  // 假用户目录 + 有效 manifest.json + root/sources/claude/projects junction。
  // 全部使用真实 fs 与真实路径解析实现，不注入 canonicalize。
  function buildJunctionSandbox(label) {
    const sandbox = createRunSandbox(label);
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    // 假"真实"来源目录：<fake home>/.claude/projects/<slug>/session.jsonl
    const realProjects = sandbox.dir("home", ".claude", "projects");
    const sentinel = sandbox.file(
      "home",
      ".claude",
      "projects",
      PROJECT_SLUG,
      "session.jsonl",
      "REAL USER LOG LINE\n",
    );
    sandbox.dir("home", "workspace", "sources", "claude");
    // <root>/sources/claude/projects 是一个真实 junction，指向假受保护目录
    sandbox.link(path.join(root, "sources", "claude", "projects"), realProjects);
    writeManifest(sandbox, root, acceptanceManifest(root));
    return { sandbox, home, root, realProjects, sentinel };
  }

  /** 模拟准备脚本按清单写合成日志：路径穿过 junction 就落到"真实"目录里。 */
  function prepareFixture(root) {
    const target = path.join(root, "sources", "claude", "projects", PROJECT_SLUG, "session.jsonl");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "SYNTHETIC ACCEPTANCE LINE\n", "utf8");
    return target;
  }

  it("junction 位于 sources/claude/projects：预检拒绝，零准备零启动，哨兵不变", async () => {
    const { sandbox, home, root, sentinel } = buildJunctionSandbox("junction-deep");
    try {
      let prepared = 0;
      let launched = 0;
      const gate = await startIfPreflightAllows({
        preflight: () =>
          evaluateAcceptanceLaunchPreflight({
            root,
            outDir: path.join(sandbox.root, "evidence"),
            exePath: path.join(sandbox.root, "tokenscope.exe"),
            homeDir: home,
            attachExisting: true,
          }),
        prepare: async () => {
          prepared += 1;
          prepareFixture(root);
        },
        launch: async () => {
          launched += 1;
        },
      });
      expect(gate.started).toBe(false);
      expect(gate.preflight.paths.ok).toBe(false);
      // 新防线（链接必须落在验收根内）先于受保护目录检查触发；两条理由都成立
      expect(gate.preflight.paths.reason).toMatch(/指向隔离根之外|受保护目录/);
      // 损害模型：准备脚本若要写的合成日志文件，真实位置就是受保护目录里的哨兵
      expect(
        canonicalizeAcceptancePath(
          path.join(root, "sources", "claude", "projects", PROJECT_SLUG, "session.jsonl"),
        ).path,
      ).toBe(path.resolve(sentinel));
      expect(prepared).toBe(0);
      expect(launched).toBe(0);
      expect(fs.readFileSync(sentinel, "utf8")).toBe("REAL USER LOG LINE\n");
    } finally {
      sandbox.cleanup();
    }
  });

  it("junction 深一层（projects/<slug>）也要被现有条目走查抓到", async () => {
    const sandbox = createRunSandbox("junction-deeper");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    const realSlug = sandbox.dir("home", ".claude", "projects", PROJECT_SLUG);
    const sentinel = sandbox.file(
      "home",
      ".claude",
      "projects",
      PROJECT_SLUG,
      "session.jsonl",
      "REAL USER LOG LINE\n",
    );
    sandbox.dir("home", "workspace", "sources", "claude", "projects");
    sandbox.link(path.join(root, "sources", "claude", "projects", PROJECT_SLUG), realSlug);
    writeManifest(sandbox, root, acceptanceManifest(root));
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      // 新防线（链接必须落在验收根内）先于受保护目录检查触发；两条理由都成立
      expect(result.reason).toMatch(/指向隔离根之外|受保护目录/);
      // 走查本身证明该 junction 被识别为别名（而不是靠固定布局列表命中）
      const tree = collectAcceptanceTreePaths(root);
      expect(tree.links.map((link) => path.basename(link))).toContain(PROJECT_SLUG);
      expect(fs.readFileSync(sentinel, "utf8")).toBe("REAL USER LOG LINE\n");
    } finally {
      sandbox.cleanup();
    }
  });

  it("无 junction 但清单将 claude_projects_dir 声明进假受保护目录时同样拒绝", () => {
    const sandbox = createRunSandbox("manifest-pointer");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    const realProjects = sandbox.dir("home", ".claude", "projects");
    const sentinel = sandbox.file("home", ".claude", "projects", "session.jsonl", "REAL\n");
    sandbox.dir("home", "workspace", "sources", "claude", "projects");
    writeManifest(
      sandbox,
      root,
      acceptanceManifest(root, { overrides: { claude_projects_dir: realProjects } }),
    );
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/受保护目录/);
      expect(fs.readFileSync(sentinel, "utf8")).toBe("REAL\n");
    } finally {
      sandbox.cleanup();
    }
  });

  it("清单里的每个 files 条目也参与解析：指向受保护目录即拒绝", () => {
    const sandbox = createRunSandbox("manifest-files");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    sandbox.dir("home", "workspace", "sources", "claude", "projects");
    const smuggled = sandbox.file("home", ".tokenscope", "settings.toml", "[real]");
    const manifest = acceptanceManifest(root);
    manifest.files = { [smuggled]: { path: smuggled, bytes: 7, sha256: "0".repeat(64) } };
    writeManifest(sandbox, root, manifest);
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/受保护目录/);
    } finally {
      sandbox.cleanup();
    }
  });

  it("断链 junction 无法解析真实路径时 fail closed", () => {
    const sandbox = createRunSandbox("broken-link");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    const target = path.join(sandbox.root, "gone");
    sandbox.dir("home", "workspace", "sources", "claude");
    const brokenLink = sandbox.link(path.join(root, "sources", "claude", "projects"), target);
    fs.mkdirSync(target, { recursive: true });
    fs.rmdirSync(target);
    // 沙箱清理对断链链接拒绝递归删除（无法证明解析在沙箱内）——本测试自行
    // 拆除现场：先 unlink 断链，再清沙箱。
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/无法解析|不存在/);
    } finally {
      fs.unlinkSync(brokenLink);
      fs.rmSync(sandbox.root, { recursive: true, force: true });
    }
  });

  it("根内文件与根外文件的真实硬链接（nlink>1）：预检保守拒绝，不是已证明独立的普通文件", () => {
    // 八轮复核问题 3（先红）：NTFS 真实硬链接（linkSync，同 inode、nlink=2）
    // 让验收实例可经根内文件读写根外数据——nlink>1 的普通文件不能当作已
    // 证明独立的普通文件放行。外部哨兵由本测试自建自拆。
    const outside = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "ac10-hardlink-out-"));
    const outsideFile = path.join(outside, "sentinel.txt");
    fs.writeFileSync(outsideFile, "REAL USER DATA", "utf8");
    const sandbox = createRunSandbox("hardlink");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    sandbox.dir("home", "workspace", "tokenscope");
    sandbox.dir("home", "workspace", "sources", "claude", "projects");
    const linked = path.join(root, "tokenscope", "linked.db");
    fs.linkSync(outsideFile, linked);
    writeManifest(sandbox, root, acceptanceManifest(root));
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/硬链接|nlink|链接|拒绝启动/);
      // 外部哨兵原样保留（预检不写不删）
      expect(fs.readFileSync(outsideFile, "utf8")).toBe("REAL USER DATA");
    } finally {
      fs.rmSync(linked, { force: true });
      fs.rmSync(outside, { recursive: true, force: true });
      sandbox.cleanup();
    }
  });

  it("受控绑定（真实合成 ZIP + 完整工作流身份）：构建 step 成功 + 四方哈希一致才 verified", () => {
    const exeBytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const localSha = sha256(exeBytes);
    const record = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: path.resolve("C:/build/target/release/tokenscope.exe"),
      artifact_sha256: localSha,
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "b49f535abcdef1234567890abcdef1234567890",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: true,
      builder: "ci",
      ci: { run_id: "12345", repository: "ywltby/tokenscope", run_attempt: "1" },
    };
    // GitHub 实际响应形态：run 顶层 name=工作流名、workflow_id=工作流身份、
    // repository.full_name=仓库；job 有 run_id 字段；构建是 native-ui 内 step。
    const runMetadata = {
      id: "12345",
      name: "Native UI E2E",
      workflow_id: 777,
      head_sha: "b49f535abcdef1234567890abcdef1234567890",
      conclusion: null,
      repository: { full_name: "ywltby/tokenscope" },
    };
    const jobList = [
      { name: "native-ui", run_id: 12345, steps: [ { name: "Build embedded frontend and isolated acceptance executable", conclusion: "success" } ] },
    ];
    const archiveBytes = makeStoredZip([["tokenscope.exe", exeBytes]]);
    const artifacts = [
      { name: "acceptance-build-b49f535", digest: `sha256:${sha256(archiveBytes)}`, workflow_run: { id: "12345" } },
    ];
    const ok = verifyBuildOriginBinding({
      record,
      runMetadata,
      jobList,
      artifacts,
      trustedRepository: "ywltby/tokenscope",
      trustedWorkflowName: "Native UI E2E",
      trustedWorkflowId: 777,
      trustedHeadSha: "b49f535abcdef1234567890abcdef1234567890",
      archiveBytes,
      localExeBytes: exeBytes,
    });
    expect(ok).toMatchObject({ ok: true, build_identity: "verified" });
    // 预检本体仍无 verified 出口：绑定核验是显式受控步骤，未接线
    const preflight = verifyAcceptanceExecutable(record.artifact_path, {
      readFile: () => exeBytes,
      buildRecord: parseAcceptanceBuildRecord(record),
      sourceCommit: record.source_commit,
    });
    expect(preflight.build_identity).toBe("unverified");
  });

  it("受控绑定反例（完整工作流身份）：相同显示名不同 workflow_id、不同 run 的 job、非预期仓库/提交 → 全部 unverified", () => {
    const exeB = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const record = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: path.resolve("C:/b.exe"),
      artifact_sha256: sha256(exeB),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "b49f535abcdef1234567890abcdef1234567890",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: true,
      builder: "ci",
      ci: { run_id: "12345", repository: "ywltby/tokenscope", run_attempt: "1" },
    };
    const runMetadata = {
      id: "12345",
      name: "Native UI E2E",
      workflow_id: 777,
      head_sha: "b49f535abcdef1234567890abcdef1234567890",
      repository: { full_name: "ywltby/tokenscope" },
    };
    const zip = makeStoredZip([["tokenscope.exe", exeB]]);
    const goodJobs = [
      { name: "native-ui", run_id: 12345, steps: [ { name: "Build embedded frontend and isolated acceptance executable", conclusion: "success" } ] },
    ];
    const goodArtifacts = [
      { name: "acceptance-build-b49f535", digest: `sha256:${sha256(zip)}`, workflow_run: { id: "12345" } },
    ];
    const cases = [
      {
        name: "相同显示名、不同工作流身份（workflow_id 888 vs 777）",
        runMetadata,
        trustedWorkflowId: 888,
        expect: /相同显示名的工作流不是同一身份/,
      },
      {
        name: "缺受信工作流 ID",
        runMetadata,
        trustedWorkflowId: null,
        expect: /缺少受信工作流 ID/,
      },
      {
        name: "job 来自另一次 run（run_id 99999 vs 12345）",
        jobList: [ { name: "native-ui", run_id: 99999, steps: goodJobs[0].steps } ],
        expect: /该 job 来自另一次 run/,
      },
      {
        name: "job 缺 run_id 字段",
        jobList: [ { name: "native-ui", steps: goodJobs[0].steps } ],
        expect: /run_id.*缺失|来自另一次 run/,
      },
      {
        name: "非预期仓库（记录与响应内部一致也拒绝）",
        runMetadata: { ...runMetadata, repository: { full_name: "other/repo" } },
        trustedRepository: "other/trusted",
        expect: /与受信仓库.*不一致/,
      },
      {
        name: "非预期受测提交（记录与响应内部一致也拒绝）",
        runMetadata: { ...runMetadata, head_sha: "2222222222222222222222222222222222222222" },
        trustedHeadSha: "3333333333333333333333333333333333333333",
        expect: /与受信受测提交.*不一致/,
      },
    ];
    for (const c of cases) {
      const r = verifyBuildOriginBinding({
        record,
        runMetadata: c.runMetadata ?? runMetadata,
        jobList: c.jobList ?? goodJobs,
        artifacts: goodArtifacts,
        trustedRepository: c.trustedRepository ?? "ywltby/tokenscope",
        trustedWorkflowName: "Native UI E2E",
        trustedWorkflowId: Object.prototype.hasOwnProperty.call(c, "trustedWorkflowId") ? c.trustedWorkflowId : 777,
        trustedHeadSha: c.trustedHeadSha ?? "b49f535abcdef1234567890abcdef1234567890",
        archiveBytes: zip,
        localExeBytes: exeB,
      });
      expect(r.ok, c.name).toBe(false);
      expect(r.build_identity, c.name).toBe("unverified");
      expect(r.reason, c.name).toMatch(c.expect);
    }
  });

  it("ZIP 边界（真实合成 ZIP 字节）：CD/local 名不一致、越界、长度不一致、deflate 均安全处理", () => {
    const crc32 = (() => {
      const t = new Int32Array(256);
      for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c; }
      return (b) => { let c = -1; for (const x of b) c = (c >>> 8) ^ t[(c ^ x) & 0xff]; return (c ^ -1) >>> 0; };
    })();
    const crc32Of = crc32;
    const mkEntry = (name, content, opts = {}) => {
      const nb = Buffer.from(name);
      const nb2 = Buffer.from(opts.localName ?? name);
      const crc = opts.crc ?? crc32(content);
      const l = Buffer.alloc(30);
      l.writeUInt32LE(0x04034b50, 0);
      l.writeUInt16LE(20, 4);
      l.writeUInt16LE(opts.method ?? 0, 8);
      l.writeUInt32LE(opts.localCompressedSize ?? opts.compressedSize ?? content.length, 18);
      l.writeUInt32LE(opts.localUncompressedSize ?? content.length, 22);
      l.writeUInt16LE(nb2.length, 26);
      const c = Buffer.alloc(46);
      c.writeUInt32LE(0x02014b50, 0);
      c.writeUInt16LE(opts.method ?? 0, 10);
      c.writeUInt32LE(opts.crc ?? crc32(content), 16);
      c.writeUInt32LE(opts.compressedSize ?? content.length, 20);
      c.writeUInt32LE(opts.uncompressedSize ?? content.length, 24);
      c.writeUInt16LE(nb.length, 28);
      c.writeUInt32LE(0, 42);
      return { local: Buffer.concat([l, nb2, opts.localData ?? content]), central: Buffer.concat([c, nb]) };
    };
    const buildZip = (entries, overrides = {}) => {
      const locals = [], centrals = [];
      let off = 0;
      for (const e of entries) { locals.push(e.local); centrals.push(e.central); off += e.local.length; }
      const cdSize = centrals.reduce((a, b) => a + b.length, 0);
      const eocd = Buffer.alloc(22);
      eocd.writeUInt32LE(0x06054b50, 0);
      eocd.writeUInt16LE(entries.length, 8);
      eocd.writeUInt16LE(entries.length, 10);
      eocd.writeUInt32LE(overrides.cdSize ?? cdSize, 12);
      eocd.writeUInt32LE(overrides.cdOffset ?? off, 16);
      return Buffer.concat([...locals, ...centrals, eocd]);
    };
    // 正例：stored 可读
    const zipOK = buildZip([mkEntry("tokenscope.exe", Buffer.from("EXE-B"))]);
    expect(readZipEntryBytes(zipOK, "tokenscope.exe").bytes.toString()).toBe("EXE-B");
    // 正例：deflate 可读（uncompressedSize=原文长度）
    const orig = Buffer.from("EXE-DEFLATED");
    const deflated = zlib.deflateRawSync(orig);
    const zipDef = buildZip([mkEntry("a.exe", orig, { method: 8, compressedSize: deflated.length, localData: deflated })]);
    const rDef = readZipEntryBytes(zipDef, "a.exe");
    expect(rDef.ok).toBe(true);
    expect(rDef.bytes.toString()).toBe("EXE-DEFLATED");
    // 反例 1：中央目录名与本地头名不一致
    const zipMismatch = buildZip([mkEntry("tokenscope.exe", Buffer.from("EXE-B"), { localName: "other-name.exe" })]);
    const rMismatch = readZipEntryBytes(zipMismatch, "tokenscope.exe");
    expect(rMismatch.ok).toBe(false);
    expect(rMismatch.reason).toMatch(/与本地头名.*不一致|归档被拼接/);
    // 反例 2：中央目录偏移越界
    const zipCdOff = buildZip([mkEntry("tokenscope.exe", Buffer.from("EXE-B"))], { cdOffset: 0xfffffff0 });
    const rCdOff = readZipEntryBytes(zipCdOff, "tokenscope.exe");
    expect(rCdOff.ok).toBe(false);
    expect(rCdOff.reason).toMatch(/中央目录区间越界/);
    // 反例 3：中央目录大小越界
    const zipCdSize = buildZip([mkEntry("tokenscope.exe", Buffer.from("EXE-B"))], { cdSize: 0xffff0000 });
    const rCdSize = readZipEntryBytes(zipCdSize, "tokenscope.exe");
    expect(rCdSize.ok).toBe(false);
    expect(rCdSize.reason).toMatch(/中央目录区间越界/);
    // 反例 4：条目 compressedSize 越界（数据区间超出归档）
    const cdOff = zipOK.readUInt32LE(zipOK.length - 22 + 16);
    const zipCompressed = Buffer.from(zipOK);
    zipCompressed.writeUInt32LE(0xffff0000, cdOff + 20);
    const rCompressed = readZipEntryBytes(zipCompressed, "tokenscope.exe");
    expect(rCompressed.ok).toBe(false);
    // 十四轮复核后本地头/中央目录大小一致性检查先于数据区间越界触发（两条防线都拒绝）
    expect(rCompressed.reason).toMatch(/数据区间越界|与中央目录.*不一致/);
    // 反例 5：条目 localOffset 越界
    const zipLocalOff = Buffer.from(zipOK);
    zipLocalOff.writeUInt32LE(0xffff0000, cdOff + 42);
    const rLocalOff = readZipEntryBytes(zipLocalOff, "tokenscope.exe");
    expect(rLocalOff.ok).toBe(false);
    // 反例 6：解压后长度与登记 uncompressedSize 不一致
    const zipLenMismatch = buildZip([mkEntry("a.exe", orig, { method: 8, compressedSize: deflated.length, uncompressedSize: orig.length + 7, localData: deflated })]);
    const rLenMismatch = readZipEntryBytes(zipLenMismatch, "a.exe");
    expect(rLenMismatch.ok).toBe(false);
    // 十四轮复核后本地头一致性检查可能先触发（两条防线都拒绝）
    expect(rLenMismatch.reason).toMatch(/解压后长度.*不一致|内容损坏|与中央目录.*不一致/);
    // 反例 7：重复条目名
    const zipDup = makeStoredZip([["tokenscope.exe", Buffer.from("A")], ["tokenscope.exe", Buffer.from("B")]]);
    const rDup = readZipEntryBytes(zipDup, "tokenscope.exe");
    expect(rDup.ok).toBe(false);
    expect(rDup.reason).toMatch(/重复条目/);
    // 反例 8：归档内没有登记条目名
    const zipOther = makeStoredZip([["other.txt", Buffer.from("x")]]);
    expect(readZipEntryBytes(zipOther, "tokenscope.exe").ok).toBe(false);
    // 反例 9（十四轮复核）：登记未压缩大小为 0，条目实际有非空内容
    const zipZeroUncompressed = buildZip([
      mkEntry("tokenscope.exe", Buffer.from("EXE-B"), {
        uncompressedSize: 0,
        compressedSize: 5,
        localUncompressedSize: 0,
      }),
    ]);
    const rZero = readZipEntryBytes(zipZeroUncompressed, "tokenscope.exe");
    expect(rZero.ok).toBe(false);
    expect(rZero.reason).toMatch(/解压后长度 5 与登记 0 不一致/);
    // 反例 10（十四轮复核）：条目内容与登记 CRC 不一致
    const zipCrcMismatch = buildZip([
      mkEntry("tokenscope.exe", Buffer.from("EXE-B"), { crc: crc32Of(Buffer.from("OTHER")) }),
    ]);
    const rCrc = readZipEntryBytes(zipCrcMismatch, "tokenscope.exe");
    expect(rCrc.ok).toBe(false);
    expect(rCrc.reason).toMatch(/CRC.*不一致/);
    // 反例 11（十四轮复核）：中央目录登记压缩大小为 0（短于条目），实际包含数据
    const zipCdZero = buildZip([
      mkEntry("tokenscope.exe", Buffer.from("EXE-B"), { compressedSize: 0 }),
    ]);
    const rCdZero = readZipEntryBytes(zipCdZero, "tokenscope.exe");
    expect(rCdZero.ok).toBe(false);
    expect(rCdZero.reason).toMatch(/解压后长度 0 与登记 5 不一致/);
    // 反例 12（十四轮复核）：本地头与中央目录大小不一致（拼接归档的另一种形态）
    const zipLocalMismatch = buildZip([
      mkEntry("tokenscope.exe", Buffer.from("EXE-B"), {
        compressedSize: 5,
        localCompressedSize: 99,
      }),
    ]);
    const rLocalMismatch = readZipEntryBytes(zipLocalMismatch, "tokenscope.exe");
    expect(rLocalMismatch.ok).toBe(false);
    expect(rLocalMismatch.reason).toMatch(/本地头大小.*与中央目录.*不一致/);
  });

  it("普通目录 nlink=2 也必须继续遍历：内含外部 junction 的反例不被漏掉", () => {
    // NTFS 目录 nlink 恒 1，无法在磁盘上真实造出 nlink=2 的目录——用注入的
    // lstatSync 模拟"报 nlink>1 的普通目录"（ReFS/网络盘/未来行为）。修复前
    // 该目录被当"硬链接"跳过，其中的外部 junction 漏检 → 反例红。
    const outside = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "ac10-nlink-out-"));
    const sentinel = path.join(outside, "keep.txt");
    fs.writeFileSync(sentinel, "must survive", "utf8");
    const sandbox = createRunSandbox("nlink2");
    const fakeHome = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    const normalDir = sandbox.dir("home", "workspace", "normal-nlink2");
    sandbox.link(path.join(normalDir, "escape"), outside);
    const realLstat = fs.lstatSync.bind(fs);
    const fakeLstat = (p, opts) => {
      const s = realLstat(p, opts);
      if (path.resolve(String(p)) === path.resolve(normalDir)) {
        // 模拟非常规文件系统：普通目录但 nlink=2（保留 Stats 原型方法）
        const copy = Object.create(Object.getPrototypeOf(s));
        Object.assign(copy, s);
        copy.nlink = 2;
        return copy;
      }
      return s;
    };
    try {
      // 走查：nlink=2 的普通目录必须被遍历（links 里出现 escape）
      const tree = collectAcceptanceTreePaths(root, { lstatSync: fakeLstat });
      expect(tree.links.map((link) => path.basename(link))).toContain("escape");
      // 完整预检：同样抓到该 junction 并拒绝启动
      const result = validateAcceptancePaths({ root, homeDir: fakeHome, collectTree: (r) => collectAcceptanceTreePaths(r, { lstatSync: fakeLstat }) });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/指向隔离根之外|受保护目录|验收路径/);
      // 外部哨兵原样保留
      expect(fs.readFileSync(sentinel, "utf8")).toBe("must survive");
    } finally {
      fs.rmSync(path.join(normalDir, "escape"), { force: true });
      fs.rmSync(outside, { recursive: true, force: true });
      sandbox.cleanup();
    }
  });

  it("固定布局项与树走查链接同路径时链接身份不丢失：sources/claude 指向根外被拒", () => {
    // 七轮复核缺口 1 回归（先红）：sources/claude 同时命中固定布局条目与树
    // 走查链接——旧去重保留首个（固定布局）条目，链接身份丢失，根内校验被
    // 绕过；修复后链接语义优先，真实目标在根外即拒绝。含可解析 manifest。
    const outside = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "ac10-dedup-out-"));
    const sentinel = path.join(outside, "sessions", "s.jsonl");
    fs.mkdirSync(path.dirname(sentinel), { recursive: true });
    fs.writeFileSync(sentinel, "REAL\n", "utf8");
    const sandbox = createRunSandbox("dedup-link");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    for (const entry of getAcceptancePathLayout(root)) {
      if (entry.kind === "dir" && !entry.path.endsWith(root)) fs.mkdirSync(entry.path, { recursive: true });
    }
    // 固定布局项 sources/claude 本身是 junction → 根外哨兵目录
    fs.rmdirSync(path.join(root, "sources", "claude"), { recursive: true, force: true });
    sandbox.link(path.join(root, "sources", "claude"), outside);
    // 可解析 manifest（data_dir 指向根内真实目录）
    writeManifest(sandbox, root, acceptanceManifest(root));
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/指向隔离根之外/);
      // 走查确实把 sources/claude 识别为链接（不是固定布局漏检）
      const tree = collectAcceptanceTreePaths(root);
      expect(tree.links.map((link) => path.basename(link))).toContain("claude");
      // 外部哨兵原样保留
      expect(fs.readFileSync(sentinel, "utf8")).toBe("REAL\n");
    } finally {
      fs.rmSync(path.join(root, "sources", "claude"), { force: true });
      fs.rmSync(outside, { recursive: true, force: true });
      sandbox.cleanup();
    }
  });

  it("走查超过条目预算时 fail closed，不把没走完当成没问题", () => {
    const sandbox = createRunSandbox("budget");
    const root = sandbox.dir("home", "workspace");
    for (let i = 0; i < 6; i += 1) sandbox.dir("home", "workspace", `d${i}`, "leaf");
    try {
      const limited = collectAcceptanceTreePaths(root, { maxEntries: 3 });
      expect(limited.ok).toBe(false);
      expect(limited.reason).toMatch(/超过/);
      const result = validateAcceptancePaths({
        root,
        homeDir: sandbox.dir("home"),
        treeBudget: 3,
      });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/超过/);
    } finally {
      sandbox.cleanup();
    }
  });
});
});

describe("sourceCommit 只从 .git 文件读出（不 spawn git）", () => {
  const FULL = "abcdef1234567890abcdef1234567890abcdef12";

  it("普通仓库：HEAD 指向 refs/heads/<branch>", () => {
    const sandbox = createRunSandbox("git-loose");
    try {
      sandbox.file(".git", "HEAD", "ref: refs/heads/feat/consolidated-delivery\n");
      sandbox.file(".git", "refs", "heads", "feat", "consolidated-delivery", `${FULL}\n`);
      expect(readGitHeadCommit(sandbox.root)).toBe(FULL);
    } finally {
      sandbox.cleanup();
    }
  });

  it("分离 HEAD 与 packed-refs 都能读出，非法内容返回 null", () => {
    const sandbox = createRunSandbox("git-packed");
    try {
      sandbox.file(".git", "HEAD", `${FULL.toUpperCase()}\n`);
      expect(readGitHeadCommit(sandbox.root)).toBe(FULL);
      sandbox.file(".git", "HEAD", "ref: refs/heads/main\n");
      sandbox.file(".git", "packed-refs", `# pack-refs with: peeled\n${FULL} refs/heads/main\n`);
      expect(readGitHeadCommit(sandbox.root)).toBe(FULL);
      sandbox.file(".git", "HEAD", "ref: refs/heads/nope\n");
      expect(readGitHeadCommit(sandbox.root)).toBe(null);
      sandbox.file(".git", "HEAD", "not a ref at all\n");
      expect(readGitHeadCommit(sandbox.root)).toBe(null);
    } finally {
      sandbox.cleanup();
    }
  });

  it("链接工作区：.git 文件 + commondir 指向公共 refs", () => {
    const sandbox = createRunSandbox("git-worktree");
    try {
      const worktreeDir = path.join(".git", "worktrees", "n12");
      sandbox.file(
        ...worktreeDir.split(path.sep),
        "HEAD",
        "ref: refs/heads/test/n12-ac10-single-instance\n",
      );
      // 链接工作区的 commondir 在 worktrees/<name> 里，指回公共 .git
      sandbox.file(...worktreeDir.split(path.sep), "commondir", `${path.join("..", "..", "")}\n`);
      sandbox.file(
        ".git",
        "refs",
        "heads",
        "test",
        "n12-ac10-single-instance",
        `${FULL}\n`,
      );
      const repo = sandbox.dir("worktree-checkout");
      sandbox.file("worktree-checkout", ".git", `gitdir: ${path.join(sandbox.root, ...worktreeDir.split(path.sep))}\n`);
      expect(readGitHeadCommit(repo)).toBe(FULL);
    } finally {
      sandbox.cleanup();
    }
  });
});

describe("manifest 正例走真实路径解析实现", () => {
  it("有效清单 + 常规文件 manifest.json：真实解析通过且清单声明路径被核对", () => {
    const sandbox = createRunSandbox("manifest-positive");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    for (const entry of getAcceptancePathLayout(root)) {
      if (entry.kind === "dir") fs.mkdirSync(entry.path, { recursive: true });
    }
    const fixtureFile = sandbox.file(
      "home",
      "workspace",
      "sources",
      "claude",
      "projects",
      PROJECT_SLUG,
      "session.jsonl",
      '{"type":"assistant"}\n',
    );
    const manifest = acceptanceManifest(root);
    manifest.files = { [fixtureFile]: { path: fixtureFile, bytes: 20, sha256: "1".repeat(64) } };
    const manifestPath = writeManifest(sandbox, root, manifest);
    try {
      // 不注入 canonicalize：由真实 canonicalizeAcceptancePath 解析常规文件
      expect(canonicalizeAcceptancePath(manifestPath, { expect: "file" })).toMatchObject({
        ok: true,
        path: path.resolve(manifestPath),
      });
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(true);
      expect(result.manifest_present).toBe(true);
      expect(result.manifest_declared).toBe(
        collectManifestAcceptancePaths(manifest).entries.length,
      );
      expect(result.write_paths).toContain(path.resolve(manifestPath));
      expect(result.write_paths).toContain(path.resolve(fixtureFile));
    } finally {
      sandbox.cleanup();
    }
  });

  it('manifest.json 是目录时被拒（不能靠"没有清单也能过"冒充该正例）', () => {
    const sandbox = createRunSandbox("manifest-is-dir");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    for (const entry of getAcceptancePathLayout(root)) {
      if (entry.kind === "dir") fs.mkdirSync(entry.path, { recursive: true });
    }
    fs.mkdirSync(path.join(root, "manifest.json"), { recursive: true });
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/不是常规文件|无法解析/);
    } finally {
      sandbox.cleanup();
    }
  });

  it("布局里声明为目录的路径实际是常规文件时拒绝", () => {
    const sandbox = createRunSandbox("layout-is-file");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    sandbox.dir("home", "workspace", "sources");
    sandbox.file("home", "workspace", "sources", "claude", "not a directory");
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/不是目录/);
    } finally {
      sandbox.cleanup();
    }
  });

  it("清单损坏/声明相对路径时 fail closed", () => {
    const sandbox = createRunSandbox("manifest-bad");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    sandbox.dir("home", "workspace", "sources", "claude");
    sandbox.file("home", "workspace", "manifest.json", "{ not json");
    try {
      const result = validateAcceptancePaths({ root, homeDir: home });
      expect(result.ok).toBe(false);
      expect(result.reason).toMatch(/不是合法 JSON/);
      sandbox.file(
        "home",
        "workspace",
        "manifest.json",
        JSON.stringify({ data_dir: "tokenscope" }),
      );
      const relative = validateAcceptancePaths({ root, homeDir: home });
      expect(relative.ok).toBe(false);
      expect(relative.reason).toMatch(/非绝对路径/);
    } finally {
      sandbox.cleanup();
    }
  });

  it("全新空根（没有清单）是独立的一种通过，不代替清单正例", () => {
    const sandbox = createRunSandbox("manifest-absent");
    const home = sandbox.dir("home");
    const root = sandbox.dir("home", "workspace");
    try {
      const result = validateAcceptancePaths({
        root,
        homeDir: home,
        readFile: (target) => {
          if (String(target).endsWith(".exe")) throw new Error("不应读取 EXE");
          return missingFile(target);
        },
      });
      expect(result.ok).toBe(true);
      expect(result.manifest_present).toBe(false);
      expect(result.manifest_declared).toBe(0);
    } finally {
      sandbox.cleanup();
    }
  });

  it("collectManifestAcceptancePaths 只取绝对路径声明并区分目录/文件", () => {
    const collected = collectManifestAcceptancePaths({
      data_dir: "C:/acc/tokenscope",
      webview_data_dir: "C:/acc/webview",
      price_auto_sync: false,
      env_var: { name: "TOKENSCOPE_ACCEPTANCE_ROOT", value: "C:/acc" },
      files: { "C:/acc/a.jsonl": { path: "C:/acc/a.jsonl", sha256: "x" }, "C:/acc/b.jsonl": "C:/acc/b.jsonl" },
    });
    expect(collected.invalid).toEqual([]);
    expect(collected.entries.map((e) => e.kind)).toEqual(["dir", "dir", "file", "file"]);
    expect(collectManifestAcceptancePaths({ data_dir: "relative/dir" }).invalid.length).toBe(1);
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

  it("普通 PE 追加 acceptance 标记且不含 stub：标记只预筛，仍不允许启动", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const result = verifyAcceptanceExecutable("normal-with-appended.exe", {
      readFile: () => bytes,
    });
    expect(result.ok).toBe(false);
    expect(result.launch_allowed).toBe(false);
    expect(result.marker_screened).toBe(true);
    expect(result.build_identity).toBe("unverified");
    expect(result.reason).toMatch(/受控构建记录/);
  });

  it("含普通构建 stub 标记时直接判 rejected", () => {
    const result = verifyAcceptanceExecutable("normal.exe", {
      readFile: () => peWith(`${NORMAL_ACCEPTANCE_STUB_MARKER} ${ACCEPTANCE_EXE_MARKERS[0]}`),
    });
    expect(result.ok).toBe(false);
    expect(result.launch_allowed).toBe(false);
    expect(result.build_identity).toBe("rejected");
    expect(result.negative_markers.length).toBeGreaterThan(0);
  });

  it("伪造 CI 环境 + CI 记录 + 带标记普通 PE：不能放行（自报字段互证不构成来源证明）", () => {
    // 七轮复核回归（先红）：GITHUB_ACTIONS、run ID、repository 与记录全部
    // 可由本地进程填写，此前"builder=ci + CI 环境即 verified"的门被这组
    // 自报字段骗过。verified 出口已移除——无论字段如何组合，结论只能是
    // unverified（预检拒绝启动），PE 副本不被执行。
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const exePath = path.resolve("C:/build/target/release/tokenscope.exe");
    const forgedCi = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: exePath,
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "b49f535abcdef1234567890abcdef1234567890",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: true,
      builder: "ci",
      ci: { run_id: "12345", repository: "ywltby/tokenscope", run_attempt: "1" },
    };
    for (const env of [{ GITHUB_ACTIONS: "true" }, {}, undefined]) {
      const result = verifyAcceptanceExecutable(exePath, {
        readFile: () => bytes,
        buildRecord: forgedCi,
        sourceCommit: forgedCi.source_commit,
        ...(env === undefined ? {} : { buildEnvironment: env }),
      });
      expect(result.ok).toBe(false);
      expect(result.launch_allowed).toBe(false);
      expect(result.build_identity).toBe("unverified");
      expect(result.reason).toMatch(/无法独立证明产物来源/);
    }
  });

  it("本地记录（builder=local）即使字段全真也无法独立证明来源：unverified 拒绝", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const exePath = path.resolve("C:/build/target/release/tokenscope.exe");
    // 伪造干净记录：tree_clean=true、哈希/命令/提交一致——本地无法辨伪。
    const localRecord = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: exePath,
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "b49f535abcdef1234567890abcdef1234567890",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: true,
      builder: "local",
    };
    const r1 = verifyAcceptanceExecutable(exePath, {
      readFile: () => bytes,
      buildRecord: localRecord,
      sourceCommit: localRecord.source_commit,
    });
    expect(r1.ok).toBe(false);
    expect(r1.launch_allowed).toBe(false);
    expect(r1.build_identity).toBe("unverified");
    expect(r1.reason).toMatch(/无法独立证明产物来源/);
  });

  it("旧格式记录（无 builder/ci 段）parse 通过（builder/ci 为可选诊断字段）", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const legacy = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: path.resolve("C:/build/tokenscope.exe"),
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "abcdef1234567890abcdef1234567890abcdef12",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: true,
    };
    const parsed = parseAcceptanceBuildRecord(JSON.stringify(legacy));
    expect(parsed.ok).toBe(true);
    // 形状通过也不授予 verified——身份门仍拒绝
    const result = verifyAcceptanceExecutable(legacy.artifact_path, {
      readFile: () => bytes,
      buildRecord: parsed,
      sourceCommit: legacy.source_commit,
    });
    expect(result.ok).toBe(false);
    expect(result.build_identity).toBe("unverified");
  });

  it("builder=ci 但 ci 段缺 run_id/repository：parse 拒绝（保留登记契约供未来 CI 核验）", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const broken = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: path.resolve("C:/build/tokenscope.exe"),
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "abcdef1234567890abcdef1234567890abcdef12",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: true,
      builder: "ci",
      ci: { run_id: "", repository: "" },
    };
    const parsed = parseAcceptanceBuildRecord(JSON.stringify(broken));
    expect(parsed.ok).toBe(false);
    expect(parsed.reason).toMatch(/ci 段/);
  });

  it("记录缺 tree_clean（旧格式/手写）一律拒绝——来源证明字段是硬性要求", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const legacy = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: path.resolve("C:/build/target/release/tokenscope.exe"),
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "abcdef1234567890abcdef1234567890abcdef12",
      features: ["acceptance", "tauri/custom-protocol"],
    };
    const parsed = parseAcceptanceBuildRecord(JSON.stringify(legacy));
    expect(parsed.ok).toBe(false);
    expect(parsed.reason).toMatch(/tree_clean/);
    const result = verifyAcceptanceExecutable(legacy.artifact_path, {
      readFile: () => bytes,
      buildRecord: JSON.parse(JSON.stringify(legacy)),
      sourceCommit: legacy.source_commit,
    });
    expect(result.ok).toBe(false);
    expect(result.launch_allowed).toBe(false);
    expect(result.build_identity).toBe("unverified");
  });

  it("脏树记录不足以证明来源：unverified 并拒绝启动（无法从提交复现产物）", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const exePath = path.resolve("C:/build/target/release/tokenscope.exe");
    const dirty = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: exePath,
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "abcdef1234567890abcdef1234567890abcdef12",
      features: ["acceptance", "tauri/custom-protocol"],
      tree_clean: false,
      tree_dirty_count: 3,
      builder: "ci",
      ci: { run_id: "12345", repository: "ywltby/tokenscope", run_attempt: "1" },
    };
    const parsed = parseAcceptanceBuildRecord(JSON.stringify(dirty));
    expect(parsed.ok).toBe(true);
    const result = verifyAcceptanceExecutable(exePath, {
      readFile: () => bytes,
      buildRecord: parsed,
      sourceCommit: dirty.source_commit,
      buildEnvironment: { GITHUB_ACTIONS: "true" },
    });
    // 脏树上的产物无法仅凭提交哈希复现——来源无法独立证明 → unverified。
    expect(result.ok).toBe(false);
    expect(result.launch_allowed).toBe(false);
    expect(result.build_identity).toBe("unverified");
    expect(result.reason).toMatch(/脏树|来源/);
  });

  it("登记产物不是本次 EXE、构建命令不合法、源提交不一致都拒绝", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const good = {
      kind: ACCEPTANCE_BUILD_RECORD_KIND,
      artifact_path: path.resolve("C:/build/tokenscope.exe"),
      artifact_sha256: sha256(bytes),
      build_command: ACCEPTANCE_BUILD_COMMAND,
      source_commit: "abcdef1234567890abcdef1234567890abcdef12",
      features: ["acceptance"],
      tree_clean: true,
      builder: "ci",
      ci: { run_id: "12345", repository: "ywltby/tokenscope", run_attempt: "1" },
    };
    const other = verifyAcceptanceExecutable("C:/other/tokenscope.exe", {
      readFile: () => bytes,
      buildRecord: good,
      buildEnvironment: { GITHUB_ACTIONS: "true" },
    });
    expect(other.ok).toBe(false);
    expect(other.reason).toMatch(/不是构建记录登记的产物/);
    const wrongCommand = verifyAcceptanceBuildIdentity({
      exePath: good.artifact_path,
      record: { ...good, build_command: "cargo build -p tokenscope-app --release" },
      readFile: () => bytes,
      buildEnvironment: { GITHUB_ACTIONS: "true" },
    });
    expect(wrongCommand.ok).toBe(false);
    expect(wrongCommand.reason).toMatch(/构建命令/);
    const wrongCommit = verifyAcceptanceBuildIdentity({
      exePath: good.artifact_path,
      record: good,
      readFile: () => bytes,
      sourceCommit: "1111111111111111111111111111111111111111",
      buildEnvironment: { GITHUB_ACTIONS: "true" },
    });
    expect(wrongCommit.ok).toBe(false);
    expect(wrongCommit.reason).toMatch(/源提交/);
  });

  it("登记后被追加字节的产物（含标记的普通 EXE）哈希不一致，拒绝启动", () => {
    const original = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const tampered = Buffer.concat([original, Buffer.from("APPENDED-LATER", "utf8")]);
    const result = verifyAcceptanceExecutable("C:/build/tokenscope.exe", {
      readFile: () => tampered,
      sourceCommit: "abcdef1234567890abcdef1234567890abcdef12",
      buildEnvironment: { GITHUB_ACTIONS: "true" },
      buildRecord: {
        kind: ACCEPTANCE_BUILD_RECORD_KIND,
        artifact_path: "C:/build/tokenscope.exe",
        artifact_sha256: sha256(original),
        build_command: ACCEPTANCE_BUILD_COMMAND,
        source_commit: "abcdef1234567890abcdef1234567890abcdef12",
        features: ["acceptance"],
        tree_clean: true,
        builder: "ci",
        ci: { run_id: "12345", repository: "ywltby/tokenscope", run_attempt: "1" },
      },
    });
    expect(result.ok).toBe(false);
    expect(result.launch_allowed).toBe(false);
    expect(result.build_identity).toBe("hash-mismatch");
    expect(result.reason).toMatch(/哈希/);
  });

  it("产物被替换成普通 PE（哈希不匹配）时拒绝，即使记录字段全部合法", () => {
    // 场景：登记时是 acceptance 产物，之后 target/release/tokenscope.exe 被
    // 普通构建覆盖（无标记、含 stub 都一样）——预筛 + 哈希双层都拦。
    const accepted = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    const replaced = peWith(NORMAL_ACCEPTANCE_STUB_MARKER);
    const result = verifyAcceptanceExecutable("C:/build/tokenscope.exe", {
      readFile: () => replaced,
      sourceCommit: "abcdef1234567890abcdef1234567890abcdef12",
      buildRecord: {
        kind: ACCEPTANCE_BUILD_RECORD_KIND,
        artifact_path: "C:/build/tokenscope.exe",
        artifact_sha256: sha256(accepted),
        build_command: ACCEPTANCE_BUILD_COMMAND,
        source_commit: "abcdef1234567890abcdef1234567890abcdef12",
        features: ["acceptance", "tauri/custom-protocol"],
        tree_clean: true,
      },
    });
    expect(result.ok).toBe(false);
    expect(result.launch_allowed).toBe(false);
    expect(result.negative_markers).toContain(NORMAL_ACCEPTANCE_STUB_MARKER);
    expect(result.build_identity).toBe("rejected");
  });

  it("不完整/损坏的构建记录一律 unverified，不授予启动许可", () => {
    const bytes = peWith(ACCEPTANCE_EXE_MARKERS.join(" "));
    for (const broken of [
      { kind: "other", artifact_path: "C:/b.exe", artifact_sha256: sha256(bytes), build_command: ACCEPTANCE_BUILD_COMMAND, source_commit: "abcdef1234567890", features: ["acceptance"], tree_clean: true },
      { kind: ACCEPTANCE_BUILD_RECORD_KIND, artifact_path: "C:/b.exe", artifact_sha256: "XYZ", build_command: ACCEPTANCE_BUILD_COMMAND, source_commit: "abcdef1234567890", features: ["acceptance"], tree_clean: true },
      { kind: ACCEPTANCE_BUILD_RECORD_KIND, artifact_path: "C:/b.exe", artifact_sha256: sha256(bytes), build_command: ACCEPTANCE_BUILD_COMMAND, source_commit: "abcdef1234567890", features: [], tree_clean: true },
      { kind: ACCEPTANCE_BUILD_RECORD_KIND, artifact_path: "C:/b.exe", artifact_sha256: sha256(bytes), build_command: ACCEPTANCE_BUILD_COMMAND, source_commit: "abcdef1234567890", features: ["acceptance"], tree_clean: "yes" },
      "{ not json",
      null,
    ]) {
      const result = verifyAcceptanceExecutable("C:/b.exe", { readFile: () => bytes, buildRecord: broken });
      expect(result.ok).toBe(false);
      expect(result.launch_allowed).toBe(false);
    }
  });

  it("非 PE / 空 / 不可读的 EXE 一律拒绝", () => {
    expect(
      verifyAcceptanceExecutable("text.exe", { readFile: () => Buffer.from(ACCEPTANCE_EXE_MARKERS[0]) })
        .ok,
    ).toBe(false);
    expect(verifyAcceptanceExecutable("empty.exe", { readFile: () => Buffer.alloc(0) }).ok).toBe(false);
    expect(
      verifyAcceptanceExecutable("missing.exe", {
        readFile: () => {
          throw new Error("ENOENT");
        },
      }).ok,
    ).toBe(false);
  });

  it("预检拒绝受保护根/无构建身份时 prepare 与 launch 零调用", async () => {
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

    const sandbox = createRunSandbox("preflight-no-record");
    try {
      const normalExe = evaluateAcceptanceLaunchPreflight({
        root: sandbox.dir("home", "workspace"),
        homeDir: sandbox.dir("home"),
        exePath: "normal.exe",
        canonicalize,
        readFile: (target) => {
          if (String(target).endsWith(".exe")) return peWith(NORMAL_ACCEPTANCE_STUB_MARKER);
          return missingFile(target);
        },
      });
      expect(normalExe.ok).toBe(false);
      expect(normalExe.paths.ok).toBe(true);
      expect(normalExe.exe.ok).toBe(false);
      expect(normalExe.exe.launch_allowed).toBe(false);
    } finally {
      sandbox.cleanup();
    }
  });

  it("attach 模式只校验路径，绝不读取 EXE", () => {
    const readTargets = [];
    const result = evaluateAcceptanceLaunchPreflight({
      root: ISOLATED_ROOT,
      exePath: "missing.exe",
      homeDir: os.homedir(),
      attachExisting: true,
      canonicalize,
      readFile: (target) => {
        readTargets.push(String(target));
        return missingFile(target);
      },
    });
    expect(result.ok).toBe(true);
    expect(result.exe.skipped).toBe(true);
    expect(result.exe.launch_allowed).toBe(false);
    expect(readTargets.some((target) => target.endsWith(".exe"))).toBe(false);
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
