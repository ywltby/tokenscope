# S1b · TokenTracker 值得直接照抄的实现细节

> 本文是 [S1-pipeline-and-collection.md](./S1-pipeline-and-collection.md) 的实用附录：从 TokenTracker（v1.3.14，HEAD `b19d27b0`）采集管线里筛出**可以做工程照抄**的具体机制。
> 每条按同一模板写：**要解决的问题 → 实现要点（行号 + 关键代码）→ 为何这样设计 → 移植注意（含 Rust / Windows 等价物）**。
> 排序规则：并发正确性 > 数据完整性 > 性能 > 兼容性；越靠前越通用、越靠前越值得先抄。只做“能否照抄”的工程判断，不做风格评价；无法从源码确认的一律标 `【未能确认】`。
> 所有行号相对仓库根，来自只读核验。
> **关于各节「移植注意」的前置说明**：以下平台/语言层断言（Rust 与 tokio 的语义差异、Windows/Unix 系统调用与时间戳粒度、NFS/SMB 行为、`rusqlite`/`blake3` 等）**无本仓库证据，属工程推断**，与逐条核对过的源码事实**不是同一证据等级**；只有带行号引用、且确能在样本仓库中命中的句子才属于已验证事实。

## 1. 锁：独占创建 + 持有者令牌 + 心跳 + 独立的回收闸

**要解决的问题**：多个生产者（hook 触发的 sync、60s 后台轮询、本地 API 触发的 sync、自动重试进程）会同时想跑同一份解析。同时还要解决三件衍生问题：进程被 `kill -9` 后锁文件残留、PID 被复用导致误判、以及“回收残留锁”这个动作本身需要互斥。

**实现要点**（`src/lib/fs.js:69-74,284-414`）：

```js
const LOCK_STALE_MS = 5 * 60 * 1000;      // 心跳超期即视为遗弃
const LOCK_HEARTBEAT_MS = 30 * 1000;
const MAX_RECLAIM_DEPTH = 4;              // .reclaim 链最大嵌套
const MAX_RECLAIM_SWEEP_DEPTH = 64;       // 迭代清扫链上限
```

- 加锁用 `fs.open(lockPath, "wx")`（O_EXCL 语义），文件内容与心跳文件都带**随机 token**（`:296-306`）：

```js
const handle = await fs.open(lockPath, "wx");
const token = crypto.randomUUID();
const heartbeatPath = heartbeatPathFor(lockPath, token);   // `${lockPath}.heartbeat.${sha256(token)}`
await handle.writeFile(JSON.stringify({ pid: process.pid, token, createdAt: ... }) + "\n", "utf8");
heartbeatHandle = await fs.open(heartbeatPath, "wx");
```

- 心跳 30s 一次，同时 `utimes` 锁文件与心跳文件，`timer.unref()`（`:206-224`）。
- 回收判定：先看 PID 是否存活（`process.kill(pid, 0)`，`ESRCH` 才算死，`EPERM` 保守当作存活），再看**token 专属心跳**是否超过 `LOCK_STALE_MS`；老格式锁（无心跳）退回“mtime 超期”判断（`:105-144`）。
- 回收动作本身用 `.reclaim` 次级锁串行化，且**持闸后重新校验一次**（TOCTOU）：`if (!(await existingLockCanBeReclaimed(lockPath))) return null;`（`:342-368`）。
- 释放时同样持 `.reclaim` 闸，并且**只有 token 匹配才 unlink**（`:264-275`）：

```js
const owner = parseLockOwner(await fs.readFile(lockPath, "utf8"));
if (owner?.token === token) {
  if (typeof beforeReleaseUnlink === "function") await beforeReleaseUnlink({ lockPath });
  await fs.unlink(lockPath).catch(() => {});
  await fs.unlink(heartbeatPathFor(lockPath, token)).catch(() => {});
}
```

- 被 kill 留下的**嵌套** `.reclaim` 闸会永久堵死后续加锁（issue #431），因此有一次性迭代清扫：从深到浅只删“owner 已死或心跳超期”的那一级（`:178-204`）。

**为何这样设计**：单纯的“PID 存活 + 时间阈值”无法区分**PID 复用**（新进程顶了旧号）与**长时间运行但健康的锁**；加入 token 专属心跳文件后，“这个锁还活着吗”变成了“这个 token 的心跳还新鲜吗”，两个问题被解耦。`.reclaim` 闸解决的是“多个收割者同时抢同一份残留”的竞态——没有它，两个进程可能各自把一份锁搬走，其中一个误删了新安装的锁。

**移植注意**：

- Rust 里对应物：`OpenOptions::new().write(true).create_new(true).open()`（等价 `wx`）；令牌用 `uuid` 或 `rand`；心跳用 `tokio::time::interval` 并显式 `abort()`（`unref` 的等价物是不要让定时任务阻止进程退出——Rust 里天然如此，只要不 `block_on` 等待它）。
- 判断进程存活：Rust 没有 `kill(pid, 0)` 的 std 等价物；Linux 用 `kill(pid, 0)` 系统调用（`nix::sys::signal::kill`）或读 `/proc/<pid>`；Windows 用 `OpenProcess(SYNCHRONIZE)` + `WaitForSingleObject(0)` 判断信号状态，或 `QueryFullProcessImageName` 校验进程身份。**Windows 上 PID 复用更频繁，务必配 token 心跳，不要只信 PID。**
- Windows 的 `unlink` 在文件仍被打开时会失败，因此释放顺序是“先关句柄再删文件”（源码即如此，`:266-267`）；Rust 里要把 `File` drop 掉再 `remove_file`。
- 若要跨机共享锁（NFS/SMB），`.reclaim` 那套基于 `rename` 的原子性假设会失效【未能确认：源码未覆盖网络文件系统语义】。

## 2. 目录探测：把“存在”定义为“能被列举”

**要解决的问题**：判断一个目录是否该进入扫描。只做 `stat` 会踩坑——一个权限为 `000`（或 ACL 拒绝）的目录 `stat` 成功，于是被判为“存在”，进扫描后产出 0 个文件，在状态页显示“存在”，还会**绕过后续的 deferred-repair 守卫**（守卫的语义是“根不存在就不许重算历史”）。

**实现要点**（`src/lib/scan-roots.js:89-110`）：

```js
function scanRootDirState(target, deps = {}) {
  const statSync = deps.statSync || fs.statSync;
  const opendirSync = deps.opendirSync || fs.opendirSync;
  try {
    if (!statSync(target).isDirectory()) return { exists: false, error: null };
    // Probe listability without reading entries; large roots stay cheap.
    opendirSync(target).closeSync();
    return { exists: true, error: null };
  } catch (e) {
    const code = e && typeof e.code === "string" ? e.code : "EUNKNOWN";
    if (code === "ENOENT" || code === "ENOTDIR") return { exists: false, error: null };
    return { exists: false, error: code };
  }
}
```

三态语义是重点：`{exists:true}` / `{exists:false, error:null}`（真不存在）/ `{exists:false, error:"EACCES"}`（存在但读不了）。第三种在 UI 上必须显示“unreadable: EACCES”而不是“missing”（`src/lib/scan-roots.js:248-253`）。

**为何这样设计**：`opendir().close()` 只取目录句柄、不读条目，成本与 `stat` 同阶，却把“能否读取”变成可证事实。区分 `error` 与“不存在”后，调用方能做出**不同决策**：不存在可以跳过，读不了必须推迟重算（否则会在数据不全的情况下重写历史）。

**移植注意**：Rust 用 `std::fs::read_dir(path)` 后立刻 `drop` 迭代器即可——`read_dir` 成功返回标志着能打开目录，不需要真的迭代。注意 `read_dir` 在 Windows 对文件返回错、对目录权限拒绝返回 `PermissionDenied`，与 POSIX 的 `EACCES` 对应；把 `ErrorKind::NotFound` 与 `PermissionDenied` 分开映射到“不存在/不可读”两态。**不要用 `Path::exists()`**——它把“不可读”和“不存在”都吞成 `false`。

## 3. 追加型文件的读侧：签名缓存 + 锚点续读探针 + 主动让出事件循环

**要解决的问题**：Dashboard 一次刷新会并发打多个端点，每个端点都要读同一个 append-only 的 `queue.jsonl`（可能几十 MB、几十万行）。朴素做法是每次请求全量读 + 全量 `JSON.parse`，直接把事件循环打满。

**实现要点**（`src/lib/async-queue-reader.js`）：

```js
// :8-10 文件签名（bigint stat）
function signature(stat) {
  return `${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:${stat.ctimeNs}`;
}
```

```js
// :40-47 只有“同 dev:ino + size 变大 + 头部与锚点内容一致”才从 consumedBytes 续读
if (cached && cached.identity === identity && size > cached.size && cached.consumedBytes > 0) {
  const head = await probe(file, 0, cached.head.length);
  const anchor = await probe(file, cached.consumedBytes - cached.anchor.length, cached.anchor.length);
  if (head.equals(cached.head) && anchor.equals(cached.anchor)) {
    offset = cached.consumedBytes;
    seen = cached.seen;
  }
}
```

配套三条：

- **只把完整行计入已消费位置**：只有遇到 `\n` 才推进 `consumedBytes`，尾部残行单独放在 `tail`，且其 key **不进已提交 map**（`:64-70,87-91`），因为写者可能还没写完这行、键值还可能变。
- **分块让出**：`CHUNK_BYTES = 64 * 1024`，每读一块 `await yieldToRequests()`（`node:timers/promises` 的 `setImmediate`，`:3,79`）。
- **并发合并 + LRU**：同一路径的并发请求共享同一次读取（`pending` map，`:109-114`）；缓存最多 4 个文件（`:16,94`）；出错时回退到上一份缓存行而不是报错（`:102-103`）。

**为何这样设计**：这里其实是三个独立问题被分别解决——① 重复读用签名缓存；② 增量读用“锚点探针”而不是“相信 offset 仍然有效”（文件可能被原子替换或压缩重写）；③ 事件循环饥饿用显式让出。最妙的是残行策略：**“可见行集合”与“已消费字节位置”分离**，于是读者不需要回退，也不会把半行当成数据。

**移植注意**：

- Rust 的 `tokio` 里，`tokio::task::yield_now()` 是 `setImmediate` 的等价物，把控制权交回调度器；但**语义不完全等价**：Node 的 `setImmediate` 保证让出到下一个事件循环轮次（允许 IO 回调插入），`yield_now()` 只是让出一次 poll。若目标是“不让长任务饿死其它任务”，`yield_now()` 足够；若目标是“让 socket 事件优先处理”，需要 `tokio::time::sleep(Duration::ZERO)` 或显式 `tokio::select!` 结构。
- 如果我们的读侧有**同步**的 SQLite 访问，光靠 `yield_now()` 没用——必须像本项目那样把重活丢到 `spawn_blocking`（对照 `src/commands/serve.js:311-315` 的注释：SQLite 用 `execFileSync` 会冻住整个 HTTP 事件循环，所以他们把轮询 sync 放进子进程）。
- 签名里的 `mtimeNs`：Node 的 `stat({bigint:true})` 才给纳秒；Rust `std::fs::Metadata` 只有 `SystemTime`（Windows 上 NTFS 时间粒度通常 100 ns，Linux 视挂载选项可能只有秒级）。**不要假设时间戳有纳秒唯一性**，需要强判据时用内容锚点（如本条的 head/anchor）。

## 4. 压缩追加型大文件：行边界 + 原子替换 + ino/size/mtime 竞态校验

**要解决的问题**：append-only 队列会无限增长（尤其有历史重算的日子里，同一小时键会被反复重写）。需要在不打断并发写者、不破坏上传字节 offset 的前提下，把“同键的旧行”删掉。

**实现要点**（`src/lib/upload-queue.js:64-132`）：

```js
// :73-78 只在“够大 + offset 落在行边界”时动手
if (!before.isFile() || !Number.isSafeInteger(offset) || offset < 0 || offset >= before.size ||
    pendingBytesBefore < minPendingBytes) return unchanged;      // minPendingBytes 默认 1_000_000
if (offset > 0) {
  const boundary = Buffer.alloc(1);
  await input.read(boundary, 0, 1, offset - 1);
  if (boundary[0] !== 10) return unchanged;                      // offset-1 必须是 '\n'
}
```

```js
// :111-121 替换前后的竞态校验
await output.sync();
await output.close();
output = null;
// Windows cannot replace the destination while our read handle is open.
await input.close();
input = null;
const current = await fs.stat(queuePath);
if (current.ino !== before.ino || current.size !== before.size || current.mtimeMs !== before.mtimeMs) {
  throw new Error("Queue changed during pending compaction");
}
await fs.rename(temporary, queuePath);
```

保留语义是显式的：`[0, offset)` 原样字节复制（已上传历史不变），`[offset, size)` 里每个键只保留**最后一条**（`:79-91,105-110`），并且 `account_session_state` 用的比较键是 `verifiedMs` 严格更大而非“最后一条”（防时钟回拨）。

**为何这样设计**：直接 `truncate`/重写整文件会破坏 `queue.state.json` 的字节 offset 语义（offset 是“已上传到第几字节”）。只在 `offset` 之后的 pending 区做整理，就把“压缩”与“上传进度”完全解耦。最后的 `ino/size/mtime` 三重校验是对“压缩期间有写者追加”的兜底——**若不校验，一次并发 append 就会永久丢失刚刚写入的行**。

**移植注意**：

- Rust 等价实现：`OpenOptions::new().create_new(true)` 建临时文件 → 写入 → `File::sync_all()` → `drop`（关闭读句柄）→ `fs::metadata` 校验 → `fs::rename`。Rust 的 `fs::rename` 在 Windows 上走 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`，可覆盖已存在的**文件**；但若目标仍被打开会失败——这与 Node 面临的限制同源，所以“先关句柄再 rename”的顺序同样必要。
- `File::sync_all()`（fsync）在压缩场景是必需的：否则机器掉电后可能出现“新文件存在但内容为空/半写”。
- 别用“读全文件到内存再写回”的实现替代——队列可能是几百 MB，且那会放大竞态窗口。
- 若改用 `renameat2(RENAME_EXCHANGE)` 之类的原子交换，可省掉一次 stat 校验，但只有 Linux 支持【未能确认：本项目未采用该路线】。

## 5. 原子写 JSON：tmp 名带 UUID + rename + 失败清理

**要解决的问题**：并发写同一个 JSON 状态文件时会互相踩（半写、丢失、以及 `Date.now()` 冲突）。项目里有多个这样的文件：`queue.state.json`、`upload.throttle.json`、`config.json`、`cursors.json`、`sync.skip.json`。

**实现要点**（`src/lib/fs.js:9-33`）：

```js
async function writeFileAtomic(filePath, content, { mode } = {}) {
  const dir = path.dirname(filePath);
  await ensureDir(dir);
  // Include a UUID so two writes in the same millisecond do not share a tmp
  // path (Date.now() alone collides under concurrent writers).
  const tmp = `${filePath}.tmp.${Date.now()}.${crypto.randomUUID()}`;
  try {
    await fs.writeFile(tmp, content, mode == null ? { encoding: "utf8" } : { encoding: "utf8", mode });
    await fs.rename(tmp, filePath);
  } catch (err) {
    try { await fs.unlink(tmp); } catch { /* tmp may not exist */ }
    throw err;
  }
}
```

配套：`updateJsonLocked` 用 `<file>.lock` 做“读-改-写”整体互斥，超时 30s（`src/lib/fs.js:416-443`），写后尽力 `chmod 600`。

**为何这样设计**：`rename` 在同一文件系统内是原子的，读者永远看到完整旧版或完整新版。tmp 名追加 UUID 是因为只靠 `Date.now()` 会在同一毫秒内冲突，导致两个写者写同一个 tmp 文件并互相覆盖内容；失败路径必须删 tmp，否则崩溃会在数据目录里留下垃圾（项目里多个 `*.tmp.*` 会污染目录枚举）。

**移植注意**：

- Rust：写 `path.with_extension("tmp-<uuid>")` → `sync_all` → `rename`。注意 `std::fs::rename` 在跨文件系统时会失败（`CrossesDevices`），所以 tmp 必须与目标**同目录**（本实现正是如此，用 `path.dirname`）。
- 权限：Node 的 `mode` 只在创建时生效（源码注释特别说明这点，`src/lib/fs.js:15-17`），避免“先创建为 0644 再 chmod”的窗口。Rust 用 `OpenOptions::mode(0o600)`（Unix）或在创建后立刻设置 ACL（Windows）。
- 若同一个 JSON 有多个写者，**光有原子写不够**，还需要 `updateJsonLocked` 那种读改写锁，否则会丢失更新。

## 6. 三代保留 + 清单原子切换的“代际存储”

**要解决的问题**：游标状态（几十万文件游标 + 事件去重集合）既大又关键。用单个 JSON 文件更新会出现两个问题：① 每次提交都全量重写、写放大严重；② 写一半崩溃就得到损坏状态，且没有回退点。

**实现要点**（`src/lib/cursor-store.js:13-23,855-1016`）：

```js
const STORE_VERSION = 2;
const STORE_DIRNAME = "cursor-store-v2";
const MANIFEST_FILENAME = "manifest.json";
const GENERATIONS_DIRNAME = "generations";
const SHARDED_CORE_FILENAME = "core-v3.json";
```

提交流程（`:855-989`）：新代目录 `gen-<ms>-<uuid8>` → 从不需改写的旧代分片**整文件克隆** → 只重写脏分片 → 写 `core-v3.json` → 写 `generation.json`（含各分片 count 与完整性摘要）→ **最后**原子替换 `manifest.json`：

```js
const nextManifest = {
  version: STORE_VERSION,
  current: generationId,
  rollback: this.generation?.id || null,
  // Older v2 readers reject core-v3.json and fall back to this frozen,
  // fully-inline checkpoint instead of opening an empty message index.
  previous: compatibleGeneration,
  legacyFingerprint: this.manifest?.legacyFingerprint || null,
  updatedAt: new Date().toISOString(),
};
await writeJson(path.join(this.storeRoot, MANIFEST_FILENAME), nextManifest);
published = true;
```

- 未 publish 前的任何失败都会 `fs.rm(generationDir, {recursive:true, force:true})` 清理半成品（`:1010-1015`）。
- 成功后只保留 `current/rollback/previous` 三代，其余清理（`:1005-1009`）。
- 读取侧依次尝试 `current → rollback → previous`（`:124-139`）；分片完整性用 metadata 里的摘要校验（`buildShardIntegrity` `:1466-1471`、`assertShardIntegrity` `:1473-1481`）。
- 外部改动检测：`legacyFingerprint` 记录旧 `cursors.json` 的指纹，一旦被外部改过就重新迁移（`:44-58`）。

**为何这样设计**：“清单指向哪一代”是唯一的原子切换点，代目录内部的多个分片写入无需原子——因为**还没被任何人指向**。这让“大状态 + 多分片 + 崩溃安全”三件事同时成立，代价是磁盘上最多存在 3 份数据（旧代分片由 `cloneGenerationFiles` → `cloneOrCopy` 克隆，`cloneOrCopy` 是 `await fs.copyFile(source, target, fssync.constants.COPYFILE_FICLONE)`，`:1361`——**reflink / 写时复制**，文件系统不支持时自动回退为普通复制，**与硬链接无关**；克隆循环 `cloneGenerationFiles` 在 `:1349-1355`，逐文件调 `cloneOrCopy`（`:1357-1362`））。

**移植注意**：

- Rust：同一目录下建 `generations/<id>/`，写完后 `sync_all` 目录（Linux 需要 `File::open(dir)?.sync_all()` 来持久化 rename 与新建目录项；Windows 无需），再原子替换 `manifest.json`。用 `serde` 序列化分片，把摘要（如 `blake3`）写进 metadata。
- 分片键必须**白名单化**再拼路径（本项目用 `safeShardFilename` 只允许 `^\d{4}-\d{2}-\d{2}$`，否则写 `misc`，`src/lib/cursor-store.js:1807-1809`）——否则一个来自文件路径的分片键就能变成路径穿越。
- **克隆语义要看清**：本项目用 `COPYFILE_FICLONE`（CoW/reflink），**不是硬链接**，因此“旧代被新代改写波及”这一风险在本实现里不存在；真正需要防守的是“**对共享存储做原地改写**”。本项目的脏分片是**整文件重写**：先 `await fs.unlink(targetPath)`（`:907`）再新建写入，所以旧代始终自洽。Rust 侧对应要求：分片重写必须“先删再建（或写临时文件后 rename）”，不要 `truncate` 后原地覆盖；**若**你改用硬链接做克隆，这条就从“建议”变成“必须”。

## 7. 一次性历史重算的“数据不全就不修”守卫

**要解决的问题**：解析口径变化后，唯一正确的修法是“按当前口径重算历史”。但重算的前提是**磁盘上还留着当初的数据**——这个前提经常不成立（用户清过日志、日志被产品自身滚动删除、归档目录不在扫描范围内、远端根临时不可用）。项目在这里翻过车：0.26.3 版本的重算把 `~/.claude` 日志已被清理的小时从队列中**静默抹掉**，一位用户损失 21.7 亿 claude token（`src/commands/sync.js:234-248`）。

**实现要点**：三件套守卫 + 一条“推迟而非跳过”的原则。

① **可行性预检**：每个 repair 先用廉价信号判断是否真的需要/能够执行。

```js
// src/commands/sync.js:258-268
// One-time repair (#187): until the codexHashes event-dedup landed, a Codex
// session file rewritten with a new inode ... was re-scanned from offset 0 ...
// GUARDED: skips if any codex session file that previously contributed is no longer on disk
// (deleted, or moved to ~/.codex/archived_sessions/ which sync does not scan) — clearing its bucket
// would lose that history (ref the v6 ground-truth-repair data-loss incident).
```

② **扫描根可用性守卫**：非 native 的根当前 `!exists` 或不可读、且该根历史上贡献过文件 → **推迟**（不设迁移键，等下次全量再试），普通扫描照常进行（`src/commands/sync.js:1147-1192`）：

```js
const unavailableClaudeRoots = scanRoots.claude
  .filter((entry) => {
    if (entry.origin === "native") return false;
    if (!entry.exists) return true;
    const projectsState = scanRootDirState(path.join(entry.path, "projects"));
    if (projectsState.error !== null) return true;
    return !projectsState.exists && rootPreviouslySuppliedFiles(entry.path);
  })
  .map((entry) => entry.path);
if (unavailableClaudeRoots.length > 0) { /* 打印 deferred 提示，不修 */ }
else { await repairClaudeQueueFromGroundTruth({ ... }); }
```

③ **重建前后的文件快照校验**：重建前记录每个输入文件的快照（`readCodexFileSnapshot`/`sameCodexFileSnapshot`，`:6271-6293`），重建后逐文件比对（`validateCodexRebuildFileSnapshots`，`:6294-6315`），并检查“历史是否被盘上数据覆盖”（`isCodexHistoryCovered`，`:6316-6346`）。另有 `scanForForkedCodexRollout` 这类廉价预检避免无谓重建（`:6347-6370`）。

④ 迁移键本身是**幂等闸**：`cursors.migrations[KEY]` 存在即不再跑；失败路径**故意不写**该键，让下次全量重试（`:952-959` 的 sentinel 是“本轮跳过但留待重试”的显式表达）。

**为何这样设计**：一次性迁移的失败模式有两类——“该修没修”（可恢复，下次再跑）与“不该修的修了”（**不可恢复的数据丢失**）。守卫的取舍是：宁可把修复推迟到条件齐备，也绝不基于残缺数据重写历史。这直接推翻了“全量重算=安全”的直觉。

**移植注意**：

- 任何 `DELETE`/重写历史的操作都应先证明“新数据覆盖了旧数据的全部时间范围”，而不是相信“盘上还有日志”。Rust 侧建议把守卫写成返回 `Result<RepairDecision>` 的独立函数并在测试里覆盖“根不可读/文件被删/文件被移到不扫的目录”三种情形。
- “推迟”的实现要显式可观测：本项目会打印 `Claude ground-truth repair deferred: configured scan root(s) unavailable: ...`（`:1177-1181`），并且把 sentinel 记进 cursor（`skipped: true, reason: "background_repair_requires_full_sync"`，`:953-958`）。**不要把推迟做成静默 return**。**但要注意前提**：这行提示被 `if (!opts.auto)` 包住（`src/commands/sync.js:1177-1181`），即 **`--auto`（hook / 后台轮次）下是静默推迟的**——可观测性只在人工执行 `tracker sync` 时成立。移植时若要求后台轮次也留痕，应写 marker / 状态文件，而不是仅靠 stderr 打印。
- 若我们的重算会同时改本地库与云端，务必把“重放/重传”也做成幂等 upsert（本项目云端整行 upsert，`src/commands/sync.js:249-256`），否则守卫只保护了一半。

## 8. 事件级去重：路径无关身份 + “追加视图/历史视图”双闭包

**要解决的问题**：日志文件会被原子替换（新 inode）、会被截断重写、同一个会话还可能出现在两条路径下（native 安装 + WSL 安装、`sessions/` 与 `archived_sessions/`）。仅靠“文件字节 offset”无法保证不重不漏。

**实现要点**（`src/lib/rollout.js:302-380`）：去重集合按“**本轮新键**”与“**历史键**”分成两种视图。

```js
const recordNewEvent = (key) => {
  ensureNewEventKeys();
  if (newEventKeySet.has(key)) return;
  newEventKeySet.add(key);
  newEventKeys.push(key);
  if (seenEvents) seenEvents.add(key);
  externalEventStore?.add(key);
};
const getAppendOnlyEvents = () => ({ has: (key) => Boolean(newEventKeySet?.has(key) || seenEvents?.has(key)), add: recordNewEvent });
```

关键在**何时选历史视图**（`src/lib/rollout.js:428-461`）：

```js
// Same session under another cursor path — the two roots of a `union` install ...
// This MUST be decided before the steady-state short-circuit below: the
// append-only tracker only knows keys written during the current run ...
const sessionId = codexSessionIdFromPath(filePath);
if (!sessionId) return true;
const knownPaths = getCursorSessionPaths().get(sessionId);
if (knownPaths) { for (const knownPath of knownPaths) { if (knownPath !== filePath) return true; } }
// Steady state: one path, already read past, not rotated.
if (sameInode && !truncated && startOffset > 0) return false;
if (prev?.lastTotal) return true;   // :459
return false;                       // :460
```

即：**只有“单路径 + 同 inode + 未截断 + 有历史 offset”四条同时成立时**才允许走廉价的本轮集合；但这四条之后还有两条兜底——`if (prev?.lastTotal) return true;`（`:459`）与 `return false;`（`:460`），合计 `src/lib/rollout.js:456-460`。含义是：**稳态四条件不成立时，只要游标里带着 `prev.lastTotal`（Codex 累计基线）就仍然动用持久集合**，两条都不命中才回落到本轮集合。【未能确认】`:459` 这条兜底的确切动机源码未加注释，推测与 `rebuildingCodexBaseline`（§4.5）那条“从 0 重读”的路径有关。

两套身份键（按 provider 不同粒度）：

- Codex：`sessionUUID:eventTimestamp`（`:292-295`）
- Claude：`claudeMessageDedupKey = msgId ? (reqId ? \`${msgId}:${reqId}\` : msgId) : null`（`:4914-4919`），持久化时截断到最近 10 万条（`:1233-1237`）

**为何这样设计**：历史视图需要把几十万键读进内存（v2 存储里是按日分片文件，按需加载），本轮视图则只需一个 `Set`。“什么时候可以省掉历史视图”被化简成一个**可证伪的四条件判断**，而判断依据全部来自游标本身（不需要扫盘）。

**移植注意**：

- Rust：把两种视图做成两个 trait 对象或两个 `HashSet` 包装；历史视图若来源是分片文件，用 `HashMap<DayKey, HashSet<String>>` 惰性加载（本项目 `loadEventSet`，`src/lib/cursor-store.js:681-726`）。
- **身份键的选择比实现更关键**：键必须满足“同一逻辑事件在任何路径拼写下都得到同一字符串”。Codex 用 `(sessionUUID, timestamp)`；若我们的事件没有天然唯一 id，就用 `(session_id, 序号, 四类 token)` 之类的内容指纹，但要在注释里声明它只是**保守重播指纹**，不能对外宣称是独立请求的强身份。
- 集合规模必须有预算：本项目 Claude 侧硬截断 10 万条，Codex 侧靠按日分片落盘。Rust 侧若用 SQLite 存事件键，记得给 `(day, key)` 建主键并把过期日整表删除，而不是逐行清理。
- 若身份键含时间戳，注意**时钟回拨**：本项目在 snapshots 比较处用“严格更大才替换”（`src/commands/sync.js:4478-4484`）。

## 9. 跨环境文件级去重：用“文件身份哈希 + duplicateOf”代替路径比较

**要解决的问题**：Windows 主机可以同时看到本地 `C:\Users\x\.claude\projects\...` 与 WSL 的 `\\wsl$\Ubuntu\home\x\.claude\projects\...`——同一批会话文件有两个路径拼写。若按路径去重会双计，按 realpath 去重在某些挂载形态下也未必可靠。

**实现要点**（`src/lib/rollout.js:1056-1107`）：

```js
let fileId = null;
if (seenFileIds) {
  const cachedFileId = sameInode && typeof prev?.fileId === "string" && prev?.fileIdSize === st.size
    ? prev.fileId : null;
  fileId = cachedFileId || (await claudeFileIdentityHash(filePath, st));
  if (fileId) {
    if (prev && (prev.fileId !== fileId || prev.fileIdSize !== st.size)) {
      prev.fileId = fileId; prev.fileIdSize = st.size;      // 缓存在内存游标上，空转文件不再重复哈希
    }
    const primary = seenFileIds.get(fileId);
    if (primary === undefined) seenFileIds.set(fileId, filePath);
    else if (!prev && primary !== filePath) {
      cursors.files[key] = { inode, offset: st.size, updatedAt: ..., fileId, fileIdSize: st.size,
                             duplicateOf: primary, ... };
      continue;      // 直接标记为“已追平”，不解析
    }
  }
}
```

三个细节值得抄：① 哈希结果按 `(inode, size)` 缓存在游标上，空转文件不再重算；② 只有**从未解析过**的重复项才走 `duplicateOf` 快路径（已经解析过的重复项交给消息哈希层处理）；③ native 副本排在前面，因此被保留的是 native 那份（`src/commands/sync.js:683-684` 注释：单路径解析首选 native）。

**为何这样设计**：文件身份（内容/元数据派生）比路径稳定，跨挂载拼写、软链接、大小写差异都能归一。注意它并没有替代事件级去重——`duplicateOf` 只解决“同一文件两处可见”，若两份副本内容已经开始分叉，仍需消息哈希兜底（源码注释明写这一点，`src/lib/rollout.js:1074-1077`）。

**移植注意**：

- Rust 里算文件身份要按平台分支：Unix 用 `std::os::unix::fs::MetadataExt::{ino, dev}`；**Windows 上没有 Unix 语义的 `ino()`**，等价物是 `std::os::windows::fs::MetadataExt::file_index()`（对应 NTFS file index，也就是 Node `st.ino` 的来源）配合 `volume_serial_number()` —— 用 `#[cfg(unix)]` / `#[cfg(windows)]` 封装成统一的 `FileIdentity`。跨 WSL UNC 或 SMB 时 `file_index()` 可能为 0，需按“取得不到就退回内容哈希/路径身份”的容错策略实现。若走内容哈希，用 `blake3` 流式读取并配合 `(len, mtime)` 做缓存键，避免每次全量哈希。
- 缓存键必须带 size（本实现用 `fileIdSize`）：文件被追加后 size 变化，若不重算就会把两个不同文件误判为同一份。
- 跨网络文件系统（SMB）上 inode/file-id 语义可能不可靠，应把“哈希失败”设计为**回退到路径身份**而不是报错。

## 10. 双队列分离：本地维度状态与云端事实状态彻底分开

**要解决的问题**：同一批解析结果既要服务云端（按 `source/model/hour` 计费与展示），又要服务本地项目视图（按 `project_key/source/hour`）。若混在一个文件里，就会出现两类污染：项目维度字段进入云端 schema；以及**本地队列的积压被误算成待上传量**。

**实现要点**：

- 一次解析同时向两个队列 append（`src/lib/rollout.js:683-686`）：

```js
const bucketsQueued = await enqueueTouchedBuckets({ queuePath, hourlyState, touchedBuckets });
const projectBucketsQueued = projectEnabled
  ? await enqueueTouchedProjectBuckets({ projectQueuePath, projectState, projectTouchedBuckets })
  : 0;
```

- 项目行是主行的**裁剪版本，不是同构**（项目行 `src/lib/rollout.js:3515-3532` vs 主行 `:3328-3344`）：比主行**多** `project_ref` + `project_key` 两个前导字段，同时**少三类列**——`model`（项目视图不按模型细分）、`usage_precision`（主行在 `:3341` 写入）、以及 `...prioritySubsetOf(totals)` 展开的 `priority_*` 子集列（主行在 `:3342` 展开，字段表见 `:4073-4079`，值 > 0 才出现）。即项目视图既不区分模型，也不区分精度/priority 档位；token 六桶、`billable_total_tokens`、`total_cost_usd`、`conversation_count` 与主行同源（`project_ref`/`project_key` 缺失的项目桶在 `:3513` 直接跳过）。
- 这条裁剪与读侧 key **互为因果**：项目队列读侧 key 是 `project_key|source|hour_start`（`src/lib/local-api.js:145-153`），**不含 `model`**；主队列读侧 key 是 `source|model|hour_start`（`:202-211`）。两套 key 各自与自己的行字段一一对应（key 与字段都决定“同键后写覆盖前写”的快照语义）。
- 云端路径**只读主队列**（`src/commands/sync.js:3654-3659`）：

```js
const afterState = (await readJson(queueStatePath)) || { offset: 0 };
const queueSize = await safeStatSize(queuePath);
// Only the main queue is uploaded by drainQueueToCloud. project.queue.jsonl
// is local project-usage state, so counting it here creates false backlog
// and can keep auto retry alive even after cloud sync has drained.
const pendingBytes = Math.max(0, queueSize - Number(afterState.offset || 0));
```

- 两侧状态文件也分离：`queue.state.json`（上传字节 offset）/ `project.queue.state.json`（本地读侧 offset）。读侧 key 公式不同（`src/lib/local-api.js:145-153` vs `:202-211`）。

**为何这样设计**：把“云端事实”与“本地视图”当成两条**独立演进**的通道：项目维度是纯本地的产品特性，可以随时增删而不动云端契约；反过来，云端重放/回填也不会因为项目字段变化而产生副作用。`pendingBytes` 只算主队列，直接把“本地项目数据堆积导致自动重试永不停歇”这个 bug 从设计上排除。

**移植注意**：

- Rust 里可以用两个独立表/两个 append-only 文件，但**必须让“是否需要上传”只依赖上传通道自己的水位**。常见错误是共用一个 `dirty` 标志。
- 项目维度的键要显式规定归并方向（本项目在 source 层做归一，见 S1 §6），否则跨工具同名目录会分裂成多个项目。
- 若项目队列也要参与 UI 高频读取，给它独立的读侧缓存（本项目 `readProjectQueueData` 与 `readQueueData` 是两个独立的 reader 实例，`src/lib/local-api.js:145,202`）。

## 11. 上传进度点：逐批推进 + 只发完整行 + 控制行计入上限

**要解决的问题**：上传可能被拆成多批、跨多次进程（hook 触发的 sync 每次都只有几十秒）。进度必须**可恢复**，且不能把“写者正在写的半行”发出去，也不能让某类控制记录把批次撑爆。

**实现要点**（`src/commands/sync.js:4315-4413`）：

```js
for (let batch = 0; batch < batchBudget; batch++) {
  if (Date.now() >= deadline) break;
  if (offset >= queueSize) break;
  const result = await readQueueBatch(queuePath, offset, limit);
  ...
  offset = result.nextOffset;
  state.offset = offset;
  state.updatedAt = new Date().toISOString();
  await writeJson(queueStatePath, state);       // 每批成功后立即落盘
}
```

- 只发完整行（`src/commands/sync.js:4447-4449`）：`if (!record.complete) break;`
- 批上限同时约束两类记录（`:4488-4490,4511-4512`）：

```js
// Counted like a bucket row: the record rides THIS batch and its bytes are
// covered by nextOffset, so the next batch resumes right after it.
linesRead += 1;
if (linesRead >= maxBuckets) break;
```

- 上限链：`MAX_INGEST_BUCKETS = 500`（硬上限，`:4313`）→ `batchSize` 默认 200（`:3564`）→ `upload_scope === "community"` 时批数压到 10（`:4392`）。
- 总预算 `UPLOAD_BUDGET_MS = 90_000`，并把剩余预算作为 `AbortSignal.timeout` 传给 fetch（`:4360-4362`）。
- 超时但有成功批次时**不抛错**，保留已推进的 offset（`:4402-4411`）。

**为何这样设计**：三层限制解决三类不同故障——① 单请求记录数上限对应用户端 500 条的硬约束（源码注释记录了“TRAE 首次同步每个会话一条 state 记录、不设上限就永久 400”的事故，`:4433-4441`）；② 批数上限控制单次运行时长；③ 总预算防止某次网络慢把 hook 触发的子进程拖死。**“进度逐批落盘 + 只发完整行”**是让“随时被杀”变成安全操作的关键两条。

**移植注意**：

- Rust：用“读文件句柄 + 记录已消费字节”的游标（`File::seek` + `read_exact` 或按行缓冲），每批结束后 `fsync` 状态文件。注意 offset 必须是**字节**位置，不能用字符数或行号（本项目在 `src/lib/upload-queue.js:20-21` 特别声明这点）。
- 若上传带重试，务必保证服务端对该批次**幂等**（整行 upsert 或带 `upload_id`/批次 id 去重）。本项目传 `upload_id`（`:4368`），但幂等性主要靠云端整行 upsert。
- 把“已发送字节”与“已确认字节”区分开：本项目在服务端返回 OK 之后才推进 offset（避免“发了但没被接受”也推进）。

## 12. 把昂贵的全量扫描摊销成后台审计

**要解决的问题**：为了保持“秒级新鲜”，hook 触发的增量扫只扫近期文件；但历史文件也需要偶尔被重新审计（口径变化、早期 bug、文件被外部改动）。既不能每轮全量扫，也不能永远不扫。

**实现要点**（`src/commands/sync.js:3763-3819`）：

```js
const CODEX_COLD_SCAN_AUDIT_INTERVAL_MS = 24 * 60 * 60 * 1000;   // :294
const CODEX_COLD_SCAN_AUDIT_MAX_SYNCS = 288;                     // :301
...
if (!state || typeof state !== "object" || state.version !== 1) return true;
if (!Number.isFinite(lastFullScanAtMs) || lastFullScanAtMs <= 0) return true;
if (Number.isFinite(nowMs) && lastFullScanAtMs - nowMs > 5 * 60 * 1000) return true;   // 时钟回拨保护
if (Number.isFinite(nowMs) && nowMs - lastFullScanAtMs >= CODEX_COLD_SCAN_AUDIT_INTERVAL_MS) return true;
const syncsSinceFullScan = Number(state.syncsSinceFullScan || 0) + Math.max(0, Number(deferredSyncs) || 0);
return Number.isFinite(syncsSinceFullScan) && syncsSinceFullScan >= CODEX_COLD_SCAN_AUDIT_MAX_SYNCS;
```

配套：状态写进游标（`{version, lastFullScanAtMs, syncsSinceFullScan, lastSkippedFiles, updatedAt}`）；当本轮**跳过游标提交**时，把审计增量写进独立文件 `deferred-codex-audit.json`（`src/commands/sync.js:3456-3460`）。

**为何这样设计**：双阈值（时间 24h / 次数 288 ≈ 5 分钟粒度下的 24h）覆盖两种场景：机器长期开着、以及机器频繁开关机。`lastFullScanAtMs - nowMs > 5min` 这条看似奇怪的判断是**时钟回拨保护**——如果用户改了系统时间，`nowMs - lastFullScanAtMs` 会变成负数，普通阈值判断永远不会到期，审计就被永久饿死。

**移植注意**：

- Rust：用 `std::time::Instant` 做进程内计时、用 `SystemTime` 持久化；持久化时间戳的比较要显式处理“未来时间”（本项目做法可照抄）。
- 别把“审计”实现成“每 24h 全量重扫并全量重写”——本项目把全量重扫与**修复**分开：审计只是让 `parseRolloutIncremental` 看到全部文件（增量逻辑仍然按 offset 跳过没变的文件），真正的重建只在 repair 里发生。
- 备份/暂停“审计计数”的机制要有，否则大量被跳过的 sync（锁争用）会让计数失真（本项目用 `deferred` 累加解决）。

## 13. 读时纠错：不改历史文件，只改解释方式

**要解决的问题**：口径变了、早期版本写错了字段，但如果去重写历史数据文件，会引入不可逆风险（一次 bug 就是永久损坏）。而完全不修则历史数据显示错误。

**实现要点**（`src/lib/local-api.js:155-200`）：读侧对每一行做**纯函数式纠正**，不改文件。

```js
function isLegacyInclusiveCodexRow(row) {
  if (!row || (row.source !== "codex" && row.source !== "every-code")) return false;
  const inputTokens = Number(row.input_tokens || 0);
  const cachedInputTokens = Number(row.cached_input_tokens || 0);
  const outputTokens = Number(row.output_tokens || 0);
  const totalTokens = Number(row.total_tokens || 0);
  if (!Number.isFinite(inputTokens) || !Number.isFinite(cachedInputTokens)) return false;
  if (cachedInputTokens <= 0 || inputTokens < cachedInputTokens) return false;
  // 旧口径 input 含 cache read，而 total 仍是 input + output —— 用这个不变量识别
  return totalTokens === inputTokens + outputTokens;
}
```

- 修正动作：`input_tokens -= cached_input_tokens`；对 cursor/antigravity 这类“`billable_total_tokens` 曾为 0”的行，读时抬到 `total_tokens`（`:191-198`）。
- 同一函数被**读侧与上传侧共用**（上传前也过一遍，`src/commands/sync.js:4504-4509`），保证本地与云端口径一致。

**为何这样设计**：用**不变量**（而不是版本号或时间戳）识别历史坏行，使判定与“什么时候修的”解耦——任何来源的坏行都能被识别，且不需要记录迁移状态。代价是每次读取都要做这个判断（非常便宜：几个数值比较）。

**移植注意**：

- Rust 侧把这类纠正写成 `fn normalize_row(row: &mut UsageRow)` 的纯函数，读路径与发送路径**共用一个**调用点，并在类型层面禁止绕过（例如所有外部数据必须经 `From<RawRow>` 转换）。
- 用不变量识别要谨慎：如果口径曾多次变化，不变量可能对某些版本不成立（本项目对 Codex 就是这样，`total_tokens === input + output` 只对“input 含缓存”的一代成立）。若无法用单一不变量覆盖，就退回“版本号字段 + 迁移记录”。
- 长期看，读时纠错会累积成“解释层越来越厚”。建议给每条纠错标注引入原因与 issue 编号，并设定“什么时候可以删掉这条纠错”的条件（本项目用注释记录 issue 号，但没有删除条件）。

## 14. 把“被跳过的运行”做成可见事实

**要解决的问题**：采集系统最危险的失败模式是**静默停摆**——hook 还在触发、进程也正常退出、但数据一直不更新。常见根因就是锁争用或残留锁。

**实现要点**（`src/commands/sync.js:484-506`）：

```js
const SYNC_SKIP_MARKER = "sync.skip.json";
async function recordSyncSkip(trackerDir, lockPath) {
  const holder = await inspectLock(lockPath);
  const reason = holder.exists && holder.pid && !holder.alive ? "lock_debris" : "lock_busy";
  const detail = !holder.exists ? "sync lock could not be acquired"
    : holder.pid ? `sync lock held by pid ${holder.pid} (${holder.alive ? "running" : "not running"})`
    : "sync lock has no owner record";
  await writeJson(path.join(trackerDir, SYNC_SKIP_MARKER), { reason, detail, at: new Date().toISOString(), lockPath });
  return `Sync skipped: ${reason} — ${detail}\n`;
}
```

- 抢到锁后**立即删除** marker（`:565`），因此 marker 存在即代表“最近一次尝试被跳过”。
- 状态命令读它并上报（`src/commands/status.js:285,1058`）。
- 同类的“可见化”还有：上传退避写 `upload.throttle.json` 并在 status 暴露 `next_upload_after` / `backoff_until` / `last_upload_error`（`src/commands/status.js:1054-1057`）。

**为何这样设计**：`lock_busy` 与 `lock_debris` 的区分是核心——前者是正常并发（无需处理），后者是**需要人工介入**的状态（锁残留）。把这两者写进同一个可查询的产物，让“看板/诊断命令”能直接给出结论，而不是靠猜。

**移植注意**：

- Rust 侧等价物：一个 `<data_dir>/last_skip.json`，字段至少含 `reason` / `detail` / `at` / `holder_pid` / `holder_alive`。
- **别退化成一个布尔量**——“被跳过”这件事的排障价值全在 reason 上。
- 时间戳用 UTC ISO8601，与其它状态文件保持同一时区口径（本项目全部用 `new Date().toISOString()`）。

## 15. hook 触发的三件套：先落信号、再节流、然后脱离进程

**要解决的问题**：provider 的 hook 会**同步等待**你的命令返回。如果 hook 里直接跑解析，几秒到几十秒的耗时会被算进用户的交互延迟；如果每个 turn 都触发一次完整解析，机器会被拖垮。

**实现要点**（`src/commands/init.js:1453-2231`，宿主函数 `buildNotifyHandler` 为 `:1445-2232`；生成的是 `~/.tokentracker/bin/notify.cjs` 的字面内容）：

```js
// :1446-1447 文件头注释直接写明了设计约束
// Keep this file dependency-free: Node built-ins only.
// It must never block Codex; it spawns sync in the background and exits 0.
```

```js
// :1495-1498 ① 先落信号（即使后续节流跳过，也留下“有过活动”的证据）
try { fs.mkdirSync(trackerDir, { recursive: true });
      fs.writeFileSync(signalPath, new Date().toISOString(), { encoding: 'utf8' }); } catch (_) {}

// :1519-1536 ② 20 秒节流 + ③ 脱离进程
const throttlePath = path.join(trackerDir, 'sync.throttle');
const now = Date.now();
let last = 0;
try { last = Number(fs.readFileSync(throttlePath, 'utf8')) || 0; } catch (_) {}
if (now - last > 20_000) {
  try { fs.writeFileSync(throttlePath, String(now), 'utf8'); } catch (_) {}
  const syncArgs = ['sync', '--auto', '--from-notify', '--source', source];
  if (hasLocalRuntime && hasLocalDeps) spawnDetached([process.execPath, trackerBinPath, ...syncArgs]);
  else spawnDetached(['npx', '--yes', fallbackPkg, ...syncArgs]);
}
```

```js
// :1563-1573
function spawnDetached(argv) {
  const child = cp.spawn(argv[0], argv.slice(1), { detached: true, windowsHide: true, stdio: 'ignore', env: process.env });
  child.unref();
}
```

再加两件：④ **链式调用用户原有的 hook**（先备份原命令再在末尾追回，`:1538-1559`）；⑤ 无条件 `process.exit(0)`（`:1561`），保证 hook 永不因我们失败而失败。

另需知道一处**冗余判断**：上面 `if (hasLocalRuntime && hasLocalDeps)` 里两个变量指向**同一个路径**——模板内 `trackerBinPath`（`:1483`）与 `depsMarkerPath`（`:1484`）都是 `path.join(trackerDir,'app','bin','tracker.js')`，字面相同，因此该条件退化为**一次 `existsSync`**（同一 quirk 也出现在 OpenClaw 插件模板：`src/lib/openclaw-session-plugin.js:358` 与 `:361`）。移植时按“本地 runtime 存在 → 本地 exe；否则 `npx` 兜底”实现即可，不必照抄这层冗余判断。

**为何这样设计**：三点顺序不可换——signal 先写，所以即使节流拦掉了本次 spawn，也能知道“用户刚刚在活动”；节流在 spawn 之前写时间戳，避免并发 hook 同时通过判断造成雪崩；detached + unref + `stdio: 'ignore'` 让子进程完全脱离父进程生命周期（父进程退出不会杀掉它，也不会继承管道导致卡住）。

**移植注意**：

- Rust 里等价做法：`std::process::Command::new(current_exe).args([...]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()`，然后**不要** `wait()`。真正的等价“detached”需要平台支持（Windows 上 `CREATE_NEW_PROCESS_GROUP` / `DETACHED_PROCESS`，可通过 `std::os::windows::process::CommandExt::creation_flags` 设置；Unix 上 `setsid`）。若不做 detached，父进程被 kill 时子进程可能一起死。
- 节流状态文件是**读-判断-写**三步，本身不是原子的（并发 hook 可能都通过）。若要做到严格，需要文件锁或原子创建（本项目接受这个近似，因为重复 spawn 的后果只是一次多余的解析）。
- hook 处理器**不要依赖仓库/依赖包**：本项目生成的文件只用 Node 内置模块，且在本地 runtime 缺失时回退 `npx --yes <pkg>`。Rust 侧同理：hook 应指向一个**自包含**的入口（静态链接的 exe 或 shell 脚本），不要指向需要 `cargo run` 的路径。

## 16. 向用户配置文件写入：三种幂等范式 + 一类“整体覆盖” + 一律先备份

**要解决的问题**：往用户的 `settings.json` / `config.toml` 里插自己的 hook，必须满足：重复安装不出重复项、命令路径变化时不产生“旧条目 + 新条目”、卸载时能精确摘掉自己的项且不动别人的配置。

**实现要点**：三种幂等范式（按配置文件类型选择），外加一类**不带哨兵**的“整体覆盖”范式。

① **命令身份识别**（跨路径漂移仍认出自己）——`src/lib/notify-command.js:23-36`：

```js
function notifyIdentity(command) {
  if (typeof command !== "string") return null;
  const tokens = command.match(/"(?:\\.|[^"\\])*"|'(?:''|[^'])*'|[^\s]+/g) || [];
  const args = tokens.map(decode);
  if (args[0] === "&") args.shift();
  if (args[0] === "/usr/bin/env" && args[1] === "node") args.shift();
  if (args.length !== 3 || !/(?:^|[\\/])node(?:\.exe)?$/i.test(args[0])) return null;
  if (!/(?:^|[\\/])notify\.cjs$/i.test(args[1]) || !/^--source=[a-z0-9-]+$/i.test(args[2])) return null;
  const file = args[1].replace(/\\/g, "/");
  return `${/^[a-z]:\//i.test(file) ? file.toLowerCase() : file}\u0000${args[2]}`;
}
```

② **就地修复 + 同 scope 去重**（JSON hooks 结构）——`src/lib/claude-config.js:181-230`：把过时形态改写为 `{type:"command", command, shell?}`，并按 `matcher` scope 去重。

③ **哨兵串 + 拒绝覆盖外来文件**（插件/扩展类，**本项目只对 omp 这么做**）——`src/lib/omp-hook.js:17,388-395`：文件带自己的标记（`// @tokentracker-managed-omp-extension`）则覆盖，不带则跳过并回报 `skippedReason: "unmanaged-extension-present"`（该字符串**全仓仅此一处产生**，消费方 `src/commands/init.js:813`）。

④ **内容等值短路 + 无条件备份覆盖**（OpenCode 的插件文件）——`src/lib/opencode-config.js:66-87`：`upsertOpencodePlugin` 先比对内容，`if (existing === next) return { changed: false }`（`:73-75`，**这是内容等值短路，与哨兵无关**）；内容一旦不等，**无论原文件里是什么都先备份再覆盖**（`:80-85`）。标记检测（`hasPluginMarker`）只出现在 `removeOpencodePlugin`：内容不带标记时返回 `skippedReason: "unexpected-content"`（`:94`）。**OpenCode 不会拒绝覆盖外来文件**——别把它当哨兵范式照抄。

配套：所有写入路径先复制 `settings.json.bak.<ISO>`（`src/lib/claude-config.js:248-262`；`src/lib/gemini-config.js:265-279`；`src/lib/codex-config.js:89-104`），仅 Codex 系需要额外把**被覆盖的原命令**存到 `*_notify_original.json`（因为 notify 是独占槽位）。TOML 侧不引入解析库，用手写的最小行编辑（`src/lib/codex-config.js:245-309`）。

**为何这样设计**：不同配置文件类型对“幂等”的容错能力不同——JSON hooks 是数组结构，适合结构化比对与修复；TOML 只有一行 `notify = [...]`，适合等值比较；插件/扩展文件是**整个文件属于别人**，最稳的形态是「**不认识就不写**」（哨兵串），但代价是用户原有的同名插件会被永久跳过——本项目只有 omp 走这条路，OpenCode 则改为「**先备份再覆盖**」，用 `.bak.<ISO>` 而不是哨兵来兜底（`src/lib/opencode-config.js:80-85`）。把这几类分别实现，比试图写一个通用“配置补丁器”可靠得多。

**移植注意**：

- Rust 侧 JSON 用 `serde_json::Value` 做结构化改写（保留未知字段：本项目通过 spread 保留 `settings` 其它键，Rust 里对应 `Value::Object` 的 in-place 修改）。
- TOML 若用 `toml_edit`，务必用它的“保留注释与格式”的编辑 API；本项目的手写行编辑会在**复杂 TOML 特性**（多行数组、行内注释、多行字符串）下失效【未能确认：源码注释自称支持“single-line arrays / multi-line arrays”，但未覆盖面更广的 TOML 语法】。
- **备份策略要一致**：本项目 Grok 分支无备份、无条件覆盖（`src/lib/grok-hook.js:85-91`），是明显的策略缺口，别照抄这一点（见下节）。

## 17. 发现层的三个反直觉细节

**要解决的问题**：多安装（native + WSL）与多根（自定义 home）场景下，容易踩三类坑：空壳安装顶掉有数据的安装、共享受理掉的旧游标、以及“以斜杠开头的 Windows 路径”被误当 POSIX 路径。

**实现要点**：

① **“必须包含子目录”的判定要折进探测本身**（`src/lib/install-resolver.js:25-36`）：

```js
const populated = (p) => !requireAnyChild || hasAnyChild(p, requireAnyChild, deps.existsSync);
const wslExists = requireAnyChild
  ? (p) => Boolean(pathExists(p, deps.existsSync)) && hasAnyChild(p, requireAnyChild, deps.existsSync)
  : deps.existsSync;
const wslCandidate = wslValue !== undefined
  ? (wsl.shouldProbeWsl(env) && pathExists(wslValue, deps.existsSync) && populated(wslValue) ? wslValue : null)
  : (wslDir && wsl.shouldProbeWsl(env) ? wsl.discoverWslHome(wslDir, { ...deps, env, existsSync: wslExists }) : null);
```

② **合并模式只在解析器具备路径无关身份时才启用**（`src/lib/install-resolver.js:6-18` 的注释即为契约）：`union: true` 仅用于 Codex/acode（事件按 `sessionUUID:eventTimestamp` 去重）与 Claude（手工 union + 文件哈希 + 消息哈希）。

③ **WSL 会话的 POSIX cwd 要用“转录文件自身的 UNC 前缀”重锚**（`src/lib/wsl-probe.js:232-242`）：

```js
function mapWslCwdToUnc(cwd, transcriptPath) {
  if (typeof cwd !== "string" || !cwd.startsWith("/")) return cwd;
  // `new URL("file:///C:/dev/app").pathname` is "/C:/dev/app" — a leading
  // slash, but a Windows path rather than a POSIX one. Qoder feeds exactly
  // that shape in via `project_uri`; re-anchoring it would invent
  // `\\wsl$\Distro\C:\dev\app`.
  if (WINDOWS_DRIVE_PREFIX_RE.test(cwd)) return cwd;
  const root = wslUncRoot(transcriptPath);
  if (!root) return cwd;
  return root + cwd.slice(1).replaceAll("/", "\\");
}
```

**为何这样设计**：① 的“折进探测”是因为 `discoverWslHome` 的语义是“返回第一个存在的候选”——如果先拿回候选再做校验，第一个空壳就会胜出并且**再也不会去看第二个发行版**（`src/commands/sync.js:766-772`）。② 的约束防止把“双计”当“覆盖”。③ 的前缀取自实际读取的文件路径，因此自动选中本机真正可用的 `\\wsl$` 或 `\\wsl.localhost` 之一（两者并非总是等价），同时显式避免把 `C:\...` 变形成 `\\wsl$\Distro\C:\...`。

**移植注意**：

- Rust 侧判断路径是否 POSIX：`Path::new(s).has_root() && !s.starts_with(['A'..'Z'])` 之类手写判断往往不够；建议显式实现“Windows 盘符前缀”正则（本项目用 `/^\/[A-Za-z]:[\\/]/`），因为 `file://` URI 解析结果经常长这样。
- `\\wsl$` 与 `\\wsl.localhost` 的可用性差异在 WSL1/WSL2 与不同 Windows 版本间不同（本项目按发行版版本排序两种前缀，`src/lib/wsl-probe.js:193-201`）；Rust 侧同样要双前缀尝试，不要硬编码其一。
- 跨 UNC 读 SQLite 时用“复制 db + `-wal` + `-shm` + `-journal` 到临时目录”的方式（`src/lib/wsl-probe.js:244-262`）；Rust 侧若用 `rusqlite`，优先尝试 `VACUUM INTO`，失败则退回复制文件组（网络盘上 `VACUUM INTO` 可能无法获取必要的锁）。

## 18. 可测性与兼容性上的两个小成本高收益做法

**要解决的问题 A**：采集逻辑依赖大量外部事实（文件系统、WSL 子进程、时间、网络）。若不做注入缝，只能靠写真实文件 + 真实 `wsl.exe` 来测试，慢且不可靠。

**实现要点 A**：**依赖注入默认参数化**，全部落在函数签名上（生产代码零分支）。

```js
// src/lib/wsl-probe.js:57-63
function probeWslDistros(deps = {}) {
  const strict = deps.strict === true;
  const hasDeps = Object.keys(deps).some((key) => key !== "strict");
  ...
  const runWsl = deps.runWsl || defaultRunWsl;
```

```js
// src/lib/cursor-store.js:25-32（failureInjector 用于精确测试崩溃点）
async function openCursorStore({ trackerDir, cursorsPath = ..., activationBytes = DEFAULT_ACTIVATION_BYTES,
  forceV2 = false, codexRoots = [], failureInjector = null } = {}) {
```

```js
// src/commands/sync.js:516-538（context 承载 trackerDataDir/home/各类测试实现）
const cursorSyncDeps = context && typeof context.cursorSyncDeps === "object" ? context.cursorSyncDeps : {};
```

`maybeInjectFailure(this.failureInjector, "beforeManifestSwap")`（`src/lib/cursor-store.js:972`）允许测试精确验证“manifest 切换前崩溃”的清理路径。

【未能确认】`context.traeCnFetchImpl` / `context.cursorSyncDeps` 这类测试缝在生产路径是否完全惰性（源码注释称 “no public CLI flag/config is introduced”，`src/commands/sync.js:526-531`），但从签名看生产调用者不传即为 no-op。

**要解决的问题 B**：同一个工具会被不同来源的触发器引用，命名不一致（`code` / `every-code` / `everycode`；`kilo-code` / `kilocode`）。

**实现要点 B**：显式别名表 + 双层白名单（`src/commands/sync.js:310-373`）：

```js
const AUTO_SYNC_SOURCE_ALIASES = new Map([
  ["code", "every-code"], ["commandcode", "command-code"], ["deepseek", "dsh"],
  ["everycode", "every-code"], ["kilo", "kilo-cli"], ["kilo-code", "kilocode"],
  ["kimi_code", "kimi-code"], ["roo-code", "roocode"],
]);
const AUTO_SYNC_SOURCES = new Set([ /* 45 项 */ ]);
const BACKGROUND_AUTO_SYNC_SOURCES = new Set(["acode", "codex", "every-code", "reasonix"]);
```

**为何这样设计**：别名表把“外部叫法”与“内部 id”解耦，使外部参数可以随产品改名而不动内部状态；双层白名单让“后台轻量扫描”的源集合成为一个显式的、可审计的常量，而不是散落在 if 里的隐式判断。

**移植注意**：

- 别名归一只做一次（在解析参数的出口），后续所有路径使用内部 id。本项目 `normalizeSyncSource`（`:3755-3762`）即单一归一入口。
- 白名单集合要能回答“为什么这个源在/不在里面”的问题；本项目用注释说明（`BACKGROUND_AUTO_SYNC_SOURCES` 的注释是 “Keep unscoped native 5-minute syncs bounded to dated local session trees.”）。

---

## 不建议照抄的设计

以下条目是**明确应避免**的做法，多数有源码内的事故记录或明显的可维护性代价支撑。

1. **`cmdSync` 单函数 3190 行 / `sync.js` 7251 行的巨型编排**（`src/commands/sync.js:516-3709`）。它把锁、路径解析、40+ provider 的分支、迁移、上传、重试、输出全部塞进一个 async 函数，靠 `let` 变量的顺序赋值维持隐式状态机。任何改动都需要理解整条控制流；`skipNoOpCursorCommit`（`:3424-3437`）这类判据要同时引用 **13 个条件项**——`opts.auto`、`!isFullSourceScan`、`cursorStore.mode === "v2"`、`cursorStore.requiresCommit !== true`、`totalParsed === (commandCodeResult.cursorUnchanged ? commandCodeResult.recordsProcessed : 0)`、`totalBuckets === 0`、两处 project 桶均为 0（`grokResult.projectBucketsQueued`、`commandCodeResult.projectBucketsQueued`）、`!commandCodeResult.schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok`。**我们应拆成显式阶段（discover → parse → persist → upload），用类型/结构体传递阶段产物。**
2. **`.reclaim` 闸链需要“迭代清扫”才能自愈**（`src/lib/fs.js:178-204`，issue #431）。锁协议复杂到需要专门清扫自己的残留闸，说明协议过重。更简单的替代：单一 `O_EXCL` 锁 + token 心跳 + 超时接管，且**接管动作本身**也走同一把锁的“原子 rename 到隔离名”路径（无需第二层闸）。
3. **队列写入无任何写侧互斥**（`src/lib/rollout.js:3483`）。它依赖“所有写者都是兄弟 sync 进程且已被 `sync.lock` 串行化”这一全局约束；一旦未来有人（包括我们）加第二个写者，两个 `appendFile` 会交错写出多行撕裂。**建议：要么给队列加单写者约束（如“只有持锁进程能打开 append 句柄”并用 flock 强制），要么用 SQLite/日志合并结构代替。**
4. **`repair` 的“原子删除 + 重扫”模式本身**（0.26.3 的真实数据丢失事故，`src/commands/sync.js:234-248`）。它已用守卫弥补，但根因是**把“重算”实现成了“先删后建”**。更安全的形态：写入新快照并在**新数据集完整生成后**整体切换（即本项目 cursor-store 里已经用得很好的 generations 模式），绝不在旧数据上做删除。
5. **Grok 分支无条件覆盖、无备份**（`src/lib/grok-hook.js:85-91`，注释 `always overwrite to keep command up-to-date if bin path changes`）。与其它 provider 的“幂等 + 备份 + 还原”策略不一致；`~/.grok/hooks/99-tokentracker-usage.json` 这一固定文件名下的用户自定义内容会被直接覆盖。**要抄就抄 Claude/Codex 范式，不要抄 Grok。**
6. **同一语义存在三套并行纠错机制**：一次性 `repair` 迁移键（`sync.js`）、读时 `normalizeQueueRow`（`local-api.js:170-200`）、以及云端整行 upsert。三者口径必须人工保持同步——历史上就出现过“云端 `conversation_count` 恒为 0 而本地正确”这类不一致（`src/commands/sync.js:249-256` 的注释即为此事故）。**建议：把口径收敛成一份可执行规范（含测试向量），让三条路径都跑同一份用例。**
7. **文件体积驱动的行为切换**（16 MB 阈值决定用 LegacyCursorStore 还是 v2，`src/lib/cursor-store.js:21,77-86`）。同一用户的存储形态会在某次运行后**静默改变**，且两套实现的能力不等价（legacy 下 `codexEventStore = null`，事件去重退化为内存数组，`src/lib/cursor-store.js:165-197`）。**建议：一次性迁移到一个形态，用显式 schema 版本字段而不是体积阈值。**
8. **`legacyGroups` / `groupQueued` 那条“整小时聚合成 `model=DEFAULT_MODEL`”的遗留路径**（`src/lib/rollout.js:3181-3194,3430-3480`）。它的语义依赖“某个键存在与否”作为隐含标记，S1 §8 第 1 条已标为未能验证；这类**用状态形状编码语义**的做法极易在后续修复中互相干扰（v2→v3 的注释正是这类冲突的产物，`src/commands/sync.js:216-226`）。
9. **代理配置错误让整个进程 `exit(1)`**（`bin/tracker.js:36-40`），且 hook 路径同样受影响。结果是“代理挂了 → 采集静默停止”，数据不更新但没有任何面向用户的告警（只有被 `stdio: 'ignore'` 丢弃的 stderr）。**建议：采集路径不能因为网络配置问题硬失败；应降级跳过上传并留下可查询状态。**
10. **`local-api.js`（3543 行 / 156 KB）职责混杂**：读队列、项目归因、账号体系、头像代理、宠物、静态资源与 SPA 回退都在一个 handler 工厂里。虽然每个功能内部质量不差，但耦合度高到任何改动都要回归全部端点。**建议按端点族拆分模块，用一个薄路由层组合。**
11. **测试专用钩子进入生产函数签名**（`context.traeCnFetchImpl`、`context.cursorSyncDeps`、`context.scanSources`，`src/commands/sync.js:519-538`）。成本低但会持续污染 API 表面积；更干净的做法是把这些依赖收进一个 `Deps` 结构体，或用 `#[cfg(test)]` / 依赖注入容器统一管理。
12. **依赖 `st.ino` 的变化来检测原子替换**（`src/lib/rollout.js:489-492`）。在 inode 语义不可靠的环境（某些网络盘、容器叠加文件系统、WSL UNC）上，`ino` 可能恒为 0 或恒定，导致“替换后从 0 重读”的检测失效——此时只能靠事件哈希兜底。**建议：把“文件身份”做成可配置策略（inode → file-id → 内容抽样哈希），并在启动诊断里显示实际采用的策略。**
13. **把 Windows 侧 7680 端口的占用处理交给“WSL 默认换端口 7681”**（`src/commands/serve.js:15-21`）。这是对 Delivery Optimization 服务行为的补偿性绕行，但仍会与其它占用 7681 的进程冲突。**建议：端口发现走“嗅探 + 用户可见提示”，而不是固定的 +1 规则。**
14. **`status` 的 `hookStatus` 传参里有两个键在 passive 判定中永不使用**（`src/commands/status.js:1030-1031` vs `src/lib/passive-mode.js:93-186`），以及 `workbuddy-ai` 这个带连字符的特殊键名。这类“传参与消费不一致”的接口会随 provider 增加持续腐化。**建议：hook 状态用一个显式的 provider 枚举 + 结构体，编译期保证双方一致（Rust 的 enum 天然能做到）。**

> 结论性判断：该仓库**在“崩溃安全、并发正确性、历史数据不可逆风险”这三类问题上给出的机制质量很高，可以直接照抄**（本文第 1–13 条）；而**在编排结构、状态形状语义、多路径口径一致性、以及“网络/平台失败是否应硬退出”这些地方是反面教材**（本文“不建议照抄”节）。移植时应把两半严格分开对待。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-A-pipeline.md`（样本 HEAD `b19d27b0` 未变）。本节只记录**改动**，未改动处即视为复核通过。修正 agent：FIX-A，所有改动均回读源码逐条确认后落笔。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| E2（§15） | notify.cjs 模板 `init.js:1445-1574` | 模板区间 **`:1453-2231`**，宿主函数 `buildNotifyHandler` **`:1445-2232`** | `init.js:1445,1453,2231,2232` |
| E3（§16 ③） | 「哨兵范式」把 omp 与 **OpenCode** 并列，并称 OpenCode 会以 `unmanaged-extension-present` 拒绝覆盖 | ③ 哨兵范式**只剩 omp**（`omp-hook.js:17,388-395`；字符串全仓唯一产生点，消费方 `init.js:813`）；**新增 ④「内容等值短路 + 无条件备份覆盖」**描述 OpenCode（`opencode-config.js:66-87`、`:73-75`、`:80-85`、`:94`），并明确「**OpenCode 不会拒绝覆盖外来文件**」；标题与「为何这样设计」同步改 | `opencode-config.js:66-96`；`omp-hook.js:17,388-395`；`init.js:813` |
| E8（§6） | `cloneGenerationFiles` 用**硬链接**/复制二选一（`:1357-1362`），并据此推出“硬链接会污染旧代” | `cloneOrCopy` 用 `fs.copyFile(..., COPYFILE_FICLONE)`（`:1361`）= **CoW/reflink，不支持时回退普通复制**，与硬链接无关；论证改为「脏分片是**整文件重写**（先 `unlink` 再 create，`:907`），故旧代自洽」，**结论“重写前先 unlink”保留** | `cursor-store.js:907,1349-1362` |
| E9（不建议照抄 1） | `skipNoOpCursorCommit` 判据「同时引用 **9 个变量**」 | 条件项 **13 个**，逐项列出（`opts.auto` … `!opts.repairGrok`） | `sync.js:3424-3437` |
| I2（§15） | 未提示「双路径」判据冗余 | 增补：模板内 `trackerBinPath`（`:1483`）与 `depsMarkerPath`（`:1484`）**字面相同**，判据退化为一次 `existsSync`；同 quirk 见 `openclaw-session-plugin.js:358,361` | `init.js:1483-1484`；`openclaw-session-plugin.js:358,361` |
| I5（§8） | 「只有当……四条同时成立时才用本轮集合」，未提兜底 | 代码块补 `if (prev?.lastTotal) return true;` 与 `return false;` 两行，正文说明「稳态四条件不成立但游标带 `prev.lastTotal` 时**仍走持久集合**」；动机标【未能确认】 | `rollout.js:456-460` |
| I7（§7 ②） | 把 deferred 提示当作无条件的可观测性保证 | 补前提：提示被 `if (!opts.auto)` 包住，**`--auto` 轮次静默推迟**；要后台留痕应写 marker/状态文件 | `sync.js:1177-1181` |
| U1–U10 | 各节「移植注意」把跨语言/平台断言与源码事实混排 | 文件头新增**统一前置说明**：这些断言**无本仓库证据、属工程推断**，与带行号的已验证事实不同证据等级（各条断言本身按复核结论**保留不改**） | 复核报告「无法验证（U）」节 |

未在本文件落实的项：E4/E5/E6/E7/E9 的 §3.5 部分、I1/I3/I4/I6/I8/I9 均属 S1 正文内容，已在 [S1-pipeline-and-collection.md](./S1-pipeline-and-collection.md) 修正。

### R2 修正

依据 `REVIEW-R2-1-pipeline-framework.md`（本文件相关条目：E2、I2）。修正 agent：FIX-R2-A；改动前已回读源码两处逐字确认；**样本仓库未修改**；R1 已完成内容全部保留。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| R2-E2（§10） | 「项目行**只多两个字段，其余同构**（`rollout.js:3515-3532`）：`project_ref` + `project_key`；且没有 `model`」 | 改为**裁剪版本**：项目行比主行多 `project_ref`/`project_key` 两个前导字段，同时**少三类列**——`model`、`usage_precision`、`...prioritySubsetOf(totals)` 展开的 `priority_*` 子集列（值 > 0 才写出）。项目视图既不按模型细分，也不区分精度/priority 档位 | 项目行 `rollout.js:3515-3532`；主行 `:3328-3344`（`:3341` `usage_precision`、`:3342` `...prioritySubsetOf(totals)`）；子集字段表 `:4073-4079` |
| R2-I2（§10） | 只写「读侧 key 公式不同（`local-api.js:145-153` vs `:202-211`）」，未点出与行字段的因果 | 补明：项目队列读侧 key 为 `project_key\|source\|hour_start`、**不含 `model`**，与「项目行无 `model` 字段」互为因果；主队列 key 为 `source\|model\|hour_start`，两套 key 各自与自己的行字段一一对应 | `local-api.js:145-153`、`:202-211` |

**未采纳 / 反驳**：0 条。R2-E2 经比对 `rollout.js:3328-3344` 与 `:3515-3532` 两处确认成立（原「同构」论断确实漏了两类列），R2-I2 为纯补强。
