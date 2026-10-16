"""真实旧包升级夹具：生成来源日志与旧设置，绝不生成旧缓存。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sqlite3
import sys
import tomllib

from inspect_history import (VERIFY_FIELDS, VERIFY_KEY, compare_events, load_expected,
                             open_readonly)

FIXTURE = Path(__file__).resolve().parent / "fixtures/upgrade-0.1.0"
EXPECTED = FIXTURE / "expected.json"
MIGRATIONS = Path(__file__).resolve().parent / "fixtures/settings_migrations.json"
PROFILES = ("legacy-cache", "history")
MISSING = object()


def prepare(data, sources, profile="legacy-cache"):
    data, sources = Path(data), Path(sources)
    if profile not in PROFILES:
        raise ValueError(f"未知的起点夹具类型：{profile}")
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
    # history 起点（0.2.0 起）没有同意就不采集，因此夹具要预置同意；
    # legacy 起点（0.1.0）没有这个键，同意由升级后写入。
    consent = "privacy_policy_accepted = true\n" if profile == "history" else ""
    settings = (consent + 'price_auto_sync = false\nclose_action = "quit"\n\n'
                '[sources.claude]\nenabled = true\ndir = '
                + json.dumps(claude.as_posix(), ensure_ascii=False)
                + '\n\n[sources.codex]\nenabled = true\ndir = '
                + json.dumps(codex.as_posix(), ensure_ascii=False) + '\n')
    pricing = ('# 保留测试中的自定义价格\n[[model]]\nprefix = "claude-sonnet-4-5"\n'
               'input = 3.0\noutput = 15.0\ncache_write = 3.75\ncache_read = 0.3\n')
    for name, text in (("settings.toml", settings), ("pricing.toml", pricing)):
        with (data / name).open("w", encoding="utf-8", newline="\n") as fh:
            fh.write(text)
    return {"claude": str(claude), "codex": str(codex), "profile": profile}


def grant_test_consent(data):
    path = Path(data) / "settings.toml"
    text = path.read_text(encoding="utf-8")
    settings = tomllib.loads(text)
    if "privacy_policy_accepted" in settings:
        raise ValueError("旧版夹具不应预先存在当前隐私同意状态")
    with path.open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("privacy_policy_accepted = true\n" + text)


def ensure_test_consent(data):
    """升级链每一跳安装后、启动前使用：已有同意就不重复写入。"""
    path = Path(data) / "settings.toml"
    text = path.read_text(encoding="utf-8")
    if "privacy_policy_accepted" in tomllib.loads(text):
        return {"ok": True, "test_consent_added": False}
    with path.open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("privacy_policy_accepted = true\n" + text)
    return {"ok": True, "test_consent_added": True}


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


def load_migrations(path=MIGRATIONS):
    """读取声明式设置迁移表：只支持恒等、重命名/移动与默认值三种操作。"""
    payload = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        raise ValueError("设置迁移表 schema_version 必须为 1")
    migrations = payload.get("migrations")
    if not isinstance(migrations, dict) or not migrations:
        raise ValueError("设置迁移表必须包含规则")
    rules = {}
    for key, rule in migrations.items():
        match = re.fullmatch(r"(\d+)->(\d+)", str(key))
        if match is None or not isinstance(rule, dict):
            raise ValueError(f"设置迁移规则非法：{key!r}")
        operations = rule.get("operations")
        if not isinstance(operations, list):
            raise ValueError(f"设置迁移规则必须给出 operations 列表：{key!r}")
        for operation in operations:
            validate_operation(operation, key)
        rules[(int(match.group(1)), int(match.group(2)))] = operations
    return rules


def validate_operation(operation, key):
    if not isinstance(operation, dict):
        raise ValueError(f"设置迁移操作必须是对象：{key!r}")
    kind = operation.get("kind")
    if kind == "rename":
        if not field_path(operation.get("from")) or not field_path(operation.get("to")):
            raise ValueError(f"rename 操作必须给出 from 与 to：{key!r}")
    elif kind == "default":
        if not field_path(operation.get("path")) or "value" not in operation:
            raise ValueError(f"default 操作必须给出 path 与 value：{key!r}")
    else:
        raise ValueError(f"不支持的设置迁移操作：{kind!r}")


def field_path(value):
    return isinstance(value, str) and bool(value.strip()) and value == value.strip()


def flatten(table, prefix=""):
    result = {}
    for key, value in table.items():
        path = f"{prefix}{key}"
        if isinstance(value, dict):
            result.update(flatten(value, path + "."))
        else:
            result[path] = value
    return result


def lookup(table, path):
    value = table
    for part in path.split("."):
        if not isinstance(value, dict) or part not in value:
            return MISSING
        value = value[part]
    return value


def verify_settings(data, original, from_format, to_format, migrations=MIGRATIONS):
    """按『起点设置格式 → 当前设置格式』的声明式规则核对，不硬编码字段名。"""
    rules = load_migrations(migrations)
    operations = rules.get((int(from_format), int(to_format)))
    if operations is None:
        raise ValueError(f"未声明的设置格式迁移：{from_format}->{to_format}")
    expected = tomllib.loads(Path(original).read_text(encoding="utf-8"))
    current = tomllib.loads((Path(data) / "settings.toml").read_text(encoding="utf-8"))
    if current.get("privacy_policy_accepted") is not True:
        raise ValueError("当前验收同意状态未保留")
    renames = {operation["from"]: operation["to"] for operation in operations
               if operation["kind"] == "rename"}
    defaults = [(operation["path"], operation["value"]) for operation in operations
                if operation["kind"] == "default"]
    for path, value in flatten(expected).items():
        target = renames.get(path, path)
        found = lookup(current, target)
        if found is MISSING or found != value:
            raise ValueError(f"旧设置未保留：{path}"
                             + (f" -> {target}" if target != path else ""))
    for path, value in defaults:
        found = lookup(current, path)
        if found is MISSING or found != value:
            raise ValueError(f"迁移默认值不一致：{path}")
    return {"ok": True, "from_format": int(from_format), "to_format": int(to_format)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    command = sub.add_parser("prepare")
    command.add_argument("--data-dir", type=Path, required=True)
    command.add_argument("--source-dir", type=Path, required=True)
    command.add_argument("--profile", choices=PROFILES, default="legacy-cache")
    command = sub.add_parser("consent")
    command.add_argument("--data-dir", type=Path, required=True)
    command = sub.add_parser("ensure-consent")
    command.add_argument("--data-dir", type=Path, required=True)
    command = sub.add_parser("verify")
    command.add_argument("--database", type=Path, required=True)
    command = sub.add_parser("settings")
    command.add_argument("--data-dir", type=Path, required=True)
    command.add_argument("--original", type=Path, required=True)
    command.add_argument("--from-format", type=int, required=True)
    command.add_argument("--to-format", type=int, required=True)
    args = parser.parse_args()
    try:
        if args.mode == "prepare":
            result = prepare(args.data_dir, args.source_dir, args.profile)
        elif args.mode == "consent":
            grant_test_consent(args.data_dir)
            result = {"ok": True, "test_consent_added": True}
        elif args.mode == "ensure-consent":
            result = ensure_test_consent(args.data_dir)
        elif args.mode == "settings":
            result = verify_settings(args.data_dir, args.original,
                                     args.from_format, args.to_format)
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
