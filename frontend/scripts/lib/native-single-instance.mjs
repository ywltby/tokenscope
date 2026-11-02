// AC10（N12 单实例）纯逻辑：应用标识后缀、隔离根预检、第二次启动的
// 退出/窗口恢复/托盘观察判定。所有外部观察由调用方注入；本文件不 spawn
// 进程、不读写真实数据，便于在 node 环境做有区分度的合成测试。
import fs from "node:fs";
import crypto from "node:crypto";
import path from "node:path";
import zlib from "node:zlib";

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
  {
    readFileSync = fs.readFileSync,
    statSync = fs.statSync,
    existsSync = fs.existsSync,
  } = {},
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
    else if (!path.isAbsolute(entry.path)) invalid.push(`${entry.source}: 非绝对路径 ${entry.path}`);
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
  for (const link of tree.links) layout.push({ path: link, kind: "any", source: "link", discovered: true });

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
    // lstat 失败不吞（十一轮复核）：
    //   - discovered（树走查实际发现过）的路径：后续 ENOENT = 走查后被删除
    //     （竞态或篡改）→ 拒绝；其他错误（权限等）= 无法检查 → 拒绝；
    //   - 从未发现（走查时不存在）的合法布局路径：豁免 ENOENT——全新隔离根
    //     里尚未创建的布局路径本来被 canonicalize 允许，不能误拒。
    let entryStat = null;
    try {
      entryStat = lstat(entry.path);
    } catch (error) {
      if (error?.code === "ENOENT" && !entry.discovered) {
        entryStat = null; // 从未存在，准备阶段将创建
      } else {
        return {
          ok: false,
          reason: `验收路径 ${entry.path} 无法检查（${String(error?.message ?? error)}${entry.discovered ? "；该路径在走查时已被发现" : ""}），无法证明独立，拒绝启动`,
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
      missing.push("ci 段（builder=ci 必须携带 run_id 与 repository，供未来 CI artifact 核验使用）");
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
  // 来源证明信任边界（七轮复核，取代 builder/GITHUB_ACTIONS 自报字段门）：
  // 记录的全部字段（JSON/命令/哈希/提交/tree_clean/builder/ci 段）以及
  // GITHUB_ACTIONS 环境变量都可由本地进程填写，自报字段之间互相印证不构成
  // 来源证明——继续加同类字段只会扩大可伪造面。可核验的绑定链是：
  //   record → (run_id, repository) → GitHub API artifact 清单 → EXE 与
  //   record 同 artifact 且 digest 与本地字节一致 → 来源成立。
  // 该链路需要联网 GitHub API 与 token，且要绑定 run 的 checkout 提交；当前
  // 未实现（后续批次），因此**当前没有任何可独立证明来源的路径**：
  //   - verified 不再由记录字段授予（verifyAcceptanceBuildIdentity 只返回
  //     unverified / hash-mismatch / rejected 三种结论，无 verified 出口）；
  //   - 预检一律拒绝启动（零准备、零启动），AC10 真实 Native 链路留待 CI
  //     绑定核验落地（AC10/N12 保持开放，不冒充完成）。
  // 已保留并继续生效的防线：PE 预筛、stub 标记拒绝、产物哈希核对（字节被
  // 替换即 hash-mismatch）、源提交一致性、tree_clean 形状校验——它们证明
  // "磁盘产物与记录描述一致"，但不证明"记录描述的来源为真"。
  if (trusted.tree_clean === false) {
    return unverified(
      `构建记录为脏树产物（${trusted.tree_dirty_count ?? "?"} 个未提交变更），产物无法从源提交复现，拒绝启动`,
    );
  }
  return unverified(
    "无法独立证明产物来源：构建记录可本地伪造，可核验的 CI artifact 绑定（GitHub API 核验 run/EXE 同源与 digest）尚未接线到预检——保持 unverified 并拒绝启动（核验核心见 verifyBuildOriginBinding，AC10 真实 Native 链路留待接线）",
  );
}

/**
 * 最小 ZIP 读取器（十轮复核问题 2）：解析 End of Central Directory →
 * Central Directory → 定位条目并解出原始字节。仅支持 stored(0) 与
 * deflate(8)，满足 GitHub artifact zip（Windows 产物多用 deflate）与测试
 * 合成 zip。安全：拒绝路径分隔符异常（`..` 段、绝对盘符）与同名重复条目
 * ——重复名无法确定哪个是登记产物，直接失败而非猜测。
 */
// CRC-32（IEEE 802.3，ZIP 登记值）：查表实现，用于条目内容完整性校验。
const CRC32_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();
function crc32Of(buf) {
  let crc = -1;
  for (let i = 0; i < buf.length; i++) crc = (crc >>> 8) ^ CRC32_TABLE[(crc ^ buf[i]) & 0xff];
  return (crc ^ -1) >>> 0;
}

// 归档读取/解压的可解释资源上限（十六轮复核问题 2）：默认值按 EXE 产物的
// 实际量级设定，调用方可注入更小值。上限在解析与解压**过程中**强制——
// 解压用 zlib 的 maxOutputLength，超限在展开中途失败，不先完整展开再检查。
export const ZIP_READ_LIMITS = {
  // 归档 zip 字节上限（GitHub artifact 通常 10–50 MiB）
  maxArchiveBytes: 512 * 1024 * 1024,
  // 单条目解压后字节上限（EXE 通常 10–50 MiB）
  maxEntryBytes: 256 * 1024 * 1024,
};

export function readZipEntryBytes(archiveBytes, entryName, limits = {}) {
  const bytes = archiveBytes;
  const fail = (reason) => ({ ok: false, reason });
  const maxArchiveBytes = Number.isInteger(limits?.maxArchiveBytes)
    ? limits.maxArchiveBytes
    : ZIP_READ_LIMITS.maxArchiveBytes;
  const maxEntryBytes = Number.isInteger(limits?.maxEntryBytes)
    ? limits.maxEntryBytes
    : ZIP_READ_LIMITS.maxEntryBytes;
  if (!Number.isInteger(maxArchiveBytes) || maxArchiveBytes <= 0) {
    return fail("归档字节上限配置非法，拒绝启动");
  }
  if (!Number.isInteger(maxEntryBytes) || maxEntryBytes <= 0) {
    return fail("单条目解压上限配置非法，拒绝启动");
  }
  // 全程先检查长度再读，杜绝越界读抛出未处理异常（Buffer.read* 越界会抛
  // ERR_OUT_OF_RANGE）。
  const safeU32 = (off) => {
    if (!Number.isInteger(off) || off < 0 || off + 4 > bytes.length) return null;
    return bytes.readUInt32LE(off);
  };
  const safeU16 = (off) => {
    if (!Number.isInteger(off) || off < 0 || off + 2 > bytes.length) return null;
    return bytes.readUInt16LE(off);
  };
  if (!Buffer.isBuffer(bytes) || bytes.length < 22) {
    return fail("归档字节过短，不是有效的 ZIP 归档");
  }
  if (bytes.length > maxArchiveBytes) {
    return fail(`归档字节 ${bytes.length} 超过上限 ${maxArchiveBytes}，拒绝启动`);
  }
  // EOCD：从尾部向前找 signature 0x06054b50（注释长度上限 65535）
  const minEocd = 22;
  let eocd = -1;
  const scanFloor = Math.max(0, bytes.length - minEocd - 65536);
  for (let i = bytes.length - minEocd; i >= scanFloor; i--) {
    const sig = safeU32(i);
    if (sig === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) return fail("不是有效的 ZIP 归档（找不到 End of Central Directory）");
  const entryCount = safeU16(eocd + 10);
  const cdSize = safeU32(eocd + 12);
  const cdOffset = safeU32(eocd + 16);
  if (entryCount === null || cdSize === null || cdOffset === null) {
    return fail("ZIP EOCD 字段越界，归档损坏");
  }
  // 中央目录区间必须完整落在归档字节内
  if (cdOffset < 0 || cdSize < 0 || cdOffset + cdSize > bytes.length) {
    return fail(`ZIP 中央目录区间越界（offset ${cdOffset} + size ${cdSize} > ${bytes.length}），归档损坏或被篡改`);
  }
  if (entryCount < 0) {
    return fail("ZIP EOCD 条目数为负，归档损坏");
  }
  // 中央目录解析受 EOCD 登记区间约束（十五轮复核问题 1）：登记 size 为 0
  // 却声明条目、或登记短于实际目录，都是损坏/篡改形态——不允许越过登记
  // 区间继续读条目。
  const cdEnd = cdOffset + cdSize;
  let cursor = cdOffset;
  let found = null;
  for (let i = 0; i < entryCount; i++) {
    if (cursor + 46 > cdEnd || safeU32(cursor) !== 0x02014b50) {
      return fail("ZIP Central Directory 损坏（条目越界或签名不符）");
    }
    const method = safeU16(cursor + 10);
    const cdFlags = safeU16(cursor + 8);
    const crcRegistered = safeU32(cursor + 16);
    const compressedSize = safeU32(cursor + 20);
    const uncompressedSize = safeU32(cursor + 24);
    const nameLen = safeU16(cursor + 28);
    const extraLen = safeU16(cursor + 30);
    const commentLen = safeU16(cursor + 32);
    const localOffset = safeU32(cursor + 42);
    if ([method, cdFlags, crcRegistered, compressedSize, uncompressedSize, nameLen, extraLen, commentLen, localOffset].some((v) => v === null)) {
      return fail("ZIP Central Directory 字段越界，归档损坏");
    }
    // general-purpose bit 3（十五轮复核问题 2）：data descriptor 流式写出的
    // 归档，本地头的 CRC/大小是占位 0，真实值在数据后的描述符与中央目录里。
    // 该形态合法（Node zlib、Java ZipOutputStream 等向不可 seek 流写出时都
    // 用它），不得判损坏；描述符在读取后与 CD 登记值核对。
    const hasDataDescriptor = (cdFlags & 0x08) !== 0;
    // 登记未压缩大小超过上限：解压前就拒绝，不先展开再检查。
    if (uncompressedSize > maxEntryBytes) {
      return fail(
        `ZIP 条目 "${entryName}" 登记未压缩大小 ${uncompressedSize} 超过上限 ${maxEntryBytes}，拒绝启动`,
      );
    }
    if (cursor + 46 + nameLen + extraLen + commentLen > bytes.length) {
      return fail("ZIP Central Directory 条目名越界，归档损坏");
    }
    if (cursor + 46 + nameLen + extraLen + commentLen > cdEnd) {
      return fail(
        `ZIP Central Directory 条目越出登记区间（${cursor + 46 + nameLen + extraLen + commentLen} > ${cdEnd}），登记大小与实际目录不一致，归档损坏或被篡改`,
      );
    }
    const name = bytes.toString("utf8", cursor + 46, cursor + 46 + nameLen);
    if (name === entryName) {
      // 同名重复条目：无法确定哪个是登记产物，直接失败而非猜测。
      if (found !== null) {
        return fail(`ZIP 归档内存在重复条目 "${entryName}"，无法确定登记产物，拒绝启动`);
      }
      // local header 必须完整落在归档内
      if (localOffset < 0 || localOffset + 30 > bytes.length || safeU32(localOffset) !== 0x04034b50) {
        return fail("ZIP local header 越界或签名不符，归档损坏");
      }
      const localFlags = safeU16(localOffset + 6);
      const localNameLen = safeU16(localOffset + 26);
      const localExtraLen = safeU16(localOffset + 28);
      if (localFlags === null || localNameLen === null || localExtraLen === null) {
        return fail("ZIP local header 字段越界，归档损坏");
      }
      // 本地头与中央目录的 general-purpose flags 必须一致——bit 3 有无决定
      // 大小字段语义（真实值 vs 占位），两侧不一致即拼接形态。
      if ((localFlags & 0x08) !== (cdFlags & 0x08)) {
        return fail(`ZIP 条目 "${entryName}" 本地头与中央目录的 data descriptor 标志不一致，归档被拼接或损坏`);
      }
      // 中央目录名与本地头名必须一致——不一致说明目录与数据错位或被拼接
      const localName = bytes.toString("utf8", localOffset + 30, localOffset + 30 + localNameLen);
      if (localName !== name) {
        return fail(`ZIP 中央目录名（"${name}"）与本地头名（"${localName}"）不一致，归档被拼接或损坏`);
      }
      // 本地头登记的压缩/未压缩大小必须与中央目录一致——不一致说明目录
      // 与数据错位或被拼接，按哪一份读都可能截断或越界。bit 3 流式归档的
      // 本地头大小是占位 0（真实值在描述符里），跳过此项、改由描述符核对。
      const localCompressedSize = safeU32(localOffset + 18);
      const localUncompressedSize = safeU32(localOffset + 22);
      if (localCompressedSize === null || localUncompressedSize === null) {
        return fail("ZIP local header 字段越界，归档损坏");
      }
      if (!hasDataDescriptor && (localCompressedSize !== compressedSize || localUncompressedSize !== uncompressedSize)) {
        return fail(
          `ZIP 条目 "${entryName}" 本地头大小（压缩 ${localCompressedSize}，未压缩 ${localUncompressedSize}）与中央目录（压缩 ${compressedSize}，未压缩 ${uncompressedSize}）不一致，归档被拼接或损坏`,
        );
      }
      if (hasDataDescriptor && (localCompressedSize !== 0 || localUncompressedSize !== 0 || safeU32(localOffset + 14) !== 0)) {
        return fail(
          `ZIP 条目 "${entryName}" 声明 data descriptor 但本地头 CRC/大小不是占位 0（${localCompressedSize}/${localUncompressedSize}），归档形态非法`,
        );
      }
      const dataStart = localOffset + 30 + localNameLen + localExtraLen;
      // 数据区间必须完整落在归档内
      if (dataStart < 0 || compressedSize < 0 || dataStart + compressedSize > bytes.length) {
        return fail(`ZIP 条目 "${entryName}" 数据区间越界（${dataStart} + ${compressedSize} > ${bytes.length}），归档损坏或被篡改`);
      }
      const raw = bytes.subarray(dataStart, dataStart + compressedSize);
      let content;
      if (method === 0) {
        content = Buffer.from(raw);
      } else if (method === 8) {
        try {
          // maxOutputLength：zlib 在展开过程中达到上限即中止（不先完整展开），
          // 超限抛 ERR_BUFFER_TOO_LARGE。
          content = zlib.inflateRawSync(raw, { maxOutputLength: maxEntryBytes });
        } catch (error) {
          return fail(`ZIP 条目 "${entryName}" 解压失败：${String(error?.message ?? error)}`);
        }
      } else {
        return fail(`ZIP 条目 "${entryName}" 使用不支持的压缩方式 ${method}，拒绝启动`);
      }
      // 内容完整性三重校验（十四轮复核）：
      //   - 解压后长度必须等于登记的未压缩大小（登记 0 就必须为空——
      //     "登记 0 但有内容"与"登记短于实际"同样拒绝）；
      //   - 内容 CRC-32 必须与登记值一致（截断/替换/位翻转在此暴露）。
      if (content.length !== uncompressedSize) {
        return fail(
          `ZIP 条目 "${entryName}" 解压后长度 ${content.length} 与登记 ${uncompressedSize} 不一致，内容损坏`,
        );
      }
      const crcActual = crc32Of(content);
      if (crcActual !== crcRegistered) {
        return fail(
          `ZIP 条目 "${entryName}" 内容 CRC（${crcActual.toString(16)}）与登记（${crcRegistered.toString(16)}）不一致，内容被篡改或损坏`,
        );
      }
      if (hasDataDescriptor) {
        // data descriptor 必须**完全位于条目数据之后、中央目录之前**
        // （十六轮复核问题 1）：描述符不能从中央目录借字节——那说明归档
        // 根本没有描述符，借来的是 CD 条目头。
        const dataEnd = dataStart + compressedSize;
        if (dataEnd > cdOffset) {
          return fail(
            `ZIP 条目 "${entryName}" 数据结束位置 ${dataEnd} 越过中央目录起点 ${cdOffset}，归档损坏`,
          );
        }
        const available = cdOffset - dataEnd;
        const minDescriptorBytes = 12; // 无签名形态的最小尺寸
        if (available < minDescriptorBytes) {
          return fail(
            `ZIP 条目 "${entryName}" 声明 data descriptor 但数据后只剩 ${available} 字节（需要至少 ${minDescriptorBytes}），描述符缺失，流式归档不完整`,
          );
        }
        // 两种合法形态都接受（ZIP 规范允许签名存在与否），但**不能用前 4 字节
        // 是否等于 0x08074b50 判定**——无签名描述符的 CRC 可能恰好等于该值
        // （十六轮复核问题 3）。改为两种解读都校验，取唯一与 CD 一致者。
        const matches = (offset) => {
          const ddCrc = safeU32(offset);
          const ddCompressed = safeU32(offset + 4);
          const ddUncompressed = safeU32(offset + 8);
          if (ddCrc === null || ddCompressed === null || ddUncompressed === null) return null;
          if (offset + 12 > cdOffset) return null;
          if (ddCrc !== crcActual) return null;
          if (ddCompressed !== compressedSize || ddUncompressed !== uncompressedSize) return null;
          return { crc: ddCrc, compressed: ddCompressed, uncompressed: ddUncompressed };
        };
        const signed = available >= 16 && safeU32(dataEnd) === 0x08074b50
          ? matches(dataEnd + 4)
          : null;
        const unsigned = matches(dataEnd);
        if (!signed && !unsigned) {
          const ddCrc = safeU32(dataEnd);
          const ddCompressed = safeU32(dataEnd + 4);
          const ddUncompressed = safeU32(dataEnd + 8);
          return fail(
            `ZIP 条目 "${entryName}" 的 data descriptor 与中央目录登记不一致（读到 CRC ${ddCrc === null ? "缺失" : ddCrc.toString(16)}，压缩 ${ddCompressed ?? "缺失"}，未压缩 ${ddUncompressed ?? "缺失"}；期望 CRC ${crcActual.toString(16)}，压缩 ${compressedSize}，未压缩 ${uncompressedSize}），归档被篡改或描述符缺失`,
          );
        }
      }
      found = content;
    }
    cursor += 46 + nameLen + extraLen + commentLen;
  }
  if (found === null) {
    return fail(`ZIP 归档内没有条目 "${entryName}"，无法核验 EXE，拒绝启动`);
  }
  return { ok: true, bytes: found };
}

/**
 * 受控绑定核验核心（八轮复核问题 1）：验证"EXE 产自某个成功 CI run 且未被
 * 篡改"。信任不在任何自报字段，而在 **GitHub 侧事实**——调用方把 GitHub API
 * 的 artifact 清单与 run 元数据注入（生产：真实 API 调用；合成测试：固定清
 * 单），本函数只做绑定判定：
 *
 *   record.builder === "ci"
 *     且 ci.run_id/repository 与 run 元数据一致
 *     且 run 元数据 conclusion === "success"、head_sha === record.source_commit
 *     且清单里存在登记名字的 artifact、其 digest === 本地产物哈希
 *   → "verified"（EXE 字节 = 该成功 run 在该提交上产出的字节，来源成立且未
 *     被篡改；不证明受保护工作流未被绕过——那落在 GitHub 托管 Runner 与分支
 *     保护的信任上）
 *   任一环节缺失/不一致 → "unverified"（fail closed，不虚标）
 *
 * 联网与 token 由调用方承担：该核验是**显式的受控步骤**（CI 独立 step 或
 * 本地 --verify-build-origin），不能塞进离线预检。当前未接到
 * native-acceptance.mjs——预检仍无 verified 出口（AC10 保持开放）。
 */
export function verifyBuildOriginBinding({
  record,
  runMetadata = null,
  jobList = [],
  artifacts = [],
  // 受信配置（十一轮复核问题 3 / 十二轮复核问题 1）：预期仓库、工作流身份
  // 与受测提交来自独立于构建记录的受信任清单（如发布配置），全部必填——
  // 缺失即无法证明"这次 run 是预期的那次"，即使记录与 GitHub 响应内部一致
  // 也拒绝。工作流身份用 workflow_id（数字 ID）核对——显示名（run.name）在
  // 不同工作流之间可重复，不能单独作为身份。
  trustedRepository = null,
  trustedWorkflowName = null,
  trustedWorkflowId = null,
  trustedHeadSha = null,
  buildStepName = "Build embedded frontend and isolated acceptance executable",
  trustedBuildJobName = "native-ui",
  artifactNamePrefix = "acceptance-build",
  // 字节输入（十一轮复核）：核验链从下载的归档字节、归档内 EXE 字节与本地
  // EXE 字节自行计算全部摘要——不接收任何现成哈希，杜绝"传入任意哈希冒充
  // 已验证来源"。哈希比较仍是纯函数语义，但输入只有字节。
  archiveBytes = null,
  archiveExeName = "tokenscope.exe",
  localExeBytes = null,
} = {}) {
  const unverified = (reason) => ({ ok: false, build_identity: "unverified", reason });
  const parsed = record?.ok === true ? record : parseAcceptanceBuildRecord(record);
  if (!parsed.ok) return unverified(`${parsed.reason}，拒绝启动`);
  const trusted = parsed.record;
  if (trusted.builder !== "ci") {
    return unverified("构建记录不是 CI 构建（builder=local），无 CI 事实可核验，拒绝启动");
  }
  const ci = trusted.ci ?? {};
  const runId = String(ci.run_id ?? "").trim();
  const recordRepository = String(ci.repository ?? "").trim();
  if (!runId || !recordRepository) {
    return unverified("CI 记录缺 run_id/repository，无法定位 GitHub 侧事实，拒绝启动");
  }
  if (!/^[^/\s]+\/[^/\s]+$/.test(recordRepository)) {
    return unverified(`repository 形态非法（期望 owner/name）：${recordRepository}，拒绝启动`);
  }

  // 完整来源证明链（九轮复核重写，按 GitHub 实际响应形态）：
  //
  // 1. 可信仓库/工作流/受测提交——全部来自 GitHub 侧响应，独立于可自填的
  //    构建记录：
  //    - run 响应顶层 `repository.full_name`（owner/repo）必须等于记录的
  //      repository（run 的 `name` 字段是**工作流名**，不是仓库，不能混用）；
  //    - 可信工作流 = run 响应顶层 `name`，必须等于调用方注入的受信工作流名
  //      （来自受信任清单，不由记录决定）；
  //    - 受测提交 = run 响应 `head_sha`，必须等于记录 source_commit。
  // 2. 构建与验证的排序：ui-e2e.yml 的构建是 `native-ui` **job 内的 step**
  //    （"Build embedded frontend…"），不是独立 job。核验读取 run 的 jobs
  //    响应：受信 job（native-ui）存在、其 `steps` 里构建 step
  //    conclusion=success、job 自身不处于 failure——**不要求整个 run
  //    success**（Native 场景 step 在构建 step 之后且可能仍在运行，等整体
  //    success 会死锁）。先后关系 = 构建 step 成功 → 核验（本函数）→
  //    Native 场景启动（预检）。
  // 3. artifact 与 EXE 的真绑定：GitHub 的 artifact digest 是对 **zip 归档**
  //    计算的（`sha256:` 前缀）；EXE 在归档内部，解压后对登记的 EXE 名重算
  //    得到归档内 EXE 摘要。归档摘要、归档内 EXE 摘要、记录 artifact_sha256、
  //    本地磁盘 EXE 哈希四者必须一致——归档内是 A 而记录/本地是 B 时拒绝。
  if (!runMetadata || typeof runMetadata !== "object") {
    return unverified("缺少 GitHub run 元数据（受控核验步骤未执行），拒绝启动");
  }
  if (String(runMetadata.id ?? "") !== runId) {
    return unverified(`run 元数据 id（${runMetadata.id ?? "缺失"}）与记录 run_id（${runId}）不一致，拒绝启动`);
  }
  // run 响应顶层 repository：{ full_name }（GitHub 实际形态）
  const runRepositoryFullName = String(
    runMetadata.repository?.full_name ?? runMetadata.repository ?? "",
  ).trim();
  if (!runRepositoryFullName) {
    return unverified("run 元数据缺 repository.full_name，无法确定可信仓库，拒绝启动");
  }
  // 受信仓库（独立于记录）必须先与 run 响应一致——非预期仓库即使内部字段
  // 一致也拒绝。
  if (!trustedRepository) {
    return unverified("缺少受信仓库配置（受信任清单注入），无法证明 run 来自预期仓库，拒绝启动");
  }
  if (runRepositoryFullName !== String(trustedRepository)) {
    return unverified(
      `run 的仓库（${runRepositoryFullName}）与受信仓库（${trustedRepository}）不一致，拒绝启动`,
    );
  }
  if (runRepositoryFullName !== recordRepository) {
    return unverified(
      `run 的 repository.full_name（${runRepositoryFullName}）与记录 repository（${recordRepository}）不一致，拒绝启动`,
    );
  }
  // 可信工作流：run 响应顶层 name 是工作流名（不是仓库），不同工作流可
  // 显示同名——身份必须用 workflow_id（数字 ID，GitHub run 响应实际字段）
  // 核对；显示名仅作附加一致性检查。
  if (!trustedWorkflowId) {
    return unverified("缺少受信工作流 ID（受信任清单注入），无法证明 run 来自正确工作流，拒绝启动");
  }
  if (String(runMetadata.workflow_id ?? "") !== String(trustedWorkflowId)) {
    return unverified(
      `run 的工作流 ID（${runMetadata.workflow_id ?? "缺失"}）与受信工作流 ID（${trustedWorkflowId}）不一致——相同显示名的工作流不是同一身份，拒绝启动`,
    );
  }
  if (!trustedWorkflowName) {
    return unverified("缺少受信工作流名（受信任清单注入），无法证明 run 来自正确工作流，拒绝启动");
  }
  if (String(runMetadata.name ?? "") !== String(trustedWorkflowName)) {
    return unverified(
      `run 的工作流名（${runMetadata.name ?? "缺失"}）与受信工作流名（${trustedWorkflowName}）不一致，拒绝启动`,
    );
  }
  const commit = String(trusted.source_commit).toLowerCase();
  // 受测提交：受信配置的 head_sha 必须与 run 响应 head_sha 一致（非预期
  // 提交即使记录与响应内部一致也拒绝）。
  if (!trustedHeadSha) {
    return unverified("缺少受信受测提交（受信任清单注入），无法证明 run 构建自预期提交，拒绝启动");
  }
  if (String(runMetadata.head_sha ?? "").toLowerCase() !== String(trustedHeadSha).toLowerCase()) {
    return unverified(
      `run head_sha（${runMetadata.head_sha ?? "缺失"}）与受信受测提交（${trustedHeadSha}）不一致，拒绝启动`,
    );
  }
  if (String(runMetadata.head_sha ?? "").toLowerCase() !== commit) {
    return unverified(
      `run head_sha（${runMetadata.head_sha ?? "缺失"}）与记录 source_commit（${commit}）不一致，拒绝启动`,
    );
  }

  // jobs 响应形态：{ jobs: [...] } 或直接数组；受信 job（native-ui）的 steps
  // 里构建 step conclusion=success。job 自身结论不要求 success（Native 场景
  // step 可能在跑），但构建 step 必须成功。
  const jobsArray = Array.isArray(jobList) ? jobList : Array.isArray(jobList?.jobs) ? jobList.jobs : [];
  const trustedJob = jobsArray.find((j) => j?.name === trustedBuildJobName);
  if (!trustedJob) {
    return unverified(`run 的 job 清单中没有受信 job "${trustedBuildJobName}"，拒绝启动`);
  }
  // job 的 run_id 必须与受检 run 一致——不同 run 的 job 不得为本核验提供
  // 构建成功证据（GitHub jobs 响应实际字段 run_id）。
  if (String(trustedJob.run_id ?? "") !== runId) {
    return unverified(
      `受信 job 的 run_id（${trustedJob.run_id ?? "缺失"}）与受检 run（${runId}）不一致——该 job 来自另一次 run，拒绝启动`,
    );
  }
  const steps = Array.isArray(trustedJob.steps) ? trustedJob.steps : [];
  const buildStep = steps.find((s) => s?.name === buildStepName);
  if (!buildStep) {
    return unverified(`受信 job "${trustedBuildJobName}" 的 steps 中没有构建 step "${buildStepName}"，拒绝启动`);
  }
  if (buildStep.conclusion !== "success") {
    return unverified(
      `构建 step 结论为 ${buildStep.conclusion ?? "未知"}（期望 success），拒绝启动`,
    );
  }

  // artifact 清单（GET .../runs/{id}/artifacts 的 artifacts 数组）。
  if (!Array.isArray(artifacts) || artifacts.length === 0) {
    return unverified("该 run 没有任何 artifact，无法建立产物绑定，拒绝启动");
  }
  const shortCommit = commit.slice(0, 7);
  const expectedArtifactName = `${artifactNamePrefix}-${shortCommit}`;
  const artifact = artifacts.find((a) => a?.name === expectedArtifactName);
  if (!artifact) {
    return unverified(
      `run 的 artifact 清单中没有 "${expectedArtifactName}"（绑定受测提交，防跨提交串用），拒绝启动`,
    );
  }
  if (String(artifact.workflow_run?.id ?? "") !== runId) {
    return unverified("artifact 不属于记录的 run，拒绝启动");
  }
  const archiveDigestRaw = String(artifact.digest ?? "").trim();
  if (!archiveDigestRaw) {
    return unverified("artifact 没有 digest（GitHub 侧未背书归档摘要），拒绝启动");
  }
  const archiveDigest = archiveDigestRaw.startsWith("sha256:") ? archiveDigestRaw.slice(7) : null;
  if (!archiveDigest || !/^[0-9a-f]{64}$/.test(archiveDigest)) {
    return unverified(
      `artifact digest 形态非法（期望 "sha256:<64位十六进制>"）：${archiveDigestRaw}，拒绝启动`,
    );
  }
  // 核验链（十一轮复核）：全部摘要从字节自行计算——
  //   1. 归档摘要 = sha256(archiveBytes)，与 GitHub 登记的 digest 核对；
  //   2. 归档内 EXE 摘要 = sha256(readZipEntry(archiveBytes, EXE 名))，
  //      与记录 artifact_sha256 核对（归档内 A 而记录 B 在此被拒）；
  //   3. 本地 EXE 摘要 = sha256(localExeBytes)，与记录核对（受测 EXE 被
  //      替换在此被拒）。
  if (!archiveBytes || !Buffer.isBuffer(archiveBytes) || archiveBytes.length === 0) {
    return unverified("缺少下载的归档字节（受控核验步骤未提供），拒绝启动");
  }
  const archiveSha256 = crypto.createHash("sha256").update(archiveBytes).digest("hex");
  if (archiveDigest !== archiveSha256) {
    return unverified(
      `归档摘要与 GitHub 登记不一致（登记 ${archiveDigest.slice(0, 12)}…，重算 ${archiveSha256.slice(0, 12)}…），拒绝启动`,
    );
  }
  const recordExeDigest = String(trusted.artifact_sha256).toLowerCase();
  const extracted = readZipEntryBytes(archiveBytes, archiveExeName);
  if (!extracted.ok) {
    return unverified(`${extracted.reason}（登记 EXE 名 ${archiveExeName}）`);
  }
  const archiveExeSha256 = crypto.createHash("sha256").update(extracted.bytes).digest("hex");
  if (archiveExeSha256 !== recordExeDigest) {
    return unverified(
      `归档内 EXE 摘要（${archiveExeSha256.slice(0, 12)}…）与记录 artifact_sha256（${recordExeDigest.slice(0, 12)}…）不一致——归档内容与记录不符，拒绝启动`,
    );
  }
  if (!localExeBytes || !Buffer.isBuffer(localExeBytes) || localExeBytes.length === 0) {
    return unverified("缺少实际受测 EXE 的字节（本地磁盘读取），拒绝启动");
  }
  const localSha256 = crypto.createHash("sha256").update(localExeBytes).digest("hex");
  if (localSha256 !== recordExeDigest) {
    return unverified(
      `实际受测 EXE 哈希（${localSha256.slice(0, 12)}…）与记录（${recordExeDigest.slice(0, 12)}…）不一致，EXE 已被替换，拒绝启动`,
    );
  }
  return {
    ok: true,
    build_identity: "verified",
    reason: `EXE 字节与 CI run ${runId}（提交 ${commit.slice(0, 12)}…，工作流 ${trustedWorkflowName}#${trustedWorkflowId}，构建 step 成功）的 artifact "${expectedArtifactName}" 绑定一致——归档摘要、归档内 EXE 摘要（从归档字节解压重算）、记录与受测 EXE（本地字节重算）四方核对通过，来源成立`,
  };
}
/**
 * EXE 验收构建筛查：PE + acceptance 标记 + 无普通构建 stub 标记只是**预筛**。
 * 追加了标记的普通 PE、或缺 stub 标记都不能授予启动许可；许可只能来自
 * verifyAcceptanceBuildIdentity 的 `verified` 结论。
 */
export function verifyAcceptanceExecutable(
  exePath,
  { readFile = fs.readFileSync, buildRecord = null, sourceCommit = null } = {},
) {
  const raw = String(exePath ?? "").trim();
  if (!raw) {
    return { ok: false, reason: "EXE 路径为空", launch_allowed: false, build_identity: "unverified" };
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
    reason: `构建身份已验证：${identity.reason}（标记预筛命中 ${positive.length} 个）`,
    markers: positive,
    negative_markers: [],
    marker_screened: true,
    launch_allowed: true,
    build_identity: "verified",
    identity,
  };
}

/**
 * 启动前预检：完整路径隔离（固定布局 + manifest 声明 + 现有条目别名）与实际
 * EXE 构建身份。attach 模式不启动 EXE，只校验路径；非 attach 模式必须拿到
 * `verified` 身份，`marker-screened` 一律不允许启动。
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
