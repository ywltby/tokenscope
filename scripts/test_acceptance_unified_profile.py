"""AC02: the unified profile is synthetic and complete before any GUI launch."""

from contextlib import closing
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import tomllib
import unittest


class UnifiedProfileTests(unittest.TestCase):
    def test_profile_covers_identity_boundaries_schedules_and_ccs(self):
        with tempfile.TemporaryDirectory(prefix="tokenscope-unified-profile-") as temporary:
            root = Path(temporary) / "isolated"
            script = Path(__file__).with_name("prepare-native-acceptance.ps1")
            output = subprocess.run(
                ["pwsh", "-NoProfile", "-File", str(script), "-Root", str(root), "-Profile", "unified"],
                capture_output=True, check=False, timeout=30,
            )
            # 重定向时 pwsh 按控制台代码页输出（本机 936），这里只取诊断文本。
            self.assertEqual(output.returncode, 0,
                             (output.stdout + output.stderr).decode("utf-8", errors="replace"))
            manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["profile"], "unified")
            events = [json.loads(line) for file in (root / "sources/claude").rglob("*.jsonl")
                      if not file.name.startswith("broken-") for line in file.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(len(events), 8)
            self.assertTrue({272000, 272001}.issubset({row["message"]["usage"]["input_tokens"] for row in events}))
            self.assertIn("claude-opus-5-5", {row["message"]["model"] for row in events})
            codex = next((root / "sources/codex").rglob("*.jsonl")).read_text(encoding="utf-8")
            self.assertIn("Claude_Opus_5.5", codex)
            pricing = tomllib.loads((root / "tokenscope/pricing.toml").read_text(encoding="utf-8"))
            self.assertTrue(any(row.get("segment") for row in pricing["model"]))
            self.assertTrue(any(row.get("schedule") for row in pricing["model"]))
            self.assertEqual(pricing["model_policy"][0]["cache_read"], "same_as_input")
            self.assertTrue(any("output" not in row for row in pricing["model"]))
            with closing(sqlite3.connect((root / "sources/cc-switch/cc-switch.db").as_uri() + "?mode=ro", uri=True)) as database:
                self.assertEqual(database.execute("SELECT count(*) FROM proxy_request_logs").fetchone()[0], 2)
                self.assertEqual(database.execute("SELECT count(*) FROM usage_daily_rollups").fetchone()[0], 1)


if __name__ == "__main__":
    unittest.main()
