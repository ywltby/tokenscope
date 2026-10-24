"""M06 事实校验器契约测试（stdlib unittest + 临时 SQLite）。

被测对象：scripts/inspect_history.py --verify-manifest。
先按本文件用例确认红灯（当前脚本无 --verify-manifest），实现后转绿。

覆盖（对应整合计划 N12 修复设计的反例/正例表）：
- 精确匹配通过（混合 null / 非空 event_cwd、UTF-8 路径）
- 缺失 / 额外 / 重复身份 / 空身份 → 失败且定位身份
- 单条 session_initial_cwd 改 NULL、两条互换、project 改错 → 逐字段定位
- 期望 null 改成 "" 或字面 "NULL"、非空 event_cwd 丢失 → 失败
- 任一 token 桶（尤其 cache_write/cache_read）改变 → 失败
- 行序 / SQLite id 改变 → 通过（业务身份无关行序）
- 库缺失、缺表、缺列、期望文件损坏、版本错误 → 退出码 2
- 生成器读回契约：scripts/make_legacy_cache.py 的旧缓存输入与期望文件一致
"""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT = REPO_ROOT / "scripts" / "inspect_history.py"
EXPECTED_JSON = REPO_ROOT / "scripts" / "fixtures" / "install_history_expected.json"
MAKE_LEGACY = REPO_ROOT / "scripts" / "make_legacy_cache.py"

SCHEMA = """
CREATE TABLE usage_events (
    id INTEGER PRIMARY KEY,
    event_key TEXT NOT NULL UNIQUE,
    app TEXT NOT NULL,
    ts_seconds INTEGER NOT NULL,
    ts_nanos INTEGER NOT NULL CHECK (ts_nanos BETWEEN 0 AND 999999999),
    model_raw TEXT NOT NULL,
    model_identity TEXT NOT NULL,
    identity_revision INTEGER NOT NULL,
    session_id TEXT,
    record_id TEXT,
    project_key TEXT,
    session_initial_cwd TEXT,
    event_cwd TEXT,
    input_tokens TEXT NOT NULL,
    output_tokens TEXT NOT NULL,
    cache_write_tokens TEXT NOT NULL,
    cache_read_tokens TEXT NOT NULL,
    origin_rank INTEGER NOT NULL DEFAULT 1,
    observed_at_utc TEXT NOT NULL,
    observed_at_seconds INTEGER NOT NULL,
    observed_at_nanos INTEGER NOT NULL CHECK (observed_at_nanos BETWEEN 0 AND 999999999),
    parser_revision INTEGER NOT NULL
);
"""

COLS = ("app", "session_id", "record_id", "model_raw", "project_key",
        "session_initial_cwd", "event_cwd",
        "input_tokens", "output_tokens", "cache_write_tokens", "cache_read_tokens")


def base_event(record_id: str, session_id: str, project: str, sic: str,
               event_cwd, inp: int, outp: int) -> dict:
    return {
        "app": "claude-code",
        "session_id": session_id,
        "record_id": record_id,
        "model_raw": "claude-sonnet-4-5",
        "project_key": project,
        "session_initial_cwd": sic,
        "event_cwd": event_cwd,
        "input_tokens": inp,
        "output_tokens": outp,
        "cache_write_tokens": 0,
        "cache_read_tokens": 0,
    }


def expected_events() -> list:
    with open(EXPECTED_JSON, encoding="utf-8") as fh:
        return json.load(fh)["events"]


def insert_events(db: str, events: list) -> None:
    con = sqlite3.connect(db)
    con.executescript(SCHEMA)
    for i, ev in enumerate(events):
        con.execute(
            "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw, "
            "model_identity, identity_revision, session_id, record_id, project_key, "
            "session_initial_cwd, event_cwd, input_tokens, output_tokens, "
            "cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc, "
            "observed_at_seconds, observed_at_nanos, parser_revision) "
            "VALUES(?, ?, 1784307600, 0, ?, 'mid', 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, "
            "'x', 1784307600, 0, 1)",
            (
                f"k-{ev['record_id']}-{i}",
                ev["app"],
                str(ev["model_raw"]),
                ev["session_id"],
                ev["record_id"],
                str(ev["project_key"]),
                None if ev["session_initial_cwd"] is None else str(ev["session_initial_cwd"]),
                None if ev["event_cwd"] is None else str(ev["event_cwd"]),
                str(ev["input_tokens"]),
                str(ev["output_tokens"]),
                str(ev["cache_write_tokens"]),
                str(ev["cache_read_tokens"]),
            ),
        )
    con.commit()
    con.close()


class VerifyManifestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.expected_path = self.dir / "expected.json"
        with open(EXPECTED_JSON, encoding="utf-8") as src, \
                open(self.expected_path, "w", encoding="utf-8") as dst:
            dst.write(src.read())

    def run_verify(self, db: str, expected: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT), db, "--verify-manifest", expected],
            capture_output=True, text=True, encoding="utf-8",
        )

    def make_db(self, events: list) -> str:
        counter = getattr(self, "_db_seq", 0) + 1
        self._db_seq = counter
        db = str(self.dir / f"history-{counter}.db")
        insert_events(db, events)
        return db

    # ---------- 正例 ----------

    def test_exact_match_passes_with_mixed_null_and_values(self):
        db = self.make_db(expected_events())
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        result = json.loads(r.stdout)
        self.assertTrue(result["ok"])
        self.assertEqual(result["problems"], [])
        self.assertEqual(result["summary"]["requests"], 3)
        self.assertEqual(result["summary"]["input"], 180)
        self.assertEqual(result["summary"]["output"], 35)
        self.assertEqual(result["summary"]["projects"], 2)

    def test_row_order_and_rowid_do_not_matter(self):
        events = list(reversed(expected_events()))
        db = self.make_db(events)
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    # ---------- 缺失 / 额外 / 身份 ----------

    def test_missing_record_fails_and_reports_identity(self):
        events = [e for e in expected_events() if e["record_id"] != "m2"]
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        result = json.loads(r.stdout)
        self.assertFalse(result["ok"])
        kinds = {(p["kind"], tuple(p["identity"])) for p in result["problems"]}
        self.assertIn(("missing", ("claude-code", "s-alpha", "m2")), kinds)

    def test_extra_record_fails_and_reports_identity(self):
        events = expected_events() + [base_event("m9", "s-beta", "C:/work/beta",
                                                 "C:/work/beta", None, 1, 1)]
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        result = json.loads(r.stdout)
        kinds = {(p["kind"], tuple(p["identity"])) for p in result["problems"]}
        self.assertIn(("unexpected", ("claude-code", "s-beta", "m9")), kinds)

    def test_empty_record_identity_fails(self):
        events = expected_events()
        events[0]["record_id"] = ""
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        result = json.loads(r.stdout)
        self.assertTrue(any(p["kind"] == "invalid_identity" for p in result["problems"]))

    def test_duplicate_identity_fails(self):
        events = expected_events()
        events[1]["record_id"] = "m1"
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        result = json.loads(r.stdout)
        self.assertTrue(any(p["kind"] == "duplicate_identity" for p in result["problems"]))

    # ---------- 字段逐个核对 ----------

    def test_single_session_initial_cwd_null_is_detected(self):
        events = expected_events()
        events[0]["session_initial_cwd"] = None
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        problem = json.loads(r.stdout)["problems"][0]
        self.assertEqual(problem["kind"], "field")
        self.assertEqual((problem["identity"][1], problem["identity"][2]), ("s-alpha", "m1"))
        self.assertEqual(problem["field"], "session_initial_cwd")
        self.assertIsNone(problem["actual"])
        self.assertEqual(problem["expected"], "C:/work/alpha")

    def test_swapped_cwds_are_detected(self):
        events = expected_events()
        events[1]["session_initial_cwd"], events[2]["session_initial_cwd"] = \
            events[2]["session_initial_cwd"], events[1]["session_initial_cwd"]
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        problems = json.loads(r.stdout)["problems"]
        self.assertEqual({p["identity"][2] for p in problems}, {"m2", "m3"})

    def test_wrong_project_is_detected(self):
        events = expected_events()
        events[2]["project_key"] = "C:/work/beta-incorrect"
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        problem = json.loads(r.stdout)["problems"][0]
        self.assertEqual((problem["identity"][2], problem["field"]),
                         ("m3", "project_key"))

    def test_event_cwd_null_vs_empty_string_distinguished(self):
        events = expected_events()
        events[0]["event_cwd"] = ""
        expected = json.loads(json.dumps(expected_events()))
        expected[0]["event_cwd"] = ""
        with open(self.expected_path, "w", encoding="utf-8") as dst:
            json.dump({"schema_version": 1, "events": expected}, dst)
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 0, "空字符串与 null 语义不同：双方都是空串则一致")
        # 反向：期望 null、实际空串 → 失败。
        db2 = self.make_db(events)
        r2 = self.run_verify(db2, str(EXPECTED_JSON))
        self.assertEqual(r2.returncode, 1)
        problem = json.loads(r2.stdout)["problems"][0]
        self.assertEqual((problem["field"], problem["expected"], problem["actual"]),
                         ("event_cwd", None, ""))

    def test_event_cwd_literal_null_string_is_not_null(self):
        events = expected_events()
        events[0]["event_cwd"] = "NULL"
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        problem = json.loads(r.stdout)["problems"][0]
        self.assertEqual((problem["field"], problem["expected"], problem["actual"]),
                         ("event_cwd", None, "NULL"))

    def test_non_null_event_cwd_lost_is_detected(self):
        events = expected_events()
        events[1]["event_cwd"] = None
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        problem = json.loads(r.stdout)["problems"][0]
        self.assertEqual((problem["field"], problem["identity"][2]),
                         ("event_cwd", "m2"))

    def test_token_bucket_changes_are_detected_independently(self):
        for field in ("input_tokens", "output_tokens", "cache_write_tokens",
                      "cache_read_tokens"):
            with self.subTest(field=field):
                events = expected_events()
                events[0][field] += 1
                r = self.run_verify(self.make_db(events), str(self.expected_path))
                self.assertEqual(r.returncode, 1)
                problem = json.loads(r.stdout)["problems"][0]
                self.assertEqual((problem["field"], problem["identity"][2]),
                                 (field, "m1"))

    def test_model_raw_change_is_detected(self):
        events = expected_events()
        events[0]["model_raw"] = "claude-opus-5-5"
        r = self.run_verify(self.make_db(events), str(self.expected_path))
        self.assertEqual(r.returncode, 1)
        self.assertEqual(json.loads(r.stdout)["problems"][0]["field"], "model_raw")

    # ---------- 读取 / 参数 / 期望文件错误（退出码 2） ----------

    def test_missing_db_is_exit_2(self):
        r = self.run_verify(str(self.dir / "no-such.db"), str(self.expected_path))
        self.assertEqual(r.returncode, 2)

    def test_missing_table_is_exit_2(self):
        db = str(self.dir / "empty.db")
        con = sqlite3.connect(db)
        con.execute("CREATE TABLE other(x)")
        con.commit()
        con.close()
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 2)

    def test_missing_column_is_exit_2(self):
        db = str(self.dir / "old-schema.db")
        con = sqlite3.connect(db)
        con.executescript(SCHEMA.replace("    project_key TEXT,\n", ""))
        con.commit()
        con.close()
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 2)

    def test_non_numeric_token_text_is_exit_2(self):
        events = expected_events()
        db = self.make_db(events)
        con = sqlite3.connect(db)
        con.execute("UPDATE usage_events SET input_tokens = 'not-a-number' "
                    "WHERE record_id = 'm1'")
        con.commit()
        con.close()
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 2)

    def test_corrupted_expected_file_is_exit_2(self):
        db = self.make_db(expected_events())
        bad = self.dir / "bad.json"
        bad.write_text("{ not json", encoding="utf-8")
        r = self.run_verify(db, str(bad))
        self.assertEqual(r.returncode, 2)

    # ---------- WB02：新 app 白名单 ----------

    def test_workbuddy_app_values_accepted(self):
        # 两版 WorkBuddy 的期望事件可写入、可校验通过（白名单扩展）。
        events = expected_events()
        wb = base_event("wb-1", "wb-sess-1", "C:/work/wb", "C:/work/wb", None, 10, 5)
        wb["app"] = "workbuddy"
        wba = base_event("wba-1", "wba-sess-1", "C:/work/wba", "C:/work/wba", None, 7, 3)
        wba["app"] = "workbuddy-ai"
        payload = {"schema_version": 1, "events": events + [wb, wba]}
        self.expected_path.write_text(json.dumps(payload), encoding="utf-8")
        db = self.make_db(payload["events"])
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        result = json.loads(r.stdout)
        self.assertTrue(result["ok"])
        self.assertEqual(result["summary"]["requests"], 5)

    def test_unknown_app_still_rejected(self):
        # 未知 app 的拒绝校验保留（不能因扩展白名单而放开）。
        events = expected_events()
        rogue = base_event("x-1", "x-sess", "C:/work/x", "C:/work/x", None, 1, 1)
        rogue["app"] = "mystery-app"
        payload = {"schema_version": 1, "events": events + [rogue]}
        self.expected_path.write_text(json.dumps(payload), encoding="utf-8")
        db = self.make_db(payload["events"])
        r = self.run_verify(db, str(self.expected_path))
        self.assertEqual(r.returncode, 2, "未知 app 必须在期望文件校验阶段拒绝")

    def test_wrong_expected_schema_version_is_exit_2(self):
        db = self.make_db(expected_events())
        bad = self.dir / "wrong-version.json"
        payload = json.loads(json.dumps(expected_events()))
        bad.write_text(json.dumps({"schema_version": 99, "events": payload}),
                       encoding="utf-8")
        r = self.run_verify(db, str(bad))
        self.assertEqual(r.returncode, 2)

    def test_negative_expected_token_is_exit_2(self):
        db = self.make_db(expected_events())
        bad = self.dir / "negative.json"
        payload = json.loads(json.dumps(expected_events()))
        payload[0]["input_tokens"] = -1
        bad.write_text(json.dumps({"schema_version": 1, "events": payload}),
                       encoding="utf-8")
        r = self.run_verify(db, str(bad))
        self.assertEqual(r.returncode, 2)


class SummaryModesCase(unittest.TestCase):
    """--records / --cwd / --projects 与默认汇总模式（C-P3 修复的回归）。

    - NULL 与空串必须区分显示（验收断言依赖该语义）；
    - 表检查**之后**的模式查询遇损坏库同样退出 2，不打 traceback；
    - 重构后各模式互不短路（records 分支的 return 不得吞掉其它模式）。
    """

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_cli(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT), *args],
            capture_output=True, text=True, encoding="utf-8", check=False,
        )

    def make_db(self, rows: list) -> str:
        db = str(self.dir / "mode.db")
        con = sqlite3.connect(db)
        con.executescript(SCHEMA)
        for i, (project, sic, ecwd) in enumerate(rows):
            con.execute(
                "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw, "
                "model_identity, identity_revision, session_id, record_id, project_key, "
                "session_initial_cwd, event_cwd, input_tokens, output_tokens, "
                "cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc, "
                "observed_at_seconds, observed_at_nanos, parser_revision) "
                "VALUES(?, 'claude-code', 1784307600, 0, 'm', 'mid', 1, 's', ?, ?, ?, ?, "
                "'10', '20', '0', '0', 1, 'x', 1784307600, 0, 1)",
                (f"k-{i}", f"r{i}", project, sic, ecwd),
            )
        con.commit()
        con.close()
        return db

    def test_records_distinguish_null_and_empty_string(self):
        db = self.make_db([("proj-a", None, ""), ("proj-b", "/w", "/e")])
        r = self.run_cli(db, "--records")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(
            r.stdout.splitlines(),
            [
                "record_id=r0|project=proj-a|session_initial_cwd=NULL"
                "|event_cwd=|input=10|output=20",
                "record_id=r1|project=proj-b|session_initial_cwd=/w"
                "|event_cwd=/e|input=10|output=20",
            ],
        )

    def test_cwd_and_projects_modes(self):
        db = self.make_db([("p-b", "/w1", None), ("p-a", "/w2", "/e")])
        self.assertEqual(
            self.run_cli(db, "--cwd").stdout.splitlines(),
            ["session_initial_cwd=/w1", "session_initial_cwd=/w2",
             "event_cwd_nonnull=1"],
        )
        self.assertEqual(
            self.run_cli(db, "--projects").stdout.splitlines(), ["p-a", "p-b"]
        )

    def test_default_summary_mode_prints_counts(self):
        db = self.make_db([("p", "/w", "/e"), ("p", "/w", None)])
        r = self.run_cli(db)
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout.strip(), "2 20 40 1")

    def test_default_mode_sums_exact_at_u64_scale(self):
        # SQLite SUM 对 TEXT 列在 u64 规模会隐式转 REAL 丢精度——
        # 受检解析后按 Python int 累加必须逐位精确（E-P3-1）。
        db = str(self.dir / "big.db")
        con = sqlite3.connect(db)
        con.executescript(SCHEMA)
        for i, value in enumerate(["18446744073709551615", "1"]):
            con.execute(
                "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw, "
                "model_identity, identity_revision, session_id, record_id, project_key, "
                "session_initial_cwd, event_cwd, input_tokens, output_tokens, "
                "cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc, "
                "observed_at_seconds, observed_at_nanos, parser_revision) "
                "VALUES(?, 'claude-code', 1784307600, 0, 'm', 'mid', 1, 's', ?, 'p', "
                "'/w', '/e', ?, '0', '0', '0', 1, 'x', 1784307600, 0, 1)",
                (f"k-{i}", f"r{i}", value),
            )
        con.commit()
        con.close()
        r = self.run_cli(db)
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout.strip(), "2 18446744073709551616 0 1")

    def test_default_mode_rejects_bad_token_text_with_exit_2(self):
        # Rust 侧持久化拒绝非受检十进制——汇总读数不得把坏值静默计 0。
        db = self.make_db([("p", "/w", "/e")])
        con = sqlite3.connect(db)
        con.execute("UPDATE usage_events SET input_tokens = 'abc'")
        con.commit()
        con.close()
        r = self.run_cli(db)
        self.assertEqual(r.returncode, 2)
        self.assertIn("input_tokens", r.stderr)

    def test_absent_db_default_prints_absent_and_modes_print_nothing(self):
        absent = str(self.dir / "no-such.db")
        default = self.run_cli(absent)
        self.assertEqual(default.returncode, 0)
        self.assertEqual(default.stdout.strip(), "absent")
        for mode in ("--records", "--cwd", "--projects"):
            r = self.run_cli(absent, mode)
            self.assertEqual(r.returncode, 0, mode)
            self.assertEqual(r.stdout, "", f"{mode} 对缺库不得输出空行")

    def test_mode_query_on_corrupted_db_is_exit_2(self):
        # sqlite_master 在第 1 页完好，表内容在第 2 页——清零第 2 页让
        # "表存在但查询失败"，验证模式查询也走 DatabaseError → 退出 2。
        db = self.make_db([("p", "/w", "/e")] * 20)
        con = sqlite3.connect(db)
        con.execute("VACUUM")
        con.close()
        with open(db, "r+b") as fh:
            fh.seek(4096)
            fh.write(b"\x00" * 8192)
        r = self.run_cli(db, "--records")
        self.assertEqual(r.returncode, 2)
        self.assertIn("无法按 SQLite 打开", r.stderr)


class Utf8OutputCase(unittest.TestCase):
    """CLI 输出必须是 UTF-8，不能随 Windows 管道的系统编码改变。"""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name) / "中文路径"
        self.dir.mkdir()
        self.events = expected_events()
        for event in self.events:
            event["model_raw"] = "模型-测试"
            for field in ("project_key", "session_initial_cwd", "event_cwd"):
                if event[field] is not None:
                    event[field] = event[field].replace("/work/", "/工作/")
        self.expected_path = self.dir / "期望.json"
        self.expected_path.write_text(
            json.dumps({"schema_version": 1, "events": self.events},
                       ensure_ascii=False), encoding="utf-8",
        )

    def make_db(self, name: str) -> str:
        db = str(self.dir / f"{name}.db")
        insert_events(db, self.events)
        return db

    def run_cli(self, *args: str) -> subprocess.CompletedProcess:
        env = os.environ.copy()
        # 显式覆盖调用者的 UTF-8 设置；旧实现即使在现代 Python 上也会红。
        # capture_output 读取原始字节，随后严格按 CLI 的 UTF-8 契约解码。
        env.update(PYTHONIOENCODING="cp1252", PYTHONUTF8="0")
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT), *args],
            capture_output=True, env=env, check=False,
        )

    def test_manifest_emits_utf8_with_unicode_paths_and_model(self):
        r = self.run_cli(self.make_db("valid"), "--verify-manifest",
                         str(self.expected_path))
        self.assertEqual(r.returncode, 0, r.stderr.decode("utf-8"))
        payload = json.loads(r.stdout.decode("utf-8"))
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["expected_file"], str(self.expected_path))
        self.assertEqual(payload["summary"]["requests"], 3)

    def test_invalid_token_preserves_exit_2_and_utf8_diagnostics(self):
        for field in ("input_tokens", "output_tokens",
                      "cache_write_tokens", "cache_read_tokens"):
            with self.subTest(field=field):
                db = self.make_db(field)
                con = sqlite3.connect(db)
                try:
                    con.execute(f"UPDATE usage_events SET {field} = ? "
                                "WHERE record_id = ?", ("not-a-number", "m1"))
                    con.commit()
                finally:
                    con.close()
                r = self.run_cli(db, "--verify-manifest", str(self.expected_path))
                self.assertEqual(r.returncode, 2, r.stderr.decode("utf-8"))
                payload = json.loads(r.stdout.decode("utf-8"))
                self.assertFalse(payload["ok"])
                problem = next(p for p in payload["problems"] if p["field"] == field)
                self.assertEqual(problem["expected"], "非负十进制")
                self.assertEqual(problem["actual"], "not-a-number")

    def test_corrupt_db_preserves_exit_2_and_utf8_stderr(self):
        db = self.dir / "损坏.db"
        db.write_text("not a SQLite database", encoding="utf-8")
        for mode in ((), ("--projects",), ("--cwd",), ("--records",)):
            with self.subTest(mode=mode):
                r = self.run_cli(str(db), *mode)
                self.assertEqual(r.returncode, 2)
                self.assertEqual(r.stdout, b"")
                diagnostic = r.stderr.decode("utf-8")
                self.assertIn("无法按 SQLite 打开", diagnostic)
                self.assertIn(str(db), diagnostic)
                self.assertNotIn("Traceback", diagnostic)

    def test_line_modes_emit_utf8(self):
        db = self.make_db("line-modes")
        for mode in ("--projects", "--cwd", "--records"):
            with self.subTest(mode=mode):
                r = self.run_cli(db, mode)
                self.assertEqual(r.returncode, 0, r.stderr.decode("utf-8"))
                self.assertIn("C:/工作/", r.stdout.decode("utf-8"))


class GeneratorContractCase(unittest.TestCase):
    """生成器读回契约：旧缓存输入必须与期望文件逐条一致。"""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.cache = Path(self.tmp.name) / "cache.db"

    def test_legacy_cache_matches_expected_manifest(self):
        r = subprocess.run(
            [sys.executable, "-B", str(MAKE_LEGACY), str(self.cache)],
            capture_output=True, text=True, encoding="utf-8",
        )
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

        con = sqlite3.connect(self.cache)
        files = {row[0]: row[1] for row in con.execute("SELECT id, path FROM files")}
        rows = con.execute(
            "SELECT file_id, session_id, record_id, project, session_initial_cwd, "
            "event_cwd, model, input, output, cache_write, cache_read "
            "FROM events ORDER BY rowid"
        ).fetchall()
        con.close()

        with open(EXPECTED_JSON, encoding="utf-8") as fh:
            expected = {e["record_id"]: e for e in json.load(fh)["events"]}

        self.assertEqual(len(rows), len(expected), "旧缓存事件数与期望文件一致")
        seen = set()
        total_in = total_out = 0
        for file_id, session_id, record_id, project, sic, ecwd, model, \
                inp, outp, cw, cr in rows:
            self.assertIn(file_id, files)
            ev = expected[record_id]
            seen.add(record_id)
            self.assertEqual(session_id, ev["session_id"])
            self.assertEqual(project, ev["project_key"])
            self.assertEqual(sic, ev["session_initial_cwd"])
            self.assertEqual(ecwd, ev["event_cwd"])
            self.assertEqual(model, ev["model_raw"])
            self.assertEqual(int(inp), ev["input_tokens"])
            self.assertEqual(int(outp), ev["output_tokens"])
            self.assertEqual(int(cw), ev["cache_write_tokens"])
            self.assertEqual(int(cr), ev["cache_read_tokens"])
            total_in += int(inp)
            total_out += int(outp)
        self.assertEqual(seen, set(expected))
        self.assertEqual((total_in, total_out), (180, 35))
        self.assertEqual(len({e["project_key"] for e in expected.values()}), 2)


if __name__ == "__main__":
    unittest.main()
