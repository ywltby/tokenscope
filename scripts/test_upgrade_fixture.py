"""真实旧包夹具契约：只造来源日志；旧缓存读数在合成临时库中验证。"""

import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import tomllib
import unittest

import upgrade_fixture as fixture
from inspect_history import load_expected


class UpgradeFixtureCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-upgrade-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.data = self.root / "profile/.tokenscope"
        self.sources = self.root / "sources"

    def make_legacy(self):
        self.data.mkdir(parents=True, exist_ok=True)
        path = self.data / "cache.db"
        with closing(sqlite3.connect(path)) as con, con:
            con.executescript("""
                CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO meta VALUES('schema_version','6');
                CREATE TABLE files(id INTEGER PRIMARY KEY, agent TEXT NOT NULL);
                INSERT INTO files VALUES(1,'claude-code');
                INSERT INTO files VALUES(2,'claude-code');
                CREATE TABLE events(file_id INTEGER, session_id TEXT, record_id TEXT,
                    model TEXT, project TEXT, input INTEGER, output INTEGER,
                    cache_write INTEGER, cache_read INTEGER);
            """)
            for row in load_expected(fixture.EXPECTED):
                con.execute("INSERT INTO events VALUES(?,?,?,?,?,?,?,?,?)",
                            (1 if row['project_key'] == 'alpha' else 2,
                             row['session_id'], row['record_id'], row['model_raw'], row['project_key'],
                             row['input_tokens'], row['output_tokens'],
                             row['cache_write_tokens'], row['cache_read_tokens']))
        return path

    def test_prepare_writes_only_supported_settings_prices_and_source_logs(self):
        fixture.prepare(self.data, self.sources)
        self.assertFalse((self.data / "cache.db").exists())
        self.assertFalse((self.data / "history.db").exists())
        settings = tomllib.loads((self.data / "settings.toml").read_text(encoding="utf-8"))
        self.assertNotIn("privacy_policy_accepted", settings)
        self.assertFalse(settings["price_auto_sync"])
        self.assertEqual(settings["close_action"], "quit")
        self.assertTrue((self.data / "pricing.toml").stat().st_size)
        rows = []
        for path in sorted(self.sources.rglob("*.jsonl")):
            for line in path.read_text(encoding="utf-8").splitlines():
                value = json.loads(line)
                self.assertNotIn("cwd", value)
                rows.append((value["sessionId"], value["message"]["id"], value["message"]["usage"]))
        self.assertEqual(len(rows), 3)
        self.assertEqual(sum(r[2]["cache_creation_input_tokens"] for r in rows), 12)
        self.assertEqual(sum(r[2]["cache_read_input_tokens"] for r in rows), 20)

    def test_preparation_refuses_existing_database_and_consent_preserves_old_settings(self):
        path = self.make_legacy()
        before = path.read_bytes()
        with self.assertRaises(ValueError):
            fixture.prepare(self.data, self.sources)
        self.assertEqual(path.read_bytes(), before)
        path.unlink()
        fixture.prepare(self.data, self.sources)
        old = tomllib.loads((self.data / "settings.toml").read_text(encoding="utf-8"))
        fixture.grant_test_consent(self.data)
        current = tomllib.loads((self.data / "settings.toml").read_text(encoding="utf-8"))
        self.assertTrue(current.pop("privacy_policy_accepted"))
        self.assertEqual(current, old)

    def test_real_legacy_schema_readonly_facts_include_all_four_buckets(self):
        path = self.make_legacy()
        before = path.read_bytes()
        result = fixture.verify_legacy(path)
        self.assertTrue(result["ok"])
        self.assertEqual(result["summary"], {"requests": 3, "input": 180, "output": 35,
                                           "cache_write": 12, "cache_read": 20, "projects": 2})
        self.assertEqual(path.read_bytes(), before)

    def test_after_upgrade_preserves_each_old_setting_and_requires_explicit_consent(self):
        fixture.prepare(self.data, self.sources)
        original = self.root / "original.toml"
        original.write_bytes((self.data / "settings.toml").read_bytes())
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original)
        fixture.grant_test_consent(self.data)
        self.assertTrue(fixture.verify_settings(self.data, original)["ok"])
        path = self.data / "settings.toml"
        path.write_text(path.read_text(encoding="utf-8").replace('"quit"', '"minimize"'),
                        encoding="utf-8")
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original)

    def test_wrong_schema_unknown_identity_extra_record_and_bucket_changes_fail(self):
        mutations = ("UPDATE meta SET value='10'", "UPDATE events SET input=input+1 WHERE record_id='m1'",
                     "UPDATE events SET output=output+1 WHERE record_id='m1'",
                     "UPDATE events SET cache_write=0 WHERE record_id='m1'",
                     "UPDATE events SET cache_read=0 WHERE record_id='m1'",
                     "UPDATE events SET record_id='' WHERE record_id='m1'",
                     "INSERT INTO events SELECT * FROM events WHERE record_id='m1'",
                     "UPDATE events SET project='other' WHERE record_id='m1'",
                     "DELETE FROM events WHERE record_id='m1'")
        for sql in mutations:
            with self.subTest(sql=sql):
                path = self.make_legacy()
                with closing(sqlite3.connect(path)) as con, con:
                    con.execute(sql)
                try:
                    self.assertFalse(fixture.verify_legacy(path)["ok"])
                except ValueError:
                    pass
                path.unlink()


if __name__ == "__main__":
    unittest.main()
