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
/// 语义修复（input = raw − cached − cache_write）。
const SCHEMA_VERSION: &str = "2";

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

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建缓存目录失败: {}", dir.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("打开缓存失败: {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(2000))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
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
            CREATE INDEX IF NOT EXISTS idx_events_file ON events(file_id);",
        )?;
        // 解析版本不符 → 旧缓存整体失效（清空后按当前规则重建）。
        // 此前版本号只写不查（R04）：解析规则升级后旧缓存继续供数。
        let stored: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap_or(None);
        if stored.as_deref() != Some(SCHEMA_VERSION) {
            if stored.is_some() {
                log::warn!(
                    "缓存解析版本变化（{:?} → {SCHEMA_VERSION}），清空重建",
                    stored
                );
            }
            conn.execute("DELETE FROM events", [])?;
            conn.execute("DELETE FROM files", [])?;
        }
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [SCHEMA_VERSION],
        )?;
        Ok(Self { conn })
    }

    /// 命中：指纹一致才返回缓存产物（事件按来源 agent 标注）。
    /// 文件行与事件行包在同一个读事务里（R04）：并发清理/写入时读到的是
    /// 单一快照，不会出现"有文件行无事件行"的半截命中。
    pub fn lookup_file(
        &self,
        path: &str,
        agent: AgentKind,
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
                 FROM files WHERE path = ?1 AND size = ?2 AND mtime_ms = ?3",
            )?;
            stmt.query_row(rusqlite::params![path, size_s, mtime_s], |r| {
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
            })
            .optional()?
        };
        let Some((id, lines_seen, bad_lines, sidechain, synthetic, zero, no_model, tur)) = row
        else {
            return Ok(None);
        };
        let mut events = Vec::new();
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
            for row in rows {
                let (ts, record_id, model, session_id, project, input, output, cw, cr) = row?;
                events.push(crate::model::UsageEvent {
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
                });
            }
        }
        drop(tx);
        let mut stats = crate::source::CollectStats {
            lines_seen: lines_seen.max(0) as u64,
            bad_lines: bad_lines.max(0) as u64,
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

    pub fn store_file(
        &self,
        path: &str,
        agent: AgentKind,
        size: u64,
        mtime_ms: i64,
        parse: &FileParse,
    ) -> Result<()> {
        let (size, mtime_ms) = fingerprint(size, mtime_ms);
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
        tx.execute(
            "INSERT INTO files(path, agent, size, mtime_ms, lines_seen, bad_lines,
                               skipped_sidechain, skipped_synthetic, skipped_zero_usage,
                               skipped_no_model, ignored_token_usage_record)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                path,
                agent.as_str(),
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
        let file_id: i64 =
            tx.query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get(0))?;
        let mut stmt = tx.prepare(
            "INSERT INTO events(file_id, ts, record_id, model, session_id, project,
                                input, output, cache_write, cache_read)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for e in &parse.events {
            stmt.execute(rusqlite::params![
                file_id,
                e.ts.to_string(),
                e.record_id,
                e.model,
                e.session_id,
                e.project,
                e.input_tokens as i64,
                e.output_tokens as i64,
                e.cache_write_tokens as i64,
                e.cache_read_tokens as i64,
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
    pub fn purge_agent(&self, agent: AgentKind, keep: &[String]) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path FROM files WHERE agent = ?1")?;
        let rows = stmt.query_map([agent.as_str()], |r| {
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
        c.store_file("a.jsonl", AgentKind::Codex, 10, 100, &parse_with(2))
            .unwrap();
        assert_eq!(c.stats().unwrap().events, 2);
        drop(c);
        // 模拟旧版本缓存：把版本号改回 "1"。
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute("UPDATE meta SET value='1' WHERE key='schema_version'", [])
            .unwrap();
        drop(raw);
        let c = Cache::open(&path).unwrap();
        assert_eq!(c.stats().unwrap().events, 0, "版本不符必须清空重建");
        assert_eq!(c.stats().unwrap().files, 0);
        // 失效后可正常重新入库。
        c.store_file("a.jsonl", AgentKind::Codex, 10, 100, &parse_with(2))
            .unwrap();
        assert_eq!(c.stats().unwrap().events, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_purge_agent_scoped() {
        // B5（F06）：清理只作用于指定 agent——查看单 agent 不再清空其他来源。
        let dir = tmp_dir("purge");
        let c = Cache::open(&dir.join("cache.db")).unwrap();
        c.store_file("codex-a.jsonl", AgentKind::Codex, 1, 1, &parse_with(1))
            .unwrap();
        c.store_file(
            "claude-a.jsonl",
            AgentKind::ClaudeCode,
            2,
            2,
            &parse_with(3),
        )
        .unwrap();
        // 只清理 codex 的过期行：claude 行必须原样保留。
        let n = c
            .purge_agent(AgentKind::Codex, &["codex-kept.jsonl".to_string()])
            .unwrap();
        assert_eq!(n, 1);
        let st = c.stats().unwrap();
        assert_eq!(st.files, 1, "另一来源的缓存行不得被清理");
        assert_eq!(st.events, 3);
        // claude 自身范围：keep 中的行不清。
        let n = c
            .purge_agent(AgentKind::ClaudeCode, &["claude-a.jsonl".to_string()])
            .unwrap();
        assert_eq!(n, 0);
        assert_eq!(c.stats().unwrap().files, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_lookup_snapshot_all_or_nothing() {
        // B5（R04）：文件行与事件行同一读事务（WAL 快照）——并发 clear 期间
        // 每次命中要么完整要么不命中，绝不出现"有文件行无事件行"的半截命中。
        let dir = tmp_dir("snapshot");
        let path = dir.join("cache.db");
        let c = Cache::open(&path).unwrap();
        c.store_file("a.jsonl", AgentKind::Codex, 1, 1, &parse_with(5))
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
            if let Some(hit) = c.lookup_file("a.jsonl", AgentKind::Codex, 1, 1).unwrap() {
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
}
