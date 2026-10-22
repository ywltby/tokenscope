"""Create the synthetic CCS v20 database used by isolated native acceptance."""

import argparse
from contextlib import closing
import datetime
import hashlib
import json
from pathlib import Path
import sqlite3


SCHEMA = """
PRAGMA user_version = 20;
CREATE TABLE proxy_request_logs (
    request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL,
    app_type TEXT NOT NULL, model TEXT NOT NULL, request_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', status_code INTEGER NOT NULL DEFAULT 200,
    session_id TEXT, created_at INTEGER NOT NULL, data_source TEXT NOT NULL DEFAULT 'proxy',
    pricing_model TEXT, input_token_semantics INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE usage_daily_rollups (
    date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL,
    model TEXT NOT NULL, request_model TEXT NOT NULL DEFAULT '', pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0, success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
"""


def create_fixture(target: Path) -> dict:
    target = target.resolve()
    target.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive creation prevents overwriting any existing database, including real inputs.
    with target.open("xb"):
        pass
    with closing(sqlite3.connect(target)) as database, database:
        database.executescript(SCHEMA)
        timestamp = int(datetime.datetime(2026, 10, 6, 1, tzinfo=datetime.timezone.utc).timestamp())
        rows = [
            ("session:msg-acceptance-1", "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "claude_session", timestamp, 1200, 640, 3000, 100),
            ("ccs-only-request", "ccs-only-session", "proxy", timestamp + 3600, 700, 90, 30, 10),
        ]
        for request, session, source, created, input_tokens, output, read, write in rows:
            database.execute(
                """INSERT INTO proxy_request_logs
                (request_id,provider_id,app_type,model,request_model,input_tokens,output_tokens,
                 cache_read_tokens,cache_creation_tokens,total_cost_usd,status_code,session_id,
                 created_at,data_source,pricing_model,input_token_semantics)
                VALUES (?,'synthetic','claude','claude-opus-5-5','claude-opus-5-5',?,?,?,?,'0.25',200,?,?,?,'claude-opus-5-5',0)""",
                (request, input_tokens, output, read, write, session, created, source),
            )
        database.execute(
            """INSERT INTO usage_daily_rollups
            (date,app_type,provider_id,model,request_count,success_count,input_tokens,output_tokens,
             cache_read_tokens,cache_creation_tokens,total_cost_usd)
            VALUES ('2026-10-05','claude','synthetic','claude-opus-5-5',3,3,900,60,20,10,'1.5')"""
        )
    return {"path": str(target), "requests": 2, "rollups": 1,
            "sha256": hashlib.sha256(target.read_bytes()).hexdigest()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(create_fixture(args.output), ensure_ascii=False))


if __name__ == "__main__":
    main()
