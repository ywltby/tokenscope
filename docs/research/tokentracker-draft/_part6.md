## 10. 与 TokenScope 的对照：可抄与需避

以下判断只做**工程可移植性**评价，不做风格评价。行号为样本仓库 `xiufengsun/TokenTracker`（HEAD `b19d27b0`，v1.3.14）的实际行号，口径为 **UTF-8 解码 + 按 `\n` 分行**。带行号的源码事实与跨语言/跨平台的工程推断**不是同一证据等级**——凡属后者的，本节标注「工程推断」。

### 10.1 值得直接抄的实现细节

排序遵循「并发正确性 > 数据完整性 > 性能 > 兼容性」。

**① 锁协议：独占创建 + 持有者令牌 + 心跳 + 独立回收闸**

问题：多个生产者（hook 触发的 sync、后台轮询、本地 API 触发的 sync、自动重试）会同时抢跑同一份解析，还要处理 `kill -9` 残留、PID 复用、以及「回收残留锁」这个动作自身的互斥。

要点（`src/lib/fs.js:69-74,284-414`）：`LOCK_STALE_MS = 5 min`、`LOCK_HEARTBEAT_MS = 30 s`、`MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64`（`:69-72`）。
```js
const handle = await fs.open(lockPath, "wx");        // O_EXCL 语义
const token = crypto.randomUUID();
const heartbeatPath = heartbeatPathFor(lockPath, token);   // ${lockPath}.heartbeat.${sha256(token)}
await handle.writeFile(JSON.stringify({ pid: process.pid, token, createdAt }) + "\n", "utf8");
```
- 心跳 30s 一次，同时 `utimes` 锁文件与心跳文件，`timer.unref()`（`:206-224`）。
- 回收判定：先看 PID 是否存活（`process.kill(pid, 0)`，`ESRCH` 才算死，`EPERM` 保守当作存活），再看 **token 专属心跳**是否超过 `LOCK_STALE_MS`；老格式锁（无心跳）退回 mtime 超期判断（`:105-144`）。
- 回收动作本身用 `.reclaim` 次级锁串行化，且**持闸后重新校验一次**（TOCTOU，`:342-368`）。
- 释放时同样持闸，**只有 token 匹配才 unlink**（`:264-275`）；先关句柄再删文件（Windows 上文件被打开时无法 unlink）。

动机：单纯的「PID 存活 + 时间阈值」无法区分 **PID 复用**与**长时间运行但健康的锁**；引入 token 专属心跳后，「这个锁还活着吗」被解耦成「这个 token 的心跳还新鲜吗」。`.reclaim` 闸解决的是多个收割者抢同一份残留的竞态。
移植（工程推断）：Rust 用 `OpenOptions::new().write(true).create_new(true).open()`；令牌用 `uuid`；心跳用 `tokio::time::interval` 并显式 `abort()`。进程存活无 std 等价物——Windows 用 `OpenProcess(SYNCHRONIZE)` + `WaitForSingleObject(0)`，或 `QueryFullProcessImageName` 校验身份。**Windows 上 PID 复用更频繁，务必配 token 心跳，不要只信 PID。** 若跨机共享锁（NFS/SMB），`.reclaim` 基于 `rename` 的原子性假设会失效。

**② 目录探测：把「存在」定义为「能被列举」**

问题：只做 `stat` 会把权限 `000`（或 ACL 拒绝）的目录判为「存在」，进入扫描后产出 0 个文件，在状态页显示「存在」，还会**绕过 deferred-repair 守卫**（守卫语义是「根不存在就不许重算历史」）。

要点（`src/lib/scan-roots.js:89-110`）：
```js
if (!statSync(target).isDirectory()) return { exists: false, error: null };
// Probe listability without reading entries; large roots stay cheap.
opendirSync(target).closeSync();
return { exists: true, error: null };
// catch: ENOENT/ENOTDIR → {exists:false,error:null}；其余 → {exists:false,error:code}
```

三态语义是重点：`{exists:true}` / `{exists:false,error:null}`（真不存在）/ `{exists:false,error:"EACCES"}`（存在但读不了）。第三种在 UI 上必须显示 `unreadable: EACCES` 而不是 `missing`（`:248-253`）。

动机：`opendir().close()` 只取目录句柄、不读条目，成本与 `stat` 同阶，却把「能否读取」变成可证事实。区分后调用方能做**不同决策**——不存在可跳过，读不了必须推迟重算。
移植（工程推断）：Rust 用 `std::fs::read_dir(path)` 后立刻 drop 迭代器即可。**不要用 `Path::exists()`**——它把「不可读」和「不存在」都吞成 `false`。把 `ErrorKind::NotFound` 与 `PermissionDenied` 分开映射。

**③ 追加型文件的读侧：签名缓存 + 锚点续读 + 显式让出**

问题：一次刷新会并发打多个端点，每个都要读同一个 append-only 的 `queue.jsonl`（可能几十 MB）。朴素做法每次全量读 + 全量 `JSON.parse`，直接打满事件循环。

要点（`src/lib/async-queue-reader.js`）：文件签名为 `${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:${stat.ctimeNs}`（`:8-10`，bigint stat）。
```js
// :40-47 只有"同 dev:ino + size 变大 + 头部与锚点内容一致"才从 consumedBytes 续读
if (cached && cached.identity === identity && size > cached.size && cached.consumedBytes > 0) {
  const head = await probe(file, 0, cached.head.length);
  const anchor = await probe(file, cached.consumedBytes - cached.anchor.length, cached.anchor.length);
  if (head.equals(cached.head) && anchor.equals(cached.anchor)) { offset = cached.consumedBytes; seen = cached.seen; }
}
```

动机：三个独立问题被分开解决——重复读用签名缓存；增量读用**锚点探针**而不是「相信 offset 仍有效」（文件可能被原子替换或压缩重写）；事件循环饥饿用显式让出。最妙的是残行策略：**「可见行集合」与「已消费字节位置」分离**，读者不需要回退，也不会把半行当数据。
移植（工程推断）：`tokio::task::yield_now()` 与 `setImmediate` **语义不完全等价**——前者只让出一次 poll，后者保证让出到下一个事件循环轮次。若读侧有同步 SQLite 访问，光让出没用，必须把重活丢到 `spawn_blocking`。`mtimeNs` 只有 Node 的 `stat({bigint:true})` 才给；Rust `Metadata` 只有 `SystemTime`，**不要假设时间戳有纳秒唯一性**，强判据要用内容锚点。

**④ 压缩追加型大文件：行边界 + 原子替换 + ino/size/mtime 三重校验**

问题：append-only 队列会无限增长（历史重算的日子同一小时键被反复重写），需要在不打断写者、不破坏上传字节 offset 的前提下删掉同键旧行。

要点（`src/lib/upload-queue.js:64-132`）：
```js
// :73-78 只在"够大 + offset 落在行边界"时动手
if (!before.isFile() || !Number.isSafeInteger(offset) || offset < 0 || offset >= before.size ||
    pendingBytesBefore < minPendingBytes) return unchanged;   // minPendingBytes 默认 1_000_000
if (offset > 0) {
  const boundary = Buffer.alloc(1);
  await input.read(boundary, 0, 1, offset - 1);
  if (boundary[0] !== 10) return unchanged;                   // offset-1 必须是 '\n'
}

// :111-121 替换前后的竞态校验
await output.sync(); await output.close(); output = null;
await input.close(); input = null;   // Windows: 目标仍被打开时无法替换
const current = await fs.stat(queuePath);
if (current.ino !== before.ino || current.size !== before.size || current.mtimeMs !== before.mtimeMs) {
  throw new Error("Queue changed during pending compaction");
}
await fs.rename(temporary, queuePath);
```

保留语义显式：`[0, offset)` 原样字节复制（已上传历史不变），`[offset, size)` 里每个键只保留**最后一条**（`:79-91,105-110`），且 `account_session_state` 用 `verifiedMs` 严格更大而非「最后一条」（防时钟回拨）。常量：`BACKLOG_BYTES = 1_000_000`、`CHUNK_BYTES = 64 * 1024`（`:5-6`；该文件全文 134 行）。

动机：直接 `truncate`/重写整文件会破坏 `queue.state.json` 的字节 offset 语义。只在 pending 区整理，把「压缩」与「上传进度」完全解耦。最后的三重校验是兜底——**不校验的话，一次并发 append 就会永久丢失刚写入的行**。
移植（工程推断）：Rust 用 `create_new(true)` 建临时文件 → 写 → `sync_all()` → `drop` 读句柄 → `fs::metadata` 校验 → `fs::rename`。`fsync` 在压缩场景必需，否则掉电可能出现「新文件存在但半写」。别用「读全文件到内存再写回」替代——队列可能几百 MB，且会放大竞态窗口。

**⑤ 原子写 JSON：tmp 名带 UUID + rename + 失败清理**

问题：并发写同一个 JSON 状态文件会互相踩（半写、丢失、`Date.now()` 冲突）。项目里有多个此类文件：`queue.state.json`、`upload.throttle.json`、`config.json`、`cursors.json`、`sync.skip.json`。

要点（`src/lib/fs.js:9-33`）：
```js
const tmp = `${filePath}.tmp.${Date.now()}.${crypto.randomUUID()}`;
try {
  await fs.writeFile(tmp, content, mode == null ? { encoding: "utf8" } : { encoding: "utf8", mode });
  await fs.rename(tmp, filePath);
} catch (err) { try { await fs.unlink(tmp); } catch { /* tmp may not exist */ } throw err; }
```

配套：`updateJsonLocked` 用 `<file>.lock` 做读-改-写整体互斥，超时 30s（`:416-443`），写后尽力 `chmod 600`。

动机：`rename` 在同一文件系统内原子，读者永远看到完整旧版或完整新版。tmp 名追加 UUID 是因为只靠 `Date.now()` 会在同一毫秒内冲突，两个写者写同一 tmp 并互相覆盖。失败路径必须删 tmp，否则崩溃会留下 `*.tmp.*` 污染目录枚举。
移植（工程推断）：tmp 必须与目标**同目录**（`std::fs::rename` 跨文件系统会 `CrossesDevices`）。Node 的 `mode` 只在创建时生效（源码注释特别说明，`:15-17`），Rust 用 `OpenOptions::mode(0o600)`。**若同一个 JSON 有多个写者，光有原子写不够**，还需要读改写锁。

**⑥ 三代保留 + 清单原子切换的代际存储**

问题：游标状态（几十万文件游标 + 事件去重集合）既大又关键。单 JSON 更新有两个问题：每次提交全量重写（写放大）；写一半崩溃得到损坏状态且无回退点。

要点（`src/lib/cursor-store.js:13-23,855-1016`）：`STORE_VERSION = 2`、`STORE_DIRNAME = "cursor-store-v2"`、`MANIFEST_FILENAME = "manifest.json"`、`GENERATIONS_DIRNAME = "generations"`、`SHARDED_CORE_FILENAME = "core-v3.json"`（`:13-18`）。

提交流程（`:855-989`）：新代目录 `gen-<ms>-<uuid8>` → 从不需改写的旧代分片整文件克隆 → 只重写脏分片 → 写 `core-v3.json` → 写 `generation.json`（含各分片 count 与完整性摘要）→ **最后**原子替换 `manifest.json`（`:977-990`）。未 publish 前任何失败都会 `fs.rm(generationDir,{recursive:true,force:true})` 清理半成品（`:1010-1015`）；成功后只保留 `current/rollback/previous` 三代（`:1005-1009`）；读取侧依次尝试 `current → rollback → previous`（`:124-139`）；分片完整性用 metadata 摘要校验（`:1466-1481`）。

动机：「清单指向哪一代」是唯一的原子切换点，代目录内部的多分片写入无需原子——因为**还没被任何人指向**。这让「大状态 + 多分片 + 崩溃安全」同时成立，代价是磁盘上最多 3 份数据。
移植：Rust 侧写完要 `sync_all` 目录（Linux 需要，Windows 无需）再替换 manifest；摘要用 `blake3` 写进 metadata。**分片键必须白名单化再拼路径**（源码用 `safeShardFilename` 只允许 `^\d{4}-\d{2}-\d{2}$`，否则写 `misc`，`:1807-1809`），否则来自文件路径的分片键就是路径穿越。要看清克隆语义：`cloneOrCopy` 用 `fs.copyFile(..., COPYFILE_FICLONE)`（`:1361`）= **CoW/reflink，与硬链接无关**，不支持时回退普通复制；真正需要防守的是「对共享存储做原地改写」——本项目脏分片是**整文件重写**（先 `unlink` 再 create，`:907`），所以旧代始终自洽。Rust 侧对应要求：分片重写必须「先删再建或写临时文件后 rename」，不要 `truncate` 后原地覆盖。

**⑦ 一次性历史重算的「数据不全就不修」守卫**

问题：口径变化后唯一正确的修法是按当前口径重算历史，但重算前提是**磁盘上还留着当初的数据**——这个前提经常不成立（用户清过日志、产品自身滚动删除、归档目录不在扫描范围、远端根临时不可用）。项目在这里翻过车：0.26.3 的重算把 `~/.claude` 日志已被清理的小时从队列中**静默抹掉**，一位用户损失 21.7 亿 claude token（`src/commands/sync.js:234-248`；源码自述分项 `-1.27B / -474M / -376M / -48M / -6M` 求和 = 2174M ≈ 2.174B，与总数自洽）。

要点——三件套守卫 + 一条「推迟而非跳过」原则：
- **可行性预检**：每个 repair 先用廉价信号判断能否执行（`:258-268` 的注释即契约）。
- **扫描根可用性守卫**：非 native 的根当前 `!exists` 或不可读、且历史上贡献过文件 → **推迟**（不设迁移键，等下次全量再试），普通扫描照常（`:1147-1192`）。
- **重建前后文件快照校验**：重建前记录每个输入文件的快照（`readCodexFileSnapshot`/`sameCodexFileSnapshot`，`:6271-6293`），重建后逐文件比对（`validateCodexRebuildFileSnapshots`，`:6294-6315`），并检查「历史是否被盘上数据覆盖」（`isCodexHistoryCovered`，`:6316-6346`）。
- **迁移键即幂等闸**：`cursors.migrations[KEY]` 存在即不再跑；失败路径**故意不写**该键，让下次全量重试（`:952-959`）。

动机：一次性迁移有两类失败模式——「该修没修」（可恢复）与「不该修的修了」（**不可恢复的数据丢失**）。守卫的取舍是宁可推迟，绝不基于残缺数据重写历史。这直接推翻了「全量重算=安全」的直觉。
移植：任何 `DELETE`/重写历史的操作都应先证明「新数据覆盖了旧数据的全部时间范围」。**「推迟」要显式可观测**——源码会打印 `Claude ground-truth repair deferred: ...`（`:1177-1181`），但**该行提示被 `if (!opts.auto)` 包住**，即 `--auto`（hook / 后台轮次）下是静默推迟的；要后台留痕应写 marker / 状态文件而不是仅靠 stderr。

**⑧ 事件级去重：路径无关身份 + 「追加视图/历史视图」双闭包**

问题：日志文件会被原子替换（新 inode）、被截断重写，同一会话还可能出现在两条路径下（native + WSL 安装、`sessions/` 与 `archived_sessions/`）。仅靠文件字节 offset 无法保证不重不漏。

要点（`src/lib/rollout.js:302-380`）：去重集合按「本轮新键」与「历史键」分成两种视图。
```js
const recordNewEvent = (key) => {
  ensureNewEventKeys();
  if (newEventKeySet.has(key)) return;
  newEventKeySet.add(key); newEventKeys.push(key);
  if (seenEvents) seenEvents.add(key);
  externalEventStore?.add(key);
};
const getAppendOnlyEvents = () => ({ has: (key) => Boolean(newEventKeySet?.has(key) || seenEvents?.has(key)), add: recordNewEvent });
```
关键在**何时选历史视图**（`:428-461`）：只有「单路径 + 同 inode + 未截断 + 有历史 offset」四条同时成立才允许走廉价的本轮集合；四条之后还有两条兜底——`if (prev?.lastTotal) return true;`（`:459`）与 `return false;`（`:460`）。含义是：稳态四条件不成立时，**只要游标里带着 `prev.lastTotal`（Codex 累计基线）就仍然动用持久集合**。【未能确认】`:459` 这条兜底的确切动机源码未加注释，推测与 `rebuildingCodexBaseline` 那条「从 0 重读」的路径有关。

两套身份键（按 provider 不同粒度）：Codex 用 `sessionUUID:eventTimestamp`（`:292-295`）；Claude 用 `claudeMessageDedupKey = msgId ? (reqId ? \`${msgId}:${reqId}\` : msgId) : null`（`:4914-4919`），持久化时截断到最近 10 万条（`:1233-1237`）。

动机：历史视图需要把几十万键读进内存（v2 存储里按日分片、按需加载），本轮视图只需一个 `Set`。「什么时候可以省掉历史视图」被化简成一个**可证伪的四条件判断**，依据全部来自游标本身（不需要扫盘）。
移植：**身份键的选择比实现更关键**——键必须满足「同一逻辑事件在任何路径拼写下都得到同一字符串」。若事件没有天然唯一 id，就用 `(session_id, 序号, 四类 token)` 之类的内容指纹，但要在注释里声明它只是**保守重播指纹**，不能对外宣称是独立请求的强身份。集合规模必须有预算：Claude 侧硬截断 10 万条，Codex 侧靠按日分片落盘；若改用 SQLite 存事件键，给 `(day, key)` 建主键并整表删除过期日。若身份键含时间戳，注意**时钟回拨**——本项目在 snapshots 比较处用「严格更大才替换」（`src/commands/sync.js:4478-4484`）。

**⑨ 跨环境文件级去重：文件身份哈希 + `duplicateOf`**

问题：Windows 主机可同时看到 `C:\Users\x\.claude\projects\...` 与 `\\wsl$\Ubuntu\home\x\.claude\projects\...`——同一批会话文件有两种路径拼写。按路径去重会双计，按 realpath 去重在某些挂载形态下也不可靠。

要点（`src/lib/rollout.js:1056-1107`）：
```js
const cachedFileId = sameInode && typeof prev?.fileId === "string" && prev?.fileIdSize === st.size ? prev.fileId : null;
fileId = cachedFileId || (await claudeFileIdentityHash(filePath, st));
if (fileId) {
  if (prev && (prev.fileId !== fileId || prev.fileIdSize !== st.size)) { prev.fileId = fileId; prev.fileIdSize = st.size; }
  const primary = seenFileIds.get(fileId);
  if (primary === undefined) seenFileIds.set(fileId, filePath);
  else if (!prev && primary !== filePath) {
    cursors.files[key] = { inode, offset: st.size, ..., fileId, fileIdSize: st.size, duplicateOf: primary, ... };
    continue;      // 直接标记为"已追平"，不解析
  }
}
```

三个细节：哈希结果按 `(inode, size)` 缓存在游标上，空转文件不再重算；只有**从未解析过**的重复项才走 `duplicateOf` 快路径（已解析过的重复项交给消息哈希层）；native 副本排在前面，因此被保留的是 native 那份（`src/commands/sync.js:683-684`）。

动机：文件身份（内容/元数据派生）比路径稳定，跨挂载拼写、软链接、大小写差异都能归一。注意它**没有**替代事件级去重——`duplicateOf` 只解决「同一文件两处可见」，两份副本内容开始分叉时仍需消息哈希兜底（源码注释明写，`:1074-1077`）。
移植（工程推断）：Unix 用 `MetadataExt::{ino,dev}`；Windows 没有 Unix 语义的 `ino()`，等价物是 `MetadataExt::file_index()` 配 `volume_serial_number()`，用 `#[cfg]` 封装成统一的 `FileIdentity`。跨 WSL UNC 或 SMB 时 `file_index()` 可能为 0，需按「取不到就退回内容哈希/路径身份」实现。缓存键**必须带 size**（本实现用 `fileIdSize`），否则文件被追加后会把两个不同文件误判为同一份。哈希失败应设计为**回退到路径身份**而不是报错。

**⑩ 双队列分离：本地维度状态与云端事实状态彻底分开**

问题：同一批解析结果既要服务云端（按 `source/model/hour` 计费与展示），又要服务本地项目视图（按 `project_key/source/hour`）。混在一个文件里会造成两类污染：项目维度字段进入云端 schema；**本地队列的积压被误算成待上传量**。

要点：一次解析同时向两个队列 append（`src/lib/rollout.js:683-686`）；项目行是主行的**裁剪版本，不是同构**——比主行**多** `project_ref` + `project_key` 两个前导字段，同时**少三类列**：`model`（项目视图不按模型细分）、`usage_precision`（主行在 `:3341` 写入）、以及 `...prioritySubsetOf(totals)` 展开的 `priority_*` 子集列（主行在 `:3342` 展开，字段表 `:4073-4079`，值 > 0 才出现）。项目行 `:3515-3532` vs 主行 `:3328-3344`；`project_ref`/`project_key` 缺失的项目桶在 `:3513` 直接跳过。

这条裁剪与读侧 key 互为因果：项目队列读侧 key 是 `project_key|source|hour_start`（`src/lib/local-api.js:145-153`，**不含 model**）；主队列 key 是 `source|model|hour_start`（`:202-211`）。云端路径**只读主队列**（`src/commands/sync.js:3654-3659`）：
```js
// Only the main queue is uploaded by drainQueueToCloud. project.queue.jsonl
// is local project-usage state, so counting it here creates false backlog
// and can keep auto retry alive even after cloud sync has drained.
const pendingBytes = Math.max(0, queueSize - Number(afterState.offset || 0));
```

两侧状态文件也分离：`queue.state.json`（上传字节 offset）/ `project.queue.state.json`（本地读侧 offset）；两个读侧 reader 实例独立（`src/lib/local-api.js:145,202`）。

动机：把「云端事实」与「本地视图」当作两条**独立演进**的通道——项目维度是纯本地的产品特性，可随时增删而不动云端契约。`pendingBytes` 只算主队列，把「本地项目数据堆积导致自动重试永不停歇」这个 bug 从设计上排除。
移植：可以用两个独立表/两个 append-only 文件，但**必须让「是否需要上传」只依赖上传通道自己的水位**（常见错误是共用一个 `dirty` 标志）。项目维度的键要显式规定归并方向，否则跨工具同名目录会分裂成多个项目。

**⑪ 上传进度点：逐批推进 + 只发完整行 + 控制行计入上限**

问题：上传可能被拆成多批、跨多次进程（hook 触发的 sync 每次只有几十秒）。进度必须**可恢复**，不能把写者正在写的半行发出去，也不能让某类控制记录把批次撑爆。

要点（`src/commands/sync.js:4315-4413`）：
```js
for (let batch = 0; batch < batchBudget; batch++) {
  if (Date.now() >= deadline) break;
  if (offset >= queueSize) break;
  const result = await readQueueBatch(queuePath, offset, limit);
  offset = result.nextOffset;
  state.offset = offset; state.updatedAt = new Date().toISOString();
  await writeJson(queueStatePath, state);       // 每批成功后立即落盘
}
```
- 只发完整行（`:4447-4449`）：`if (!record.complete) break;`
- 批上限同时约束两类记录（`:4488-4490,4511-4512`）：控制行 `linesRead += 1; if (linesRead >= maxBuckets) break;`（源码注释说明：该记录随本批发送且字节被 `nextOffset` 覆盖，故下一批紧接其后恢复）。
- 上限链：`MAX_INGEST_BUCKETS = 500`（硬上限，`:4313`）→ `batchSize` 默认 200（`:4315`）→ `upload_scope === "community"` 时批数压到 10（`:4392`）；夹取式 `Math.min(Math.max(1, batchSize), MAX_INGEST_BUCKETS)`（`:4329`）。
- 总预算 `UPLOAD_BUDGET_MS = 90_000`（`:168`），剩余预算作为 `AbortSignal.timeout` 传给 fetch（`:4360-4362`）。
- 超时但有成功批次时**不抛错**，保留已推进的 offset（`:4402-4411`）。

动机：三层限制解决三类故障——单请求记录数上限对应用户端 500 条硬约束（源码注释记录了「TRAE 首次同步每个会话一条 state 记录、不设上限就永久 400」的事故，`:4433-4441`）；批数上限控制单次运行时长；总预算防止某次网络慢把 hook 子进程拖死。**「进度逐批落盘 + 只发完整行」**是让「随时被杀」变成安全操作的关键两条。
移植：offset 必须是**字节**位置，不能用字符数或行号（源码在 `src/lib/upload-queue.js:20-21` 特别声明）。若上传带重试，务必保证服务端对该批次**幂等**（整行 upsert 或带 `upload_id` 去重）。把「已发送字节」与「已确认字节」区分开。

**⑫ 把昂贵的全量扫描摊销成后台审计**

问题：为保持「秒级新鲜」，hook 触发的增量扫只扫近期文件；但历史文件也需偶尔重新审计（口径变化、早期 bug、文件被外部改动）。既不能每轮全量扫，也不能永远不扫。

要点（`src/commands/sync.js:3763-3819`）：
```js
const CODEX_COLD_SCAN_AUDIT_INTERVAL_MS = 24 * 60 * 60 * 1000;   // :294
const CODEX_COLD_SCAN_AUDIT_MAX_SYNCS = 288;                     // :301
if (!state || typeof state !== "object" || state.version !== 1) return true;
if (!Number.isFinite(lastFullScanAtMs) || lastFullScanAtMs <= 0) return true;
if (Number.isFinite(nowMs) && lastFullScanAtMs - nowMs > 5 * 60 * 1000) return true;   // 时钟回拨保护
if (Number.isFinite(nowMs) && nowMs - lastFullScanAtMs >= CODEX_COLD_SCAN_AUDIT_INTERVAL_MS) return true;
const syncsSinceFullScan = Number(state.syncsSinceFullScan || 0) + Math.max(0, Number(deferredSyncs) || 0);
return Number.isFinite(syncsSinceFullScan) && syncsSinceFullScan >= CODEX_COLD_SCAN_AUDIT_MAX_SYNCS;
```
配套：状态写进游标（`{version, lastFullScanAt, lastFullScanAtMs, syncsSinceFullScan, lastSkippedFiles, updatedAt}`，共 6 个字段）；本轮**跳过游标提交**时，把审计增量写进独立文件 `deferred-codex-audit.json`（`:3456-3460`）。

动机：双阈值（时间 24h / 次数 288 ≈ 5 分钟粒度下的 24h）覆盖两种场景——机器长期开着、机器频繁开关机。`lastFullScanAtMs - nowMs > 5min` 这条看似奇怪的判断是**时钟回拨保护**：用户改系统时间后 `nowMs - lastFullScanAtMs` 会变负，普通阈值永远不到期，审计被饿死。
移植：别把「审计」实现成「每 24h 全量重扫并全量重写」——本项目把全量重扫与**修复**分开：审计只让 `parseRolloutIncremental` 看到全部文件（增量逻辑仍按 offset 跳过没变的文件），真正的重建只在 repair 里发生。

**⑬ 读时纠错：不改历史文件，只改解释方式**

问题：口径变了、早期版本写错了字段，但重写历史数据文件会引入不可逆风险；完全不修则历史数字错误。

要点（`src/lib/local-api.js:155-200`）：
```js
function isLegacyInclusiveCodexRow(row) {
  ...
  if (cachedInputTokens <= 0 || inputTokens < cachedInputTokens) return false;
  // 旧口径 input 含 cache read，而 total 仍是 input + output —— 用这个不变量识别
  return totalTokens === inputTokens + outputTokens;
}
```

修正动作：`input_tokens -= cached_input_tokens`；对 `cursor`/`antigravity` 这类「`billable_total_tokens` 曾为 0」的行，读时抬到 `total_tokens`（`:191-198`，对应 issue #106）。同一函数被**读侧与上传侧共用**（上传前也过一遍，`src/commands/sync.js:4504-4509`），保证本地与云端口径一致。

动机：用**不变量**（而不是版本号或时间戳）识别历史坏行，使判定与「什么时候修的」解耦——任何来源的坏行都能被识别，且不需要记录迁移状态。代价是每次读取都做这个判断（几个数值比较，非常便宜）。
移植：读路径与发送路径**共用一个**纠正调用点，并在类型层面禁止绕过。但用不变量识别要谨慎：口径多次变化时不变量可能对某些版本不成立（本项目对 Codex 就是这样，`total_tokens === input + output` 只对「input 含缓存」的一代成立）；无法用单一不变量覆盖时，退回「版本号字段 + 迁移记录」。长期看读时纠错会累积成越来越厚的解释层，建议给每条纠错标注引入原因与 issue 编号，并设定「什么时候可以删掉」的条件。

**⑭ 把「被跳过的运行」做成可见事实**

问题：采集系统最危险的失败模式是**静默停摆**——hook 还在触发、进程也正常退出，但数据一直不更新。常见根因是锁争用或残留锁。

要点（`src/commands/sync.js:484-506`）：
```js
const SYNC_SKIP_MARKER = "sync.skip.json";
const reason = holder.exists && holder.pid && !holder.alive ? "lock_debris" : "lock_busy";
const detail = !holder.exists ? "sync lock could not be acquired"
  : holder.pid ? `sync lock held by pid ${holder.pid} (${holder.alive ? "running" : "not running"})`
  : "sync lock has no owner record";
await writeJson(path.join(trackerDir, SYNC_SKIP_MARKER), { reason, detail, at: new Date().toISOString(), lockPath });
```
抢到锁后**立即删除** marker（`:565`），因此 marker 存在即代表「最近一次尝试被跳过」；状态命令读它并上报（`src/commands/status.js:285,1058`）。同类的可见化还有：上传退避写 `upload.throttle.json` 并在 status 暴露 `next_upload_after` / `backoff_until` / `last_upload_error`（`:1054-1057`）。

动机：`lock_busy` 与 `lock_debris` 的区分是核心——前者是正常并发（无需处理），后者是**需要人工介入**的状态（锁残留）。把两者写进同一个可查询的产物，让看板能直接给结论而不是靠猜。
移植：**别退化成一个布尔量**——「被跳过」的排障价值全在 reason 上。时间戳统一用 UTC ISO8601。

**⑮ hook 触发的三件套：先落信号、再节流、然后脱离进程**

问题：provider 的 hook 会**同步等待**命令返回。hook 里直接跑解析会把几秒到几十秒算进用户交互延迟；每个 turn 都触发一次完整解析会拖垮机器。

要点（生成 `~/.tokentracker/bin/notify.cjs` 字面内容的模板 `src/commands/init.js:1453-2231`，宿主函数 `buildNotifyHandler` `:1445-2232`）：
```js
// :1446-1447 文件头注释直接写明了设计约束
// Keep this file dependency-free: Node built-ins only.
// It must never block Codex; it spawns sync in the background and exits 0.
// :1495-1498 ① 先落信号（即使后续节流跳过，也留下"有过活动"的证据）
try { fs.mkdirSync(trackerDir, { recursive: true });
      fs.writeFileSync(signalPath, new Date().toISOString(), { encoding: 'utf8' }); } catch (_) {}
// :1519-1536 ② 20 秒节流 + ③ 脱离进程
if (now - last > 20_000) {
  try { fs.writeFileSync(throttlePath, String(now), 'utf8'); } catch (_) {}
  const syncArgs = ['sync', '--auto', '--from-notify', '--source', source];
  if (hasLocalRuntime && hasLocalDeps) spawnDetached([process.execPath, trackerBinPath, ...syncArgs]);
  else spawnDetached(['npx', '--yes', fallbackPkg, ...syncArgs]);
}
// :1563-1573
function spawnDetached(argv) {
  const child = cp.spawn(argv[0], argv.slice(1), { detached: true, windowsHide: true, stdio: 'ignore', env: process.env });
  child.unref();
}
```
再加两件：④ **链式调用用户原有的 hook**（先备份原命令再在末尾追回，`:1538-1559`）；⑤ 无条件 `process.exit(0)`（`:1561`），保证 hook 永不因我们失败而失败。

另需知道一处**冗余判断**：`if (hasLocalRuntime && hasLocalDeps)` 里两个变量指向**同一个路径**——模板内 `trackerBinPath`（`:1483`）与 `depsMarkerPath`（`:1484`）都是 `path.join(trackerDir,'app','bin','tracker.js')`，字面相同，因此该条件退化为**一次 `existsSync`**；同一 quirk 也出现在 OpenClaw 插件模板（`src/lib/openclaw-session-plugin.js:358,361`）。移植时按「本地 runtime 存在 → 本地 exe；否则走兜底命令」实现即可，不必照抄这层冗余。

动机：三点顺序不可换——signal 先写，所以即使节流拦掉本次 spawn，也能知道「用户刚刚在活动」；节流在 spawn 之前写时间戳，避免并发 hook 同时通过判断造成雪崩；detached + unref + `stdio:'ignore'` 让子进程完全脱离父进程生命周期。
移植（工程推断）：真 detached 在 Windows 上要 `CREATE_NEW_PROCESS_GROUP` / `DETACHED_PROCESS`（`std::os::windows::process::CommandExt::creation_flags`），Unix 用 `setsid`；若不做 detached，父进程被 kill 时子进程可能一起死。节流状态文件是**读-判断-写**三步，本身不原子（并发 hook 可能都通过），本项目接受这个近似（重复 spawn 的后果只是多一次解析）。hook 处理器**不要依赖仓库/依赖包**——本项目生成的文件只用 Node 内置模块，并在本地 runtime 缺失时回退 `npx --yes <pkg>`。

**⑯ 向用户配置文件写入：三种幂等范式 + 一类整体覆盖 + 一律先备份**

问题：往用户的 `settings.json` / `config.toml` 里插自己的 hook，必须重复安装不出重复项、命令路径变化时不产生「旧条目 + 新条目」、卸载时能精确摘掉自己的项且不动别人的配置。

要点（按配置类型选范式）：
- **① 命令身份识别**（跨路径漂移仍认出自己）——`src/lib/notify-command.js:23-36`：解析命令行 token，只接受 `node(.exe) notify.cjs --source=<id>` 这一形状，返回 `${file}\u0000${args[2]}` 作为身份。
- **② 就地修复 + 同 scope 去重**（JSON hooks 结构）——`src/lib/claude-config.js:181-230`：把过时形态改写为 `{type:"command", command, shell?}`，并按 `matcher` scope 去重。
- **③ 哨兵串 + 拒绝覆盖外来文件**（**本项目只对 omp 这么做**）——`src/lib/omp-hook.js:17,388-395`：文件带 `// @tokentracker-managed-omp-extension` 则覆盖，不带则跳过并回报 `skippedReason: "unmanaged-extension-present"`（该字符串**全仓仅此一处产生**，消费方 `src/commands/init.js:813`）。
- **④ 内容等值短路 + 无条件备份覆盖**（OpenCode 插件）——`src/lib/opencode-config.js:66-87`：先比对内容，`if (existing === next) return { changed: false }`（`:73-75`，**这是内容等值短路，与哨兵无关**）；内容一旦不等，**无论原文件里是什么都先备份再覆盖**（`:80-85`）。标记检测（`hasPluginMarker`）只出现在 `removeOpencodePlugin`：内容不带标记时返回 `skippedReason: "unexpected-content"`（`:94`）。**OpenCode 不会拒绝覆盖外来文件**——不要把它当哨兵范式。

配套：所有写入路径先复制 `settings.json.bak.<ISO>`（`src/lib/claude-config.js:248-262`、`src/lib/gemini-config.js:265-279`、`src/lib/codex-config.js:89-104`）；仅 Codex 系需要额外把**被覆盖的原命令**存到 `*_notify_original.json`（因为 notify 是独占槽位）。TOML 侧不引入解析库，用手写的最小行编辑（`src/lib/codex-config.js:245-309`）。

动机：不同配置文件类型对「幂等」的容错能力不同——JSON hooks 是数组结构，适合结构化比对与修复；TOML 只有一行 `notify = [...]`，适合等值比较；插件/扩展文件是**整个文件属于别人**，最稳的形态是「不认识就不写」（哨兵），代价是用户原有的同名插件会被永久跳过，所以 OpenCode 改用「先备份再覆盖」。把这几类分别实现，比试图写一个通用「配置补丁器」可靠得多。
移植：Rust 侧 JSON 用 `serde_json::Value` 做结构化改写并保留未知字段；TOML 若用 `toml_edit` 务必用其「保留注释与格式」的编辑 API——本项目的手写行编辑会在复杂 TOML 特性（多行数组、行内注释、多行字符串）下失效【未能确认：源码注释自称支持 single-line / multi-line arrays，未覆盖面更广的语法】。**备份策略要一致**：Grok 分支无备份、无条件覆盖（`src/lib/grok-hook.js:85-91`），是明显的策略缺口，别照抄（见 10.2）。

**⑰ 发现层的三个反直觉细节**

① **「必须包含子目录」的判定要折进探测本身**（`src/lib/install-resolver.js:25-36`）：`populated(p) = !requireAnyChild || hasAnyChild(p, requireAnyChild, deps.existsSync)`，WSL 候选也套同一谓词。原因是 `discoverWslHome` 的语义是「返回第一个存在的候选」——若先拿回候选再校验，第一个空壳就会胜出并且**再也不会去看第二个发行版**（`src/commands/sync.js:766-772`）。

② **合并模式只在解析器具备路径无关身份时才启用**（`src/lib/install-resolver.js:6-18` 的注释即契约）：`union: true` 仅用于 Codex/acode（事件按 `sessionUUID:eventTimestamp` 去重）与 Claude（手工 union + 文件哈希 + 消息哈希）。这条约束防止把「双计」当「覆盖」。

③ **WSL 会话的 POSIX cwd 要用转录文件自身的 UNC 前缀重锚**（`src/lib/wsl-probe.js:232-242`）：
```js
if (typeof cwd !== "string" || !cwd.startsWith("/")) return cwd;
// `new URL("file:///C:/dev/app").pathname` is "/C:/dev/app" — a leading
// slash, but a Windows path rather than a POSIX one. Qoder feeds exactly
// that shape in via `project_uri`; re-anchoring it would invent
// `\\wsl$\Distro\C:\dev\app`.
if (WINDOWS_DRIVE_PREFIX_RE.test(cwd)) return cwd;
const root = wslUncRoot(transcriptPath);
if (!root) return cwd;
return root + cwd.slice(1).replaceAll("/", "\\");
```
前缀取自实际读取的文件路径，因此自动选中本机真正可用的 `\\wsl$` 或 `\\wsl.localhost` 之一（两者并非总是等价，本项目按发行版版本排序两种前缀，`:193-201`），同时显式避免把 `C:\...` 变形成 `\\wsl$\Distro\C:\...`。

移植：Rust 侧判断「以斜杠开头但不是 POSIX 路径」建议显式实现盘符前缀正则（本项目用 `/^\/[A-Za-z]:[\\/]/`），因为 `file://` URI 解析结果经常长这样。跨 UNC 读 SQLite 用「复制 db + `-wal` + `-shm` + `-journal` 到临时目录」的方式（`:244-262`）；用 `rusqlite` 时优先尝试 `VACUUM INTO`，失败退回复制文件组。

**⑱ 可测性与兼容性的两个小成本高收益做法**

**A. 依赖注入默认参数化**，全部落在函数签名上（生产代码零分支）：`probeWslDistros(deps = {})`（`src/lib/wsl-probe.js:57-63`，`deps.runWsl` 可替换）；`openCursorStore({ ..., failureInjector = null } = {})`（`src/lib/cursor-store.js:25-32`）；`const cursorSyncDeps = context && typeof context.cursorSyncDeps === "object" ? context.cursorSyncDeps : {};`（`src/commands/sync.js:516-538`）。`maybeInjectFailure(this.failureInjector, "beforeManifestSwap")`（`src/lib/cursor-store.js:972`）允许测试精确验证「manifest 切换前崩溃」的清理路径。【未能确认】这些测试缝在生产路径是否完全惰性（源码注释称 "no public CLI flag/config is introduced"，`src/commands/sync.js:526-531`），但从签名看生产调用者不传即为 no-op。

**B. 显式别名表 + 双层白名单**（`src/commands/sync.js:310-373`）：
```js
const AUTO_SYNC_SOURCE_ALIASES = new Map([
  ["code", "every-code"], ["commandcode", "command-code"], ["deepseek", "dsh"],
  ["everycode", "every-code"], ["kilo", "kilo-cli"], ["kilo-code", "kilocode"],
  ["kimi_code", "kimi-code"], ["roo-code", "roocode"],
]);
const AUTO_SYNC_SOURCES = new Set([ /* 实测 45 项 */ ]);
const BACKGROUND_AUTO_SYNC_SOURCES = new Set(["acode", "codex", "every-code", "reasonix"]);
```
别名表把外部叫法与内部 id 解耦，使外部参数可随产品改名而不动内部状态；双层白名单让「后台轻量扫描」的源集合成为显式、可审计的常量。别名归一只做一次（入口 `normalizeSyncSource`，`:3755-3762`），后续所有路径使用内部 id。`AUTO_SYNC_SOURCES` 实测成员 45 项（逐项读取 `:320-366`）：`acode, antigravity, anythingllm, claude, claude-science, cline, codebuddy, codex, command-code, copilot, craft, cursor, devin, droid, dumate, dsh, every-code, gemini, goose, grok, hermes, kilo-cli, kilocode, kiro, kimi, kimi-code, lmstudio, mimo, minimax-code, omo, omp, opencode, openclaw, pi, qoder, qoder-cn, reasonix, roocode, trae, trae-cn, unsloth, workbuddy, workbuddy-ai, zcode, zed`。

### 10.2 不建议照抄的设计

以下条目附源码依据或源码内的事故记录。

1. **`cmdSync` 单函数 3190 行 / `sync.js` 7251 行的巨型编排**（`src/commands/sync.js:516-3709`）。锁、路径解析、40+ provider 分支、迁移、上传、重试、输出全塞进一个 async 函数，靠 `let` 变量的顺序赋值维持隐式状态机。`skipNoOpCursorCommit`（`:3424-3437`）的判据要同时引用 **13 个条件项**（`opts.auto`、`!isFullSourceScan`、`cursorStore.mode === "v2"`、`cursorStore.requiresCommit !== true`、`totalParsed === (…)`、`totalBuckets === 0`、两处 project 桶均为 0、`!commandCodeResult.schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok`）。**应拆成显式阶段（discover → parse → persist → upload），用类型/结构体传递阶段产物。**
2. **`.reclaim` 闸链需要「迭代清扫」才能自愈**（`src/lib/fs.js:178-204`，issue #431）。锁协议复杂到需要专门清扫自己的残留闸，说明协议过重。更简单的替代：单一 `O_EXCL` 锁 + token 心跳 + 超时接管，且**接管动作本身**也走「原子 rename 到隔离名」路径，无需第二层闸。
3. **队列写入无任何写侧互斥**。主队列 append 写点**至少 6 处**（`src/lib/rollout.js:3483`、`:20031`、`:22594`，`src/commands/sync.js:5126`、`:5207`、`:5281`），另 `src/commands/sync.js:4114` 写 fallback、`src/lib/rollout.js:3536` 写 project 队列——全部假定调用方持有 `sync.lock`。一旦加第二个写者，两个 `appendFile` 会交错写出多行撕裂。**建议：要么给队列加单写者约束（只有持锁进程能打开 append 句柄并用 flock 强制），要么用 SQLite/日志合并结构代替。**
4. **`repair` 的「原子删除 + 重扫」模式本身**（0.26.3 的真实数据丢失事故，`src/commands/sync.js:234-248`）。它已用守卫弥补，但根因是**把「重算」实现成了「先删后建」**。更安全的形态是写入新快照、**新数据集完整生成后**整体切换（即本项目 cursor-store 里已用得很好的 generations 模式），绝不在旧数据上做删除。
5. **Grok 分支无条件覆盖、无备份**（`src/lib/grok-hook.js:85-91`，注释 `always overwrite to keep command up-to-date if bin path changes`）。与其它 provider 的「幂等 + 备份 + 还原」策略不一致；`~/.grok/hooks/99-tokentracker-usage.json` 这一固定文件名下的用户自定义内容会被直接覆盖。**要抄就抄 Claude/Codex 范式。**
6. **同一语义存在三套并行纠错机制**：一次性 `repair` 迁移键、读时 `normalizeQueueRow`（`src/lib/local-api.js:170-200`）、云端整行 upsert。三者口径必须人工保持同步——历史上就出现过「云端 `conversation_count` 恒为 0 而本地正确」这类不一致（`src/commands/sync.js:249-256` 的注释即为此事故）。**建议：把口径收敛成一份可执行规范（含测试向量），让三条路径都跑同一份用例。**
7. **文件体积驱动的行为切换**（16 MB 阈值决定用 LegacyCursorStore 还是 v2，`src/lib/cursor-store.js:21,77-86`）。同一用户的存储形态会在某次运行后**静默改变**，且两套实现能力不等价（legacy 下 `codexEventStore = null`，事件去重退化为内存数组，`:165-197`）。**建议：一次性迁移到一个形态，用显式 schema 版本字段而不是体积阈值。**
8. **`legacyGroups` / `groupQueued` 那条「整小时聚合成 `model=DEFAULT_MODEL`」的遗留路径**（`src/lib/rollout.js:3181-3194,3430-3480`）。其语义依赖「某个键存在与否」作为隐含标记，**新装机会不会写入该键尚未验证**（见 §12 A4）；这类**用状态形状编码语义**的做法极易在后续修复中互相干扰（v2→v3 的注释正是这类冲突的产物，`src/commands/sync.js:216-226`）。
9. **代理配置错误让整个进程 `exit(1)`**（`bin/tracker.js:36-40`），hook 路径同样受影响。结果是「代理挂了 → 采集静默停止」，数据不更新但没有任何面向用户的告警（只有被 `stdio:'ignore'` 丢弃的 stderr）。**建议：采集路径不能因为网络配置问题硬失败；应降级跳过上传并留下可查询状态。**
10. **`local-api.js`（3543 行 / 156 KB）职责混杂**：读队列、项目归因、账号体系、头像代理、宠物、静态资源与 SPA 回退都在一个 handler 工厂里。虽然每个功能内部质量不差，但耦合度高到任何改动都要回归全部端点。**建议按端点族拆分模块，用薄路由层组合。**
11. **测试专用钩子进入生产函数签名**（`context.traeCnFetchImpl`、`context.cursorSyncDeps`、`context.scanSources`，`src/commands/sync.js:519-538`）。成本低但持续污染 API 表面积；更干净的做法是把依赖收进一个 `Deps` 结构体，或用 `#[cfg(test)]` / 依赖注入容器统一管理。
12. **依赖 `st.ino` 的变化来检测原子替换**（`src/lib/rollout.js:489-492`）。在 inode 语义不可靠的环境（某些网络盘、容器叠加文件系统、WSL UNC）上 `ino` 可能恒为 0 或恒定，导致「替换后从 0 重读」的检测失效，此时只能靠事件哈希兜底。**建议：把「文件身份」做成可配置策略（inode → file-id → 内容抽样哈希），并在启动诊断里显示实际采用的策略。**
13. **把 Windows 侧 7680 端口占用交给「WSL 默认换端口 7681」**（`src/commands/serve.js:15-21`）。这是对 Delivery Optimization 服务行为的补偿性绕行，但仍会与其它占用 7681 的进程冲突。**建议：端口发现走「嗅探 + 用户可见提示」，而不是固定的 +1 规则。**
14. **`status` 的 `hookStatus` 传参里有两个键在 passive 判定中永不使用**（`src/commands/status.js:1030-1031` vs `src/lib/passive-mode.js:93-186`），以及 `workbuddy-ai` 这个带连字符的特殊键名。这类「传参与消费不一致」的接口会随 provider 增加持续腐化。**建议：hook 状态用显式 provider 枚举 + 结构体，编译期保证双方一致（Rust 的 enum 天然能做到）。**
15. **关键常量双份定义、无测试绑定**。`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 在 `src/lib/pricing/index.js:31` 定义并 `:375` 导出，但**在本文件内从未被读取**；真正生效的是 `src/lib/codex-rollout-parser.js:51` 的另一份同名常量（用于 `:733`）。两处目前都是 `272_000`，改一处不另一处不会有测试失败。同类：半小时桶的 `toUtcHalfHourStart` 有两份实现（`src/lib/rollout.js:4173-4190` 与 `src/lib/claude-categorizer.js:1175-1191`，**语义等价、文本不同**），`test/` 下两个测试都只指向 `rollout.js` 那份，副本**无人测**——真值修复与增量解析一旦分叉，队列会出现两套时间分桶。
16. **「不变量」式识别若无对等回归测试，会静默双计**。Codex 侧的 reasoning 不并入 output 依赖 `src/lib/pricing/index.js:248-256` 的 `source` 白名单硬编码（`codex`/`acode`/`every-code`/`omo`/`cline`）；一旦上游新增一个 `*-code` 变体而白名单漏加，就会**静默双计**每条 reasoning token，而现有测试全绿（`test/claude-reasoning-usage.test.js:45-47` 只锁定了 Claude 的「列分布可换、费用不变」）。**建议白名单改为结构性判定**（如 `source.endsWith("-code") || source === "codex"`）并补对等断言。
17. **回退链的本地/云端能力不对等**。云端 `computeRowCost`（`dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:569-613`）只有纯四桶 + 条件性推理折叠，**没有**长上下文 / Fast 溢价分支；该表与 `curated-overrides.json` 存在 **9 键**差集，其中 `grok-build-free`（本地 $0 vs 云端按 `grok-build` = 1.25/2.5/0.2 计）与 `grok-4.5-build-free`（本地 $0 vs 云端 `cursor-grok-4.5` = 2/6/0.5）是**价格层面的实际分歧**。parity 测试（`test/edge-pricing-parity.test.js:76-85`）只做 edge↔edge 字节比对，**不含**「edge ⊇ curated」断言，抓不到这类漂移。

> 结论性判断：该仓库**在「崩溃安全、并发正确性、历史数据不可逆风险」这三类问题上给出的机制质量很高，可以直接照抄**（10.1 全部条目）；而**在编排结构、状态形状语义、多路径口径一致性、以及「网络/平台失败是否应硬退出」这些地方是反面教材**（10.2）。移植时应把两半严格分开对待。

### 10.3 TokenScope 直接相关的差异提示

以下差异不是优劣判断，而是**会让两边数字不同**的口径事实，移植时必须显式决策。

**① 项目身份：仓库 URL vs 规范化绝对路径 + 层级归并**

TokenTracker 的用量侧项目身份来自 `deriveProjectKeyFromRef`，**只吃 URL**（`src/lib/rollout.js:4429-4442`：按 `/` 切段后 `segments.join("/")`），无 cwd、无层级归并，且 `.git` 后缀不剥离；会话侧项目键另取 basename（`src/lib/session-analytics.js:405-408`）。TokenScope 的既定口径是「规范化绝对路径 + 按项目根归并、判定按分量边界前缀、不做文件系统访问」。**二者不可互相替代**：前者在远程托管场景下跨机器天然一致，后者在本地多目录场景下才正确。若照搬 URL 身份，会丢掉「同名不同盘」「子目录归属」这两类信息。

**② `conversation_count` 跨来源不可比，且同名指标在 21 家里语义五家五样**

至少三种口径并存：产生差分的消息数（Gemini/Opencode）、每条事件 +1（Kimi 一族）、仅首次（Cline）、仅首次会话或重置（Droid）、恒 1（Goose/Lmstudio/Unsloth）、DB `message_count` 差分（Hermes）、线程更新次数（Zed）。Claude 侧按**主会话 user 文本消息**计（`src/lib/rollout.js:2807-2837`，`:2833` 落在 `isMainSession && type==="user"` → `hasText` 内层分支），而 Craft 侧的注释自称 "count one-per-assistant-message" **与代码冲突**（`:17233-17234`）——源码自身两处说法互相打架，以代码为准。**任何跨来源「会话数」汇总都只是粗略值**，若不打算把它当可比指标，就不要在 UI 上并列展示。

**③ 队列是累计快照 + 撤回行，而不是事件流**

TokenTracker 的 `queue.jsonl` 是**每 `(source, model, 半小时)` 一行的累计快照**，写入是纯追加（`src/lib/rollout.js:3482-3484`，仅在有新增行时执行），读侧同键「最后一条胜出」（`src/lib/local-api.js:202-211`；`src/lib/async-queue-reader.js:51-57,86`）。当 `unknown` 桶被对齐到真实模型时，会**先追加一条全零行**抵消旧归属（retraction，`src/lib/rollout.js:3251-3317,3351-3402`）。因此：
- 「同一键出现多行」是常态而非异常，零行必须被解释为撤回而不是「零用量」；
- `setMaxTotal`（`src/commands/sync.js:6585-6589`）与 `mergeMaxTotals`（`:6574-6583`）**只在 codex rescan/fork 修复的数据丢失守卫里使用**（唯一调用族 `:5848-5879`，两处 `skipping to avoid data loss`）：因为撤回零行会把 last-wins 的末行压成 0，只有 MAX 才能保住历史峰值。**两套口径并存不矛盾，但用途必须分清。**
- TokenScope 是 SQLite `history.db` + 每来源事件，没有「零行撤回」概念，对应语义要重新设计（例如显式的 correction 事件或归属修订）。

**④ 时区：TokenTracker 的两级回退 vs TokenScope 的单一权威源**

TokenTracker 的时间口径分两级：① `rowDayKey` 一族（日键 + 日键区间过滤）在 `tz`/`tz_offset_minutes` 皆缺时**静默退化为 UTC 日**（`src/lib/local-api.js:213-225`，注释在 `:224`），不报错；② 半小时桶本身**恒为 UTC**（`toUtcHalfHourStart`），日归属完全交给请求参数。此外同一面板内存在两套口径：热力图网格按 UTC 日（`:2540`），`night_owl` 徽章按请求时区小时（`:482`）。TokenScope 的既定口径是「存储层一律 UTC 原样持有、全链路只做一次时区转换、时区唯一权威来源是 `settings.toml`、一次查询只在入口解析一次『今天』并冻结」。**TokenTracker 没有「冻结 as_of」这个概念**——每次请求都重新解析 `new Date()`（`:2382`）。移植时不要去仿它的两级回退。

**⑤ 计价：只有 LiteLLM 一家动态来源，且没有外置价格文件**

TokenTracker 的价格是三层：`curated-overrides.json`（内置策展表，84 条 `exact` + `source_exact.acode` 64 条 + `fuzzy` 47 条，最高优先级、永不被网络覆盖）→ LiteLLM 运行期数据（磁盘缓存 `~/.tokentracker/cache/pricing.json` → 上游 URL → 陈旧缓存 → 内置 seed，四级降级，`src/lib/pricing/litellm-fetcher.js:107-164`）→ 内置 `seed-snapshot.json`（2284 条模型，最低优先级、离线兜底）。**没有 models.dev、没有 OpenRouter、没有用户可编辑的外置价格文件**（`src/` 全目录零命中；数据目录下与价格有关的派生物只有 `cache/pricing.json`）。

与前序研究的对照更能说明差异：ccusage 是**单表 + 覆盖**（键级整条替换、用户 override 字段级、内置表默认 `or_insert`），并**有编译期内嵌价格表**（编译期裁剪 LiteLLM + models.dev 快照）；tokscale 是**6 个上游来源（3 动态 + 3 内置）+ 1 张用户自定义表**、候选择优；TokenTracker 是**3 层、只有 1 家动态上游、有内置 seed 但无用户外置表**；TokenScope 是**3 来源候选（外置 `pricing.toml` / models.dev / OpenRouter）且无编译期内置表**。四者的「内置表」取舍各不相同：TokenScope 用「首次启动需同步」换「未收录模型始终未知、不静默按 0 吞掉」；TokenTracker 用「内置 2284 条 seed」换离线可用，代价是**离线/首启状态下价表年龄可能远大于 TTL**（seed 的 `_meta.generated_at` 是 `2026-05-26T11:12:29.761Z`，而运行期磁盘缓存的 TTL 只有 24 小时，`src/lib/pricing/litellm-fetcher.js:12`）。

另有两处 TokenScope 已明确规避、而 TokenTracker 存在的行为：**缓存写入只有单一 `cache_write` 槽位**（`src/lib/pricing/matcher.js:410-412`，不区分 5 分钟/1 小时缓存档，因此会低估 1 小时缓存写入的成本）；**推理折叠名单是硬编码白名单**而非结构性判定（见 10.2 第 16 条）。还有一处数量级差异：LiteLLM 侧约 **75% 的模型没有缓存读价、约 92% 没有缓存写价**（`seed-snapshot.json` 2284 条中 `cache_read_input_token_cost` 仅 566 条、`cache_creation_input_token_cost` 仅 188 条），走该路径且事件带非零 `cached_input_tokens` 的模型，缓存读取部分会被算成 **$0**——这不是 bug 而是上游数据缺口，移植时若只接 LiteLLM 一家会继承同样的缺口。

**⑥⑦ 去重分层与增量策略的取舍方向**

TokenTracker 的去重分层达七层，其中两个概念 TokenScope 目前没有对应物：**账户级来源**（`src/lib/source-metadata.js:1,7-13` 把 `cursor`、`trae-cn` 标为 account 级，可被 personal/local 过滤排除）与**双安装命名空间光标**。这两者是否需要在 TokenScope 落地，应在设计阶段显式回答，而不是靠移植代码时顺手带上。
增量策略上三者方向相反：TokenTracker 每次全量扫描文件系统但**省重解析**（文件 offset + inode + 事件哈希三层游标）；ccusage 每次全量重扫但**省读取**（按 mtime + 会话组并查集剪枝 + 行级预过滤）；tokscale 只对 Codex 做文件 append 增量。TokenScope 是「每次串行全量解析 + `(size, mtime)` 指纹缓存」。三种取舍各有代价，建议在显式记录口径后再决定是否引入剪枝——**若引入 mtime 剪枝，必须按会话组整体判断**，否则会踩 sidechain 跨文件去重的坑。

---

## 11. 关键常量与阈值速查

| 常量名 | 值 | 位置 | 含义 |
| --- | --- | --- | --- |
| 半小时桶 | UTC 半点对齐 | `src/lib/rollout.js:4173-4190` | `toUtcHalfHourStart`：所有行的桶起点恒为 UTC 半小时；另有语义等价副本 `src/lib/claude-categorizer.js:1175-1191` |
| 日键回退 | 缺失 `tz` 时退化 UTC 日 | `src/lib/local-api.js:213-225` | `rowDayKey`：有有效时区时用 `Intl.DateTimeFormat` 分区格式化，否则 `hour_start.slice(0,10)`（注释在 `:224`） |
| 队列批量与上限 | `batchSize` 默认 **200**、`MAX_INGEST_BUCKETS = 500`（硬上限）、社区档批数 **10** | `src/commands/sync.js:4313,4315,4392`（夹取 `:4329`） | 单批记录数 / 单请求硬上限（对应用户端 500 条约束，事故注释 `:4433-4441`）/ `upload_scope === "community"` 时 `batchBudget = Math.min(batchBudget, 10)` |
| 上传总预算 | `UPLOAD_BUDGET_MS = 90_000`（90 s） | `src/commands/sync.js:168` | 单次 drain 的总时长预算，剩余预算作为 `AbortSignal.timeout` 传给 fetch（`:4360-4362`）；超时但有成功批次时不抛错（`:4402-4411`） |
| hook 节流 | **20 s** | `src/commands/init.js:1519-1536` | notify.cjs 模板：`if (now - last > 20_000)` 才 spawn sync；时间戳在 spawn 前写入 |
| 锁心跳 / 锁超期 | `LOCK_HEARTBEAT_MS = 30 s` / `LOCK_STALE_MS = 5 min` | `src/lib/fs.js:69-70` | 持锁期间 `utimes` 锁文件与心跳文件并 `timer.unref()`；心跳超期即视为遗弃（老格式锁退回 mtime 判断 `:105-144`） |
| 回收闸 / 过渡守卫 | `MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64`、`TRANSITION_GUARD_RETRY_MS = 5`、`TRANSITION_GUARD_ATTEMPTS = 1000` | `src/lib/fs.js:71-74` | `.reclaim` 链最大嵌套 / 迭代清扫链上限（issue #431 自愈）/ 锁状态转换的自旋重试参数 |
| JSON 读改写锁超时 | **30 s** | `src/lib/fs.js:416-443` | `updateJsonLocked` 用 `<file>.lock` 做读-改-写整体互斥 |
| 冷扫描触发 | 24 h（`CODEX_COLD_SCAN_AUDIT_INTERVAL_MS`）或 288 次（`CODEX_COLD_SCAN_AUDIT_MAX_SYNCS`）；时间戳落在未来 > 5 min（时钟回拨保护）亦强制触发 | `src/commands/sync.js:294,301,3772-3781` | 双阈值覆盖「机器长期开着」与「频繁开关机」两种场景；回拨保护避免审计被永久饿死 |
| 游标阈值 | `DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024` | `src/lib/cursor-store.js:21` | 旧 `cursors.json` 超过 16 MB 才启用 v2 代际存储（`:77-86`） |
| 项目上下文重扫 | `PROJECT_ABSENT_CONTEXT_RESCAN_MS = 24 h` | `src/lib/cursor-store.js:22` | 项目上下文缺失的重新探测周期（`:1404-1406`） |
| 长上下文阈值 | `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000` | 定义 `src/lib/pricing/index.js:31`（**本文件零读取**）；生效者 `src/lib/codex-rollout-parser.js:51` → `:733` | 输入（含缓存）超 272K 才启用 OpenAI 长上下文溢价；仅 `gpt-5.6`/`gpt-5.6-sol`/`gpt-6-astra`/`gpt-6-sol`/`gpt-6.1-sol` 系列启用（`src/lib/pricing/index.js:266-306`） |
| DeepSeek 周末起点 | `Date.UTC(2026, 7, 22, 16, 0, 0)` | `src/lib/pricing/index.js:186` | off-peak 判定：按 +08:00 平移取 weekday，周末即 off-peak；工作日 `peak = [1,4) ∪ [6,10)`（UTC 小时，`:183-207`） |
| DeepSeek 折扣 | 四类单价 **×0.5** | `src/lib/pricing/index.js:34-39,209-222` | 命中 `DEEPSEEK_TIME_PRICED_MODELS` 且判为 off-peak；`acode` 来源豁免 |
| 价格缓存 TTL 与降级链 | `DEFAULT_TTL_MS = 24 h`、`DEFAULT_FETCH_TIMEOUT_MS = 10_000`；降级 **4 级** | `src/lib/pricing/litellm-fetcher.js:12-13`、`:107-164` | 磁盘缓存新鲜度阈值与上游取数超时；disk-cache（`:128-137`）→ upstream（`:139-146`）→ stale-cache（`:148-158`）→ seed-snapshot（`:160-163`） |
| 负缓存 | `state.negativeCache`（`Set`） | `src/lib/pricing/index.js:64-66,120-131` | 按 `source\0model` 记忆未命中，避免每次全表遍历；`revision` 变化时清空（`:86`、`:105`） |
| 策展表条目 | `exact` **84** / `alias` 1 / `source_exact.acode` **64** / `fuzzy` **47** | `src/lib/pricing/curated-overrides.json`（775 行 / 29754 字节） | 最高优先级内置表；`MODEL_PRICING = curatedOverrides.exact`（`src/lib/pricing/index.js:363`） |
| seed 快照条目 | 顶层 2285 = 1 `_meta` + **2284** 模型 | `src/lib/pricing/seed-snapshot.json`（256869 字节） | 最低优先级离线兜底；`require` 时同步加载（`src/lib/pricing/index.js:44-55`） |
| 云端手抄表 | **119** 键（+ iFlytek MaaS 64 键） | `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142-332`（119）/ `:339-404`（64） | 与 `curated-overrides.json` 存在 9 键差集，parity 测试不校验超集关系 |
| priority 乘数条目 | **3** 条（`gpt-6-astra`/`gpt-6.1-sol`/`gpt-6-sol` = 2） | `src/lib/pricing/curated-overrides.json` 的 `exact` | 只有 `pricing.priority_multiplier > 1` 才启用 Fast 档溢价 |
| fork 判定 | `FORK_REPLAY_GAP_MS = 500` | `src/lib/rollout.js:4210` | 重放行间隔 < 500 ms 视为 fork 重放突发（实证：重放 flush 亚毫秒到几毫秒，真实轮次 ≥ ~4.6 s，重放→实况断点 ≥11 s） |
| fork 判定（应用点） | 双条件 | `src/lib/rollout.js:2588-2601` | 跨日靠日期守卫（`:4212-4214` `isForkedReplayToken`），同日靠 500ms 突发检测；`replayPrefixActive = startOffset === 0`（`:2339`） |
| Claude 哈希上限 | **100 000**（保留最后 10 万） | `src/lib/rollout.js:1236-1237` | `cursors.claudeHashes` 截断；注释 `:1021-1025` 自述淘汰会让批量历史双计，故加文件级去重作为第二层 |
| 事件集合 | 按日分片持久化 | `src/lib/cursor-store.js:681-726` | `loadEventSet`：Codex 侧事件键按日分片落盘，Claude 侧靠 10 万条硬截断 |
| project 汇总上限 | **10** | `src/lib/local-api.js:2737-2740` | `/functions/tokentracker-project-usage-summary` 的 `limit` 硬上限（排序按 `billable_total_tokens` 降序，`:2785`） |
| sessions 分页上限 | **2000** | `src/lib/local-api.js:2505` | `/functions/tokentracker-sessions`：`Math.min(limitParam, 2000)`，缺省 0（不分页）；其余用量端点一律返回全量数组（`:95`） |
| 队列压缩块 | `CHUNK_BYTES = 64 KiB` / `BACKLOG_BYTES = 1_000_000` | `src/lib/upload-queue.js:5-6` | 压缩读写分块 / 触发压缩的最小 pending 字节 |
| 读侧分块 | `CHUNK_BYTES = 64 * 1024` | `src/lib/async-queue-reader.js:5` | 每块后 `setImmediate` 让出；读缓存最多 4 个文件（`:16,94`） |
| seenIds / 认领 / 快照上限 | 各 provider `seenIds` **10000**；`usageClaims` 保留 7 天、上限 10000；会话快照 10000 | `src/lib/rollout.js:16041-16050`、`:16680`、`:18089-18091`（seenIds）；`:17375-17405`、`:18058-18064`（claims）；`:19334-19355`、`:19645-19649`（快照） | entry id 去重集合 / Copilot 三通道用量认领 / 非零会话优先保留、零值按 `updatedAt` 倒序补位 |
| 记录大小上限 | `OMP_LIKE_MAX_RECORD_BYTES = 64 MB` / `LMSTUDIO_WINDOW_BYTES = 8 MB` | `src/lib/rollout.js:15712-15715`、`:12853` | 单行/窗口大小上限；**Codex 主路径不设单行上限**（`parseRolloutFile` 只传 `invalidUtf8`，`:2397`） |
| 上传退避 | 60 s 起、上限 30 分钟、指数 `2^step`、step 上限 10（记录时 20） | `src/lib/upload-throttle.js:8-9,109-110,123` | 失败退避；状态经 `upload.throttle.json` 暴露 |
| 云端归档包 | 单包上限 10000 条 / 8 MiB | `src/lib/cloud/usage-archive.js:35-39` | `cloud_prepare_usage_archive` 与 `cloud_commit_usage_archive` 的两步提交 |
| 后台轻量源集 | `BACKGROUND_AUTO_SYNC_SOURCES` = **4** 项 | `src/commands/sync.js:367` | `acode, codex, every-code, reasonix`；注释称「把无 scope 的 native 5 分钟同步限制在有日期的本地会话树上」 |
| 自动同步源集 | `AUTO_SYNC_SOURCES` = **45** 项 | `src/commands/sync.js:320-366` | 与 45 个 `parse*Incremental` 数量一致；别名表 8 条见 `:310-318` |
| 帮助文本自述 | `sync parses ~/.codex/sessions/**/rollout-*.jsonl …` | `src/cli.js:97-101` | 与实测覆盖范围（45 家）差距悬殊，以源码为准 |

---

## 12. 未验证项与风险清单

分类口径：**A**＝需运行时/真实语料才能判定；**B**＝需外部信息（上游 API、线上部署、真实壳行为）；**C**＝已知设计缺陷（有源码证据但当前版本未修）；**D**＝已结案（列出结论）。每条给判定条件。

### A. 需运行时或真实语料才能判定（18 条）

| # | 事项 | 判定条件 |
| --- | --- | --- |
| A1 | **Claude 增量游标无行边界提交**：`const endOffset = st.size;` 共 3 处（`src/lib/rollout.js:2291`、`:2682`、`:2793`），只有 `:2793` 落到 token 游标（`:1194`）。若同步瞬间读到半行，`readline` 会 yield 未闭合尾行、`JSON.parse` 失败被吞（`:2843-2847`），游标仍推进到文件尾，该行字节被跳过。非对称性：Codex 的 token 路径用 `committedEndOffset`（`:2652`），连项目上下文扫描位也做了提交保护（`:2765`），OpenClaw 另有 `openclawOffsetEndsAtLineBoundary`（`:1949`）——**只有 Claude 一条路径完全没有** | 构造「写入半行 JSONL → 跑 `parseClaudeIncremental` → 补全该行 → 再跑」的定向测试，看补全后的行是否被计入。**注意收窄**：该跳过会在该文件下次 inode 变化或截断时被全量重扫抵消（`:488-504` 的三态判定重置 `startOffset=0`），准确说法是「本代文件内不可恢复」而非严格永久 |
| A2 | **Claude 缓存字段别名未兼容**：`src/lib/rollout.js:4927-4928` 只读 `cache_creation_input_tokens`，而 `src/lib/codex-token-usage.js:25-27` 侧存在 `cache_write_input_tokens` 写法。anthropic 兼容端点（Kimi/DeepSeek/Mimo/MiniMax）若用别名，缓存写会被静默丢为 0 | 统计这些 provider 的合成 fixture 或真实样本里 `cache_write_input_tokens` 的出现率；若该字段非 0 而 `cache_creation_input_tokens` 缺失，即为确定性丢量 |
| A3 | **`<synthetic>` 模型行**：全库 3 处出现（`src/lib/session-analytics.js:101` 集合、`:621-624`、`:1968-1971` 注释），无显式 usage 处理 | 在本机 `~/.claude/projects/**` 里检索 `"model":"<synthetic>"` 并看同行 usage 是否非零；若非零，面板会出现名为 `<synthetic>` 的模型行 |
| A4 | **`legacyGroups`/`groupQueued` 分支对现有安装是否死代码**：`src/lib/rollout.js:3181-3194,3430-3480` 存在「整小时聚合成 `model=DEFAULT_MODEL`」路径，`src/commands/sync.js:216-220` 注释说 `groupQueued` 的存在本身就是 legacy 标记 | 构造含 `groupQueued` 的 v1 游标 fixture，跑 `enqueueTouchedBuckets` 观察输出行数与模型名；若不写该键则为死代码，若会写则存在与修复意图冲突的再膨胀路径 |
| A5 | **同批内同键双行的覆盖方向**：`readQueueBatch`（`src/commands/sync.js:4422-4512`）在同一批内用 `bucketMap.set(...)` 覆盖，取决于文件顺序 | 构造同键双行（旧在前、新在后）的队列 fixture 直接跑 `readQueueBatch`，确认是否以旧覆盖新 |
| A6 | **压缩与上传 offset 的交互边界**：`compactPendingQueue` 只重写 `[offset, size)` 并原样复制 `[0, offset)`（`src/lib/upload-queue.js:97-110`） | 测「pending 区出现跨行边界残缺（写者正在写）时 `iterateQueueLines` 的行为」与「压缩后 offset 恰落在文件末尾」的边界（`input.close()` 与 `fs.stat` 校验之间存在窗口） |
| A7 | **`multiInstallParse` 异常分支的写回语义** | 读 `src/lib/multi-install-parser.js:49-54` 并构造异常路径 fixture |
| A8 | **`claudeHashes` 10 万上限的淘汰窗口**：`src/lib/rollout.js:1236-1237` 超限保留最后 10 万；`fileId` 第二层去重只在「同一次运行内同时看到 `\\wsl$` 与本地路径」时启用（`sawUncFile && sawLocalFile`，`:1035`） | 构造 >100k 哈希 + 同内容双路径的用例，验证是否双计 |
| A9 | **`rebuildingCodexBaseline` 与 `prev.lastTotal` 兜底**（`src/lib/rollout.js:459`） | 读 `:459` 附近上下文与 `rebuildingCodexBaseline` 路径，确认兜底动机 |
| A10 | **Copilot 三通道互斥完备性**：三份实现共用 `source = "copilot"`，靠 `usageClaims`（OTEL↔store）、`shouldCountCopilotAppSession`（app-db 跳过 CLI/VS Code）、`usageClaimsComplete` 三重机制分工 | 给出 `test/copilot-app-parser.test.js` 与 `test/copilot-session-store-parser.test.js` 的**交叉通道用例断言清单**（含「同时活跃」场景的输入与期望输出）；缺该清单则维持未确认 |
| A11 | **`qoder(new)` 全零 token 行是否真的入队**：`src/lib/rollout.js:6639-6660` 构造 `conversation_count = 1`（token 全 0）的 totals 并 `addTotals` 到桶，是否入队取决于 `totalsKey` 比较（`:3325-3347`） | 以 fixture 复现「billable 零 token 行」跑完整 `parseQoderNewIncremental` + 入队，检查队列行是否出现全零 `total_tokens` 且 `conversation_count = 1` |
| A12 | **`trae-cn` 真实响应是否出现全零会话行**：源码层「允许」已确认（`src/lib/rollout.js:22477-22492` 只校验 `Number.isSafeInteger(value) && value >= 0`，`:22500-22508` 写全零 totals，`:22749-22751` 无零值护栏直接 `addTotals`） | 取得一次真实 `query_user_usage_group_by_session` 响应样本（含全零 token 字段的会话行），或用同形 fixture 跑完整 `parseTraeCnApiIncremental` 并检查桶行 |
| A13 | **`reasonix` 跨 home 重复计数**：`reasonixHomeCandidates` 同时返回 `~/.reasonix` 与 `%APPDATA%\reasonix`（`src/lib/rollout.js:16925-16934`），`resolveReasonixTelemetryFiles` 只去重**路径**（`:16970-16975`），**没有按 inode/内容去重**（对比 Copilot 的 `uniqueCopilotDbPaths` 用 `dev:ino`） | Windows 上实测双目录并存且互为拷贝时的表现 |
| A14 | **`trae` 的 `T_STORE` 冲突裁决在三份以上拷贝时**：冲突规则只处理「两份拷贝」（`src/lib/rollout.js:22111`：`previous[T_STORE] !== store && modifiedAt <= previous[T_MODIFIED]`） | 三份以上拷贝轮流被扫描时的最终归属收敛性；注释只声明「不能让陈旧库回撤更新的用量」，未给收敛性证明 |
| A15 | **`grok` 兜底路径的时间归属**：桶起点为 `toUtcHalfHourStart(event.timestamp) \|\| toUtcHalfHourStart(lastActive) \|\| toUtcHalfHourStart(Date.now())`（`src/lib/rollout.js:20249-20252`、`:20265`）；`lastActive` 也缺失时回落 `new Date().toISOString()`（`:19691`），**历史用量会被记进「当前」半小时桶** | 统计该组合在真实老会话上的触发频率 |
| A16 | **`craft` 与 `reasonix` 在极端截断下 `total_tokens` 失真**：两家都用「累计快照 clamp 差值」，Craft 注释承认负面差值是截断信号并 clamp 到 0（`src/lib/rollout.js:17189-17195`），Reasonix 同样 clamp（`:17011`）。若上游重置后在短时间内又累积回原量级，该段「重放」差值会被再次计入；两家都没有 Qoder 那样的「先减旧再加新」回撤路径 | 实测截断/重置场景 |
| A17 | **`mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal` 而非「最后一条」**（`src/commands/sync.js:6540`），与 `readQueueData` 的 last-wins（`src/lib/local-api.js:207`）语义不同 | 确认该函数的所有调用点是否都作用于「已被重写过的干净队列」（真值修复是原子重写，理论上不会出现同键数值回退） |
| A18 | **`model/rerouted` 归属路径从未在真实语料验证**：`src/lib/codex-model-attribution.js:47-51` 自述 `0` 命中；该分支一旦被真实触发，其信封形状与字段名（`fromModel/toModel/from_model/to_model`）都属推断 | 检索本机 rollout 中是否出现 `model/rerouted` / `model_rerouted` |

补充登记（同属 A 类）：`passive-mode.js` 只覆盖 8 个 hook 驱动 provider（`src/lib/passive-mode.js:96-185`），Grok / OpenCode / OpenClaw / omp 的「hook 缺失但日志在」状态由 `src/commands/status.js:1300-1336` 的独立路径上报，**未逐一核对**；CodeBuddy / WorkBuddy 是否真的消费 `settings.json` 的 `SessionEnd` 只有源码内「fork 同 schema」注释断言（`src/commands/init.js:623-625,1034-1036,1048-1049`），无 schema 校验、无第三方验证；`usage_precision` 的取值域与全部消费方未追踪完整（写入点 16 行 / 14 个语句已在 `src/lib/rollout.js` 内清点，「无 antigravity 写入点」这一否定性结论依赖该清单完备）；`codexServiceTier` 的语料统计（1213 文件 / 167 命中 / 25%；814 default / 58 priority）是**上游源码注释里的本机观测**而非本轮实测（`src/lib/codex-service-tier.js:8-15`）。

### B. 需外部信息才能判定（10 条）

| # | 事项 | 判定条件 |
| --- | --- | --- |
| B1 | **云端 ingest 是否会从 `cursors.files[].usageSource` 反推估算行**。「`antigravity` 估算行对下游不可见」这一结论建立在「`usage_precision` 全部写入点清单里没有 antigravity」的**否定性证据**上 | 取得云端 ingest 侧读取 `cursors.files[].usageSource` 的代码或运行证据——能反推则该披露缺口不成立，不能则维持现状 |
| B2 | **edge 五份价格表的线上部署修订**。仓库内无部署产物或版本指针，仅 `CLAUDE.md:42` 提示人工部署（"Deploy the touched edge functions after editing"） | 取线上函数代码或版本指针，与 `dashboard/edge-patches/` 逐份比对 |
| B3 | **`curated-overrides.json` 各 `note` 引用的第三方定价页与表内数字是否一致** | 该表运行在虚构时间线（`claude-opus-5-5`、`gpt-6-astra`、`deepseek-v4` 等），**不存在可核对的外部真实价页**，联网核对无事实源——只能核对表内自洽性 |
| B4 | **LiteLLM 上游当前是否仍以 `model_prices_and_context_window.json` 为唯一价目文件、字段名是否仍为那 4 个成本字段** | 联网核对上游仓库（本轮未做） |
| B5 | **Windows 托盘在本地模式下连续天数是否显示 0**。两侧源码已确认：本地端点硬编码 `streak_days: 0`（`src/lib/local-api.js:2588`，同响应 `:2587` 另给真实 `active_days`），前端 `dashboard/src/hooks/use-activity-heatmap.ts:206,220,284,295` 自行重算覆盖；`dashboard/src/lib/pet-linux-host.js:198` 直读该字段 | 运行 Windows 壳实测显示值 |
| B6 | **`usage-limits` 的 provider 数量口径**：`fetchUsageLimitsUncached` 的 `Promise.all` 解构出 18 个结果变量（`src/lib/usage-limits.js:4055`），但 `data` 对象只收录 17 家（`claudeServiceStatus` 未进入 `data`，`:4406-4440`） | 核对 `ark` 两家与 `qoder` 两家是否共用同一凭据 |
| B7 | **Windows 壳「5 分钟后台 sync 计时器」**：只在 `src/commands/serve.js:305-309` 注释中被提及，**未在壳代码中核实** | 读 `TokenTrackerWin/` 目录源码 |
| B8 | **本地 API 若干端点的字段结构**：`/functions/tokentracker-local-sync` 成功响应的 `syncResult` 键名、`/functions/tokentracker-context-health` 的字段结构、`/api/auth/*` 的完整转发规则（cookie relay、csrf 优先级、relay 回退条件，只读到 `src/lib/local-api.js:1792-1940` 片段） | 逐端点读对应 `if (p === ...)` 分支的参数与响应载荷 |
| B9 | **榜单/社区表的 SQL 细节**（`scripts/ops/*.sql`、`migrations/`） | 只通过测试注释间接确认，需逐行读 SQL 正文 |
| B10 | **云端压缩表的保留期限**：`docs/PRIVACY.md` 与 `docs/cloud-guide.md` 提到「compressed usage metrics 可长期保留」，但 `docs/cloud-usage-archive.md:74-83` 明确写「未启用」 | 取云端归档表的实际启用状态与保留策略 |

补充登记（同属 B 类）：macOS 菜单栏小组件的刷新频率（`WidgetCenter.reloadAllTimelines` 之后的系统策略）与「快照 schemaVersion 2 的兼容迁移」细节；`TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源（推测同为本地 API 或经 `UsagePoller` 推送，**未验证**）。

### C. 已知设计缺陷（有源码证据，当前版本未修）（24 条）

1. **`cmdSync` 巨型编排**：3190 行单函数、`skipNoOpCursorCommit` 引用 13 个条件项（`src/commands/sync.js:516-3709`、`:3424-3437`）。
2. **`.reclaim` 闸链需迭代清扫才能自愈**（`src/lib/fs.js:178-204`，issue #431）。
3. **队列写入无写侧互斥**：主队列 append 写点至少 6 处（`src/lib/rollout.js:3483`、`:20031`、`:22594`；`src/commands/sync.js:5126`、`:5207`、`:5281`），另 `src/commands/sync.js:4114` 写 fallback、`src/lib/rollout.js:3536` 写 project 队列。
4. **`repair` 的「先删后建」根因未变**：0.26.3 真实数据丢失事故（`src/commands/sync.js:234-248`，2.174B ≈ 21.7 亿 token），已用守卫弥补但模式仍在；`dropMimoQueueRows`（`:4744`）与 `repairClaudeQueueFromGroundTruth`（`:6844`）属同类。
5. **Grok 分支无条件覆盖、无备份**（`src/lib/grok-hook.js:85-91`）。
6. **同一语义三套并行纠错机制**（一次性 repair 键 / 读时 `normalizeQueueRow` / 云端整行 upsert），口径需人工同步；历史事故：云端 `conversation_count` 恒为 0 而本地正确（`src/commands/sync.js:249-256`）。
7. **16 MB 体积阈值静默切换存储形态**，且 legacy 与 v2 能力不等价（`src/lib/cursor-store.js:21,77-86,165-197`）。
8. **代理配置错误让整个进程 `exit(1)`**（`bin/tracker.js:36-40`），hook 路径同样受影响 → 采集静默停摆且无面向用户告警。
9. **`local-api.js` 3543 行职责混杂**（读队列 / 项目归因 / 账号体系 / 头像代理 / 宠物 / 静态资源 / SPA 回退）。
10. **测试专用钩子进入生产函数签名**（`context.traeCnFetchImpl`、`context.cursorSyncDeps`、`context.scanSources`，`src/commands/sync.js:519-538`）。
11. **依赖 `st.ino` 检测原子替换**，在网络盘/容器叠加文件系统上会失效（`src/lib/rollout.js:489-492`）。
12. **Windows 7680 端口占用靠「WSL 默认换 7681」绕行**（`src/commands/serve.js:15-21`）。
13. **`hookStatus` 传参与消费不一致**：`opencode_plugin`/`openclaw_session_plugin` 不在 passive-mode 的 provider 列表内（冗余传参）；`workbuddy-ai` 键名风格与其它下划线键不一致（`src/commands/status.js:1024-1036` vs `src/lib/passive-mode.js:96-185`）。
14. **`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 双份定义且无测试绑定**（`src/lib/pricing/index.js:31` 零内部读取；生效者 `src/lib/codex-rollout-parser.js:51`）。
15. **推理折叠名单两套**：计价侧 5 个（`src/lib/pricing/index.js:248-253`），token 审计侧 2 个（`scripts/audit-token-correctness.cjs:34-40`），无测试把两者绑在一起。
16. **云端与本地对 9 个 `curated.exact` 键的覆盖不同**，其中 `grok-build-free`、`grok-4.5-build-free` 是价格层面的实际分歧（`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 比对）。
17. **`PI_SUBSCRIPTION_SOURCES` 本地 4 个成员、云端 2 个成员**（`src/lib/pricing/index.js:18-23` vs `tokentracker-leaderboard-refresh.ts:575`），`prime-agent-*` 两条在云端不会短路为零。
18. **云端 `computeRowCost` 没有长上下文 / Fast 溢价**（`tokentracker-leaderboard-refresh.ts:569-613` 无 `long_context_*` / `priority_*` 分支），且无测试断言「云端结果必然 ≤ 本地结果」。
19. **金额精度三处不同**：`toFixed(6)`（`src/lib/local-api.js:2415`、`:2648`）、浮点原值（`src/lib/session-analytics.js:289`）、取整到分（`tokentracker-leaderboard-refresh.ts:1349`）。
20. **半小时桶两份实现无等价性锁定**（`src/lib/rollout.js:4173-4190` vs `src/lib/claude-categorizer.js:1175-1191`，副本无测试）。
21. **Codex「reasoning 不双计」无对等回归测试**：`source` 白名单漏加会静默双计（`src/lib/pricing/index.js:248-256`）。
22. **同一面板两套时间口径**：热力图网格按 UTC 日（`src/lib/local-api.js:2540`），`night_owl` 徽章按请求时区小时（`:482`）；另有 `hourly` 端点 `day` 默认值（UTC 今天）与其过滤谓词（当地日键）**基准不同**。
23. **`pricing.source` 元数据在多端点是硬编码标签而非真实来源**；`streak_days` 恒 0、`week_starts_on` 恒 `"sun"`（`src/lib/local-api.js:2586,2588`），而云端 compact 载荷会带真实值（`src/lib/heatmap-compact.js:108-109` 透传）——同一 UI 在本地/云两条路径下显示可能不一致。
24. **Claude 真值修复与增量解析的口径不一致**（至少三处：usage 取值路径 `obj.message.usage \|\| obj.usage` vs 仅 `obj.message.usage`；reasoning 字段名 `output_tokens_details.thinking_tokens` vs `usage.reasoning_output_tokens`；output 是否拆分），且修复是**一次性且不可逆**（重写 `queue.jsonl` + `cursors.claudeHashes` 全替换，`src/commands/sync.js:6871-6921`）。

补充登记：项目队列丢失模型信息——项目行的 `model` 列被裁剪（`src/lib/rollout.js:3515-3532`），而项目详情端点的 `models` 字段实际装的是 **source**（`src/lib/local-api.js:2877`），下游若按「按模型」理解会出错；`projectBucketKey` 布局 `projectKey|source|hourStart` 与 `parseBucketKey` 的 `source|model|hourStart` 不通用，两套键共用一个分隔符而解析器不匹配（`src/lib/rollout.js:4228` vs `:4238`）；Claude 项目身份的 `.git` 后缀不剥离（`src/lib/rollout.js:4429-4442`）；`src/lib/rollout.js:6726` 悬空注释（称 credits 会累积，但下一行即 `touchedBuckets.add`，上下文无实现）。

### D. 已结案（13 条）

| # | 结论 | 依据 |
| --- | --- | --- |
| D1 | `resetWslProbeCache` **生产调用为 0**（只有定义与导出），「缓存刷新时机不对」不构成缺陷；长驻进程内新增发行版不会被发现这一现象依然存在，属新功能而非修复 | `src/lib/wsl-probe.js:11`、`:268`；唯一调用者是 `test/*.test.js` |
| D2 | `init --link-code` 是**遗留参数**：init.js 内只用它关闭浏览器认证（`src/commands/init.js:1423,339`），`link_code_state.json` 在 init.js 内无任何读写（`:176,234,359` 仅为声明与传参），全仓零实际引用 → 帮助文本对该开关的暗示存在误导 | `src/lib/runtime-config.js:149`（清理清单）；`src/cli.js:78,92` |
| D3 | KiroCli 会话文件侧字段**不是**模块注释所写的 `user_prompt_length`/`response_size`，而是 `turn.input_token_count` / `turn.output_token_count`；仅两者皆 0 才回退 4 字符/token 近似；v2 支路另读 `messages.jsonl` 且**不含 token 字段** | `src/lib/rollout.js:8371-8372`、`:8316`、`:8482`、`:8475-8481` |
| D4 | Opencode storage 下层结构：`walkOpencodeMessages` 递归遍历且只收 `msg_*.json`；`listOpencodeMessageFiles` 只负责拼 `<storage>/message` 一层 | `src/lib/rollout.js:5025`、`:5033`、`:243` |
| D5 | Anythingllm 的 SELECT 本体为 `FROM workspace_chats`，列经 `json_extract(response,'$.metrics.*')`，SQL 层只过滤 `whereClause`、**不做零值过滤**（纯 total 行在 JS 侧被丢弃，与源码自洽） | `src/lib/rollout.js:13874-13887` |
| D6 | Cline 的目录解析与扫描都在 `src/lib/rollout.js`（**不在** sync.js）：`resolveClineSessionsDirs`（`:12054`，Windows 双安装 + WSL 探测 `:12069-12073`）与 `scanClineSessionFiles`（`:12086`） | 同上；`~/.cline/data/sessions` 与 `src/commands/sync.js:2110` 注释一致 |
| D7 | Kimi / KimiCode 的 home 优先级已逐字确认 | `src/lib/rollout.js:9493`（`KIMI_HOME` `:9495`，win32 WSL 分支 `:9497-9502`）、`:9669`（`KIMI_CODE_HOME` `:9671`） |
| D8 | Codebuddy 的 JSONL 支路闸门：现代 CodeBuddy 把用量写进 `providerData.rawUsage`；该支路零值判定为含 `reasoningTokens` 的五桶闸门 → 归「D 类」，F 类清空 | `src/lib/rollout.js:9940-9943`、`:10476-10485` |
| D9 | Workbuddy traces 支路字段**与 JSONL 支路不同形**：traces 用 `modelInfo.total{Input,Output,Cached}Tokens`，JSONL 用 `providerData.rawUsage.*`；`src/lib/rollout.js:11246` 属**第三条 SQLite 支路** | `src/lib/rollout.js:11100-11260` 段 |
| D10 | vite dev **不复制价格表**：`dashboard/vite.config.js:508-514` 用 `createRequire` 直接 require 同一 `src/lib/pricing` 模块；唯一差异是 dev mock 对按模型聚合后的 totals 只计一次价（`:1052-1057`），仅影响 dev | 同上 |
| D11 | 上游价格表键数：edge `MODEL_PRICING` = **119** 键（+ iFlytek MaaS 64 键）；`curated-overrides.json` 与云端差集 = **9 键**（逐字一致） | `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142-332`、`:339-404`；`test/edge-pricing-parity.test.js:76-85` |
| D12 | 三份 UTC 半小时桶实现**语义等价**（`rollout` 权威、`claude-categorizer` 副本、sync 的 grok 毫秒版），此前自标的「未确认」已解除 | `src/lib/rollout.js:4173-4190`、`src/lib/claude-categorizer.js:1175-1191`、`src/commands/sync.js:4538-4549` |
| D13 | 关键行数断言经逐字重算：`rollout.js` **24932**、`sync.js` **7251**、`init.js` **2427**、`local-api.js` **3543**、`session-analytics.js` **2934**、`status.js` **1729**、`pricing/index.js` **378**、`pricing/matcher.js` **439**、`curated-overrides.json` **775**、`upload-queue.js` **134** | `[System.IO.File]::ReadAllLines(...,UTF8)` 逐个重取 |

---

## 13. 复核记录（R1 / R2）

本研究不是单轮产物，而是**研究 → 对抗式复核 → 修正 → 再复核**的四段闭环。以下台账数字取自各复核报告自报的「抽样统计」节；每轮样本仓库 HEAD 全程未变（`b19d27b0ccd59ce621f4fa17207b06358e67987c`），复核过程**未修改样本仓库**。

### 13.1 第一轮复核（R1，6 个独立复核 agent）

| 轮次 | 对象 | 抽验条数 | E | I | U | 通过 | 关键发现 | 处置结果 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R1-A | 采集管线（触发/调度/游标/队列） | 139 | 9 | 9 | 10 | 111 | 队列写点**不是一处**（实为 6+ 处）；`skipNoOpCursorCommit` 条件项是 13 个不是 9 个；`cloneOrCopy` 是 reflink 不是硬链接 | 全部采纳并回改 |
| R1-B | Claude Code 与 Codex 深度 | 46 | 1 | 8 | 4 | 33 | 「45 个 parser 存在三族签名」实为**无统一契约**（45 声明中 5 个转发薄壳、40 个自持 return、去重后 **11 种键集合**）；Claude `endOffset` 三处分工需分清 | 采纳；同时对复核报告**反向纠正 4 处**（见 13.3） |
| R1-C | 19 家 JSONL 类 provider | 141 | 12 | 7 | 2 | 120 | KiroCli「全部靠字符估算」被**证伪**（真实 token 字段存在）；行号清单 40 条中 5 条错误，**全部落在 ≥7000 行且偏移量与 GBK 口径吻合** | 采纳；错误全部改正，并写明 GBK 漂移机制 |
| R1-D | SQLite/API 类 provider | 188（代码块行号 62 条逐字比对） | 9 | 6 | 3 | 170 | 「21 家」与「23 个 `parse*` 函数名」的口径关系需定义；「唯一一处全零 token 行」实为**两处**（qoder new + trae-cn），craft 反而应作反例 | 采纳；计数口径写入结论速览 |
| R1-E | 聚合、查询与去重 | 157 | 8 | 6 | 7 | 136 | `conversation_count` 是**主会话 user 文本消息**而非 assistant 消息（源码注释与 Craft 侧注释互相冲突，以代码为准）；`toUtcHalfHourStart` 调用数需分口径（`rollout.js` 70 次出现 / 66 行） | 采纳；同时确证两套口径（last-wins 与 setMaxTotal）**并存不矛盾** |
| R1-F | 定价与消费端 | 101 | 11 | 7 | 3 | 80 | `MODEL_PRICING` 键数 183 → **119**；edge-only 44 键 / curated-only 9 键；`savings` 类词命中从「1 处」改 **9 处 / 7 文件** | 采纳；行号与键数全部重算 |
| **合计** | — | **772** | **50** | **43** | **29** | **650** | — | E/I 共 93 条全部采纳或按修正后版本落实；U 组 29 条保留并补判定条件 |

R1 还产生若干**结论级推翻**（不只是行号微调）：除上表所列，另有「三份 UTC 桶实现语义等价」「Codex 连项目上下文扫描都做了提交位保护而 Claude 没有」「reasoning 在两侧的处理方向相反（Codex 是子集标记并置 0，Claude 是独立加成项并照 output 价）」。这些都已按修正后版本写入正文。

### 13.2 第二轮独立复核（R2，3 个复核 agent）

| 轮次 | 对象 | 工作量 | 关键发现 | 处置结果 |
| --- | --- | --- | --- | --- |
| R2-1 | 管线类文档的修正点复查 + 独立抽样 + 跨文档一致性 | A 组复查 40 条：**35 ✅ / 4 ⚠️ / 1 ❌**；B 组独立抽样 40 条：**未发现新的实质性错误**（含 8 条「文档未覆盖但源码如此」的补白）；C 组 13 处比对：**未发现互相矛盾的断言**（2 处行号范围写法差异属可接受近似） | 必须修 3 条，均为**表述级**（不影响机制结论）：一句会被自身文件证伪的元断言、「项目行其余同构」与源码不符（缺两列）、opencode 游标字段清单漏 1 个字段。**R1 点名的三类高风险项（notify.cjs 模板边界、队列「唯一写点」、OpenCode 哨兵范式）经独立回源码复核均已改对。** 结论：**没有发现会影响移植决策的机制性错误** | 3 条表述级修正全部落实 |
| R2-2 | provider 类文档的修正点复查与继续证伪 | A 组逐条复查；在修正记录之外又查出 **6 处残留漂移行号** | 残留漂移的偏移量与文档自述的漂移表**精确吻合**（≈10700 行处 +6、≈13700 行处 +10、≈15300 行处 +10），说明「只查清单内 40 条」的自查会过滤掉散落在正文里的真错项；另：R1 遗留的一处未验证项（Workbuddy traces 字段）本轮**结案**（见 D9） | 6 处全部改正；U 组减 1 条转 D 类 |
| R2-3 | 聚合/定价/消费端的修正点复查 + 新抽样 + 端到端链条 | A 组复查 S4 段 18 条 + S5 段 17 条；C 组端到端 10 处 | 端到端链条**未发现金额重复计入或漏计**的实证；新发现两处基准不一致：`hourly` 端点的 `day` 默认值（UTC 今天）与其过滤谓词（当地日键）基准不同、多端点 `pricing.source` 是硬编码标签而非真实来源 | 已登记进 C 类（见 §12 C22、C23） |

R2 的净收益是**两类**：把 R1 修正后的文本再筛一遍（抓出 6 处残留漂移 + 3 条表述级问题），并对 R1 较少触及的章节做全新抽样与补白。R2 明确否定了「修正记录写完就等于干净」的假设。

### 13.3 被反向纠正的复核结论（可信度证据）

复核报告本身也被证伪过——**这是本文可信度最高的部分**，因为它说明修正环节不是单向服从：

1. **`recordsProcessed: total` 被误读为新增键**。复核清单曾把 `parseCursorApiIncremental`、`parseTraeCnApiIncremental` 的返回字面量判为「多一个键 `total`」，把 `parseGrokBuildIncremental` 判为「键重复」。实测三处都是**值标识符被当成键名**：真实写法是 `recordsProcessed: total`（`src/lib/rollout.js:7270`、`:22791`）与 `recordsProcessed: eventsAggregated`（`:20551`）。它们的键集合分别是骨架 C、骨架 C+`skippedRows`、骨架 D，**没有多出的键也没有重复键**。
2. **「12 种返回字面量」实为 11 种**。复核漏掉了 `parseCopilotSessionStoreIncremental` 的 9 键集合（`src/lib/rollout.js:19003-19013`），同时把上述 3 个值标识符计成了键。
3. **`rollout.js` 行数：23260 → 24932**。复核报告自身也测到 24932，却判定「数据一致」，属**漏检**——这一条直接决定了全文行号口径的权威性（本文采用 UTF-8 口径的 24932）。
4. **「误引 `:2682`」不成立**。复核称某处误引了 `:2682`，实测该文档全文检索 `2682` **零命中**，误引不存在；同时复核把 `:2291` 归为「项目上下文扫描」，实测它属于 `parseRolloutFile` 的「无新字节」早退。最终只补了 3 处分工表，未删任何引用。
5. **macOS 壳 heatmap「不传 tz」的暴露面判定被反驳**。R2 曾断言「macOS 壳的 heatmap 调用不带 `tz`（`APIClient.swift:73-75` 只构造 `weeks` 参数）→ 是实际触发面」。实测 `:73-75` 只是 `queryItems` 数组字面量，整数组被传给 `withAccountQueryItems`，而它是在 `withTimeZoneQueryItems` 之上再追加 `account=1`（`APIClient.swift:241-246`、`:252-254`）——**macOS 壳的 heatmap 实际带 tz**，该结论不成立，予以反驳。
6. **Droid 行号被二次推翻**。R1 曾指出引 `15333/15338` 有误并给出「修正」；R2 逐行复核后判定修正本身仍错——实际注释在 `15343-15346`、调用点在 `15348`，`15333` 是对象字面量闭合 `};`、`15338` 是 `const bucketStart = toUtcHalfHourStart(`，两处恰好为 **+10 漂移**。
7. **「仅由 PowerShell 定位的 40 条自查清单」本身是错的**。该清单把 35 条**正确**的行号标为「未复核」，却漏掉真正的错项；更严重的是其中一条自称「已 read 确认」的引用实际是 GBK 口径附近位。教训已写入方法学：**「自称已确认」不能替代逐条重验**，自查清单本身也会骗人。

### 13.4 行号口径的硬结论
- 本仓库有多行以多字节字符结尾；PowerShell 的 `Get-Content`（不指定编码）在中文环境按 GBK 解码，会把行尾字节与随后的换行一起吞掉，导致**行号整体偏移，本仓库最多 15 行**，且**漂移量随位置累积**（≈10700 行处 +6、≈13700 行处 +10、≈15300 行处 +10、文件末尾 +15）。
- 权威口径只有两种：`read` 工具，或 `[System.IO.File]::ReadAllLines($path, [System.Text.Encoding]::UTF8)`。本文所有行号已按该口径校验；复核过程中发现的历史错误**全部落在 ≥7000 行**，且偏移量与上述漂移表吻合。

---

## 附录 A：45 个解析器行号索引

生成方式：以 UTF-8 口径 `[System.IO.File]::ReadAllLines($p, [System.Text.Encoding]::UTF8)` 读取 `src/lib/rollout.js`，用正则 `^(async\s+)?function\s+\w+` 收集全部**顶层**函数定义行（实测 572 个），筛出函数名匹配 `parse\w*Incremental` 者（实测 **45** 个），区间以「下一个顶层函数定义的上一行」截断。该数量与 README 自述的「45 AI coding tools」一致。

| # | 函数名 | 起始行 | 覆盖行数 | `sync.js` 调用点 |
| --- | --- | --- | --- | --- |
| 1 | `parseRolloutIncremental` | 251 | 457 | `src/commands/sync.js:1046`（经 `runCodexParse`）、`:5769` |
| 2 | `parseClaudeIncremental` | 954 | 288 | `:1201` |
| 3 | `parseGeminiIncremental` | 1242 | 170 | `:1258` |
| 4 | `parseOpencodeIncremental` | 1412 | 170 | `:1361` |
| 5 | `parseOpenclawIncremental` | 1654 | 271 | `:1105`、`:4117` |
| 6 | `parseOpencodeDbIncremental` | 5761 | 291 | `:1375`、`:1665` |
| 7 | `parseQoderDbIncremental` | 6218 | 198 | `:1471`、`:1549` |
| 8 | `parseQoderNewIncremental` | 6552 | 300 | `:1509`、`:1599` |
| 9 | `parseClaudeScienceIncremental` | 7051 | 90 | `:1642` |
| 10 | `parseCursorApiIncremental` | 7172 | 108 | `:2173` |
| 11 | `parseKiroIncremental` | 7472 | 132 | `kiroResult` 段，source 开关 `:2307` |
| 12 | `parseHermesIncremental` | 7900 | 180 | `:2358` |
| 13 | `parseKiroCliIncremental` | 8886 | 393 | source 开关 `:2429` |
| 14 | `parseKimiIncremental` | 9528 | 141 | `:2516` |
| 15 | `parseKimiCodeIncremental` | 9723 | 187 | `:2544` |
| 16 | `parseCodebuddyIncremental` | 10164 | 492 | `:2594`、`:5409` |
| 17 | `parseWorkbuddyIncremental` | 10803 | 549 | `:2639`、`:2674`、`:5579` |
| 18 | `parseRoocodeIncremental` | 11824 | 200 | `:2091` |
| 19 | `parseClineIncremental` | 12200 | 290 | `:2124` |
| 20 | `parseZedIncremental` | 12650 | 209 | `:2074` |
| 21 | `parseLmstudioIncremental` | 13267 | 215 | `:1879` |
| 22 | `parseUnslothIncremental` | 13691 | 117 | `:1897` |
| 23 | `parseAnythingllmIncremental` | 13938 | 168 | `:1916` |
| 24 | `parseDevinIncremental` | 14207 | 282 | `:1935` |
| 25 | `parseGooseIncremental` | 14604 | 193 | `:2005` |
| 26 | `parseDroidIncremental` | 15137 | 408 | `:2024`、`:6698` |
| 27 | `parseKilocodeIncremental` | 15545 | 142 | `:1960` |
| 28 | `parseOmpLikeIncremental` | 15753 | 334 | （共享实现，被 29 转发） |
| 29 | `parseOmpIncremental` | 16087 | 13 | `:2708` |
| 30 | `parseOmoIncremental` | 16100 | 29 | `:2747` |
| 31 | `parsePiLikeIncremental` | 16319 | 370 | （共享实现，被 32 转发） |
| 32 | `parsePiIncremental` | 16689 | 10 | `:2780` |
| 33 | `parsePrimeAgentIncremental` | 16699 | 28 | `:2809` |
| 34 | `parseMinimaxCodeIncremental` | 16775 | 55 | `:2837` |
| 35 | `parseReasonixIncremental` | 17050 | 30 | `:2890` |
| 36 | `parseCraftIncremental` | 17080 | 229 | `:2865` |
| 37 | `parseCopilotIncremental` | 17878 | 253 | `:3108` |
| 38 | `parseCopilotSessionStoreIncremental` | 18579 | 456 | `:3053`、`:3282` |
| 39 | `parseCopilotAppDbIncremental` | 19381 | 172 | `:3216` |
| 40 | `parseGrokBuildIncremental` | 20084 | 475 | `:2960` |
| 41 | `parseAntigravityIncremental` | 20800 | 597 | `:1315` |
| 42 | `parseTraeIncremental` | 22033 | 148 | `:2200` |
| 43 | `parseTraeCnApiIncremental` | 22598 | 228 | `:2280` |
| 44 | `parseDshIncremental` | 23678 | 396 | `:1819` |
| 45 | `parseCommandCodeIncremental` | 24458 | 475 | `:1855` |

**三点说明**：
- **薄壳与共享实现**。第 28、31 两项是**共享实现**（`OmpLike`、`PiLike`），第 29、32 两项是**转发薄壳**（分别 13 行、10 行），另有 30、33、35 三项同样极短（29、28、30 行）。这与「45 个声明中 5 个转发薄壳、40 个自持 return」的统计相符。**任何按函数名反查解析逻辑的代码都需先过这张转发关系**。
- **`parseDumateIncremental` 不在本表**。它出现在 `src/commands/sync.js:1701`（`sourceAllowed("dumate")` 在 `:1696`），但**定义不在 `src/lib/rollout.js`**——本索引以该文件为范围，故 45 项不含它。
- **`source` 字面量不能由函数名推断**。`sync.js` 侧的 `sourceAllowed(<字面量>)` 实测有 **46** 个不同实参（比 45 个解析器多）：`acode, antigravity, anythingllm, claude, claude-science, cline, codebuddy, codex, command-code, copilot, craft, cursor, devin, droid, dumate, dsh, every-code, gemini, goose, grok, hermes, kilo-cli, kilocode, kimi, kimi-code, kiro, lmstudio, mimo, minimax-code, omo, omp, openclaw, opencode, pi, prime-agent, qoder, qoder-cn, reasonix, roocode, trae, trae-cn, unsloth, workbuddy, workbuddy-ai, zcode, zed`。差集来自**多解析器共享一个 source**（`copilot` 三通道 = 三个解析器、`kiro` 两套 = 两个解析器）与**使用通用 DB 解析器但独立 source**（`dumate`/`mimo`/`zcode`/`kilo-cli`）。因此「按 source 反查解析器」必须查 `sync.js` 的调用点，**不能反推**。

自检结果（UTF-8 口径）：`parseRolloutIncremental` = **251** ✅、`parseClaudeIncremental` = **954** ✅、`parseCommandCodeIncremental` = **24458**（文件总行数 24932）。作为对照，同一文件用 GBK 口径读取时总行数为 **24917**（少 15 行），同一函数落在 **24443**——即任何给出 24443 的引用都应被视为 GBK 口径产物，不可采信。

## 附录 B：术语表

| 术语 | 定义 | 出处 |
| --- | --- | --- |
| **队列（queue）** | `~/.tokentracker/tracker/queue.jsonl`，append-only 的**累计快照日志**：每 `(source, model, 半小时)` 一行，不是事件流。同键后写覆盖前写（last-wins） | `src/commands/sync.js:569`；`src/lib/rollout.js:3482-3484` |
| **项目队列** | `project.queue.jsonl`，项目维度的本地状态，只有本地视图读它；**不上传**，因此其积压不计入 pending | `src/commands/sync.js:571`、`:3654-3659` |
| **桶（bucket）** | 一个 `(source, model, hour_start)` 组合的累计计数器，键形如 `source\|model\|hourStart`；`hour_start` 恒为 UTC 半小时 | `src/lib/rollout.js:4221-4245` |
| **项目桶** | `projectKey\|source\|hourStart`，无 model 维度；字段布局与主桶**不通用** | `src/lib/rollout.js:4228` vs `:4238` |
| **游标（cursor）** | 记录「每个文件读到第几字节」及各类去重集合的持久状态；v2 形态下按代际 + 分片存储 | `src/lib/cursor-store.js:13-23`；`src/lib/rollout.js:1194` |
| **代际存储（generation store）** | `cursor-store-v2/generations/gen-<ms>-<uuid8>/`，每个代目录自洽；`manifest.json` 是唯一的原子切换点，保留 `current/rollback/previous` 三代 | `src/lib/cursor-store.js:855-1016` |
| **retraction（撤回）** | 当 `unknown` 桶被对齐到真实模型时，先追加一条**全零行**抵消旧归属，避免重复计入；零行必须被解释为撤回而不是「零用量」 | `src/lib/rollout.js:3251-3317`、`:3351-3402` |
| **证件行 / billable** | `billable_total_tokens` 字段：用于对外计费的 token 总量，缺省回落 `total_tokens` | `src/lib/rollout.js:3332-3345`（`?? total_tokens`） |
| **`usage_precision`** | 队列行上的可选精度标记；参与 `queuedKey` 的相等判定，但不进项目行（项目行被裁剪掉该列）。全部写入点 16 行 / 14 个语句，**无 antigravity 写入点** | `src/lib/rollout.js:3325`、`:3341`、`:3403`、`:3422`、`:6542`、`:6648`、`:6725`、`:6731`、`:6755`、`:6760`、`:20286-20287`、`:22011`、`:22025`、`:22113`、`:22157` |
| **priority 子集** | `...prioritySubsetOf(totals)` 展开出的 `priority_*` 列（值 > 0 才出现），记录 Fast/priority 档位贡献的 token；字段表 `src/lib/rollout.js:4073-4079`，主行展开 `:3342`，`totalsKey` 内判定 `:4167-4170` | 同上 |
| **权威成本（authoritative cost）** | 来源在 `SOURCES_WITH_AUTHORITATIVE_COST = {grok, cline}` 且行上 `total_cost_usd > 0` 时**直接采信该值**；`0` 视为「未上报」哨兵，继续走价格表 | `src/lib/pricing/index.js:32`、`:236-241` |
| **last-wins** | 读侧同键覆盖语义：解析时 `target.set(entry.key, entry.row)`，末行胜出。用于展示与一般聚合 | `src/lib/local-api.js:202-211`；`src/lib/async-queue-reader.js:51-57,86` |
| **setMaxTotal** | 与 last-wins 并存的第二套语义（取 MAX），**仅**用于 codex rescan/fork 修复的数据丢失守卫——撤回零行会把 last-wins 的末行压成 0，只有 MAX 能保住历史峰值 | `src/commands/sync.js:6574-6589`，唯一调用族 `:5848-5879` |
| **血缘修复（lineage repair）** | 针对 fork/复制产生的重复计数的一次性修复路径：收集 fork 别名（`collectClaudeForkAliases`）与家族时间，重算归属 | `src/lib/rollout.js:987-999`、`:1189-1234`；OpenCode 版本 `:3729-3748`、`:3959` |
| **claim（用量认领）** | Copilot 三通道共享 `source = "copilot"` 时的互斥机制：OTEL CLI span 结算后写 `recentUsageEvents`，供 session-store 通道避免重复计数；保留 7 天、上限 10000 条 | `src/lib/rollout.js:17375-17405`、`:18058-18064` |
| **evidence（来源证据）** | 对「这条用量从哪个字段读出来的」的标记体系，包括 `usageSource`、估算标记与读时修补判据；**否定性证据**（如「清单里没有 antigravity」）是这类结论的常见形态 | `src/lib/rollout.js:6726`（悬空注释）；`src/lib/local-api.js:155-200` |
| **deferred（推迟）** | 守卫命中时的第三态：既不执行也不跳过，而是**不写迁移键**、留待下次全量重试。`--auto` 轮次下该提示是静默的 | `src/commands/sync.js:952-959`、`:1147-1192`、`:1177-1181` |
| **union 安装模式** | 同一 provider 的 native 与 WSL 两份安装合并扫描；**仅在解析器具备路径无关身份时启用**（Codex/acode 的事件键、Claude 的文件哈希 + 消息哈希） | `src/lib/install-resolver.js:6-18` |
| **扫根三态** | 目录探测的三种结果：存在 / 不存在 / 存在但不可读（带 `error` code）。第三种必须显示为 `unreadable: <code>` 并触发「推迟」而非「跳过」 | `src/lib/scan-roots.js:89-110`、`:248-253` |
| **账户级来源** | 被标为 account 级、可被 personal/local 过滤排除的来源：`cursor`、`trae-cn` | `src/lib/source-metadata.js:1`、`:7-13` |
| **半小时桶** | `toUtcHalfHourStart(ts)`：把时间戳向下取整到 UTC 半点（`:00` 或 `:30`）。这是队列行的时间粒度，与「日归属」是两件事 | `src/lib/rollout.js:4173-4190`；副本 `src/lib/claude-categorizer.js:1175-1191` |
| **fork 重放突发** | 被 fork 的 Codex/Acode rollout 会把父会话全部历史一次性写入子文件；判据是「新文件 + 行间隔 < 500 ms」的突发，跨日另有日期守卫兜底 | `src/lib/rollout.js:2330-2340`、`:2556-2601`、`:4203-4214` |
| **内容等值短路** | 写配置前的优化：内容与目标全等则直接返回 `{changed:false}`，不写盘。与「哨兵串」是两种不同机制，不可混用 | `src/lib/opencode-config.js:73-75` |
| **哨兵串** | 文件内的自有标记（如 `// @tokentracker-managed-omp-extension`），带标记才覆盖、不带则跳过并回报 `unmanaged-extension-present`。本项目**只对 omp** 使用 | `src/lib/omp-hook.js:17,388-395`；`src/commands/init.js:813` |
| **负缓存** | 按 `source\0model` 记忆未命中查询的 `Set`，避免每次全表遍历；价格表 `revision` 变化时清空 | `src/lib/pricing/index.js:64-66,120-131` |
| **候选式计价** | 与「单表覆盖」相对的定价模型：多渠道条目作为独立候选，完整候选优先，再取**最高费用**；来源等级仅在同价时决胜 | 见 §10.3；匹配顺序 0→7 在 `src/lib/pricing/matcher.js:3-12`、`:252-386` |
