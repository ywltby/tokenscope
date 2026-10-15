"""提交前检查选择器的契约。

临时 Git 仓库真实调用 hook；命令执行器使用记录 argv 的替身，不真正构建。
分类只依据暂存区，但多数工具读取工作区，因此部分暂存必须明确阻止。
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from pre_commit_checks import (
    CheckError,
    build_changes,
    classify,
    expand_commands,
    manifest_index,
    run,
)

ROOT = Path(__file__).resolve().parent.parent

MANIFEST = {
    "schema_version": 1,
    "generated": [
        {"path": "docs/privacy.md", "kind": "file",
         "consumers": [{"package": "tokenscope-app", "mode": "default"}]},
        {"path": "src-tauri/src/acceptance_csp.js", "kind": "file",
         "consumers": [{"package": "tokenscope-app", "mode": "acceptance"}]},
        {"path": "crates/tokenscope-core/tests/fixtures/cache/schema-v3.sql", "kind": "file",
         "consumers": [{"package": "tokenscope", "mode": "default"},
                       {"package": "tokenscope", "mode": "acceptance"}]},
    ],
    "manual": [],
}


def change(status, path, old_path=None):
    return {"status": status, "path": path, "old_path": old_path}


def git_env():
    """Git 钩子会注入 GIT_DIR 等变量；测试仓库必须与调用者环境隔离。"""
    return {key: value for key, value in os.environ.items()
            if not key.startswith("GIT_")}


class ClassifyCase(unittest.TestCase):
    def plan(self, changes, manifest=MANIFEST):
        return classify(changes, manifest=manifest)

    def checks(self, changes, manifest=MANIFEST):
        return self.plan(changes, manifest)["checks"]

    def test_plain_document_uses_document_checks_only(self):
        checks = self.checks([change("M", "docs/notes.md")])
        self.assertEqual(checks, ["docs"])

    def test_manifest_registered_document_uses_its_consumer_package_and_mode(self):
        plan = self.plan([change("M", "docs/privacy.md")])
        self.assertIn("rust-shell-default", plan["checks"])
        self.assertIn("docs", plan["checks"])
        self.assertNotIn("rust-core-default", plan["checks"])

    def test_acceptance_csp_selects_shell_acceptance_only(self):
        checks = self.checks([change("M", "src-tauri/src/acceptance_csp.js")])
        self.assertIn("rust-shell-acceptance", checks)
        self.assertNotIn("rust-shell-default", checks)
        self.assertNotIn("frontend-test", checks)

    def test_core_rust_runs_workspace_both_modes_and_fmt(self):
        checks = self.checks([change("M", "crates/tokenscope-core/src/report.rs")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default", "rust-fmt"])

    def test_shell_rust_runs_shell_both_modes_and_fmt(self):
        checks = self.checks([change("M", "src-tauri/src/lib.rs")])
        self.assertEqual(checks, ["rust-fmt", "rust-shell-acceptance", "rust-shell-default"])

    def test_core_fixture_directory_rule_beats_document_exemption(self):
        checks = self.checks([change("M", "crates/tokenscope-core/tests/fixtures/readme.md")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default"])

    def test_runtime_fixture_jsonl_selects_both_workspace_modes_only(self):
        checks = self.checks([change("M", "crates/tokenscope-core/tests/fixtures/codex/rate-limit-only.jsonl")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default"])

    def test_frontend_sources_and_public_assets_select_all_frontend_checks(self):
        for path in ("frontend/src/main.ts", "frontend/public/theme-boot.js",
                     "frontend/package.json", "frontend/pnpm-lock.yaml"):
            with self.subTest(path=path):
                checks = self.checks([change("M", path)])
                self.assertEqual(checks, ["frontend-build", "frontend-format",
                                          "frontend-test", "frontend-typecheck"])

    def test_scripts_and_hooks_and_build_configs_require_full_mode(self):
        for path in ("Cargo.toml", "Cargo.lock", "crates/tokenscope-core/Cargo.toml",
                     ".githooks/pre-commit", "scripts/build-inputs.json",
                     "scripts/pre_commit_checks.py", ".github/workflows/ci.yml",
                     "src-tauri/tauri.conf.json", "src-tauri/build.rs",
                     "crates/tokenscope-core/build.rs", "release-change.json"):
            with self.subTest(path=path):
                self.assertEqual(self.plan([change("M", path)])["mode"], "full")

    def test_full_mode_does_not_repeat_shell_rounds_covered_by_workspace(self):
        plan = self.plan([change("M", "Cargo.toml")])
        self.assertEqual(plan["mode"], "full")
        self.assertIn("rust-core-default", plan["checks"])
        self.assertIn("rust-core-acceptance", plan["checks"])
        self.assertNotIn("rust-shell-default", plan["checks"])
        self.assertNotIn("rust-shell-acceptance", plan["checks"])

    def test_unknown_path_falls_back_to_full_mode_and_is_reported(self):
        plan = self.plan([change("A", "Makefile")])
        self.assertEqual(plan["mode"], "full")
        self.assertIn("Makefile", plan["fallback"])

    def test_missing_or_broken_manifest_falls_back_to_full_mode(self):
        self.assertEqual(self.plan([change("M", "docs/notes.md")], manifest=None)["mode"], "full")
        with self.assertRaises(CheckError):
            classify([change("M", "docs/notes.md")],
                     manifest={"schema_version": 9, "generated": [], "manual": []})

    def test_manifest_change_itself_cannot_reduce_its_own_verification(self):
        plan = self.plan([change("M", "scripts/build-inputs.json"),
                          change("M", "docs/notes.md")])
        self.assertEqual(plan["mode"], "full")

    def test_rename_uses_both_old_and_new_paths(self):
        plan = self.plan([change("R", "docs/renamed.md",
                                 old_path="crates/tokenscope-core/src/report.rs")])
        self.assertIn("rust-core-default", plan["checks"])
        self.assertIn("docs", plan["checks"])

    def test_deleted_fixture_still_uses_fixture_rule(self):
        checks = self.checks([change("D", "crates/tokenscope-core/tests/fixtures/claude/x.jsonl")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default"])

    def test_chinese_and_space_paths_are_classified(self):
        checks = self.checks([change("M", "crates/tokenscope-core/tests/fixtures/中文 目录/x.jsonl")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default"])

    def test_mixed_core_and_shell_takes_union_and_deduplicates(self):
        checks = self.checks([change("M", "crates/tokenscope-core/src/report.rs"),
                              change("M", "src-tauri/src/lib.rs")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default", "rust-fmt",
                                  "rust-shell-acceptance", "rust-shell-default"])

    def test_empty_stage_reports_no_file_checks(self):
        plan = self.plan([])
        self.assertEqual(plan["checks"], [])
        self.assertEqual(plan["mode"], "selected")


class CommandExpansionCase(unittest.TestCase):
    def test_commands_are_argv_arrays_with_locked_cargo_and_no_shell_strings(self):
        commands = expand_commands(["rust-core-acceptance", "rust-fmt"])
        flat = [item for command in commands for item in command]
        for command in commands:
            self.assertIsInstance(command, list)
            self.assertTrue(all(isinstance(part, str) for part in command))
        self.assertIn(["cargo", "fmt", "--all", "--", "--check"], commands)
        test_command = next(c for c in commands if c[:2] == ["cargo", "test"])
        self.assertIn("--locked", test_command)
        self.assertIn("--workspace", test_command)
        self.assertIn("acceptance", test_command)
        self.assertNotIn("release", flat)

    def test_shell_checks_select_the_shell_package(self):
        commands = expand_commands(["rust-shell-default"])
        for command in commands:
            self.assertIn("-p", command)
            self.assertIn("tokenscope-app", command)

    def test_frontend_and_script_commands_are_stable(self):
        self.assertEqual(expand_commands(["frontend-typecheck"]),
                         [["pnpm", "--dir", "frontend", "typecheck"]])
        scripts = expand_commands(["scripts-contracts"])
        self.assertIn(["python", "-B", "-m", "unittest", "discover", "-s", "scripts",
                       "-p", "test_*.py"], scripts)
        self.assertIn(["pwsh", "-NoProfile", "-File",
                       "scripts/check_powershell_syntax.ps1"], scripts)
        self.assertEqual(expand_commands(["docs"]),
                         [["python", "-B", "scripts/check_staged_docs.py"]])


class UnstagedGuardCase(unittest.TestCase):
    """运行代码检查时，被选检查读取的受版本管理输入若有未暂存差异必须阻止。"""

    def test_unstaged_code_difference_blocks_selected_rust_checks(self):
        plan = classify([change("M", "crates/tokenscope-core/src/report.rs")],
                        manifest=MANIFEST,
                        unstaged=["crates/tokenscope-core/src/report.rs"])
        self.assertEqual(plan["blocked"], ["crates/tokenscope-core/src/report.rs"])

    def test_unrelated_unstaged_file_does_not_block_document_submission(self):
        plan = classify([change("M", "docs/notes.md")], manifest=MANIFEST,
                        unstaged=["crates/tokenscope-core/src/report.rs"])
        self.assertEqual(plan["blocked"], [])

    def test_unrelated_unstaged_file_does_not_block_selected_code_checks(self):
        plan = classify([change("M", "src-tauri/src/lib.rs")], manifest=MANIFEST,
                        unstaged=["frontend/src/main.ts"])
        self.assertEqual(plan["blocked"], [])


class GitIntegrationCase(unittest.TestCase):
    """真实钩子路径：临时仓库里跑分类、执行与提交阻止。"""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-hook-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "中文 仓库"
        (self.root / "scripts").mkdir(parents=True)
        (self.root / "docs").mkdir()
        (self.root / "frontend" / "public").mkdir(parents=True)
        (self.root / "scripts" / "build-inputs.json").write_text(
            json.dumps(MANIFEST, ensure_ascii=False), encoding="utf-8")
        (self.root / "docs" / "notes.md").write_text("# 说明\n", encoding="utf-8")
        (self.root / "frontend" / "public" / "theme-boot.js").write_text("// boot\n",
                                                                        encoding="utf-8")
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "TokenScope fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "baseline")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args],
                                       encoding="utf-8", stderr=subprocess.STDOUT,
                                       env=git_env())

    def stage(self, relative):
        self.git("add", "--", relative)

    def test_document_only_submission_does_not_start_cargo_or_pnpm(self):
        (self.root / "docs" / "notes.md").write_text("# 说明\n新增\n", encoding="utf-8")
        self.stage("docs/notes.md")
        changes = build_changes(self.root)
        plan = classify(changes, manifest=manifest_index(self.root))
        commands = expand_commands(plan["checks"])
        self.assertTrue(all(command[0] not in ("cargo", "pnpm") for command in commands))

    def test_mixed_staged_change_includes_rust_and_document_checks(self):
        (self.root / "docs" / "notes.md").write_text("# 说明\n新增\n", encoding="utf-8")
        (self.root / "frontend" / "public" / "theme-boot.js").write_text("// boot v2\n",
                                                                        encoding="utf-8")
        self.stage("docs/notes.md")
        self.stage("frontend/public/theme-boot.js")
        changes = build_changes(self.root)
        self.assertEqual({c["path"] for c in changes},
                         {"docs/notes.md", "frontend/public/theme-boot.js"})

    def test_empty_index_reports_nothing_to_check(self):
        self.assertEqual(build_changes(self.root), [])

    def test_git_query_failure_is_not_treated_as_empty_stage(self):
        with tempfile.TemporaryDirectory() as other:
            with self.assertRaises(CheckError):
                build_changes(Path(other))

    def test_runner_change_returns_non_zero_and_stops_on_first_failure(self):
        calls = []

        def executor(argv, cwd):
            calls.append(argv)
            return 7 if len(calls) == 2 else 0

        status = run([["a"], ["b"], ["c"]], cwd=self.root, executor=executor)
        self.assertEqual(status, 7)
        self.assertEqual(calls, [["a"], ["b"]])

    def test_missing_executable_fails_loudly_instead_of_passing(self):
        # 缺解释器/工具必须明确失败（退出码 127），不能静默放行。
        self.assertEqual(run([["tokenscope-missing-tool-xyz", "--version"]], cwd=self.root),
                         127)

    def test_run_does_not_mutate_git_index_or_stash(self):
        (self.root / "docs" / "notes.md").write_text("# 说明\n未暂存改动\n", encoding="utf-8")
        before = self.git("status", "--porcelain=v1", "-z")
        run([["a"]], cwd=self.root, executor=lambda argv, cwd: 0)
        self.assertEqual(before, self.git("status", "--porcelain=v1", "-z"))
        self.assertFalse((self.root / ".git" / "refs" / "stash").exists())


class HookWiringCase(unittest.TestCase):
    """真实钩子脚本 + 替身检查器：非零退出必须阻止真实的 git commit。

    只替换被钩子调用的检查器（临时仓库里放 `scripts/pre_commit_checks.py` 替身），
    钩子本身用仓库里受版本管理的 `.githooks/pre-commit`，因此验证的是真实接线。
    """

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-hookwire-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "hook 仓库"
        (self.root / "scripts").mkdir(parents=True)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "TokenScope fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "core.hooksPath", str(ROOT / ".githooks"))
        (self.root / "note.txt").write_text("内容\n", encoding="utf-8")

    @property
    def hooks_path(self):
        return ROOT / ".githooks"

    def git(self, *args, extra_env=None):
        environment = git_env()
        if extra_env:
            environment.update(extra_env)
        return subprocess.run(["git", "-C", str(self.root), *args],
                              capture_output=True, encoding="utf-8", errors="replace",
                              env=environment)

    def write_stub(self, code):
        (self.root / "scripts" / "pre_commit_checks.py").write_text(
            f"import sys\nprint('stub checks')\nsys.exit({code})\n", encoding="utf-8")

    def test_failing_check_blocks_commit_and_success_allows_it(self):
        self.assertTrue(self.hooks_path.is_dir(), "缺少受版本管理的 .githooks 目录")
        self.assertEqual(self.git("add", "note.txt").returncode, 0)

        self.write_stub(7)
        blocked = self.git("commit", "-m", "应被阻止")
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout)
        self.assertEqual(self.git("log", "--oneline").stdout.strip(), "",
                         "检查失败时不得产生提交")

        self.write_stub(0)
        allowed = self.git("commit", "-m", "应成功")
        self.assertEqual(allowed.returncode, 0, allowed.stderr)
        self.assertIn("应成功", self.git("log", "--oneline").stdout)


if __name__ == "__main__":
    unittest.main()
