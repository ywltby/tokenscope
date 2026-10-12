"""版本工具的实际文件、Git 基线与失败回滚契约；全部使用临时仓库。"""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import bump_version as policy


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   encoding="utf-8", stderr=subprocess.PIPE).strip()


def seed(root, version="0.1.0"):
    for name, package in (("Cargo.toml", "tokenscope"),
                          ("src-tauri/Cargo.toml", "tokenscope-app")):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'[package]\nname = "{package}"\nversion = "{version}"\n'
                        '[dependencies]\nother = "0.1.0"\n', encoding="utf-8")
    for name in ("frontend/package.json", "src-tauri/tauri.conf.json"):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"version": version, "label": "中文"},
                                   ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    external = '[[package]]\nname = "other"\nversion = "0.1.0"\n'
    for name, packages in (("Cargo.lock", ("tokenscope",)),
                           ("src-tauri/Cargo.lock", ("tokenscope", "tokenscope-app"))):
        path = root / name
        local = "".join(f'[[package]]\nname = "{p}"\nversion = "{version}"\n'
                        for p in packages)
        path.write_text('version = 4\n' + local + external, encoding="utf-8")


class VersionPolicyCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-version-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "中文 仓库"
        self.root.mkdir()
        seed(self.root)
        git(self.root, "init", "-q", "-b", "main")
        git(self.root, "config", "user.name", "TokenScope fixture")
        git(self.root, "config", "user.email", "fixture@example.invalid")
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "baseline")
        self.base = git(self.root, "rev-parse", "HEAD")
        self.origin = Path(self.temp.name) / "origin.git"
        subprocess.run(["git", "clone", "--bare", "--quiet", str(self.root),
                        str(self.origin)], check=True, capture_output=True)
        git(self.root, "remote", "add", "origin", str(self.origin))
        git(self.root, "fetch", "--quiet", "origin", "main")
        git(self.root, "switch", "-qc", "feature")

    def bump(self, kind="minor"):
        return policy.bump(self.root, kind, "origin/main", "历史用量与版本递增")

    def snapshot(self):
        return {p.relative_to(self.root).as_posix(): p.read_bytes()
                for p in self.root.rglob("*") if p.is_file() and ".git" not in p.parts}

    def test_minor_and_patch_follow_zero_major_policy(self):
        for before, kind, after in (("0.1.0", "minor", "0.2.0"),
                                    ("0.2.9", "minor", "0.3.0"),
                                    ("0.2.9", "patch", "0.2.10")):
            with self.subTest(before=before, kind=kind):
                self.assertEqual(policy.next_version(before, kind), after)
        for version in ("1.0.0", "0.01.0", "0.2", "0.2.0-rc.1", "0.2.0+build"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                policy.next_version(version, "patch")
        with self.assertRaises(ValueError):
            policy.next_version("0.1.0", "major")

    def test_six_files_and_declaration_change_together_without_dependency_drift(self):
        self.assertEqual(self.bump(), "0.2.0")
        result = policy.check(self.root, self.base)
        self.assertEqual(result["version"], "0.2.0")
        self.assertEqual(result["kind"], "minor")
        for name in ("Cargo.toml", "src-tauri/Cargo.toml"):
            self.assertIn('other = "0.1.0"', (self.root / name).read_text(encoding="utf-8"))
        for name in ("Cargo.lock", "src-tauri/Cargo.lock"):
            self.assertIn('name = "other"\nversion = "0.1.0"',
                          (self.root / name).read_text(encoding="utf-8"))
        self.assertIn("中文", (self.root / "frontend/package.json").read_text(encoding="utf-8"))

    def test_repeated_bump_is_idempotent_for_the_same_main_baseline(self):
        self.bump()
        before = self.snapshot()
        self.bump()
        self.assertEqual(before, self.snapshot())

    def test_missing_bump_mismatched_config_lock_and_kind_are_rejected(self):
        with self.assertRaises(ValueError):
            policy.check(self.root, self.base)
        self.bump()
        good = self.snapshot()
        mutations = (("Cargo.toml", 'version = "0.2.0"', 'version = "0.1.1"'),
                     ("Cargo.lock", 'version = "0.2.0"', 'version = "0.1.0"'),
                     ("release-change.json", '"minor"', '"patch"'))
        for name, old, new in mutations:
            with self.subTest(file=name):
                path = self.root / name
                path.write_text(path.read_text(encoding="utf-8").replace(old, new),
                                encoding="utf-8")
                with self.assertRaises(ValueError):
                    policy.check(self.root, self.base)
                path.write_bytes(good[name])

    def test_version_rollback_skip_and_new_main_baseline_are_rejected(self):
        self.bump()
        declaration = (self.root / "release-change.json").read_text(encoding="utf-8")
        for version in ("0.1.0", "0.0.9", "0.3.0", "0.2.1", "1.0.0"):
            with self.subTest(version=version):
                seed(self.root, version)
                (self.root / "release-change.json").write_text(declaration, encoding="utf-8")
                with self.assertRaises(ValueError):
                    policy.check(self.root, self.base)
        seed(self.root, "0.2.0")
        (self.root / "release-change.json").write_text(declaration, encoding="utf-8")
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "another main version")
        with self.assertRaises(ValueError):
            policy.check(self.root, "HEAD")

    def test_mid_write_failure_rolls_back_all_files_and_new_declaration(self):
        before = self.snapshot()
        replace = os.replace
        count = 0

        def fail_once(source, destination):
            nonlocal count
            count += 1
            if count == 3:
                raise OSError("injected replace failure")
            return replace(source, destination)

        with patch.object(policy.os, "replace", side_effect=fail_once):
            with self.assertRaises(OSError):
                self.bump()
        self.assertEqual(before, self.snapshot())

    def test_actual_event_before_supports_fast_forward_with_multiple_commits_and_merge(self):
        self.bump()
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "version")
        (self.root / "note.txt").write_text("第二个提交\n", encoding="utf-8")
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "feature")
        event = {"ref": "refs/heads/main", "before": self.base}
        self.assertEqual(policy.check_event(self.root, event, "push")["version"], "0.2.0")
        git(self.root, "switch", "-q", "main")
        git(self.root, "merge", "--no-ff", "-qm", "merge", "feature")
        self.assertEqual(policy.check_event(self.root, event, "push")["version"], "0.2.0")

    def test_pr_base_is_fetched_and_requires_main_to_be_in_tested_checkout(self):
        self.bump()
        event = {"pull_request": {"base": {"ref": "main", "sha": self.base}}}
        self.assertEqual(policy.check_event(self.root, event, "pull_request")["version"], "0.2.0")
        event["pull_request"]["base"]["ref"] = "other"
        with self.assertRaises(ValueError):
            policy.check_event(self.root, event, "pull_request")

    def test_remote_main_advancement_is_detected_even_when_minor_number_still_matches(self):
        self.bump()
        updater = Path(self.temp.name) / "updater"
        subprocess.run(["git","clone","--quiet",str(self.origin),str(updater)],
                       check=True,capture_output=True)
        git(updater,"config","user.name","TokenScope fixture")
        git(updater,"config","user.email","fixture@example.invalid")
        policy.bump(updater,"patch","origin/main","patch")
        git(updater,"add",".")
        git(updater,"commit","-qm","advance main")
        git(updater,"push","--quiet","origin","main")
        event={"pull_request":{"base":{"ref":"main","sha":self.base}}}
        with self.assertRaisesRegex(ValueError,"main 已推进"):
            policy.check_event(self.root,event,"pull_request")


if __name__ == "__main__":
    unittest.main()
