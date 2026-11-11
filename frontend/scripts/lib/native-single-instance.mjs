// AC10（N12 单实例）纯逻辑：应用标识后缀、隔离根预检、第二次启动的
// 退出/窗口恢复/托盘观察判定。所有外部观察由调用方注入；本文件不 spawn
// 进程、不读写真实数据，便于在 node 环境做有区分度的合成测试。
import fs from "node:fs";
import crypto from "node:crypto";
import path from "node:path";

/** 第二次启动必须在限定时间内退出；超时即失败并由调用方终止已记录 PID。 */
export const SINGLE_INSTANCE_EXIT_BUDGET_MS = 15000;
/** 第二次启动退出后，第一个窗口恢复可见 + 获得前台的观察上限。 */
export const SINGLE_INSTANCE_RESTORE_BUDGET_MS = 10000;
/** 第一个实例就绪后等待 TokenScope 托盘图标出现的上限。 */
export const SINGLE_INSTANCE_TRAY_BUDGET_MS = 10000;
/** 与 src-tauri/src/acceptance.rs 的 identifier 后缀保持一致。 */
export const ACCEPTANCE_IDENTIFIER_SUFFIX = ".acceptance";

/** 验收构建的 identifier：普通实例与验收实例使用不同单实例互斥名。 */
export function acceptanceIdentifier(baseIdentifier) {
  const base = String(baseIdentifier ?? "").trim();
  if (!base) throw new Error("acceptanceIdentifier 需要非空应用标识");
  return `${base}${ACCEPTANCE_IDENTIFIER_SUFFIX}`;
}

function comparePath(value) {
  const resolved = path.resolve(value);
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

/**
 * `child` 是否等于 `parent` 或位于其下（按规范化绝对路径的分量边界）。
 * `path.relative` 可能返回 `..fixture`（子目录名以 .. 开头，但不是父目录
 * 遍历），因此必须区分 `..` / `..${sep}x` 与 `..name`。
 */
function isWithin(child, parent) {
  const rel = path.relative(comparePath(parent), comparePath(child));
  if (rel === "") return true;
  if (path.isAbsolute(rel)) return false;
  return !(rel === ".." || rel.startsWith(`..${path.sep}`) || rel.startsWith("../"));
}

/**
 * 解析路径的最终真实位置，包含尚不存在的尾部与符号链接/junction 别名。
 * 失败（权限、断链、无法解析）一律 fail closed，不把别名当成独立目录。
 * `expect`（`dir` / `file` / `any`）为声明类型：路径已存在但类型不符即拒绝，
 * 例如 manifest.json 变成目录、`sources/claude` 变成常规文件或指向文件的
 * 别名。不存在的路径允许（准备脚本随后创建）。
 */
export function canonicalizeAcceptancePath(
  target,
  {
    lstatSync = fs.lstatSync,
    realpathSync = fs.realpathSync.native ?? fs.realpathSync,
    statSync = fs.statSync,
    expect = "any",
  } = {},
) {
  const raw = String(target ?? "").trim();
  if (!raw) return { ok: false, reason: "路径为空" };
  const absolute = path.resolve(raw);
  let current = absolute;
  const tail = [];
  for (;;) {
    try {
      lstatSync(current);
      break;
    } catch (error) {
      if (error?.code !== "ENOENT") {
        return { ok: false, reason: `无法检查路径：${error?.message ?? error}` };
      }
      const parent = path.dirname(current);
      if (parent === current) {
        return { ok: false, reason: `找不到存在的祖先：${absolute}` };
      }
      tail.unshift(path.basename(current));
      current = parent;
    }
  }
  let real;
  try {
    real = realpathSync(current);
  } catch (error) {
    return { ok: false, reason: `无法解析真实路径（别名/链接/权限）：${error?.message ?? error}` };
  }
  const resolved = tail.length ? path.join(real, ...tail) : real;
  try {
    const stat = statSync(resolved);
    if (expect === "dir" && !stat.isDirectory()) {
      return { ok: false, reason: `声明为目录但实际不是目录：${resolved}` };
    }
    if (expect === "file" && !stat.isFile()) {
      return { ok: false, reason: `声明为常规文件但实际不是：${resolved}` };
    }
    if (expect !== "dir" && expect !== "file" && !stat.isFile() && !stat.isDirectory()) {
      return { ok: false, reason: `不是常规文件或目录：${resolved}` };
    }
  } catch (error) {
    if (error?.code !== "ENOENT") {
      return { ok: false, reason: `无法检查路径类型：${error?.message ?? error}` };
    }
  }
  return { ok: true, path: path.resolve(resolved), expect };
}

/**
 * 受保护根预检（单 root + 单受保护目录列表，兼容旧接口）。
 */
export function isIsolatedAcceptanceRoot(root, realDataDir) {
  const result = validateAcceptanceRoot({ root, protectedDirs: [realDataDir] });
  return { ok: result.ok, reason: result.reason };
}

/**
 * 验收构建会读写的全部受保护用户目录（基于用户主目录派生）。
 * 用户主目录无法确定时返回 null（调用方拒绝）。
 */
export function getUserProtectedDirs(homeDir) {
  const dir = String(homeDir ?? "").trim();
  if (!dir || !path.isAbsolute(dir)) return null;
  return [
    path.join(dir, ".tokenscope"),
    path.join(dir, ".claude"),
    path.join(dir, ".codex"),
    path.join(dir, ".cc-switch"),
    path.join(dir, ".workbuddy"),
    path.join(dir, ".workbuddy-ai"),
  ];
}

/**
 * 只用文件系统读取当前检出的完整提交哈希（不 spawn git，保证它仍可排在
 * 任何进程启动之前）。支持普通仓库、链接工作区（.git 文件 + commondir）与
 * packed-refs；读不出来返回 null，由调用方决定拒绝。
 */
export function readGitHeadCommit(
  repoDir,
  { readFileSync = fs.readFileSync, statSync = fs.statSync, existsSync = fs.existsSync } = {},
) {
  const hash = (value) => {
    const text = String(value ?? "").trim();
    return /^[0-9a-f]{40}$/i.test(text) ? text.toLowerCase() : null;
  };
  const fromPackedRefs = (gitDir, refName) => {
    const packed = path.join(gitDir, "packed-refs");
    if (!existsSync(packed)) return null;
    let content;
    try {
      content = readFileSync(packed, "utf8");
    } catch {
      return null;
    }
    for (const line of content.split(/\r?\n/)) {
      if (!line || line.startsWith("#") || line.startsWith("^")) continue;
      const [value, ...rest] = line.trim().split(/\s+/);
      if (rest.join("") === refName) return hash(value);
    }
    return null;
  };
  const readLoose = (base, refName) => {
    // refName 形如 refs/heads/main：直接接在 git 目录之后，不再补 refs
    const target = path.join(base, ...refName.split("/"));
    if (!existsSync(target)) return null;
    try {
      return hash(readFileSync(target, "utf8"));
    } catch {
      return null;
    }
  };

  let gitDir = path.join(repoDir, ".git");
  try {
    if (statSync(gitDir).isFile()) {
      const content = readFileSync(gitDir, "utf8");
      const match = content.match(/^\s*gitdir:\s*(.+?)\s*$/m);
      if (!match) return null;
      gitDir = path.resolve(repoDir, match[1]);
    }
  } catch {
    return null;
  }
  if (!existsSync(gitDir)) return null;

  let head;
  try {
    head = readFileSync(path.join(gitDir, "HEAD"), "utf8").trim();
  } catch {
    return null;
  }
  const detached = hash(head);
  if (detached) return detached;
  const refName = head.replace(/^ref:\s*/i, "").trim();
  if (!refName || refName.startsWith("-") || path.isAbsolute(refName)) return null;

  const commonDir = (() => {
    const pointer = path.join(gitDir, "commondir");
    if (!existsSync(pointer)) return null;
    try {
      return path.resolve(gitDir, readFileSync(pointer, "utf8").trim());
    } catch {
      return null;
    }
  })();

  return (
    readLoose(gitDir, refName) ??
    (commonDir ? readLoose(commonDir, refName) : null) ??
    fromPackedRefs(gitDir, refName) ??
    (commonDir ? fromPackedRefs(commonDir, refName) : null)
  );
}

/** 预检时展开验收根现有条目的上限；超过即无法完整核对，fail closed。 */
export const ACCEPTANCE_TREE_ENTRY_BUDGET = 5000;

/**
 * 验收根内固定会被读写的路径布局。每一项都带期望类型：`dir` 必须真的是目录、
 * `file` 必须真的是常规文件（两者都还不存在时允许，由准备脚本创建）。
 * 覆盖 crates/tokenscope-core/src/acceptance.rs 的三类覆盖点（data_dir、
 * source_dir、logs）与 scripts/prepare-native-acceptance.ps1 实际创建的全部
 * 目录层级（含 `sources/<agent>/projects`、`sources/cc-switch`）。
 */
export function getAcceptancePathLayout(root) {
  const join = (...parts) => path.join(root, ...parts);
  return [
    { path: root, kind: "dir" },
    { path: join("tokenscope"), kind: "dir" },
    { path: join("tokenscope", "logs"), kind: "dir" },
    { path: join("sources"), kind: "dir" },
    { path: join("sources", "claude"), kind: "dir" },
    { path: join("sources", "claude", "projects"), kind: "dir" },
    { path: join("sources", "codex"), kind: "dir" },
    { path: join("sources", "codex", "sessions"), kind: "dir" },
    { path: join("sources", "workbuddy"), kind: "dir" },
    { path: join("sources", "workbuddy", "projects"), kind: "dir" },
    { path: join("sources", "workbuddy-ai"), kind: "dir" },
    { path: join("sources", "workbuddy-ai", "projects"), kind: "dir" },
    { path: join("sources", "cc-switch"), kind: "dir" },
    { path: join("webview"), kind: "dir" },
    { path: join("manifest.json"), kind: "file" },
  ];
}

/**
 * 验收实例会读写的浅层路径（兼容旧接口：只返回路径字符串）。
 * 深层子路径由 getAcceptancePathLayout / 清单派生 / 目录走查共同覆盖。
 */
export function getAcceptanceWritePaths(root, outDir) {
  const paths = getAcceptancePathLayout(root).map((entry) => entry.path);
  if (outDir) paths.push(outDir);
  return paths;
}

const MANIFEST_DIR_KEY = /_dir$/;

/**
 * 从 manifest 派生预检路径：所有 `*_dir` 声明的目录与 `files` 记录的每个文件。
 * 这是"检查其他实际读写路径"的权威来源——准备脚本把每个写入文件的绝对路径
 * 都记进了清单，清单里的路径同样要过真实解析与受保护目录比对。
 */
export function collectManifestAcceptancePaths(manifest) {
  const entries = [];
  const invalid = [];
  if (!manifest || typeof manifest !== "object") {
    return { entries, invalid: ["manifest 不是对象"] };
  }
  for (const [key, value] of Object.entries(manifest)) {
    if (typeof value === "string") {
      if (MANIFEST_DIR_KEY.test(key)) entries.push({ path: value, kind: "dir", source: key });
    } else if (value && typeof value === "object" && key === "files") {
      for (const [name, record] of Object.entries(value)) {
        const declared = typeof record === "string" ? record : record?.path;
        entries.push({ path: declared ?? name, kind: "file", source: `files.${name}` });
      }
    }
  }
  for (const entry of entries) {
    if (!String(entry.path ?? "").trim()) invalid.push(`${entry.source}: 空路径`);
    else if (!path.isAbsolute(entry.path))
      invalid.push(`${entry.source}: 非绝对路径 ${entry.path}`);
  }
  return { entries, invalid };
}

/**
 * 读取隔离根里的 manifest.json。缺失=全新根（正常，由准备脚本创建）；
 * 存在但读不出/解析不了/声明了非法路径=无法确定读写面，一律 fail closed。
 */
export function readAcceptanceManifest(
  manifestPath,
  { readFile = fs.readFileSync, collect = collectManifestAcceptancePaths } = {},
) {
  let raw;
  try {
    raw = readFile(manifestPath);
  } catch (error) {
    if (error?.code === "ENOENT") {
      return { ok: true, exists: false, entries: [], invalid: [], path: manifestPath };
    }
    if (error?.code === "EISDIR") {
      return {
        ok: false,
        exists: true,
        entries: [],
        invalid: [],
        path: manifestPath,
        reason: `manifest.json 不是常规文件（目录）：${manifestPath}`,
      };
    }
    return {
      ok: false,
      exists: true,
      entries: [],
      invalid: [],
      path: manifestPath,
      reason: `manifest.json 读取失败：${String(error?.message ?? error)}`,
    };
  }
  let manifest;
  try {
    manifest = JSON.parse(Buffer.isBuffer(raw) ? raw.toString("utf8") : String(raw));
  } catch (error) {
    return {
      ok: false,
      exists: true,
      entries: [],
      invalid: [],
      path: manifestPath,
      reason: `manifest.json 不是合法 JSON：${String(error?.message ?? error)}`,
    };
  }
  const collected = collect(manifest);
  if (collected.invalid.length) {
    return {
      ok: false,
      exists: true,
      entries: collected.entries,
      invalid: collected.invalid,
      path: manifestPath,
      reason: `manifest.json 声明路径非法：${collected.invalid.join("；")}`,
    };
  }
  return { ok: true, exists: true, entries: collected.entries, invalid: [], path: manifestPath };
}

/**
 * SQLite 瞬时副产物（delete 回滚日志 / WAL 影子文件）：只在写事务期间存在、
 * 提交或检查点后由 SQLite 自己删除，文件名固定为 `<数据库名>-journal` /
 * `-wal` / `-shm`（见 sqlite3 文档）。已发现路径的 ENOENT 若属于**受控数据
 * 库的预期 sidecar**，按数据库正常生命周期豁免，不按"走查后被删除"拒绝。
 *
 * "受控数据库" = 验收实例实际打开的历史库，位置由产品代码固定：
 * `<隔离根>/tokenscope/history.db`（crates/tokenscope-core/src/history.rs
 * `history_file_path` = `data_dir()/history.db`；acceptance.rs
 * `data_dir_override` = `<root>/tokenscope`；与 prepare 脚本 manifest 的
 * `data_dir` 声明一致）。后缀相同但不在这份库旁边的文件不是它的副产物，
 * 不豁免。
 */
const TRANSIENT_DB_ARTIFACT_RE = /-(?:journal|wal|shm)$/i;

export function controlledDbSidecar(root, filePath) {
  const raw = String(filePath ?? "");
  if (!TRANSIENT_DB_ARTIFACT_RE.test(path.basename(raw))) return false;
  // data_dir 是验收实例从环境变量 canonicalize 后冻结的路径；预检入口拿到
  // 的 root 可能是短名/别名，统一走 comparePath（resolve + 小写化）比对，
  // 与条目收集和去重同一套规范化。
  const rootKey = comparePath(root);
  if (!rootKey) return false;
  const fileKey = comparePath(raw);
  if (!fileKey.startsWith(rootKey + path.sep)) return false;
  const relative = fileKey.slice(rootKey.length + path.sep.length);
  return (
    relative === path.join("tokenscope", "history.db-journal") ||
    relative === path.join("tokenscope", "history.db-wal") ||
    relative === path.join("tokenscope", "history.db-shm")
  );
}

/**
 * 豁免判定（两阶段共用）：仅当 ENOENT 且路径是受控历史库
 * `<隔离根>/tokenscope/history.db` 的预期 sidecar 时为真。
 */
export function transientDbArtifactExempted(root, code, filePath) {
  return code === "ENOENT" && controlledDbSidecar(root, filePath);
}

/**
 * 原子写临时文件的 ENOENT 豁免（2026-11-05 修复测试架竞态）。
 *
 * `fsutil::atomic_write` 的落盘顺序是"同目录临时文件 `.名称.tmp-<pid>-<序号>`
 * → rename 到目标"。树走查可能在 rename 之前发现它，等逐条 lstat 时它已经被
 * rename 掉——`ENOENT` 是这条路径的**正常生命周期**，不是"走查后被删除"的
 * 竞态或篡改。此前只豁免受控历史库的 SQLite sidecar，于是验收在 CI 上偶发
 * 失败（`.pricing-index.json.tmp-<pid>-<n>` 无法检查）。
 *
 * 收紧条件：只认**隔离根内**、文件名严格匹配该临时命名的路径；其他任何
 * "走查后发现被删"的情形仍然拒绝。
 */
const ATOMIC_WRITE_TMP_RE = /^\..+\.tmp-\d+-\d+$/;

export function atomicWriteTmpExempted(root, code, filePath) {
  if (code !== "ENOENT") return false;
  const raw = String(filePath ?? "");
  if (!ATOMIC_WRITE_TMP_RE.test(path.basename(raw))) return false;
  const rootKey = comparePath(root);
  if (!rootKey) return false;
  const fileKey = comparePath(raw);
  return fileKey.startsWith(rootKey + path.sep);
}

/**
 * WebView2 自身临时文件的 ENOENT 豁免（2026-11-12 Runner 实测竞态修复）。
 *
 * `webview\EBWebView\**\*.dbtmp` 是 WebView2/Chromium 的 LevelDB 临时文件，
 * 生命周期毫秒级：树走查发现它、等逐条 lstat 时已被浏览器进程删除。这与受控
 * 历史库 sidecar、应用原子写 tmp 同属"走查时在、lstat 时没了"的正常生命周期，
 * 不是"走查后被删除"的竞态或篡改（run 43082991197：
 * `Default\Extension Scripts\000001.dbtmp` 无法检查）。
 *
 * 收紧条件：只认**隔离根内 webview 数据目录**（`<root>\webview\`）下的
 * `.dbtmp` 后缀；webview 目录外的 dbtmp、其他文件、其他错误仍 fail closed。
 */
export function webviewDbtmpExempted(root, code, filePath) {
  if (code !== "ENOENT") return false;
  const raw = String(filePath ?? "");
  if (!raw.toLowerCase().endsWith(".dbtmp")) return false;
  const rootKey = comparePath(root);
  if (!rootKey) return false;
  const fileKey = comparePath(raw);
  if (!fileKey.startsWith(rootKey + path.sep)) return false;
  const webviewKey = comparePath(path.join(root, "webview") + path.sep);
  return fileKey.startsWith(webviewKey);
}

/**
 * 走查两阶段共用的瞬时产物豁免：受控历史库 sidecar 或应用原子写临时文件。
 */
export function transientArtifactExempted(root, code, filePath) {
  return (
    transientDbArtifactExempted(root, code, filePath) ||
    atomicWriteTmpExempted(root, code, filePath) ||
    webviewDbtmpExempted(root, code, filePath)
  );
}

/**
 * 走查隔离根现有条目：普通目录继续递归（每个目录都参与真实解析），符号链接/
 * junction 只解析不再深入（深入会走别名另一侧），硬链接（nlink>1）按可疑别名
 * 解析。条目超预算时 fail closed，不把"没走完"当成"没问题"。
 */
export function collectAcceptanceTreePaths(
  root,
  {
    lstatSync = fs.lstatSync,
    readdirSync = fs.readdirSync,
    maxEntries = ACCEPTANCE_TREE_ENTRY_BUDGET,
  } = {},
) {
  const paths = [];
  const links = [];
  let visited = 0;
  const stack = [root];
  while (stack.length) {
    const current = stack.pop();
    let names;
    try {
      names = readdirSync(current);
    } catch (error) {
      if (error?.code === "ENOENT") continue;
      return {
        ok: false,
        paths,
        links,
        reason: `验收根走查失败：${current}（${String(error?.message ?? error)}）`,
      };
    }
    for (const name of names) {
      const child = path.join(current, name);
      visited += 1;
      if (visited > maxEntries) {
        return {
          ok: false,
          paths,
          links,
          reason: `验收根条目超过 ${maxEntries} 个，无法完整核对隔离，拒绝启动`,
        };
      }
      let stat;
      try {
        stat = lstatSync(child);
      } catch (error) {
        // 与后续检查阶段同一时序（2026-11-04 第二轮）：journal 等受控数据库
        // 的瞬时 sidecar 可能在 readdir 与本 lstat 之间被 SQLite 删除——
        // ENOENT 且属于受控历史库 sidecar、或应用原子写临时文件时跳过该条目
        // （已不存在，无从核对链接/硬链接）；其余文件消失、其他读取错误仍
        // fail closed。
        if (transientArtifactExempted(root, error?.code, child)) continue;
        return {
          ok: false,
          paths,
          links,
          reason: `无法检查验收根条目 ${child}：${String(error?.message ?? error)}`,
        };
      }
      // 只把符号链接当别名跳过；普通目录无论 nlink 数值为何都必须继续
      // 遍历（NTFS 目录 nlink 恒 1，但 ReFS/网络盘/未来行为可能报 >1——
      // 按 nlink 跳过目录会把其中的外部 junction 漏掉）。
      if (stat.isSymbolicLink()) {
        links.push(child);
        continue;
      }
      if (stat.isDirectory()) {
        paths.push({ path: child, kind: "dir", source: "tree" });
        stack.push(child);
      } else if (stat.isFile()) {
        // 八轮复核问题 3：普通文件也登记（kind:"file"）——硬链接文件
        // （nlink>1）只能在文件条目上被识别，目录走查漏掉文件 = 漏掉
        // "经根内文件读写根外数据"的通道。
        paths.push({ path: child, kind: "file", source: "tree" });
      }
    }
  }
  return { ok: true, paths, links, visited };
}

/**
 * 完整路径预检：固定布局 + manifest 声明路径 + 现有条目里的每个目录/别名，
 * 逐个做真实路径解析（含 junction/symlink/硬链接），并与全部受保护用户目录
 * 做双向包含检查；用户主目录本身/其祖先同样拒绝。任一路径解析失败或超预算
 * 一律 fail closed。
 */
export function validateAcceptancePaths({
  root,
  outDir,
  homeDir,
  canonicalize = canonicalizeAcceptancePath,
  readFile = fs.readFileSync,
  collectTree = collectAcceptanceTreePaths,
  treeBudget = ACCEPTANCE_TREE_ENTRY_BUDGET,
  lstat = fs.lstatSync,
} = {}) {
  const protectedDirs = getUserProtectedDirs(homeDir);
  if (!protectedDirs) {
    return { ok: false, reason: "无法确定用户主目录，无法证明来源目录隔离，拒绝启动" };
  }
  if (!protectedDirs.length) {
    return { ok: false, reason: "受保护目录列表为空，无法证明隔离" };
  }

  const rawRoot = String(root ?? "").trim();
  if (!rawRoot) return { ok: false, reason: "隔离根为空" };
  if (!path.isAbsolute(rawRoot)) return { ok: false, reason: `隔离根不是绝对路径：${rawRoot}` };

  const layout = getAcceptancePathLayout(rawRoot);
  if (outDir) layout.push({ path: String(outDir), kind: "dir", source: "out" });

  const manifest = readAcceptanceManifest(path.join(rawRoot, "manifest.json"), { readFile });
  if (!manifest.ok) return { ok: false, reason: manifest.reason, manifest };
  for (const entry of manifest.entries) {
    layout.push({ path: entry.path, kind: entry.kind, source: entry.source });
  }

  const tree = collectTree(rawRoot, { maxEntries: treeBudget });
  if (!tree.ok) return { ok: false, reason: tree.reason, tree_entries: tree.visited ?? null };
  for (const entry of tree.paths) layout.push({ ...entry, discovered: true });
  for (const link of tree.links)
    layout.push({ path: link, kind: "any", source: "link", discovered: true });

  // 去重（同一字符串位置只解析一次），但**链接身份与"已发现"身份不因去重
  // 丢失**（七轮复核缺口 1 / 十轮复核）：
  //   - source 归并为 "link"：固定布局/manifest 条目与树走查链接同路径时，
  //     仍按链接核对真实目标；
  //   - discovered = true：树走查**实际发现了**该路径——后续 lstat ENOENT
  //     说明它在走查后被删除（竞态或篡改），必须拒绝；豁免仅适用于"从未
  //     存在、准备阶段将创建"的合法布局路径（走查未发现）。
  const unique = new Map();
  for (const entry of layout) {
    const key = comparePath(entry.path);
    const existing = unique.get(key);
    if (!existing) {
      unique.set(key, { ...entry });
      continue;
    }
    if (existing.source !== "link" && entry.source === "link") {
      existing.source = "link";
    }
    if (!existing.discovered && entry.discovered) {
      existing.discovered = true;
    }
  }

  const canonicalWritePaths = [];
  for (const entry of unique.values()) {
    const c = canonicalize(entry.path, { expect: entry.kind ?? "any" });
    if (!c.ok) return { ok: false, reason: `验收路径 ${entry.path} 无法解析：${c.reason}` };
    // AC10（八轮复核问题 3 / 十一轮复核）：验收根内 nlink>1 的**普通文件**
    // 保守拒绝——真实硬链接（同 inode 的第二个目录项）可以在根外，验收实例
    // 可经根内文件读写根外数据；在无法枚举同 inode 其他目录项的前提下，
    // nlink>1 即"不能证明独立"，不当作已证明独立的普通文件放行（目录的
    // nlink 数值语义不同，仍继续遍历——见 collectAcceptanceTreePaths）。
    // lstat 失败不吞（十一轮复核；2026-11-04 Runner 实测修正）：
    //   - discovered（树走查实际发现过）的路径：后续 ENOENT = 走查后被删除
    //     （竞态或篡改）→ 拒绝；其他错误（权限等）= 无法检查 → 拒绝；
    //   - 豁免一：从未发现（走查时不存在）的合法布局路径——全新隔离根里尚未
    //     创建的布局路径本来被 canonicalize 允许，不能误拒；
    //   - 豁免二：走查发现但属于**受控历史库**（<隔离根>/tokenscope/history.db）
    //     的瞬时 sidecar（journal/wal/shm）——数据库以 WAL 模式运行时这些
    //     文件只在写事务/检查点间隙存在，"走查时在、lstat 时没了"是数据库的
    //     正常生命周期而非证据清理（Runner 实测：ui-single-instance 主实例
    //     在历史库迁移/写入窗口内被本防线误拒启动）。写入本来就落在隔离根
    //     内，无独立性问题；这类文件也不做 nlink 检查（生命期毫秒级，硬链接
    //     通道无意义）。范围见 controlledDbSidecar——同后缀但不在受控库旁边
    //     的文件不豁免。
    let entryStat = null;
    try {
      entryStat = lstat(entry.path);
    } catch (error) {
      // 豁免二限定受控数据库：只有 <隔离根>/tokenscope/history.db 的预期
      // sidecar（-journal/-wal/-shm）在 ENOENT 时按 SQLite 正常生命周期放行；
      // 应用原子写的 `.名称.tmp-<pid>-<序号>` 在 rename 后消失同属正常生命周期
      // （2026-11-05：此前只豁免 sidecar，CI 上偶发因 `.pricing-index.json.tmp-*`
      // 被判"走查后发现被删"）。命名不符或落在隔离根外的路径仍按竞态/篡改拒绝。
      const exempted = transientArtifactExempted(rawRoot, error?.code, entry.path);
      if (error?.code === "ENOENT" && (!entry.discovered || exempted)) {
        entryStat = null; // 未创建的布局路径，或受控历史库已提交删除的瞬时 sidecar
      } else {
        return {
          ok: false,
          reason: `验收路径 ${entry.path} 无法检查（${String(error?.message ?? error)}${entry.discovered && !exempted ? "；该路径在走查时已被发现" : ""}），无法证明独立，拒绝启动`,
        };
      }
    }
    if (entryStat && !entryStat.isDirectory() && Number(entryStat.nlink) > 1) {
      return {
        ok: false,
        reason: `验收路径 ${entry.path} 是硬链接文件（nlink=${entryStat.nlink}，可能指向验收根之外），无法证明独立，拒绝启动`,
      };
    }
    // AC10（六轮复核）：验收根内出现的链接/别名，解析目标必须仍落在验收根
    // 内——指向根外的 junction 会让验收实例把写入落根外，破坏隔离。固定布
    // 层、manifest、树走查发现的链接都经此核对（source 归并为 link）。
    if (entry.source === "link") {
      const canonicalRoot = canonicalize(rawRoot);
      if (canonicalRoot.ok && !isWithin(c.path, canonicalRoot.path)) {
        return {
          ok: false,
          reason: `验收根内的链接 ${entry.path} 指向隔离根之外（${c.path}），拒绝启动`,
        };
      }
    }
    canonicalWritePaths.push({ source: entry.path, path: c.path, kind: entry.kind ?? "any" });
  }

  const canonicalProtected = [];
  for (const pd of protectedDirs) {
    const c = canonicalize(pd);
    if (!c.ok) return { ok: false, reason: `受保护目录 ${pd} 无法解析：${c.reason}` };
    canonicalProtected.push({ source: pd, path: c.path });
  }
  const canonicalHome = canonicalize(String(homeDir));
  if (!canonicalHome.ok) {
    return { ok: false, reason: `用户主目录无法解析：${canonicalHome.reason}` };
  }

  for (const wp of canonicalWritePaths) {
    for (const pd of canonicalProtected) {
      if (comparePath(wp.path) === comparePath(pd.path)) {
        return {
          ok: false,
          reason: `验收路径 ${wp.source} 是受保护目录 ${pd.path} 本身`,
        };
      }
      if (isWithin(wp.path, pd.path)) {
        return {
          ok: false,
          reason: `验收路径 ${wp.source} 位于受保护目录 ${pd.path} 内`,
        };
      }
      if (isWithin(pd.path, wp.path)) {
        return {
          ok: false,
          reason: `验收路径 ${wp.source} 是受保护目录 ${pd.path} 的祖先`,
        };
      }
    }
    // 与 acceptance.rs 同口径：验收路径不得等于用户主目录，也不得是其祖先
    // （主目录本身在 tmpdir 之内，不能按"位于其内"拒绝）。
    if (comparePath(wp.path) === comparePath(canonicalHome.path)) {
      return { ok: false, reason: `验收路径 ${wp.source} 是用户主目录本身：${canonicalHome.path}` };
    }
    if (isWithin(canonicalHome.path, wp.path)) {
      return {
        ok: false,
        reason: `验收路径 ${wp.source} 是用户主目录 ${canonicalHome.path} 的祖先`,
      };
    }
  }

  return {
    ok: true,
    reason: "全部验收路径与受保护用户目录互不包含",
    root: canonicalWritePaths[0]?.path,
    write_paths: canonicalWritePaths.map((p) => p.path),
    protected_dirs: canonicalProtected.map((p) => p.path),
    alias_resolved: canonicalWritePaths.some((p) => p.path !== path.resolve(p.source)),
    manifest_present: manifest.exists,
    manifest_declared: manifest.entries.length,
    tree_directories: tree.paths.length,
    tree_links: tree.links.length,
    tree_visited: tree.visited ?? 0,
  };
}

/** 旧接口：单 root + 单受保护目录列表。 */
export function validateAcceptanceRoot({
  root,
  protectedDirs = [],
  canonicalize = canonicalizeAcceptancePath,
} = {}) {
  const rawRoot = String(root ?? "").trim();
  const protectedList = (Array.isArray(protectedDirs) ? protectedDirs : [protectedDirs])
    .map((value) => String(value ?? "").trim())
    .filter(Boolean);
  if (!rawRoot) return { ok: false, reason: "隔离根为空" };
  if (!protectedList.length) return { ok: false, reason: "未提供受保护目录，无法证明隔离" };
  if (!path.isAbsolute(rawRoot)) return { ok: false, reason: `隔离根不是绝对路径：${rawRoot}` };
  const canonicalRoot = canonicalize(rawRoot);
  if (!canonicalRoot.ok) {
    return { ok: false, reason: `隔离根无法解析：${canonicalRoot.reason}` };
  }
  if (canonicalRoot.path === path.parse(canonicalRoot.path).root) {
    return { ok: false, reason: `隔离根不能是盘符根：${canonicalRoot.path}` };
  }
  const protectedCanonical = [];
  for (const source of protectedList) {
    const canonical = canonicalize(source);
    if (!canonical.ok) {
      return { ok: false, reason: `受保护目录无法解析：${canonical.reason}` };
    }
    protectedCanonical.push({ source, path: canonical.path });
  }
  for (const protectedDir of protectedCanonical) {
    if (comparePath(canonicalRoot.path) === comparePath(protectedDir.path)) {
      return {
        ok: false,
        reason: `隔离根是受保护目录本身：${protectedDir.path}`,
      };
    }
    if (isWithin(canonicalRoot.path, protectedDir.path)) {
      return {
        ok: false,
        reason: `隔离根位于受保护目录内：${canonicalRoot.path}`,
      };
    }
    if (isWithin(protectedDir.path, canonicalRoot.path)) {
      return {
        ok: false,
        reason: `隔离根是受保护目录的祖先：${canonicalRoot.path}`,
      };
    }
  }
  return {
    ok: true,
    reason: `隔离根 ${canonicalRoot.path} 与受保护目录互不包含`,
    root: canonicalRoot.path,
    protected: protectedCanonical,
    alias_resolved: canonicalRoot.path !== path.resolve(rawRoot),
  };
}

/** acceptance 构建中才会编译进 EXE 的字符串标记（shell 验收模块/分支）。 */
export const ACCEPTANCE_EXE_MARKERS = [
  "验收构建要求进程级",
  "验收模式：WebView 用户数据与 identifier 已指向隔离根",
  "验收模式启动失败",
];
/** 普通构建核心库 stub 中的标记；出现即判定不是验收构建。 */
export const NORMAL_ACCEPTANCE_STUB_MARKER = "当前构建未启用 acceptance feature，不接受验收根注入";

/** 受控构建记录的类型名与唯一允许的构建命令（与 .github/workflows/ui-e2e.yml 一致）。 */
export const ACCEPTANCE_BUILD_RECORD_KIND = "tokenscope-acceptance-build";
export const ACCEPTANCE_BUILD_COMMAND =
  "cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope";

function containsBytes(bytes, text) {
  return bytes.includes(Buffer.from(text, "utf8")) || bytes.includes(Buffer.from(text, "utf16le"));
}

function isPeImage(bytes) {
  if (!bytes || bytes.length < 0x40 || bytes[0] !== 0x4d || bytes[1] !== 0x5a) return false;
  const peOffset = bytes.readUInt32LE(0x3c);
  return (
    peOffset + 4 <= bytes.length &&
    bytes.toString("latin1", peOffset, peOffset + 4) === "PE\u0000\u0000"
  );
}

function sha256Hex(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

/**
 * 校验受控构建记录的字段形状。记录由 scripts/record-acceptance-build.ps1 在
 * 同一次构建里写出（构建命令 + 源提交 + 产物哈希）；缺任何一项都不算身份。
 */
export function parseAcceptanceBuildRecord(raw, { recordPath = "" } = {}) {
  let data = raw;
  if (typeof raw === "string" || Buffer.isBuffer(raw)) {
    try {
      data = JSON.parse(Buffer.isBuffer(raw) ? raw.toString("utf8") : raw);
    } catch (error) {
      return { ok: false, reason: `构建记录不是合法 JSON：${String(error?.message ?? error)}` };
    }
  }
  if (!data || typeof data !== "object") return { ok: false, reason: "构建记录不是对象" };
  const missing = [];
  if (data.kind !== ACCEPTANCE_BUILD_RECORD_KIND) {
    missing.push(`kind 应为 ${ACCEPTANCE_BUILD_RECORD_KIND}，收到 ${String(data.kind)}`);
  }
  if (!String(data.artifact_path ?? "").trim()) missing.push("缺少 artifact_path");
  if (!/^[0-9a-f]{64}$/.test(String(data.artifact_sha256 ?? ""))) {
    missing.push("artifact_sha256 不是 64 位小写十六进制");
  }
  if (!String(data.build_command ?? "").trim()) missing.push("缺少 build_command");
  if (!/^[0-9a-f]{7,40}$/i.test(String(data.source_commit ?? ""))) {
    missing.push("source_commit 不是 git 提交哈希");
  }
  const features = Array.isArray(data.features) ? data.features : [];
  if (!features.includes("acceptance")) missing.push("features 未包含 acceptance");
  // 来源诊断字段（七轮复核）：tree_clean/builder/ci 段由
  // record-acceptance-build.ps1 登记并保留在记录里作证据，但它们都是可本地
  // 填写的自报字段——不再作为 verified 的依据（信任边界见
  // verifyAcceptanceBuildIdentity）。tree_clean 缺失仍拒收记录（登记脚本
  // 契约的一部分，帮助区分登记脚本产出与手写文件）；builder/ci 段可选。
  if (typeof data.tree_clean !== "boolean") {
    missing.push("缺少 tree_clean（必须由 record-acceptance-build.ps1 登记源树状态）");
  }
  if (data.builder === "ci") {
    const ci = data.ci && typeof data.ci === "object" ? data.ci : null;
    if (!ci || !String(ci.run_id ?? "").trim() || !String(ci.repository ?? "").trim()) {
      missing.push(
        "ci 段（builder=ci 必须携带 run_id 与 repository，供未来 CI artifact 核验使用）",
      );
    }
  }
  if (missing.length) return { ok: false, reason: `构建记录不完整：${missing.join("；")}` };
  return { ok: true, record: { ...data, recordPath } };
}

/**
 * 构建身份判定：重新计算磁盘产物的 SHA-256，并与受控构建记录逐字段核对
 * （登记产物路径、构建命令、源提交）。结论只有 unverified / hash-mismatch
 * 两种——**没有 verified 出口**：当前不存在可独立证明产物来源的路径（记录
 * 与环境自报字段皆可本地伪造，CI artifact 绑定核验未实现），预检一律拒绝
 * 启动；字符串标记本身永不授予许可。
 */
export function verifyAcceptanceBuildIdentity({
  exePath,
  record = null,
  readFile = fs.readFileSync,
  sourceCommit = null,
} = {}) {
  const unverified = (reason, extra = {}) => ({
    ok: false,
    launch_allowed: false,
    build_identity: "unverified",
    reason,
    ...extra,
  });
  const raw = String(exePath ?? "").trim();
  if (!raw) return unverified("EXE 路径为空");
  if (!record) return unverified("缺少受控构建记录：无法证明产物来自 acceptance 构建，拒绝启动");
  const parsed = record.ok === true ? record : parseAcceptanceBuildRecord(record);
  if (!parsed.ok) return unverified(`${parsed.reason}，拒绝启动`);
  const trusted = parsed.record;
  if (comparePath(raw) !== comparePath(trusted.artifact_path)) {
    return unverified(
      `启动的 EXE（${raw}）不是构建记录登记的产物（${trusted.artifact_path}），拒绝启动`,
    );
  }
  if (String(trusted.build_command).trim() !== ACCEPTANCE_BUILD_COMMAND) {
    return unverified(`构建命令不是受控的 acceptance 构建：${trusted.build_command}`);
  }
  const commit = String(sourceCommit ?? "").trim();
  if (!commit) {
    return unverified("无法确定当前检出的源提交，不能证明构建记录的来源，拒绝启动");
  }
  if (commit.toLowerCase() !== String(trusted.source_commit).toLowerCase()) {
    return unverified(`构建记录的源提交 ${trusted.source_commit} 与当前检出 ${commit} 不一致`);
  }
  let bytes;
  try {
    bytes = readFile(raw);
  } catch (error) {
    return unverified(`无法读取 EXE 以核对哈希：${String(error?.message ?? error)}`);
  }
  if (!Buffer.isBuffer(bytes) || bytes.length === 0) {
    return unverified("EXE 为空或读取结果不是 Buffer，无法核对哈希");
  }
  const sha256 = sha256Hex(bytes);
  const expected = String(trusted.artifact_sha256).toLowerCase();
  if (sha256 !== expected) {
    return {
      ok: false,
      launch_allowed: false,
      build_identity: "hash-mismatch",
      sha256,
      expected_sha256: expected,
      size: bytes.length,
      reason: `产物哈希与受控构建记录不一致（实际 ${sha256.slice(0, 12)}…，登记 ${expected.slice(
        0,
        12,
      )}…），拒绝启动`,
    };
  }
  // 受控构建假设（AC10 收敛后的许可语义）：
  // 记录的全部字段（JSON/命令/哈希/提交/tree_clean/builder/ci 段）以及
  // GITHUB_ACTIONS 环境变量都可由本地进程填写——它们是**可信本机/CI 受控
  // 构建假设**下的自述，用来防"意外不一致"（拿错 EXE、命令不对、提交不符、
  // 字节被替换），不构成来源独立防伪证明。可核验的 GitHub artifact 绑定
  // （run/工作流/job/归档摘要四方核对）已从交付代码移除，留待 AC10 接入 CI
  // 时重新评估；当前**没有任何结论可以称为 verified**。
  // 已生效的防线（都只证明"磁盘产物与记录描述一致"）：PE 预筛、stub 标记
  // 拒绝、产物哈希核对（字节被替换即 hash-mismatch）、源提交一致性、
  // tree_clean 形状校验。
  const buildIdentity = "assumed";
  if (trusted.tree_clean === false) {
    // 脏树：本地开发可用，但**不可仅凭提交哈希复现**——如实标注，不冒充。
    // CI 与正式取证要求干净树（builder=ci 的脏树记录直接拒绝）。
    if (String(trusted.builder ?? "").trim() === "ci") {
      return unverified(
        `CI 构建记录为脏树产物（${trusted.tree_dirty_count ?? "?"} 个未提交变更），CI 与正式取证要求干净树，拒绝启动`,
      );
    }
    return {
      ok: true,
      launch_allowed: true,
      build_identity: buildIdentity,
      reproducible_from_commit: false,
      tree_clean: false,
      reason:
        "受控构建假设：本地开发脏树构建（" +
        String(trusted.tree_dirty_count ?? "?") +
        " 个未提交变更）——产物无法仅凭提交哈希复现，证据不得按可复现口径引用；这不构成来源独立防伪证明",
    };
  }
  return {
    ok: true,
    launch_allowed: true,
    build_identity: buildIdentity,
    reproducible_from_commit: true,
    tree_clean: true,
    reason:
      "受控构建假设：记录与实际 EXE 一致（哈希/命令/提交/树状态），可防意外不一致；" +
      "记录可自填，不构成来源独立防伪证明（GitHub 侧绑定未接线）",
  };
}

/**
 * 最小 ZIP 读取器（十轮复核问题 2）：解析 End of Central Directory →
 * Central Directory → 定位条目并解出原始字节。仅支持 stored(0) 与
 * deflate(8)，满足 GitHub artifact zip（Windows 产物多用 deflate）与测试
 * 合成 zip。安全：拒绝路径分隔符异常（`..` 段、绝对盘符）与同名重复条目
 * ——重复名无法确定哪个是登记产物，直接失败而非猜测。
 */
/**
 * EXE 验收构建筛查：PE + acceptance 标记 + 无普通构建 stub 标记只是**预筛**。
 * 追加了标记的普通 PE、或缺 stub 标记都不能授予启动许可；许可只能来自
 * verifyAcceptanceBuildIdentity 的 `assumed`（受控构建假设）结论。
 */
export function verifyAcceptanceExecutable(
  exePath,
  { readFile = fs.readFileSync, buildRecord = null, sourceCommit = null } = {},
) {
  const raw = String(exePath ?? "").trim();
  if (!raw) {
    return {
      ok: false,
      reason: "EXE 路径为空",
      launch_allowed: false,
      build_identity: "unverified",
    };
  }
  let bytes;
  try {
    bytes = readFile(raw);
  } catch (error) {
    return {
      ok: false,
      reason: `无法读取 EXE：${error?.message ?? error}`,
      launch_allowed: false,
      build_identity: "unverified",
    };
  }
  if (!Buffer.isBuffer(bytes) || bytes.length === 0) {
    return {
      ok: false,
      reason: "EXE 为空或读取结果不是 Buffer，无法验证",
      launch_allowed: false,
      build_identity: "unverified",
    };
  }
  const screening = { size: bytes.length, sha256: sha256Hex(bytes) };
  if (!isPeImage(bytes)) {
    return {
      ...screening,
      ok: false,
      reason: "文件不是 PE 可执行文件，拒绝启动",
      markers: [],
      negative_markers: [],
      launch_allowed: false,
      build_identity: "rejected",
    };
  }
  const positive = ACCEPTANCE_EXE_MARKERS.filter((marker) => containsBytes(bytes, marker));
  const negative = containsBytes(bytes, NORMAL_ACCEPTANCE_STUB_MARKER)
    ? [NORMAL_ACCEPTANCE_STUB_MARKER]
    : [];
  const identity = verifyAcceptanceBuildIdentity({
    exePath: raw,
    record: buildRecord,
    readFile,
    sourceCommit,
  });
  if (negative.length) {
    return {
      ...screening,
      ok: false,
      reason: "EXE 含普通构建 acceptance stub 标记，判定不是 acceptance 构建",
      markers: positive,
      negative_markers: negative,
      launch_allowed: false,
      build_identity: "rejected",
      identity,
    };
  }
  if (!positive.length) {
    return {
      ...screening,
      ok: false,
      reason: "EXE 未包含 acceptance 构建标记；普通 EXE 或无法验证，拒绝启动",
      markers: [],
      negative_markers: negative,
      launch_allowed: false,
      build_identity: identity.build_identity,
      identity,
    };
  }
  if (!identity.ok) {
    return {
      ...screening,
      ok: false,
      reason: `字符串标记只用于预筛（命中 ${positive.length} 个），构建身份未验证：${identity.reason}`,
      markers: positive,
      negative_markers: [],
      marker_screened: true,
      launch_allowed: false,
      build_identity: identity.build_identity,
      identity,
    };
  }
  return {
    ...screening,
    ok: true,
    reason: `构建身份：${identity.build_identity}（${identity.reason}；标记预筛命中 ${positive.length} 个）`,
    markers: positive,
    negative_markers: [],
    marker_screened: true,
    launch_allowed: true,
    build_identity: identity.build_identity,
    reproducible_from_commit: identity.reproducible_from_commit === true,
    identity,
  };
}

/**
 * 启动前预检：完整路径隔离（固定布局 + manifest 声明 + 现有条目别名）与实际
 * EXE 构建身份。attach 模式不启动 EXE，只校验路径；非 attach 模式必须拿到
 * `assumed`（受控构建假设）身份，`marker-screened` 一律不允许启动。
 */
export function evaluateAcceptanceLaunchPreflight({
  root,
  outDir,
  exePath,
  homeDir,
  attachExisting = false,
  canonicalize = canonicalizeAcceptancePath,
  readFile = fs.readFileSync,
  buildRecord = null,
  buildRecordPath = null,
  sourceCommit = null,
} = {}) {
  const pathCheck = validateAcceptancePaths({
    root,
    outDir,
    homeDir,
    canonicalize,
    readFile,
  });
  let record = buildRecord;
  if (!record && buildRecordPath) {
    try {
      const raw = readFile(String(buildRecordPath));
      record = parseAcceptanceBuildRecord(Buffer.isBuffer(raw) ? raw.toString("utf8") : raw, {
        recordPath: String(buildRecordPath),
      });
    } catch {
      record = null;
    }
  }
  const exeCheck = attachExisting
    ? {
        ok: true,
        skipped: true,
        reason: "attach 模式不启动 EXE",
        build_identity: "skipped",
        launch_allowed: false,
      }
    : verifyAcceptanceExecutable(exePath, { readFile, buildRecord: record, sourceCommit });
  return {
    ok: pathCheck.ok && exeCheck.ok,
    paths: pathCheck,
    exe: exeCheck,
    attachExisting,
  };
}

/**
 * 预检门：只有预检通过才执行准备与启动；返回是否真的调用过。
 * 合成反例用它证明受保护根/非 acceptance EXE 下 prepare/launch 均为零调用。
 */
export async function startIfPreflightAllows({ preflight, prepare, launch } = {}) {
  const result = await preflight();
  if (!result?.ok) {
    return { started: false, preflight: result, prepared: null, launched: null };
  }
  const prepared = await prepare(result);
  const launched = await launch(result);
  return { started: true, preflight: result, prepared, launched };
}

/** 主进程存活：Node 子进程可能以 signalCode 结束而 exitCode 仍为 null。 */
export function hostProcessAlive(proc) {
  return (proc?.exitCode ?? null) === null && (proc?.signalCode ?? null) === null;
}

/**
 * 解析 `native-window-action.ps1 -Action state` 的输出：
 * `STATE found true hwnd 123 visible true minimized false rect x,y,w,h`。
 * 输出缺失/畸形时返回 `ok:false`，由调用方记为待验证而不是猜状态。
 */
export function parseWindowState(text) {
  const line = String(text ?? "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .find((value) => value.startsWith("STATE "));
  if (!line) return { ok: false, error: "窗口状态输出缺少 STATE 行" };
  const tokens = line.split(/\s+/);
  const fields = {};
  for (let i = 1; i + 1 < tokens.length; i += 2) fields[tokens[i]] = tokens[i + 1];
  if (fields.found !== "true" && fields.found !== "false") {
    return { ok: false, error: `窗口状态 found 字段非法：${fields.found ?? "缺失"}` };
  }
  if (fields.found === "true") {
    for (const key of ["visible", "minimized"]) {
      if (fields[key] !== "true" && fields[key] !== "false") {
        return { ok: false, error: `窗口状态 ${key} 字段非法：${fields[key] ?? "缺失"}` };
      }
    }
  }
  return {
    ok: true,
    found: fields.found === "true",
    hwnd: fields.hwnd ?? "0",
    visible: fields.visible === "true",
    minimized: fields.minimized === "true",
    rect: fields.rect ?? "none",
  };
}

/**
 * 解析 `native-window-action.ps1 -Action foreground` 的输出：
 * `FOREGROUND match true pid 123 hwnd 123 fg_pid 123 fg_hwnd 456`。
 */
export function parseForeground(text) {
  const line = String(text ?? "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .find((value) => value.startsWith("FOREGROUND "));
  if (!line) return { ok: false, error: "前台窗口输出缺少 FOREGROUND 行" };
  const tokens = line.split(/\s+/);
  const fields = {};
  for (let i = 1; i + 1 < tokens.length; i += 2) fields[tokens[i]] = tokens[i + 1];
  if (fields.match !== "true" && fields.match !== "false") {
    return { ok: false, error: `前台窗口 match 字段非法：${fields.match ?? "缺失"}` };
  }
  return {
    ok: true,
    match: fields.match === "true",
    pid: fields.pid ?? null,
    hwnd: fields.hwnd ?? null,
    foreground_pid: fields.fg_pid ?? null,
    foreground_hwnd: fields.fg_hwnd ?? null,
  };
}

/** 托盘图标文本的中文/英文 tooltip 都可能带后缀；只按名称包含关系计数。 */
export function countTokenScopeMatches(matches) {
  return (Array.isArray(matches) ? matches : []).filter((entry) => {
    const text = typeof entry === "string" ? entry : entry?.text;
    return /tokenscope/i.test(String(text ?? ""));
  }).length;
}

/**
 * 托盘观察一致性：计数必须是完整整数、可见+溢出等于总数、TokenScope 计数
 * 必须等于 matches 明细条数，且每条明细文本确实包含 TokenScope。
 * 任一不一致都返回不可通过，不把部分观察当成成功。
 */
export function validateTrayConsistency(observation) {
  if (!observation || observation.ok !== true) {
    return { ok: false, reason: `托盘观察不可用：${observation?.error ?? "无结果"}` };
  }
  const counts = [
    observation.visible_count,
    observation.overflow_count,
    observation.total_count,
    observation.token_scope_count,
  ];
  if (!counts.every(Number.isInteger)) {
    return { ok: false, reason: "托盘观察计数不完整（不可把缺失计数当作 0）" };
  }
  if (observation.visible_count + observation.overflow_count !== observation.total_count) {
    return {
      ok: false,
      reason: `托盘图标总数不一致：${observation.visible_count}+${observation.overflow_count} != ${observation.total_count}`,
    };
  }
  if (!Array.isArray(observation.matches)) {
    return { ok: false, reason: "托盘观察缺少 matches 明细" };
  }
  if (observation.matches.length !== observation.token_scope_count) {
    return {
      ok: false,
      reason: `TokenScope 计数 ${observation.token_scope_count} 与 matches 明细 ${observation.matches.length} 不一致`,
    };
  }
  for (const match of observation.matches) {
    if (!/tokenscope/i.test(String(match?.text ?? ""))) {
      return { ok: false, reason: "matches 明细含非 TokenScope 文本" };
    }
  }
  return { ok: true };
}

/**
 * 解析托盘观察脚本的 JSON。PowerShell 告警可能混入 stdout，因此取最后一个
 * 可解析的 JSON 行；解析不出来或计数/明细自相矛盾时返回 `ok:false`。
 */
export function parseTrayObservation(text) {
  const lines = String(text ?? "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter((value) => value.startsWith("{"));
  for (const line of lines.reverse()) {
    try {
      const data = JSON.parse(line);
      const matches =
        data.matches === null || data.matches === undefined
          ? []
          : Array.isArray(data.matches)
            ? data.matches
            : [data.matches];
      const hasCount = Number.isInteger(data.token_scope_count);
      const hasMatches = data.matches !== null && data.matches !== undefined;
      if (!hasCount && !hasMatches) {
        return {
          ok: false,
          error: "托盘观察缺少 token_scope_count 与 matches",
          token_scope_count: null,
          matches: [],
        };
      }
      const derived = countTokenScopeMatches(matches);
      const count = hasCount ? data.token_scope_count : derived;
      const observation = {
        ok: data.ok === true,
        error: data.error ?? null,
        visible_count: Number.isInteger(data.visible_count) ? data.visible_count : null,
        overflow_count: Number.isInteger(data.overflow_count) ? data.overflow_count : null,
        total_count: Number.isInteger(data.total_count) ? data.total_count : null,
        token_scope_count: count,
        matches,
      };
      const consistency = validateTrayConsistency(observation);
      if (!consistency.ok) {
        return { ...observation, ok: false, error: consistency.reason };
      }
      return observation;
    } catch {
      /* 换上一行 */
    }
  }
  return { ok: false, error: "托盘观察输出不是 JSON", token_scope_count: null, matches: [] };
}

/**
 * 第二个实例的环境：必须继承固定隔离通道（价格桩/代理），强制同一个
 * TOKENSCOPE_ACCEPTANCE_ROOT，并清掉远程调试参数（第二个实例不接 CDP）。
 */
export function secondaryLaunchEnvironment({
  inherited = {},
  controlled = {},
  scenario = {},
  root,
} = {}) {
  if (!String(root ?? "").trim()) {
    throw new Error("secondaryLaunchEnvironment 需要非空隔离根");
  }
  const env = { ...inherited, ...controlled, ...scenario, TOKENSCOPE_ACCEPTANCE_ROOT: root };
  delete env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS;
  return env;
}

/**
 * 第二个实例的退出等待：exit/error 事件与短轮询取先到者；超时或场景取消时
 * 只终止调用方传入的 child。返回原始观察，判定交由 classifySecondaryExit。
 * 独立成函数是为了能用真实短命子进程做合成验证，而不必启动 Native。
 */
export async function waitForSecondaryExit({
  child,
  ownedPid = null,
  timeoutMs = SINGLE_INSTANCE_EXIT_BUDGET_MS,
  isCanceled = () => false,
  intervalMs = 50,
  killGraceMs = 5000,
  now = () => Date.now(),
  sleepFn = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
} = {}) {
  if (!child || typeof child.once !== "function") {
    throw new Error("waitForSecondaryExit 需要 child 进程");
  }
  const startedAt = now();
  const deadline = startedAt + Number(timeoutMs);
  let terminal = null;
  const terminalPromise = new Promise((resolve) => {
    child.once("exit", (code, signal) => resolve({ kind: "exit", code, signal, at: now() }));
    child.once("error", (error) => resolve({ kind: "error", error, at: now() }));
  });
  let timedOut = false;
  let canceled = false;
  while (!terminal) {
    const next = await Promise.race([terminalPromise, sleepFn(intervalMs).then(() => null)]);
    if (next) {
      terminal = next;
      break;
    }
    if (isCanceled()) {
      canceled = true;
      break;
    }
    if (now() >= deadline) {
      timedOut = true;
      break;
    }
  }
  const exitBeforeTerminate = terminal?.kind === "exit";
  let terminateRequested = false;
  let terminateError = null;
  let exitAfterTerminate = false;
  if (!terminal && (timedOut || canceled)) {
    if (ownedPid !== null && child.pid !== ownedPid) {
      terminateError = `拒绝终止非本次拥有的 PID ${child.pid}（期望 ${ownedPid}）`;
    } else {
      terminateRequested = true;
      try {
        if (child.kill() === false) terminateError = "child.kill() 返回 false";
      } catch (error) {
        terminateError = `child.kill() 失败：${String(error?.message ?? error)}`;
      }
    }
    terminal = await Promise.race([terminalPromise, sleepFn(killGraceMs).then(() => null)]);
    exitAfterTerminate = terminal?.kind === "exit";
  }
  const exitConfirmed = terminal?.kind === "exit";
  const exitCode = exitConfirmed ? terminal.code : null;
  const signal = exitConfirmed ? terminal.signal : null;
  const spawnError =
    terminal?.kind === "error" ? String(terminal.error?.message ?? terminal.error) : null;
  return {
    kind: terminal?.kind ?? null,
    exitCode,
    signal,
    spawnError,
    elapsedMs: Math.round((terminal?.at ?? now()) - startedAt),
    timedOut,
    canceled,
    exitBeforeTerminate,
    exitConfirmed,
    exitAfterTerminate,
    terminateRequested,
    terminateError,
    terminateUnconfirmed: terminateRequested && !exitConfirmed,
  };
}

/**
 * 第二个启动的退出判定：正常移交必须是限定时间内以退出码 0 结束。
 * 超时、非零退出、被信号终止都是失败；退出码不可读时是待验证，不猜成 0。
 */
export function classifySecondaryExit({
  exitCode = null,
  signal = null,
  elapsedMs = null,
  budgetMs = SINGLE_INSTANCE_EXIT_BUDGET_MS,
  timedOut = false,
  canceled = false,
  exitConfirmed = true,
  terminateRequested = false,
  terminateError = null,
  terminateUnconfirmed = false,
} = {}) {
  const budget = Number(budgetMs);
  const elapsed = Number(elapsedMs);
  const cleanup = terminateRequested
    ? terminateError
      ? `终止请求失败：${terminateError}`
      : exitConfirmed
        ? "已发送终止请求并观察到退出"
        : "已发送终止请求但未确认退出"
    : terminateError
      ? `未终止：${terminateError}`
      : "未发送终止请求";
  if (!exitConfirmed) {
    if (timedOut) {
      return {
        status: "failed",
        reason: `第二次启动未在 ${budget} ms 内退出；${cleanup}${terminateUnconfirmed ? "，退出未确认" : ""}`,
      };
    }
    if (canceled) {
      return {
        status: "failed",
        reason: `场景取消；${cleanup}${terminateUnconfirmed ? "，退出未确认" : ""}`,
      };
    }
    return { status: "pending", reason: `未观察到第二次启动的退出事件；${cleanup}` };
  }
  if (timedOut) {
    return { status: "failed", reason: `第二次启动未在 ${budget} ms 内退出；${cleanup}` };
  }
  if (canceled) {
    return { status: "failed", reason: `场景取消；${cleanup}` };
  }
  if (signal) {
    return { status: "failed", reason: `第二次启动被信号 ${signal} 终止，不是正常移交` };
  }
  if (!Number.isFinite(elapsed) || elapsed < 0) {
    return { status: "pending", reason: "第二次启动的退出耗时不可读" };
  }
  if (!Number.isInteger(exitCode)) {
    return { status: "pending", reason: "第二次启动的退出码不可读" };
  }
  if (elapsed > budget) {
    return {
      status: "failed",
      reason: `第二次启动退出耗时 ${Math.round(elapsed)} ms 超过 ${budget} ms`,
    };
  }
  if (exitCode !== 0) {
    return { status: "failed", reason: `第二次启动退出码 ${exitCode}，期望 0` };
  }
  return {
    status: "passed",
    reason: `第二次启动 ${Math.round(elapsed)} ms 内以退出码 0 结束`,
  };
}

/**
 * 第一个窗口恢复判定：可见与前台分开取证；观察机制不可用时对应项为
 * pending，不能用“第一个进程仍存活”之类的替身结论。
 */
export function classifyWindowRestore({
  state = null,
  foreground = null,
  elapsedMs = null,
  budgetMs = SINGLE_INSTANCE_RESTORE_BUDGET_MS,
} = {}) {
  let visibility;
  if (!state || state.ok !== true) {
    visibility = {
      status: "pending",
      reason: `窗口状态观察不可用：${state?.error ?? "无结果"}`,
    };
  } else if (!state.found) {
    visibility = { status: "failed", reason: "找不到第一个实例的主窗口" };
  } else if (state.visible !== true) {
    visibility = { status: "failed", reason: "第一个窗口未恢复可见" };
  } else if (state.minimized !== false) {
    visibility = { status: "failed", reason: "第一个窗口仍处于最小化" };
  } else {
    visibility = { status: "passed", reason: "第一个窗口可见且未最小化" };
  }

  let foregroundVerdict;
  if (!foreground || foreground.ok !== true) {
    foregroundVerdict = {
      status: "pending",
      reason: `前台窗口观察不可用：${foreground?.error ?? "无结果"}`,
    };
  } else if (foreground.match !== true) {
    foregroundVerdict = {
      status: "failed",
      reason: `前台窗口不属于第一个实例（fg_pid=${foreground.foreground_pid ?? "unknown"}）`,
    };
  } else {
    foregroundVerdict = { status: "passed", reason: "前台窗口属于第一个实例" };
  }

  const statuses = [visibility.status, foregroundVerdict.status];
  const status = statuses.includes("failed")
    ? "failed"
    : statuses.includes("pending")
      ? "pending"
      : "passed";
  const elapsed = Number(elapsedMs);
  const elapsedText = Number.isFinite(elapsed)
    ? `恢复观察 ${Math.round(elapsed)} ms（上限 ${Number(budgetMs)} ms）`
    : "恢复观察耗时不可读";
  return {
    status,
    visibility,
    foreground: foregroundVerdict,
    reason: [visibility.reason, foregroundVerdict.reason, elapsedText].join("；"),
  };
}

/**
 * 托盘唯一性判定：必须有实际可观察的 TokenScope 图标计数。
 * 观察不可用时是 pending；传入 processCount 也不会自动变成通过。
 */
/**
 * 托盘归属观察（AC10）：解析 `scripts/read-tray-icon-owner.ps1` 的输出。
 * 该脚本按 (hwnd, uID) 用 Shell_NotifyIconGetRect 探测**指定进程**注册的托盘
 * 图标，因此能回答"图标是否属于本实例、注册了几个"——这是文本匹配做不到的
 * （Explorer 工具栏只有 tooltip "TokenScope"，无法区分正式实例与验收实例）。
 */
export function parseTrayOwnerObservation(stdout) {
  const text = String(stdout ?? "").trim();
  if (!text) {
    return {
      ok: false,
      error: "托盘归属观察输出为空",
      pid: null,
      windows: null,
      count: null,
      icons: [],
    };
  }
  const lines = text.split(/\r?\n/).reverse();
  for (const line of lines) {
    const candidate = line.trim();
    if (!candidate.startsWith("{")) continue;
    let parsed;
    try {
      parsed = JSON.parse(candidate);
    } catch {
      continue;
    }
    if (typeof parsed?.ok !== "boolean") continue;
    return {
      ok: parsed.ok === true,
      pid: Number.isInteger(parsed.pid) ? parsed.pid : null,
      windows: Number.isInteger(parsed.windows) ? parsed.windows : null,
      count: Number.isInteger(parsed.count) ? parsed.count : null,
      icons: Array.isArray(parsed.icons) ? parsed.icons : [],
      error: parsed.error == null ? null : String(parsed.error),
    };
  }
  return {
    ok: false,
    error: "托盘归属观察输出无法解析",
    pid: null,
    windows: null,
    count: null,
    icons: [],
  };
}

/**
 * 托盘归属判定（证据口径收敛版）。
 *
 * 探测范围本身的限制决定了结论强度：`read-tray-icon-owner.ps1` 只枚举目标进程
 * 的 `tray_icon_app` 窗口并扫 `uID 0..MaxUid`，它能确认"在这个范围内找到了 N 个
 * 属于该进程的图标"，但**不能**证明进程没注册过图标（可能尚未创建、可能已销毁、
 * 可能落在范围外），也**不能**证明全局唯一（其他进程的图标不在探测内）。
 *
 *   - 找到恰好 1 个（探测范围内）→ passed，措辞只描述"找到 1 个"，不写"唯一"；
 *   - 找到 >1 个 → failed（已确认至少两个，足以否定"只有一个"）；
 *   - 0 个或观察不可用 → pending（无法区分"未创建"与"探测不到"）；
 *   - 只有 1 个窗口但 uID 扫描区间耗尽（count>0 且命中在边界）时仍按找到的
 *     数量判定，但调用方不应把它升级成"唯一性已证明"。
 */
export function classifyTrayOwner({ observation = null, expected = 1 } = {}) {
  if (!observation || observation.ok !== true || !Number.isInteger(observation.count)) {
    return {
      status: "pending",
      count: null,
      reason: `托盘归属观察不可用：${observation?.error ?? "无结果"}`,
    };
  }
  if (observation.count > expected) {
    return {
      status: "failed",
      count: observation.count,
      reason: `当前探测范围内找到 ${observation.count} 个属于该进程的托盘图标（期望 ${expected}）：至少存在重复图标`,
    };
  }
  if (observation.count === expected) {
    return {
      status: "passed",
      count: observation.count,
      reason: `当前探测范围内找到 ${observation.count} 个属于该进程的托盘图标`,
    };
  }
  return {
    status: "pending",
    count: observation.count,
    reason:
      `当前探测范围内未找到该进程的托盘图标（0 个）：无法区分"未创建图标"与"探测范围不足/尚未创建"，` +
      `不据此判定未创建`,
  };
}

export function classifyTrayObservation({
  observation = null,
  expected = 1,
  processCount = null,
} = {}) {
  const consistency = validateTrayConsistency(observation);
  if (!consistency.ok) {
    return {
      status: "pending",
      count: null,
      processCount,
      reason: consistency.reason,
    };
  }
  const count = observation.token_scope_count;
  if (count !== expected) {
    return {
      status: "failed",
      count,
      processCount,
      reason: `TokenScope 托盘图标数 ${count}，期望 ${expected}`,
    };
  }
  return {
    status: "passed",
    count,
    processCount,
    reason: `TokenScope 托盘图标数 ${count}`,
  };
}
