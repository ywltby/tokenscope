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

const SCHEMA_VERSION: &str = "1";

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
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [SCHEMA_VERSION],
        )?;
        Ok(Self { conn })
    }

    /// 命中：指纹一致才返回缓存产物（事件按来源 agent 标注）。
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
        let mut stmt = self.conn.prepare(
            "SELECT id, lines_seen, bad_lines, skipped_sidechain, skipped_synthetic,
                    skipped_zero_usage, skipped_no_model, ignored_token_usage_record
             FROM files WHERE path = ?1 AND size = ?2 AND mtime_ms = ?3",
        )?;
        let row = stmt
            .query_row(rusqlite::params![path, size_s, mtime_s], |r| {
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
            .optional()?;
        let Some((id, lines_seen, bad_lines, sidechain, synthetic, zero, no_model, tur)) = row
        else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
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
        let mut events = Vec::new();
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

    /// 清除不再存在的文件缓存行，返回清除数。
    pub fn purge_missing(&self, keep: &[String]) -> Result<usize> {
        let mut stmt = self.conn.prepare("SELECT id, path FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
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
        self.conn.execute("DELETE FROM events", [])?;
        self.conn.execute("DELETE FROM files", [])?;
        Ok(())
    }
}
