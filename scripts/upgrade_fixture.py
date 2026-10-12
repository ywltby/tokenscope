"""真实 0.1.0 升级夹具：生成来源日志与旧设置，绝不生成旧缓存。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sqlite3
import sys
import tomllib

from inspect_history import (VERIFY_FIELDS, VERIFY_KEY, compare_events, load_expected,
                             open_readonly)

FIXTURE = Path(__file__).resolve().parent / "fixtures/upgrade-0.1.0"
EXPECTED = FIXTURE / "expected.json"


def prepare(data, sources):
    data, sources = Path(data), Path(sources)
    if any((data / name).exists() for name in ("cache.db", "history.db", "settings.toml")):
        raise ValueError("真实旧版夹具必须从没有历史文件的独立 Runner 开始")
    data.mkdir(parents=True, exist_ok=True)
    claude, codex = sources / "claude", sources / "codex"
    claude.mkdir(parents=True, exist_ok=True)
    codex.mkdir(parents=True, exist_ok=True)
    for source in (FIXTURE / "claude").rglob("*.jsonl"):
        destination = claude / source.relative_to(FIXTURE / "claude")
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("w", encoding="utf-8", newline="\n") as fh:
            fh.write(source.read_text(encoding="utf-8"))
    settings = ('price_auto_sync = false\nclose_action = "quit"\n\n'
                '[sources.claude]\nenabled = true\ndir = '
                + json.dumps(claude.as_posix(), ensure_ascii=False)
                + '\n\n[sources.codex]\nenabled = true\ndir = '
                + json.dumps(codex.as_posix(), ensure_ascii=False) + '\n')
    pricing = ('# 保留测试中的自定义价格\n[[model]]\nprefix = "claude-sonnet-4-5"\n'
               'input = 3.0\noutput = 15.0\ncache_write = 3.75\ncache_read = 0.3\n')
    for name, text in (("settings.toml", settings), ("pricing.toml", pricing)):
        with (data / name).open("w", encoding="utf-8", newline="\n") as fh:
            fh.write(text)
    return {"claude": str(claude), "codex": str(codex)}


def grant_test_consent(data):
    path = Path(data) / "settings.toml"
    text = path.read_text(encoding="utf-8")
    settings = tomllib.loads(text)
    if "privacy_policy_accepted" in settings:
        raise ValueError("旧版夹具不应预先存在当前隐私同意状态")
    with path.open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("privacy_policy_accepted = true\n" + text)


def verify_legacy(path, expected=EXPECTED):
    expected_rows = load_expected(expected)
    con = open_readonly(str(path))
    if con is None:
        raise ValueError("旧程序没有生成可读 cache.db")
    try:
        con.execute("BEGIN")
        version = con.execute("SELECT value FROM meta WHERE key='schema_version'").fetchone()
        if version != ("6",):
            raise ValueError(f"实际旧缓存不是基线 v6：{version!r}")
        rows = con.execute("""
            SELECT f.agent, e.session_id, e.record_id, e.model, e.project,
                   NULL, NULL, e.input, e.output, e.cache_write, e.cache_read
            FROM events e LEFT JOIN files f ON f.id=e.file_id
        """).fetchall()
    finally:
        if con.in_transaction:
            con.rollback()
        con.close()
    normalized = [dict(zip(VERIFY_KEY + VERIFY_FIELDS, row)) for row in rows]
    for row in normalized:
        for field in ("input_tokens", "output_tokens", "cache_write_tokens", "cache_read_tokens"):
            value = row[field]
            if type(value) is not int or not 0 <= value <= 18446744073709551615:
                raise ValueError(f"旧缓存 token 不合法：{field}={value!r}")
    problems = compare_events(normalized, expected_rows)
    summary = {"requests": len(rows),
               "input": sum(row["input_tokens"] for row in normalized),
               "output": sum(row["output_tokens"] for row in normalized),
               "cache_write": sum(row["cache_write_tokens"] for row in normalized),
               "cache_read": sum(row["cache_read_tokens"] for row in normalized),
               "projects": len({row["project_key"] for row in normalized})}
    return {"schema_version": 1, "cache_schema": "6", "ok": not problems,
            "problems": problems, "summary": summary}


def verify_settings(data, original):
    expected = tomllib.loads(Path(original).read_text(encoding="utf-8"))
    current = tomllib.loads((Path(data) / "settings.toml").read_text(encoding="utf-8"))
    if current.get("privacy_policy_accepted") is not True:
        raise ValueError("当前验收同意状态未保留")
    for name, value in expected.items():
        if name not in current or current[name] != value:
            raise ValueError(f"旧设置未保留：{name}")
    return {"ok": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    command = sub.add_parser("prepare")
    command.add_argument("--data-dir", type=Path, required=True)
    command.add_argument("--source-dir", type=Path, required=True)
    command = sub.add_parser("consent")
    command.add_argument("--data-dir", type=Path, required=True)
    command = sub.add_parser("verify")
    command.add_argument("--database", type=Path, required=True)
    command = sub.add_parser("settings")
    command.add_argument("--data-dir", type=Path, required=True)
    command.add_argument("--original", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.mode == "prepare":
            result = prepare(args.data_dir, args.source_dir)
        elif args.mode == "consent":
            grant_test_consent(args.data_dir)
            result = {"ok": True, "test_consent_added": True}
        elif args.mode == "settings":
            result = verify_settings(args.data_dir, args.original)
        else:
            result = verify_legacy(args.database)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0 if result.get("ok", True) else 1
    except (ValueError, OSError, KeyError, sqlite3.Error) as exc:
        print(f"升级夹具校验失败：{exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
