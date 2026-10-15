-- F01 fixture：3109678 版 src/cache.rs 的真实 v3 DDL（UTF-8 合成，不含真实数据）。
-- 关键特征：files 没有 root 列、path 独占唯一键；事件以 file_id 挂 files。
-- v4 起文件键 = (agent, root, path)——旧结构必须先迁移再使用新列。
CREATE TABLE IF NOT EXISTS meta (
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
CREATE INDEX IF NOT EXISTS idx_events_file ON events(file_id);
INSERT INTO meta(key, value) VALUES('schema_version', '3');
INSERT INTO files(id, path, agent, size, mtime_ms) VALUES
    (1, 'c:\synthetic\a.jsonl', 'claude', 100, 1700000000000);
INSERT INTO events(file_id, ts, record_id, model, session_id, project,
                   input, output, cache_write, cache_read) VALUES
    (1, '2026-08-01T10:00:00Z', '', 'claude-sonnet-4-5', 'c1', 'synthetic', 10, 1, 0, 0);
