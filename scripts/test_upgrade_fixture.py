"""真实旧包夹具契约：只造来源日志；旧缓存读数在合成临时库中验证。"""

import json
from contextlib import closing
from pathlib import Path
import re
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

    def test_history_profile_seeds_consent_and_legacy_profile_refuses_unknown(self):
        fixture.prepare(self.data, self.sources, "history")
        settings = tomllib.loads((self.data / "settings.toml").read_text(encoding="utf-8"))
        self.assertTrue(settings["privacy_policy_accepted"])
        self.assertEqual(settings["close_action"], "quit")
        with self.assertRaises(ValueError):
            fixture.prepare(self.data, self.sources, "猜的")
        other = self.root / "other/.tokenscope"
        with self.assertRaises(ValueError):
            fixture.prepare(other, self.sources, "history.profile")

    def test_history_prepare_carries_baseline_revision_when_requested(self):
        # 0.6.0 是首个设置格式 3 基线：旧版阶段必须按该基线自己认识的修订号
        # 预置同意，否则 StaleRevision 门不开放（CI 40226439492 单跳旧阶段
        # 等汇总超时的红先来源）。不带修订号的调用保持布尔形态。
        fixture.prepare(self.data, self.sources, "history", consent_revision=2)
        settings = tomllib.loads((self.data / "settings.toml").read_text(encoding="utf-8"))
        self.assertEqual(settings["privacy_policy_revision"], 2)
        self.assertNotIn("user_agreement_accepted", settings,
                         "0.6.0 的历史形态不能被改写进协议同意字段")
        other = self.root / "plain/.tokenscope"
        fixture.prepare(other, self.sources, "history")
        plain = tomllib.loads((other / "settings.toml").read_text(encoding="utf-8"))
        self.assertNotIn("privacy_policy_revision", plain)

    def test_consent_writes_revision_when_requested(self):
        # WB05：需要当前修订授权的跳，授予/确保同意必须与修订号同一次写入；
        # 不带修订号的旧版跳保持布尔形态（受测版本不支持该字段）。
        import tomllib

        def fresh_data(tag: str):
            data = self.root / f"profile-{tag}/.tokenscope"
            data.mkdir(parents=True, exist_ok=True)
            (data / "settings.toml").write_text("price_auto_sync = false\n",
                                                encoding="utf-8")
            return data

        # 授予同意：true 与修订号同一次写入
        data = fresh_data("grant")
        fixture.grant_test_consent(data, revision=2)
        parsed = tomllib.loads((data / "settings.toml").read_text(encoding="utf-8"))
        self.assertTrue(parsed["privacy_policy_accepted"])
        self.assertEqual(parsed["privacy_policy_revision"], 2)
        # ensure 已有当前修订同意时幂等（不重写）
        result = fixture.ensure_test_consent(data, revision=2)
        self.assertFalse(result["test_consent_added"])
        # ensure 无同意的目录：带修订号写入
        data = fresh_data("ensure")
        result = fixture.ensure_test_consent(data, revision=2)
        self.assertTrue(result["test_consent_added"])
        parsed = tomllib.loads((data / "settings.toml").read_text(encoding="utf-8"))
        self.assertEqual(parsed["privacy_policy_revision"], 2)
        # 已有旧同意（true 无修订号）+ 需要当前修订：补写修订号（闸门验证
        # 之后的显式测试授权，与真实重新同意后的落盘等价）
        data = fresh_data("upgrade")
        fixture.grant_test_consent(data)
        result = fixture.ensure_test_consent(data, revision=2)
        self.assertTrue(result.get("test_consent_upgraded"))
        parsed = tomllib.loads((data / "settings.toml").read_text(encoding="utf-8"))
        self.assertTrue(parsed["privacy_policy_accepted"])
        self.assertEqual(parsed["privacy_policy_revision"], 2)
        # 已是当前修订：不再改写
        result = fixture.ensure_test_consent(data, revision=2)
        self.assertFalse(result.get("test_consent_upgraded", False))
        self.assertFalse(result.get("test_consent_added"))
        # 已有更低修订号（privacy_policy_revision = 1）+ 需要当前修订：
        # 先删旧行再插入——写出重复键会让 tomllib 拒绝解析整个文件。
        data = fresh_data("raise")
        (data / "settings.toml").write_text(
            "privacy_policy_accepted = true\nprivacy_policy_revision = 1\n",
            encoding="utf-8")
        result = fixture.ensure_test_consent(data, revision=2)
        self.assertTrue(result.get("test_consent_upgraded"))
        text = (data / "settings.toml").read_text(encoding="utf-8")
        self.assertEqual(text.count("privacy_policy_revision"), 1, text)
        parsed = tomllib.loads(text)
        self.assertTrue(parsed["privacy_policy_accepted"])
        self.assertEqual(parsed["privacy_policy_revision"], 2)
        # 不带 revision 的旧语义：只写布尔（受测历史版本不认识修订号字段）
        data = fresh_data("legacy")
        fixture.grant_test_consent(data)
        text = (data / "settings.toml").read_text(encoding="utf-8")
        self.assertIn("privacy_policy_accepted = true", text)
        self.assertNotIn("privacy_policy_revision", text)

    def test_ensure_consent_is_idempotent_after_the_first_write(self):
        fixture.prepare(self.data, self.sources)
        first = fixture.ensure_test_consent(self.data)
        self.assertTrue(first["test_consent_added"])
        text = (self.data / "settings.toml").read_text(encoding="utf-8")
        second = fixture.ensure_test_consent(self.data)
        self.assertFalse(second["test_consent_added"])
        self.assertEqual((self.data / "settings.toml").read_text(encoding="utf-8"), text)
        with self.assertRaises(ValueError):
            fixture.grant_test_consent(self.data)

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
            fixture.verify_settings(self.data, original, 1, 1)
        fixture.grant_test_consent(self.data)
        self.assertTrue(fixture.verify_settings(self.data, original, 1, 1)["ok"])
        path = self.data / "settings.toml"
        with self.assertRaises(ValueError):
            # 1->2 已声明（v1 dir 迁移）；真正未声明的组合仍然拒绝。
            fixture.verify_settings(self.data, original, 1, 3)
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 2, 1)
        path.write_text(path.read_text(encoding="utf-8").replace('"quit"', '"minimize"'),
                        encoding="utf-8")
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 1, 1)

    def test_upgrade_consent_transition_is_verified_against_current_revision(self):
        # 0.6.0 的真实落盘形态：隐私政策同意 + 修订号 2，没有协议同意。
        # 升级后测试授权按当前共同同意修订 3 重写四项——这是合法的同意
        # 迁移，不能被「旧设置未保留」误拒（CI 40670128332）。
        fixture.prepare(self.data, self.sources, "history", consent_revision=2)
        original = self.root / "original-0.6.0.toml"
        original.write_bytes((self.data / "settings.toml").read_bytes())
        old = tomllib.loads(original.read_text(encoding="utf-8"))
        self.assertEqual(old["privacy_policy_revision"], 2)
        self.assertNotIn("user_agreement_accepted", old, "0.6.0 不写协议同意字段")
        fixture.ensure_test_consent(self.data, revision=3, agreement=True)
        current = tomllib.loads((self.data / "settings.toml").read_text(encoding="utf-8"))
        self.assertEqual(current["privacy_policy_revision"], 3)
        self.assertTrue(current["user_agreement_accepted"])
        self.assertEqual(current["user_agreement_revision"], 3)
        self.assertTrue(fixture.verify_settings(
            self.data, original, 3, 3, consent_revision=3)["ok"])
        # 缺协议同意：即使隐私修订已是当前版本也不得放行。
        path = self.data / "settings.toml"
        agreed = path.read_text(encoding="utf-8")
        without_agreement = re.sub(
            r"(?m)^[ \t]*user_agreement_(accepted|revision)[ \t]*=.*\r?\n?", "", agreed)
        path.write_text(without_agreement, encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "用户许可协议"):
            fixture.verify_settings(self.data, original, 3, 3, consent_revision=3)
        # 协议同意为 false / 修订号不匹配 / 隐私修订低于本版：逐项拒绝。
        for broken in (
            without_agreement + "user_agreement_accepted = false\nuser_agreement_revision = 3\n",
            without_agreement + "user_agreement_accepted = true\nuser_agreement_revision = 2\n",
            agreed.replace("privacy_policy_revision = 3", "privacy_policy_revision = 2"),
        ):
            with self.subTest(broken=broken):
                path.write_text(broken, encoding="utf-8")
                with self.assertRaises(ValueError):
                    fixture.verify_settings(self.data, original, 3, 3, consent_revision=3)
        # 非同意字段仍逐项核对：用户设置被改动必须失败。
        path.write_text(agreed.replace('close_action = "quit"', 'close_action = "minimize"'),
                        encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "close_action"):
            fixture.verify_settings(self.data, original, 3, 3, consent_revision=3)

    def test_agreement_is_only_written_for_explicit_current_consent(self):
        # 历史钩子（prepare / grant）保持旧版本真实形态；只有为当前版本
        # 模拟重新同意（agreement=True）才写协议两项，且一次写入不重复键。
        fixture.prepare(self.data, self.sources, "history", consent_revision=2)
        path = self.data / "settings.toml"
        text = path.read_text(encoding="utf-8")
        self.assertNotIn("user_agreement", text)
        result = fixture.ensure_test_consent(self.data, revision=3, agreement=True)
        self.assertTrue(result.get("test_consent_upgraded"))
        upgraded = path.read_text(encoding="utf-8")
        for key in ("privacy_policy_accepted", "privacy_policy_revision",
                    "user_agreement_accepted", "user_agreement_revision"):
            self.assertEqual(upgraded.count(key), 1, upgraded)
        parsed = tomllib.loads(upgraded)
        self.assertEqual(parsed["privacy_policy_revision"], 3)
        self.assertEqual(parsed["user_agreement_revision"], 3)
        again = fixture.ensure_test_consent(self.data, revision=3, agreement=True)
        self.assertFalse(again.get("test_consent_added"))
        self.assertFalse(again.get("test_consent_upgraded"))
        self.assertEqual(path.read_text(encoding="utf-8"), upgraded)
        with self.assertRaises(ValueError):
            fixture.ensure_test_consent(self.data, revision=None, agreement=True)

    def test_nested_field_changes_outside_the_migration_table_fail(self):
        fixture.prepare(self.data, self.sources)
        original = self.root / "original.toml"
        original.write_bytes((self.data / "settings.toml").read_bytes())
        fixture.grant_test_consent(self.data)
        path = self.data / "settings.toml"
        for old, new in (("enabled = true", "enabled = false"),
                         ('dir = "', 'dir = "moved-'),
                         ("price_auto_sync = false", "price_auto_sync = true")):
            with self.subTest(old=old):
                text = path.read_text(encoding="utf-8")
                path.write_text(text.replace(old, new, 1), encoding="utf-8")
                with self.assertRaises(ValueError):
                    fixture.verify_settings(self.data, original, 1, 1)
                path.write_text(text, encoding="utf-8")

    def test_declared_migration_rules_rename_and_default_fields(self):
        fixture.prepare(self.data, self.sources)
        original = self.root / "original.toml"
        original.write_text('[sources.claude]\nenabled = true\ndir = "/old"\n', encoding="utf-8")
        migrations = self.root / "migrations.json"
        migrations.write_text(json.dumps({"schema_version": 1, "migrations": {
            "1->2": {"operations": [{"kind": "rename", "from": "sources.claude.dir",
                                     "to": "sources.claude.additional_paths"},
                                    {"kind": "default", "path": "sources.claude.enabled",
                                     "value": True}]}}}), encoding="utf-8")
        path = self.data / "settings.toml"
        with_missing_default = ('privacy_policy_accepted = true\n'
                                '[sources.claude]\nadditional_paths = "/old"\n')
        path.write_text(with_missing_default, encoding="utf-8")
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 1, 2, migrations)
        migrated = with_missing_default + "enabled = true\n"
        path.write_text(migrated, encoding="utf-8")
        self.assertTrue(fixture.verify_settings(self.data, original, 1, 2, migrations)["ok"])
        path.write_text(migrated.replace('"/old"', '"/elsewhere"'), encoding="utf-8")
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 1, 2, migrations)

    def test_broken_migration_tables_are_rejected(self):
        for migrations in ({"1->2": {"operations": [{"kind": "move", "from": "a"}]}},
                           {"1->2": {"operations": [{"kind": "rename", "from": "a"}]}},
                           {"1->2": {"operations": [{"kind": "default", "path": "a"}]}},
                           {"1->2": {"operations": [{"kind": "source_dir_to_additional_path",
                                                     "agent": "gemini"}]}},
                           {"1->2": {"operations": [{"kind": "source_dir_to_additional_path"}]}},
                           {"1->2": {"operations": []}, "x": {"operations": []}},
                           {"1->2": []}):
            with self.subTest(migrations=migrations), self.assertRaises(ValueError):
                fixture.load_migrations(self.write_migrations(migrations))

    # ── v1 dir → v2 追加项（source_dir_to_additional_path，PR3 S01）──────

    OLD_WITH_DIR = ("price_auto_sync = false\n"
                    "[sources.claude]\nenabled = true\ndir = \"D:/history/claude/projects\"\n")

    def write_current(self, text):
        self.data.mkdir(parents=True, exist_ok=True)
        path = self.data / "settings.toml"
        path.write_text(text, encoding="utf-8")
        return path

    def migrated_claude(self, *, enabled=True, entry=True, entry_enabled=True,
                        auto_discover=True, keep_dir=False):
        lines = ["privacy_policy_accepted = true", "price_auto_sync = false",
                 "[sources.claude]", f"enabled = {'true' if enabled else 'false'}"]
        if keep_dir:
            lines.append('dir = "D:/history/claude/projects"')
        lines.append(f"auto_discover = {'true' if auto_discover else 'false'}")
        lines.append("excluded_auto_paths = []")
        if entry:
            lines.append("[[sources.claude.additional_paths]]")
            lines.append('path = "D:/history/claude/projects"')
            lines.append(f"enabled = {'true' if entry_enabled else 'false'}")
        return "\n".join(lines) + "\n"

    def original_with_dir(self, text=OLD_WITH_DIR):
        path = self.root / "original-v1.toml"
        path.write_text(text, encoding="utf-8")
        return path

    def test_v1_nonempty_dir_migrates_to_additional_entry_with_auto_discover(self):
        self.write_current(self.migrated_claude())
        result = fixture.verify_settings(self.data, self.original_with_dir(), 1, 2)
        self.assertTrue(result["ok"])
        self.assertEqual(result["source_dir"],
                         [{"old_dir": "D:/history/claude/projects",
                           "entry": "D:/history/claude/projects"}])

    def test_v1_dir_migration_rejects_each_broken_shape(self):
        cases = {
            "dir_dropped_without_entry": self.migrated_claude(entry=False),
            "dir_still_present": self.migrated_claude(keep_dir=True),
            "auto_discover_missing": self.migrated_claude(auto_discover=False),
            "entry_enabled_mismatch": self.migrated_claude(entry_enabled=False),
            "wrong_path_kept": self.migrated_claude().replace("D:/history/claude/projects",
                                                              "D:/elsewhere", 1),
        }
        for name, text in cases.items():
            with self.subTest(case=name):
                self.write_current(text)
                with self.assertRaises(ValueError):
                    fixture.verify_settings(self.data, self.original_with_dir(), 1, 2)

    def test_v1_empty_dir_requires_no_empty_path_entry(self):
        original = self.original_with_dir(
            "[sources.claude]\nenabled = true\ndir = \"\"\n")
        self.write_current(self.migrated_claude())
        self.assertTrue(fixture.verify_settings(self.data, original, 1, 2)["ok"])
        self.write_current(self.migrated_claude().replace(
            'path = "D:/history/claude/projects"', 'path = ""'))
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 1, 2)

    def test_v1_disabled_source_keeps_disabled_and_entry_disabled(self):
        original = self.original_with_dir(
            "price_auto_sync = false\n"
            "[sources.claude]\nenabled = false\ndir = \"D:/history/claude/projects\"\n")
        self.write_current(self.migrated_claude(enabled=False, entry_enabled=False))
        self.assertTrue(fixture.verify_settings(self.data, original, 1, 2)["ok"])
        self.write_current(self.migrated_claude(enabled=False, entry_enabled=True))
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 1, 2)

    def test_prepared_v1_settings_migrate_to_additional_entries(self):
        # prepare 写的起点设置本就是 v1 形态（dir 指向夹具来源目录）——升级核对
        # 要求它们变成追加项 + 自动发现；这里模拟新版本完成迁移后的形态，
        # S06/S07 的安装矩阵由真实应用产生同一形态。
        prepared = fixture.prepare(self.data, self.sources)
        posix_dirs = {agent: Path(prepared[agent]).as_posix() for agent in ("claude", "codex")}
        original = self.root / "original-plain.toml"
        original.write_bytes((self.data / "settings.toml").read_bytes())
        fixture.grant_test_consent(self.data)
        migrated = "privacy_policy_accepted = true\nprice_auto_sync = false\nclose_action = \"quit\"\n\n"
        for agent in ("claude", "codex"):
            # prepare 落盘用 as_posix()；核对按文件里的原样字符串比较。
            source_dir = posix_dirs[agent]
            migrated += (
                f"[sources.{agent}]\nenabled = true\nauto_discover = true\n"
                "excluded_auto_paths = []\n"
                f"[[sources.{agent}.additional_paths]]\npath = {json.dumps(source_dir, ensure_ascii=False)}\n"
                "enabled = true\n\n"
            )
        (self.data / "settings.toml").write_text(migrated, encoding="utf-8")
        result = fixture.verify_settings(self.data, original, 1, 2)
        self.assertTrue(result["ok"])
        self.assertEqual(
            result["source_dir"],
            [{"old_dir": posix_dirs["claude"], "entry": posix_dirs["claude"]},
             {"old_dir": posix_dirs["codex"], "entry": posix_dirs["codex"]}])
        # 未迁移的原始形态必须被拒绝（dir 不得原样保留到 v2）。
        self.write_current("privacy_policy_accepted = true\n" + original.read_text(encoding="utf-8"))
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 1, 2)

    def test_v2_settings_survive_identity_2_to_2(self):
        original = self.original_with_dir(
            "price_auto_sync = false\n"
            "[sources.claude]\nenabled = true\nauto_discover = true\n"
            "excluded_auto_paths = []\n"
            "[[sources.claude.additional_paths]]\n"
            'path = "D:/history/claude/projects"\nenabled = true\n')
        self.write_current(original.read_text(encoding="utf-8").replace(
            "price_auto_sync = false",
            "privacy_policy_accepted = true\nprice_auto_sync = false"))
        self.assertTrue(fixture.verify_settings(self.data, original, 2, 2)["ok"])

    def test_real_migration_table_declares_all_required_pairs(self):
        rules = fixture.load_migrations()
        self.assertIn((1, 1), rules)
        self.assertIn((1, 2), rules)
        self.assertIn((2, 2), rules)
        kinds = {op["kind"] for op in rules[(1, 2)]}
        self.assertEqual(kinds, {"source_dir_to_additional_path"})
        # WB04：格式 3（两版 WorkBuddy 来源键）的迁移规则
        self.assertIn((1, 3), rules)
        self.assertIn((2, 3), rules)
        self.assertIn((3, 3), rules)
        kinds_1_3 = {op["kind"] for op in rules[(1, 3)]}
        self.assertEqual(
            kinds_1_3, {"source_dir_to_additional_path", "default"},
            "1->3 必须同时带 v1 dir 迁移与新键默认值")
        kinds_2_3 = {op["kind"] for op in rules[(2, 3)]}
        self.assertEqual(kinds_2_3, {"default"})

    def write_migrations(self, migrations):
        path = self.root / "broken-migrations.json"
        path.write_text(json.dumps({"schema_version": 1, "migrations": migrations}),
                        encoding="utf-8")
        return path

    def test_format_3_default_allows_missing_keys_and_rejects_wrong_values(self):
        # 缺失键与默认值在读取语义上等价（缺失 = 启用 + 自动发现），校验允许缺失；
        # 但写入了与默认相反的值必须失败（程序不得擅自关掉新来源）。
        original = self.original_with_dir(
            "price_auto_sync = false\n"
            "[sources.claude]\nenabled = true\nauto_discover = true\n"
            "excluded_auto_paths = []\n"
            "[[sources.claude.additional_paths]]\n"
            'path = "D:/history/claude/projects"\nenabled = true\n')
        # 新键缺失：升级后程序尚未写入 → 通过
        self.write_current(original.read_text(encoding="utf-8").replace(
            "price_auto_sync = false",
            "privacy_policy_accepted = true\nprice_auto_sync = false"))
        self.assertTrue(fixture.verify_settings(self.data, original, 2, 3)["ok"])
        # 显式默认值（启用）：通过
        current = original.read_text(encoding="utf-8").replace(
            "price_auto_sync = false",
            "privacy_policy_accepted = true\nprice_auto_sync = false")
        current += "\n[sources.workbuddy]\nenabled = true\n"
        current += "\n[sources.workbuddy_ai]\nenabled = true\n"
        self.write_current(current)
        self.assertTrue(fixture.verify_settings(self.data, original, 2, 3)["ok"])
        # 程序擅自关闭新来源：失败
        self.write_current(current.replace(
            "[sources.workbuddy]\nenabled = true", "[sources.workbuddy]\nenabled = false"))
        with self.assertRaises(ValueError):
            fixture.verify_settings(self.data, original, 2, 3)

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
