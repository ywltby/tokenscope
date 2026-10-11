"""读取历史库的验收口径（安装/升级/卸载断言用）。

用法：
  python scripts/inspect_history.py <history.db>
      输出 `<requests> <input> <output> <projects>` 或 `absent`。
  python scripts/inspect_history.py <history.db> --projects
      逐行输出去重后的 project_key（排序），供路径级断言使用。
  python scripts/inspect_history.py <history.db> --cwd
      逐行输出 `session_initial_cwd=<路径>`（去重非空、排序）与
      `event_cwd_nonnull=<n>`，供 cwd 元数据级断言使用（M06：三个
      cwd 字段都要验到，不只 project_key）。
  python scripts/inspect_history.py <history.db> --records
      按 record_id 排序逐行输出
      `record_id=..|project=..|session_initial_cwd=..|event_cwd=..|input=..|output=..`
      （缺值输出 NULL）——供**按请求身份逐条核对** cwd 字段与 NULL
      语义的断言使用（M06：DISTINCT 集合相等验不出单行改 NULL 或
      两条互换）。
  python scripts/inspect_history.py <history.db> --verify-manifest <期望.json>
      N12/M06 严格校验：按业务身份 (app, session_id, record_id) 与期望
      文件逐条比较 model_raw、project_key、session_initial_cwd、
      event_cwd 与四个 token 桶；NULL / 空字符串 / 字面 "NULL" 严格
      区分；缺失、额外、重复、空身份、任一字段错误都失败。
      退出码：0 = 完整通过；1 = 事实差异（JSON 结果打到 stdout）；
      2 = 打开/查询/参数/期望文件错误（消息打到 stderr）。
"""

from __future__ import annotations

import json
import sqlite3
import sys


def open_readonly(db: str):
    try:
        return sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        return None


VERIFY_SCHEMA_VERSION = 1
VERIFY_KEY = ("app", "session_id", "record_id")
VERIFY_FIELDS = ("model_raw", "project_key", "session_initial_cwd", "event_cwd",
                 "input_tokens", "output_tokens",
                 "cache_write_tokens", "cache_read_tokens")


def parse_persisted_token(raw, identity, field, problems):
    """持久化的 token 列是受检十进制 TEXT；坏值按事实差异记录并返回 None。"""
    if not isinstance(raw, str):
        problems.append({"kind": "field", "identity": identity, "field": field,
                         "expected": "十进制 TEXT", "actual": raw})
        return None
    text = raw.strip()
    if not text or text.startswith(("-", "+")) or not text.isascii()             or not text.isdigit():
        problems.append({"kind": "field", "identity": identity, "field": field,
                         "expected": "非负十进制", "actual": raw})
        return None
    value = int(text)
    if value > 18446744073709551615:  # u64 上界：持久层拒绝更大的值
        problems.append({"kind": "field", "identity": identity, "field": field,
                         "expected": "u64 范围内", "actual": raw})
        return None
    return value


def index_events(rows, label):
    """按业务身份索引；空身份 / 重复身份 / 非字符串身份都抛错（不悄悄丢弃）。"""
    result = {}
    for row in rows:
        key = tuple(row[name] for name in VERIFY_KEY)
        if any(type(value) is not str or not value for value in key):
            raise ValueError(f"{label}: invalid identity {key!r}")
        if key in result:
            raise ValueError(f"{label}: duplicate identity {key!r}")
        result[key] = row
    return result


def compare_events(actual_rows, expected_rows):
    """逐字段比较；返回问题列表（kind/identity/field/expected/actual）。"""
    problems = []
    try:
        actual = index_events(actual_rows, "actual")
    except ValueError as exc:
        kind = "duplicate_identity" if "duplicate" in str(exc) else "invalid_identity"
        problems.append({"kind": kind, "detail": str(exc)})
        return problems
    try:
        expected = index_events(expected_rows, "expected")
    except ValueError as exc:
        kind = "duplicate_identity" if "duplicate" in str(exc) else "invalid_identity"
        problems.append({"kind": kind, "detail": str(exc)})
        return problems
    for key in sorted(expected.keys() - actual.keys()):
        problems.append({"kind": "missing", "identity": key})
    for key in sorted(actual.keys() - expected.keys()):
        problems.append({"kind": "unexpected", "identity": key})
    for key in sorted(expected.keys() & actual.keys()):
        for field in VERIFY_FIELDS:
            if actual[key][field] != expected[key][field]:
                problems.append({"kind": "field", "identity": key, "field": field,
                                 "expected": expected[key][field],
                                 "actual": actual[key][field]})
    return problems


def load_expected(path):
    """读取并校验期望文件（版本 / 类型 / 身份 / token 范围）。"""
    with open(path, encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        raise ValueError("期望文件 schema_version 必须为 1")
    events = payload.get("events")
    if not isinstance(events, list) or not events:
        raise ValueError("期望文件必须包含非空 events 数组")
    result = []
    for ev in events:
        if not isinstance(ev, dict):
            raise ValueError("期望事件必须是对象")
        identity = tuple(ev.get(name) for name in VERIFY_KEY)
        if any(type(v) is not str or not v for v in identity):
            raise ValueError(f"期望事件身份非法: {identity!r}")
        if ev.get("app") not in ("claude-code", "codex"):
            raise ValueError(f"期望事件 app 非法: {ev.get('app')!r}")
        row = {name: ev.get(name) for name in VERIFY_KEY + VERIFY_FIELDS}
        for field in ("input_tokens", "output_tokens",
                      "cache_write_tokens", "cache_read_tokens"):
            value = row[field]
            if type(value) is not int or value < 0 or value > 18446744073709551615:
                raise ValueError(f"期望事件 {identity} 的 {field} 非法: {value!r}")
        for field in ("model_raw", "project_key", "session_initial_cwd",
                      "event_cwd"):
            if row[field] is not None and type(row[field]) is not str:
                raise ValueError(f"期望事件 {identity} 的 {field} 类型非法")
        result.append(row)
    return result


def run_verify_manifest(db_path, expected_path) -> int:
    """严格校验；stdout 输出带版本的 JSON 结果，退出码 0/1/2。"""
    result = {"schema_version": 1, "ok": False, "expected_file": str(expected_path),
              "problems": [], "summary": None}
    try:
        expected_rows = load_expected(expected_path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(json.dumps(result, ensure_ascii=False, indent=2))
        print(f"期望文件错误: {exc}", file=sys.stderr)
        return 2
    try:
        con = open_readonly(db_path)
        if con is None:
            print(json.dumps(result, ensure_ascii=False, indent=2))
            print(f"历史库不存在: {db_path}", file=sys.stderr)
            return 2
        try:
            tables = {r[0] for r in con.execute(
                "SELECT name FROM sqlite_master WHERE type='table'").fetchall()}
            if "usage_events" not in tables:
                print(json.dumps(result, ensure_ascii=False, indent=2))
                print("历史库缺少 usage_events 表", file=sys.stderr)
                return 2
            con.execute("BEGIN")
            actual_rows = con.execute(
                "SELECT app, session_id, record_id, model_raw, project_key, "
                "session_initial_cwd, event_cwd, input_tokens, output_tokens, "
                "cache_write_tokens, cache_read_tokens FROM usage_events"
            ).fetchall()
        finally:
            if con.in_transaction:
                con.execute("ROLLBACK")
            con.close()
    except sqlite3.Error as exc:
        print(json.dumps(result, ensure_ascii=False, indent=2))
        print(f"历史库打开/查询失败: {exc}", file=sys.stderr)
        return 2
    normalized = []
    parse_problems = []
    for row in actual_rows:
        record = dict(zip(VERIFY_KEY + VERIFY_FIELDS, row))
        identity = tuple(record[name] for name in VERIFY_KEY)
        tokens = {}
        bad = False
        for field in ("input_tokens", "output_tokens",
                      "cache_write_tokens", "cache_read_tokens"):
            value = parse_persisted_token(record[field], identity, field,
                                          parse_problems)
            tokens[field] = value
            bad = bad or value is None
        if not bad:
            record.update(tokens)
            normalized.append(record)
    if parse_problems:
        # N09/R04：持久 token 的类型/范围错误属于读取阶段失败 → 退出码 2。
        result["problems"].extend(parse_problems)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 2
    result["problems"].extend(compare_events(normalized, expected_rows))
    if result["problems"]:
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 1
    result["ok"] = True
    verified = {tuple(r[name] for name in VERIFY_KEY): r for r in normalized}
    result["summary"] = {
        "requests": len(verified),
        "input": sum(r["input_tokens"] for r in verified.values()),
        "output": sum(r["output_tokens"] for r in verified.values()),
        "projects": len({r["project_key"] for r in verified.values()}),
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


def main() -> int:
    argv = sys.argv[1:]
    if "--verify-manifest" in argv:
        idx = argv.index("--verify-manifest")
        rest = argv[:idx] + argv[idx + 1:]
        if len(rest) != 2:
            print(__doc__)
            return 2
        return run_verify_manifest(rest[0], rest[1])
    args = [a for a in argv if a not in ("--projects", "--cwd", "--records")]
    modes = [a for a in argv if a in ("--projects", "--cwd", "--records")]
    records_mode = "--records" in modes
    cwd_mode = "--cwd" in modes
    projects_mode = "--projects" in modes
    if len(args) != 1:
        print(__doc__)
        return 2
    con = open_readonly(args[0])
    if con is None:
        print("absent" if modes == [] else "")
        return 0
    try:
        tables = {
            r[0]
            for r in con.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        }
        if "usage_events" not in tables:
            print("absent" if modes == [] else "")
            return 0
        if records_mode:
            rows = con.execute(
                "SELECT record_id, project_key, session_initial_cwd, event_cwd, "
                "input_tokens, output_tokens FROM usage_events "
                "WHERE record_id IS NOT NULL AND record_id <> '' ORDER BY record_id"
            ).fetchall()
            for rid, proj, sic, ecwd, inp, outp in rows:
                print(
                    f"record_id={rid}|project={proj or 'NULL'}"
                    f"|session_initial_cwd={sic or 'NULL'}"
                    f"|event_cwd={ecwd or 'NULL'}"
                    f"|input={inp}|output={outp}"
                )
            return 0
        if cwd_mode:
            sics = sorted(
                {
                    r[0]
                    for r in con.execute(
                        "SELECT DISTINCT session_initial_cwd FROM usage_events "
                        "WHERE session_initial_cwd IS NOT NULL"
                    ).fetchall()
                }
            )
            for v in sics:
                print(f"session_initial_cwd={v}")
            ecwd_nonnull = con.execute(
                "SELECT COUNT(*) FROM usage_events WHERE event_cwd IS NOT NULL"
            ).fetchone()[0]
            print(f"event_cwd_nonnull={ecwd_nonnull}")
            return 0
        if projects_mode:
            for (project,) in con.execute(
                "SELECT DISTINCT project_key FROM usage_events "
                "WHERE project_key IS NOT NULL ORDER BY project_key"
            ).fetchall():
                print(project)
            return 0
        count, total_in, total_out = con.execute(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), "
            "COALESCE(SUM(output_tokens), 0) FROM usage_events"
        ).fetchone()
        projects = con.execute(
            "SELECT COUNT(DISTINCT project_key) FROM usage_events "
            "WHERE project_key IS NOT NULL"
        ).fetchone()[0]
        print(f"{count} {total_in} {total_out} {projects}")
        return 0
    finally:
        con.close()


if __name__ == "__main__":
    sys.exit(main())
