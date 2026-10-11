"""合成 v10 旧版 cache.db（安装验收的迁移数据源）。

schema 与 tests/history_migration.rs 的 LEGACY_V10_SCHEMA 一致（真实 v10
结构：files 含 context_rev，events 含 project / session_initial_cwd /
event_cwd，无 line 列）。默认三行 Claude 事件、两个不同项目 cwd，
升级/迁移验收据此断言请求数、token 与 cwd 不丢失。

用法：python scripts/make_legacy_cache.py <输出路径>
"""

from __future__ import annotations

import os
import sqlite3
import sys

SCHEMA = """
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    root TEXT NOT NULL DEFAULT '',
    agent TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime_ms INTEGER NOT NULL,
    context_rev TEXT NOT NULL DEFAULT '',
    lines_seen INTEGER NOT NULL DEFAULT 0,
    bad_lines INTEGER NOT NULL DEFAULT 0,
    skipped_sidechain INTEGER NOT NULL DEFAULT 0,
    skipped_synthetic INTEGER NOT NULL DEFAULT 0,
    skipped_zero_usage INTEGER NOT NULL DEFAULT 0,
    skipped_no_model INTEGER NOT NULL DEFAULT 0,
    ignored_token_usage_record INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS events (
    file_id INTEGER NOT NULL,
    ts TEXT NOT NULL,
    record_id TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL,
    session_id TEXT NOT NULL,
    project TEXT NOT NULL,
    session_initial_cwd TEXT,
    event_cwd TEXT,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    cache_read INTEGER NOT NULL
);
"""

# (ts, record_id, project, session_initial_cwd, input, output, cache_write, cache_read)
EVENTS = [
    ("2026-07-17T08:00:00Z", "m1", "C:/work/alpha", "C:/work/alpha", 100, 20, 0, 0),
    ("2026-07-17T08:01:00Z", "m2", "C:/work/alpha", "C:/work/alpha", 50, 10, 0, 0),
    ("2026-07-18T09:00:00Z", "m3", "C:/work/beta", "C:/work/beta", 30, 5, 0, 0),
]

EXPECTED = {
    "events": len(EVENTS),
    "input": sum(e[4] for e in EVENTS),
    "output": sum(e[5] for e in EVENTS),
    "projects": len({e[2] for e in EVENTS}),
}


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    out = sys.argv[1]
    parent = os.path.dirname(os.path.abspath(out))
    os.makedirs(parent, exist_ok=True)
    con = sqlite3.connect(out)
    try:
        con.execute("PRAGMA foreign_keys = OFF")
        con.executescript(SCHEMA)
        con.execute("INSERT INTO meta(key, value) VALUES('schema_version', '10')")
        con.execute(
            "INSERT INTO files(id, path, root, agent, size, mtime_ms) "
            "VALUES(1, '/logs/legacy/sess.jsonl', '/logs', 'claude-code', 100, 1700000000000)"
        )
        for i, (ts, rid, project, sic, inp, outp, cw, cr) in enumerate(EVENTS):
            con.execute(
                "INSERT INTO events(file_id, ts, record_id, model, session_id, project, "
                "session_initial_cwd, event_cwd, input, output, cache_write, cache_read) "
                "VALUES(1, ?, ?, 'claude-sonnet-4-5', 's-legacy', ?, ?, NULL, ?, ?, ?, ?)",
                (ts, rid, project, sic, inp, outp, cw, cr),
            )
        con.commit()
        # 自检：写出的库必须能按验收口径读回。
        count, total_in = con.execute(
            "SELECT COUNT(*), COALESCE(SUM(input), 0) FROM events"
        ).fetchone()
        assert count == EXPECTED["events"], (count, EXPECTED)
        assert total_in == EXPECTED["input"], (total_in, EXPECTED)
    finally:
        con.close()
    print(
        f"legacy cache.db written: {out} "
        f"(events={EXPECTED['events']} input={EXPECTED['input']} "
        f"output={EXPECTED['output']} projects={EXPECTED['projects']})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
