"""AC02: synthetic CCS inputs never need a user's database."""

import hashlib
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest


class MakeCcsFixtureTests(unittest.TestCase):
    def test_fixture_contains_overlap_unique_request_and_daily_rollup(self):
        with tempfile.TemporaryDirectory(prefix="tokenscope-ccs-fixture-") as root:
            target = Path(root) / "sources" / "cc-switch" / "cc-switch.db"
            output = subprocess.run(
                [sys.executable, "-B", str(Path(__file__).with_name("make_ccs_fixture.py")), str(target)],
                capture_output=True, text=True, encoding="utf-8", check=False,
            )
            self.assertEqual(output.returncode, 0, output.stderr)
            evidence = json.loads(output.stdout)
            before = hashlib.sha256(target.read_bytes()).hexdigest()
            self.assertEqual(evidence["sha256"], before)
            with closing(sqlite3.connect(target.as_uri() + "?mode=ro", uri=True)) as database:
                self.assertEqual(database.execute("PRAGMA user_version").fetchone()[0], 20)
                rows = database.execute("SELECT request_id, session_id, data_source FROM proxy_request_logs ORDER BY request_id").fetchall()
                self.assertEqual(rows, [
                    ("ccs-only-request", "ccs-only-session", "proxy"),
                    ("session:msg-acceptance-1", "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "claude_session"),
                ])
                self.assertEqual(database.execute("SELECT date, request_count FROM usage_daily_rollups").fetchall(), [("2026-10-05", 3)])
                with self.assertRaises(sqlite3.OperationalError):
                    database.execute("DELETE FROM proxy_request_logs")
            self.assertEqual(hashlib.sha256(target.read_bytes()).hexdigest(), before)
            repeat = subprocess.run(
                [sys.executable, "-B", str(Path(__file__).with_name("make_ccs_fixture.py")), str(target)],
                capture_output=True, text=True, encoding="utf-8", check=False,
            )
            self.assertNotEqual(repeat.returncode, 0, "已存在的数据库不得覆盖")
            self.assertEqual(hashlib.sha256(target.read_bytes()).hexdigest(), before)

    def test_stage_markers_on_stderr_localize_hangs(self):
        # 2026-10-23 取证装配：内部阶段标记走 stderr（stdout 保持纯 JSON，
        # 既有 json.loads 不受影响），prepare 超时时由 spawnSyncBounded 的
        # 错误尾部保留——可区分 python 启动、db 创建、schema、写入与提交
        # 各步，不再只剩外层 ETIMEDOUT。
        with tempfile.TemporaryDirectory(prefix="tokenscope-ccs-fixture-markers-") as root:
            target = Path(root) / "cc-switch.db"
            output = subprocess.run(
                [sys.executable, "-B", str(Path(__file__).with_name("make_ccs_fixture.py")), str(target)],
                capture_output=True, text=True, encoding="utf-8", check=False,
            )
            self.assertEqual(output.returncode, 0, output.stderr)
            markers = [line for line in output.stderr.splitlines() if line.startswith("[ccs-fixture]")]
            self.assertEqual(markers, [
                "[ccs-fixture] interpreter-ready",
                "[ccs-fixture] fixture-start",
                "[ccs-fixture] db-created",
                "[ccs-fixture] schema-done",
                "[ccs-fixture] rows-done",
                "[ccs-fixture] commit-done",
                "[ccs-fixture] done",
            ])


if __name__ == "__main__":
    unittest.main()
