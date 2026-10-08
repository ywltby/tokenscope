//! 缓存层（M4）：`~/.tokenscope/cache.db` 按文件指纹缓存解析产物。
//!
//! **缓存是纯优化，不是事实源**：任何故障（打不开/损坏/被锁）都向上返回
//! `Err`，由 report 管线降级为全量内存扫描并告警，数字不受影响。
//! 失效粒度 = 文件级：`(size, mtime_ms)` 变化即整文件重解析。

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};

use crate::model::AgentKind;
use crate::source::FileParse;

pub struct Cache {
    conn: Connection,
}

/// 与文件指纹对应的缓存命中产物。
pub struct CachedFile {
    pub parse: FileParse,
}

pub struct CacheStats {
    pub files: u64,
    pub events: u64,
}

/// 解析语义版本（B5 前置/B1 依赖）：版本不符的缓存必须整体失效——否则
/// 解析规则升级后旧缓存继续供数（R04）。递增记录：v2 = Codex cache_write
/// 语义修复（input = raw − cached − cache_write）；v3 = Codex 项目身份改
/// 完整 cwd（C2/R03）+ Claude 项目相对路径；
/// v4 = 缓存身份纳入来源上下文（R05）：文件键 = agent + 规范化根目录 +
/// 规范化文件路径——旧 v3 行无 root 维度，整体失效重建。
/// SF08：恢复路径新增桶边界校验——旧版本缓存可能存有越界事件，
/// 递增版本整体清空重建（源日志不删）。
/// v6 = Codex 仅额度通知不再计坏行，重解析以清除旧 bad_lines 误报。
const SCHEMA_VERSION: &str = "6";

fn fingerprint(size: u64, mtime_ms: i64) -> (i64, i64) {
    // u64 → i64 存库；实际文件大小远小于 i64 上限。
    (size as i64, mtime_ms)
}

pub fn mtime_ms(path: &Path) -> Result<i64> {
    let meta =
        std::fs::metadata(path).with_context(|| format!("读取元数据失败: {}", path.display()))?;
    let t = meta
        .modified()
        .with_context(|| format!("读取修改时间失败: {}", path.display()))?;
    Ok(t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0))
}

/// 仅测试用：迁移事务中途注入失败以验证原子回滚（不增加生产配置）。
/// 按库路径作用域——并行测试里其他测试的 Cache::open 不受影响。
#[cfg(test)]
static TEST_MIGRATION_FAIL_PATH: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

/// 测试注入：仅对指定库文件的迁移在提交前失败。
#[cfg(test)]
fn test_inject_migration_fail(path: &Path) -> Result<()> {
    if let Some(p) = TEST_MIGRATION_FAIL_PATH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        && p.as_path() == path
    {
        anyhow::bail!("注入的迁移失败（仅测试）");
    }
    Ok(())
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建缓存目录失败: {}", dir.display()))?;
        }
        let mut conn =
            Connection::open(path).with_context(|| format!("打开缓存失败: {}", path.display()))?;
        // busy_timeout 先于 pragma 设置：并发打开时等锁而不是立即报忙。
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
        // WAL 切换需要短暂独占，且 SQLite 不为该切换调用 busy handler——
        // 并发打开时对方连接可能正持锁/切换，这里小步重试（对齐 busy_timeout
        // 的等待语义），而非把并发打开错判为损坏。
        for attempt in 0..100 {
            match conn.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                Err(e)
                    if attempt < 99
                        && matches!(
                            &e,
                            rusqlite::Error::SqliteFailure(f, _)
                                if f.code == rusqlite::ErrorCode::DatabaseBusy
                        ) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => return Err(e).context("设置 WAL 日志模式失败"),
            }
        }
        conn.pragma_update(None, "foreign_keys", "ON")
            .context("启用外键约束失败")?;
        // F01：结构迁移收敛为单一写事务。真实 v3 库的 files 没有 root 列，
        // 必须先在事务内读取版本、把旧派生表整体 drop 后再建当前结构——
        // 任一步失败整体回滚，绝不出现"新索引 + 旧表"的半迁移状态
        //（修复前先建含 root 的索引，旧库连续报 no such column: root）。
        // IMMEDIATE 写事务：并发打开在事务内重新判断版本，串行完成迁移。
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .context("开始缓存初始化事务失败")?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        // 损坏 meta 的读取异常保持诊断可见，不再吞成"首次启动"。
        let stored: Option<String> = tx
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()
            .context("读取缓存 schema 版本失败")?;
        if stored.as_deref() != Some(SCHEMA_VERSION) {
            if stored.is_some() {
                log::warn!(
                    "缓存解析版本变化（{:?} → {SCHEMA_VERSION}），清空重建",
                    stored
                );
            }
            // 只重建 TokenScope 自有派生表：先 events（引用 files），再 files
            //（索引随表删除）；不删除数据库文件/WAL/原始日志。健康当前版
            // 库不走此分支，不重复清空。
            tx.execute("DROP TABLE IF EXISTS events", [])?;
            tx.execute("DROP TABLE IF EXISTS files", [])?;
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                root TEXT NOT NULL DEFAULT '',
                agent TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime_ms INTEGER NOT NULL,
                lines_seen INTEGER NOT NULL DEFAULT 0,
                bad_lines INTEGER NOT NULL DEFAULT 0,
                skipped_sidechain INTEGER NOT NULL DEFAULT 0,
                skipped_synthetic INTEGER NOT NULL DEFAULT 0,
                skipped_zero_usage INTEGER NOT NULL DEFAULT 0,
                skipped_no_model INTEGER NOT NULL DEFAULT 0,
                ignored_token_usage_record INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS events (
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                ts TEXT NOT NULL,
                record_id TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL,
                session_id TEXT NOT NULL,
                project TEXT NOT NULL,
                input INTEGER NOT NULL,
                output INTEGER NOT NULL,
                cache_write INTEGER NOT NULL,
                cache_read INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_file ON events(file_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_files_identity
                ON files(agent, root, path);",
        )?;
        // 提交前的测试注入点：版本与表结构一起回滚。
        #[cfg(test)]
        test_inject_migration_fail(path)?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [SCHEMA_VERSION],
        )?;
        tx.commit()?;
        Ok(Self { conn })
    }

    /// 命中：指纹一致**且来源上下文（agent + 根目录）一致**才返回缓存产物。
    /// R05：SQL 真正过滤 agent/root——不同来源类型或根目录的解析产物互不
    /// 可见，杜绝"旧事件改标 agent"与"换根沿用旧项目名"。
    /// 文件行与事件行包在同一个读事务里（R04）：并发清理/写入时读到的是
    /// 单一快照，不会出现"有文件行无事件行"的半截命中。
    pub fn lookup_file(
        &self,
        path: &str,
        agent: AgentKind,
        root: &str,
        size: u64,
        mtime_ms: i64,
    ) -> Result<Option<CachedFile>> {
        let (size, mtime_ms) = fingerprint(size, mtime_ms);
        let size_s = size.to_string();
        let mtime_s = mtime_ms.to_string();
        let tx = self.conn.unchecked_transaction()?;
        let row = {
            let mut stmt = tx.prepare(
                "SELECT id, lines_seen, bad_lines, skipped_sidechain, skipped_synthetic,
                        skipped_zero_usage, skipped_no_model, ignored_token_usage_record
                 FROM files
                 WHERE path = ?1 AND agent = ?2 AND root = ?3 AND size = ?4 AND mtime_ms = ?5",
            )?;
            stmt.query_row(
                rusqlite::params![path, agent.as_str(), root, size_s, mtime_s],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()?
        };
        let Some((id, lines_seen, bad_lines, sidechain, synthetic, zero, no_model, tur)) = row
        else {
            return Ok(None);
        };
        let mut events = Vec::new();
        let restored_bad_lines;
        {
            let mut stmt = tx.prepare(
                "SELECT ts, record_id, model, session_id, project, input, output, cache_write, cache_read
                 FROM events WHERE file_id = ?1 ORDER BY rowid",
            )?;
            let rows = stmt.query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            })?;
            let mut stats_bad = 0u64;
            for row in rows {
                let (ts, record_id, model, session_id, project, input, output, cw, cr) = row?;
                let event = crate::model::UsageEvent {
                    ts: ts
                        .parse()
                        .map_err(|e| anyhow::anyhow!("缓存时间戳解析失败: {e}"))?,
                    agent,
                    model,
                    session_id,
                    project,
                    record_id,
                    input_tokens: input.max(0) as u64,
                    output_tokens: output.max(0) as u64,
                    cache_write_tokens: cw.max(0) as u64,
                    cache_read_tokens: cr.max(0) as u64,
                };
                // SF08：缓存恢复与适配器解析经过同一桶校验边界——旧缓存中
                // 不合法事件不得绕过新检查（拒绝并计 bad_lines，不回绕）。
                if event.validate_buckets().is_err() {
                    stats_bad += 1;
                    continue;
                }
                events.push(event);
            }
            restored_bad_lines = stats_bad;
        }
        drop(tx);
        let mut stats = crate::source::CollectStats {
            lines_seen: lines_seen.max(0) as u64,
            bad_lines: bad_lines.max(0) as u64 + restored_bad_lines,
            skipped_sidechain: sidechain.max(0) as u64,
            skipped_synthetic: synthetic.max(0) as u64,
            skipped_zero_usage: zero.max(0) as u64,
            skipped_no_model: no_model.max(0) as u64,
            ignored_token_usage_record: tur.max(0) as u64,
            ..crate::source::CollectStats::default()
        };
        stats.events = events.len() as u64;
        Ok(Some(CachedFile {
            parse: FileParse { stats, events },
        }))
    }

    /// 测试专用（#[doc(hidden)]）：执行原生 SQL——集成测试模拟"旧版本
    /// 缓存中的越界数据"用（新 store 路径会拒绝该组合）。
    #[doc(hidden)]
    pub fn execute_raw_for_tests(&self, sql: &str, params: &[i64]) -> Result<usize> {
        let n = self
            .conn
            .execute(sql, rusqlite::params_from_iter(params.iter()))?;
        Ok(n)
    }

    pub fn store_file(
        &self,
        path: &str,
        agent: AgentKind,
        root: &str,
        size: u64,
        mtime_ms: i64,
        parse: &FileParse,
    ) -> Result<()> {
        let (size, mtime_ms) = fingerprint(size, mtime_ms);
        let tx = self.conn.unchecked_transaction()?;
        // R05：身份键 = (agent, root, path)。删除旧行也按完整身份——
        // 不同上下文的同路径行互不覆盖。
        tx.execute(
            "DELETE FROM files WHERE path = ?1 AND agent = ?2 AND root = ?3",
            rusqlite::params![path, agent.as_str(), root],
        )?;
        tx.execute(
            "INSERT INTO files(path, agent, root, size, mtime_ms, lines_seen, bad_lines,
                               skipped_sidechain, skipped_synthetic, skipped_zero_usage,
                               skipped_no_model, ignored_token_usage_record)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                path,
                agent.as_str(),
                root,
                size,
                mtime_ms,
                parse.stats.lines_seen as i64,
                parse.stats.bad_lines as i64,
                parse.stats.skipped_sidechain as i64,
                parse.stats.skipped_synthetic as i64,
                parse.stats.skipped_zero_usage as i64,
                parse.stats.skipped_no_model as i64,
                parse.stats.ignored_token_usage_record as i64,
            ],
        )?;
        let file_id: i64 = tx.query_row(
            "SELECT id FROM files WHERE path = ?1 AND agent = ?2 AND root = ?3",
            rusqlite::params![path, agent.as_str(), root],
            |r| r.get(0),
        )?;
        let mut stmt = tx.prepare(
            "INSERT INTO events(file_id, ts, record_id, model, session_id, project,
                                input, output, cache_write, cache_read)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for e in &parse.events {
            // SF08：入库前受检转换——超出 i64 的 token 值不得经缓存往返
            // 回绕"洗白"（store 失败 = 该文件不缓存，下次重新解析）。
            let conv = |v: u64| -> anyhow::Result<i64> {
                i64::try_from(v).map_err(|_| {
                    anyhow::anyhow!("token 计数超出缓存可表示范围（ refusing 回绕存储）")
                })
            };
            let input = conv(e.input_tokens)?;
            let output = conv(e.output_tokens)?;
            let cw = conv(e.cache_write_tokens)?;
            let cr = conv(e.cache_read_tokens)?;
            if e.validate_buckets().is_err() {
                anyhow::bail!("事件桶组合不可表示，拒绝写入缓存");
            }
            stmt.execute(rusqlite::params![
                file_id,
                e.ts.to_string(),
                e.record_id,
                e.model,
                e.session_id,
                e.project,
                input,
                output,
                cw,
                cr,
            ])?;
        }
        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    /// 清除**指定 agent** 范围内不再存在的文件缓存行，返回清除数（B5/F06）。
    /// 此前全局 purge：查看单 agent 时 keep_paths 只含该来源，其余来源的
    /// 缓存被整体清除再重建（全部→Claude→全部 会反复重解析 Codex）。
    /// 调用方约定：发现失败（errors 非空或根目录缺失）的来源不调用——
    /// 无法区分"已删除"与"暂时读不到"，绝不因发现失败清缓存。
    pub fn purge_agent(&self, agent: AgentKind, root: &str, keep: &[String]) -> Result<usize> {
        // R05：清理只作用于 (agent, root) 上下文——换根后旧上下文的行由
        // keep 缺失自然清出，但绝不波及其他 agent/上下文。
        let mut stmt = self
            .conn
            .prepare("SELECT id, path FROM files WHERE agent = ?1 AND root = ?2")?;
        let rows = stmt.query_map(rusqlite::params![agent.as_str(), root], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut stale = Vec::new();
        for r in rows {
            let (id, path) = r?;
            if !keep.contains(&path) {
                stale.push(id);
            }
        }
        drop(stmt);
        let mut deleted = 0;
        for id in stale {
            // events 由 ON DELETE CASCADE 级联清除。
            self.conn.execute("DELETE FROM files WHERE id = ?1", [id])?;
            deleted += 1;
        }
        Ok(deleted)
    }

    pub fn stats(&self) -> Result<CacheStats> {
        let files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let events: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        Ok(CacheStats {
            files: files.max(0) as u64,
            events: events.max(0) as u64,
        })
    }

    pub fn clear(&self) -> Result<()> {
        // 事务化（R04）：两表清空要么全部生效要么全部不生效。
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM events", [])?;
        tx.execute("DELETE FROM files", [])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentKind, UsageEvent};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-cache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 用真实 v3 DDL fixture（3109678 版）合成旧结构库：files 无 root 列、
    /// path 独占唯一、meta 版本 = 3、含一条合成事件。
    fn seed_v3(path: &Path) {
        let sql = include_str!("../tests/fixtures/cache/schema-v3.sql");
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(sql).unwrap();
    }

    /// files 表当前列名（结构断言用）。
    fn files_columns(path: &Path) -> Vec<String> {
        let raw = rusqlite::Connection::open(path).unwrap();
        let mut stmt = raw.prepare("PRAGMA table_info(files)").unwrap();
        let cols = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        drop(stmt);
        cols
    }

    fn parse_with(n: usize) -> FileParse {
        let events = (0..n)
            .map(|i| UsageEvent {
                ts: format!("2026-07-17T15:{:02}:00Z", i).parse().unwrap(),
                agent: AgentKind::Codex,
                model: "m".into(),
                session_id: "s".into(),
                project: "p".into(),
                record_id: String::new(),
                input_tokens: i as u64 + 1,
                output_tokens: 1,
                cache_write_tokens: 0,
                cache_read_tokens: 0,
            })
            .collect();
        let stats = crate::source::CollectStats {
            lines_seen: n as u64,
            events: n as u64,
            ..crate::source::CollectStats::default()
        };
        FileParse { stats, events }
    }

    #[test]
    fn test_parser_version_invalidates_cache() {
        // B5（R04）：解析版本不符必须整体失效——修复前版本号只写不查，
        // 解析规则升级后旧缓存继续供数。
        let dir = tmp_dir("version");
        let path = dir.join("cache.db");
        let c = Cache::open(&path).unwrap();
        c.store_file("a.jsonl", AgentKind::Codex, "root", 10, 100, &parse_with(2))
            .unwrap();
        assert_eq!(c.stats().unwrap().events, 2);
        drop(c);
        // 模拟 v5 缓存：仅额度通知的误报也必须随版本升级清除。
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute("UPDATE meta SET value='5' WHERE key='schema_version'", [])
            .unwrap();
        drop(raw);
        let c = Cache::open(&path).unwrap();
        assert_eq!(c.stats().unwrap().events, 0, "版本不符必须清空重建");
        assert_eq!(c.stats().unwrap().files, 0);
        // 失效后可正常重新入库。
        c.store_file("a.jsonl", AgentKind::Codex, "root", 10, 100, &parse_with(2))
            .unwrap();
        assert_eq!(c.stats().unwrap().events, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_purge_agent_scoped() {
        // B5（F06）：清理只作用于指定 agent——查看单 agent 不再清空其他来源。
        let dir = tmp_dir("purge");
        let c = Cache::open(&dir.join("cache.db")).unwrap();
        c.store_file(
            "codex-a.jsonl",
            AgentKind::Codex,
            "root",
            1,
            1,
            &parse_with(1),
        )
        .unwrap();
        c.store_file(
            "claude-a.jsonl",
            AgentKind::ClaudeCode,
            "root",
            2,
            2,
            &parse_with(3),
        )
        .unwrap();
        // 只清理 codex 的过期行：claude 行必须原样保留。
        let n = c
            .purge_agent(AgentKind::Codex, "root", &["kept.jsonl".to_string()])
            .unwrap();
        assert_eq!(n, 1);
        let st = c.stats().unwrap();
        assert_eq!(st.files, 1, "另一来源的缓存行不得被清理");
        assert_eq!(st.events, 3);
        // claude 自身范围：keep 中的行不清。
        let n = c
            .purge_agent(
                AgentKind::ClaudeCode,
                "root",
                &["claude-a.jsonl".to_string()],
            )
            .unwrap();
        assert_eq!(n, 0);
        assert_eq!(c.stats().unwrap().files, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_schema_invalidates_contextless_rows() {
        // R05：v3 旧行没有 root 维度 → schema v4 打开即整体失效。
        let dir = tmp_dir("schema-v4");
        let path = dir.join("cache.db");
        let c = Cache::open(&path).unwrap();
        c.store_file("a.jsonl", AgentKind::Codex, "root", 1, 1, &parse_with(2))
            .unwrap();
        drop(c);
        // 模拟 v3：版本号改回 "3"。
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute("UPDATE meta SET value='3' WHERE key='schema_version'", [])
            .unwrap();
        drop(raw);
        let c = Cache::open(&path).unwrap();
        assert_eq!(c.stats().unwrap().files, 0, "v3 上下文缺失行必须清空");
        assert_eq!(c.stats().unwrap().events, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_lookup_scoped_by_agent_and_root() {
        // R05：同路径不同 agent / 不同 root 的缓存行互不可见、互不覆盖。
        let dir = tmp_dir("identity");
        let c = Cache::open(&dir.join("cache.db")).unwrap();
        c.store_file(
            "s.jsonl",
            AgentKind::ClaudeCode,
            "root-a",
            1,
            1,
            &parse_with(2),
        )
        .unwrap();
        // 同路径同 root 换 agent：未命中（不返回改标旧事件）
        assert!(
            c.lookup_file("s.jsonl", AgentKind::Codex, "root-a", 1, 1)
                .unwrap()
                .is_none()
        );
        // 同 agent 同路径换 root：未命中（不返回旧项目名解析）
        assert!(
            c.lookup_file("s.jsonl", AgentKind::ClaudeCode, "root-b", 1, 1)
                .unwrap()
                .is_none()
        );
        // 完整身份一致：命中
        assert!(
            c.lookup_file("s.jsonl", AgentKind::ClaudeCode, "root-a", 1, 1)
                .unwrap()
                .is_some()
        );
        // 换 agent 后写入不覆盖 claude 行（身份隔离共存）
        c.store_file("s.jsonl", AgentKind::Codex, "root-a", 1, 1, &parse_with(1))
            .unwrap();
        assert_eq!(c.stats().unwrap().files, 2, "不同 agent 的同路径行共存");
        assert!(
            c.lookup_file("s.jsonl", AgentKind::ClaudeCode, "root-a", 1, 1)
                .unwrap()
                .is_some(),
            "原上下文行未被覆盖"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_lookup_snapshot_all_or_nothing() {
        // B5（R04）：文件行与事件行同一读事务（WAL 快照）——并发 clear 期间
        // 每次命中要么完整要么不命中，绝不出现"有文件行无事件行"的半截命中。
        let dir = tmp_dir("snapshot");
        let path = dir.join("cache.db");
        let c = Cache::open(&path).unwrap();
        c.store_file("a.jsonl", AgentKind::Codex, "root", 1, 1, &parse_with(5))
            .unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let path2 = path.clone();
        let clearer = std::thread::spawn(move || {
            let c2 = Cache::open(&path2).unwrap();
            while !stop2.load(Ordering::Relaxed) {
                let _ = c2.clear();
            }
        });
        for _ in 0..200 {
            if let Some(hit) = c
                .lookup_file("a.jsonl", AgentKind::Codex, "root", 1, 1)
                .unwrap()
            {
                assert_eq!(
                    hit.parse.events.len(),
                    5,
                    "命中必须完整（不得出现半截快照）"
                );
            }
        }
        stop.store(true, Ordering::Relaxed);
        clearer.join().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_migrates_real_v3_schema() {
        // F01：真实 v3 的 files 没有 root 列、path 独占唯一。打开必须先把
        // 旧结构整体迁移成当前结构，而不是先建含 root 的索引（修复前连续
        // 报 no such column: root，每次退回全量扫描）。
        let dir = tmp_dir("migrate-v3");
        let path = dir.join("cache.db");
        seed_v3(&path);

        let c = Cache::open(&path).unwrap();
        // v3 行无 root 维度：迁移即整体失效重建
        assert_eq!(c.stats().unwrap().files, 0, "v3 旧行必须整体失效");
        assert_eq!(c.stats().unwrap().events, 0);
        // 新结构确有 root 列
        let cols = files_columns(&path);
        assert!(
            cols.iter().any(|x| x == "root"),
            "迁移后 files 必须有 root 列: {cols:?}"
        );
        // 复合唯一键生效：同 path 不同 root/agent 可共存
        c.store_file(
            "s.jsonl",
            AgentKind::ClaudeCode,
            "root-a",
            1,
            1,
            &parse_with(2),
        )
        .unwrap();
        c.store_file("s.jsonl", AgentKind::Codex, "root-a", 1, 1, &parse_with(1))
            .unwrap();
        c.store_file(
            "s.jsonl",
            AgentKind::ClaudeCode,
            "root-b",
            1,
            1,
            &parse_with(3),
        )
        .unwrap();
        assert_eq!(c.stats().unwrap().files, 3, "复合身份键必须允许上下文共存");
        drop(c);
        // 写入后重新打开能命中
        let c2 = Cache::open(&path).unwrap();
        assert!(
            c2.lookup_file("s.jsonl", AgentKind::ClaudeCode, "root-a", 1, 1)
                .unwrap()
                .is_some()
        );
        assert_eq!(c2.stats().unwrap().events, 6);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_migration_is_atomic() {
        // 迁移中途失败 → 版本与表结构一起回滚：旧 v3 结构与数据原样保留，
        // 下一次打开（无注入）再完成迁移。失败注入仅测试提供。
        let dir = tmp_dir("migrate-atomic");
        let path = dir.join("cache.db");
        seed_v3(&path);

        TEST_MIGRATION_FAIL_PATH
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(path.clone());
        let r = Cache::open(&path);
        TEST_MIGRATION_FAIL_PATH
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        assert!(r.is_err(), "注入失败必须使 open 返回可见降级错误");

        // 回滚后：旧结构（无 root）、版本仍 3、旧事件仍在
        let cols = files_columns(&path);
        assert!(
            !cols.iter().any(|x| x == "root"),
            "迁移失败必须整体回滚，不得留下半迁移结构: {cols:?}"
        );
        let raw = rusqlite::Connection::open(&path).unwrap();
        let v: String = raw
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "3", "版本必须随迁移失败一起回滚");
        let n: i64 = raw
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "旧事件必须随回滚恢复");
        drop(raw);

        // 无注入的后续打开完成迁移
        let c = Cache::open(&path).unwrap();
        assert_eq!(c.stats().unwrap().files, 0);
        let cols = files_columns(&path);
        assert!(cols.iter().any(|x| x == "root"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_cache_concurrent_v3_open() {
        // 两个连接并发打开 v3 库：写事务内重判版本 + 串行迁移，不得交错出
        // 半迁移结构；两个打开都必须成功，结果为健康可用的当前版结构。
        let dir = tmp_dir("migrate-conc");
        let path = dir.join("cache.db");
        seed_v3(&path);

        let path2 = path.clone();
        let h1 = std::thread::spawn(move || Cache::open(&path));
        let h2 = std::thread::spawn(move || Cache::open(&path2));
        let c1 = h1.join().unwrap().expect("并发打开 1 必须成功");
        let c2 = h2.join().unwrap().expect("并发打开 2 必须成功");

        assert_eq!(c1.stats().unwrap().files, 0, "v3 旧行整体失效一次");
        c1.store_file("a.jsonl", AgentKind::Codex, "root", 1, 1, &parse_with(2))
            .unwrap();
        assert!(
            c2.lookup_file("a.jsonl", AgentKind::Codex, "root", 1, 1)
                .unwrap()
                .is_some(),
            "另一连接必须看到已迁移的健康结构"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
