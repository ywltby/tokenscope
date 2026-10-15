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
    FULL_CHECKS,
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
        {"path": "src-tauri/capabilities", "kind": "directory",
         "consumers": [{"package": "tokenscope-app", "mode": "default"},
                       {"package": "tokenscope-app", "mode": "acceptance"}]},
        {"path": "assets", "kind": "directory",
         "consumers": [{"package": "tokenscope-app", "mode": "default"}]},
        {"path": "crates/tokenscope-core/tests/fixtures/cache/schema-v3.sql", "kind": "file",
         "consumers": [{"package": "tokenscope", "mode": "default"},
                       {"package": "tokenscope", "mode": "acceptance"}]},
    ],
    "manual": [],
}


def change(status, path, old_path=None):
    return {"status": status, "path": path, "old_path": old_path}


def setUpModule():
    """钩子会注入 GIT_DIR/GIT_INDEX_FILE；测试自建临时仓库，必须与调用者仓库隔离。

    实现本身保留这些变量（`git commit --only` 依赖它们），所以隔离只能发生在测试侧。
    """
    for key in [name for name in os.environ if name.startswith("GIT_")]:
        del os.environ[key]


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

    def test_registered_fixture_selects_workspace_modes_without_frontend(self):
        # 清单已登记的 fixture 与未登记的运行时 fixture 走同一套规则。
        checks = self.checks([change("M", "crates/tokenscope-core/tests/fixtures/cache/schema-v3.sql")])
        self.assertEqual(checks, ["rust-core-acceptance", "rust-core-default"])

    def test_fixture_renamed_into_docs_keeps_both_rules(self):
        plan = self.plan([change("R", "docs/moved.md",
                                 old_path="crates/tokenscope-core/tests/fixtures/x.jsonl")])
        self.assertIn("rust-core-default", plan["checks"])
        self.assertIn("docs", plan["checks"])

    def test_deleting_or_renaming_selects_document_checks(self):
        """删除/改名会让引用失效，引用方可能没被修改 → 必须同时跑文档检查。"""
        deleted = self.plan([change("D", "crates/tokenscope-core/src/unused.rs")])
        self.assertIn("docs", deleted["checks"])
        self.assertIn("rust-core-default", deleted["checks"])
        renamed = self.plan([change("R", "crates/tokenscope-core/src/renamed.rs",
                                    old_path="crates/tokenscope-core/src/unused.rs")])
        self.assertIn("docs", renamed["checks"])

    def test_frontend_sources_and_public_assets_select_all_frontend_checks(self):
        for path in ("frontend/src/main.ts", "frontend/public/theme-boot.js",
                     "frontend/package.json", "frontend/pnpm-lock.yaml"):
            with self.subTest(path=path):
                checks = self.checks([change("M", path)])
                self.assertEqual(checks, ["frontend-build", "frontend-format",
                                          "frontend-test", "frontend-typecheck"])

    def test_browser_acceptance_script_selects_its_browser_e2e_contract(self):
        """浏览器验收脚本的契约是浏览器 E2E，而不是只做语法检查。"""
        checks = self.checks([change("M", "frontend/scripts/check-ui-contracts.mjs")])
        self.assertIn("browser-e2e", checks)
        self.assertIn("node-syntax", checks)
        self.assertIn("frontend-test", checks)

    def test_native_acceptance_script_without_local_contract_falls_back_to_full(self):
        """原生验收驱动需要真实原生环境，本地无等价契约 → 按计划完整兜底。"""
        for path in ("frontend/scripts/native-acceptance.mjs",
                     "frontend/scripts/contracts/native-ui-refresh.mjs",
                     "frontend/scripts/lib/png.mjs",
                     "frontend/scripts/unknown-tool.mjs"):
            with self.subTest(path=path):
                self.assertEqual(self.plan([change("M", path)])["mode"], "full")

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
        # 删除 fixture 走 fixtures 规则；同时因为删除会让引用失效，还要跑文档检查。
        checks = self.checks([change("D", "crates/tokenscope-core/tests/fixtures/claude/x.jsonl")])
        self.assertEqual(checks, ["docs", "rust-core-acceptance", "rust-core-default"])

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


class CommandOrderCase(unittest.TestCase):
    """E2E 入口直接读取已有 dist，必须在构建成功之后才运行。"""

    BUILD = "pnpm --dir frontend build"
    E2E = "pnpm --dir frontend test:e2e"

    def names(self, checks):
        return [" ".join(command) for command in expand_commands(checks)]

    def test_selected_mode_runs_build_before_e2e(self):
        names = self.names(["browser-e2e", "frontend-build"])
        self.assertLess(names.index(self.BUILD), names.index(self.E2E))

    def test_full_mode_runs_build_before_e2e(self):
        names = self.names(list(FULL_CHECKS))
        self.assertLess(names.index(self.BUILD), names.index(self.E2E))

    def test_e2e_pulls_in_its_build_dependency_when_selected_alone(self):
        names = self.names(["browser-e2e"])
        self.assertIn(self.BUILD, names)
        self.assertLess(names.index(self.BUILD), names.index(self.E2E))

    def test_build_failure_stops_before_e2e(self):
        calls = []

        def executor(argv, cwd):
            calls.append(" ".join(argv))
            return 7 if argv[:2] == ["pnpm", "--dir"] and argv[-1] == "build" else 0

        status = run(expand_commands(["browser-e2e", "frontend-build"]),
                     cwd=ROOT, executor=executor)
        self.assertEqual(status, 7)
        self.assertIn(self.BUILD, calls)
        self.assertNotIn(self.E2E, calls, "构建失败后不得运行 E2E")

    def test_shared_dependency_appears_once_for_multiple_consumers(self):
        names = self.names(["browser-e2e", "frontend-typecheck"])
        self.assertEqual(names.count(self.BUILD), 1)
        self.assertEqual(names.count(self.E2E), 1)


class UnstagedGuardCase(unittest.TestCase):
    """运行代码检查时，被选检查读取的受版本管理输入若有未暂存差异必须阻止。"""

    def test_unstaged_code_difference_blocks_selected_rust_checks(self):
        plan = classify([change("M", "crates/tokenscope-core/src/report.rs")],
                        manifest=MANIFEST,
                        unstaged=["crates/tokenscope-core/src/report.rs"])
        self.assertEqual(plan["blocked"], ["crates/tokenscope-core/src/report.rs"])

    def test_unstaged_shared_configuration_and_compiled_inputs_block_rust_checks(self):
        """共享配置与清单登记的编译输入也参与构建，未暂存差异同样必须阻止。"""
        plan = classify([change("M", "crates/tokenscope-core/src/report.rs")],
                        manifest=MANIFEST,
                        unstaged=["Cargo.toml", "Cargo.lock", "src-tauri/tauri.conf.json",
                                  "docs/privacy.md"])
        self.assertEqual(plan["blocked"], ["Cargo.lock", "Cargo.toml", "docs/privacy.md",
                                           "src-tauri/tauri.conf.json"])

    def test_forced_full_mode_uses_the_same_unstaged_guard(self):
        plan = classify([change("M", "docs/notes.md")], manifest=MANIFEST,
                        unstaged=["Cargo.toml"], force_full=True)
        self.assertEqual(plan["mode"], "full")
        self.assertEqual(plan["blocked"], ["Cargo.toml"])

    def test_forced_full_mode_also_blocks_unstaged_frontend_changes(self):
        # 完整模式会跑前端检查，因此前端未暂存差异同样必须阻止。
        plan = classify([change("M", "docs/notes.md")], manifest=MANIFEST,
                        unstaged=["frontend/src/main.ts"], force_full=True)
        self.assertEqual(plan["blocked"], ["frontend/src/main.ts"])

    def test_unstaged_toolchain_configuration_blocks_rust_checks(self):
        """`.cargo/config.toml` 与 `rustfmt.toml` 都会影响 Rust 检查的读取结果。"""
        plan = classify([change("M", "crates/tokenscope-core/src/report.rs")],
                        manifest=MANIFEST,
                        unstaged=[".cargo/config.toml", "rustfmt.toml",
                                  "rust-toolchain.toml"])
        self.assertEqual(plan["blocked"], [".cargo/config.toml", "rust-toolchain.toml",
                                           "rustfmt.toml"])

    def test_unstaged_file_inside_a_manifest_directory_blocks_rust_checks(self):
        """清单登记的目录输入要按子树匹配，而不是当成精确文件路径。"""
        plan = classify([change("M", "crates/tokenscope-core/src/report.rs")],
                        manifest=MANIFEST,
                        unstaged=["assets/data.json", "assets/nested/deep.json",
                                  "src-tauri/capabilities/default.json"])
        self.assertEqual(plan["blocked"], ["assets/data.json", "assets/nested/deep.json",
                                           "src-tauri/capabilities/default.json"])

    def test_document_only_submission_is_not_blocked_by_compiled_inputs(self):
        """文档检查只读 index，不读被编译的输入，因此不该被它的未暂存改动阻止。"""
        plan = classify([change("M", "docs/notes.md")], manifest=MANIFEST,
                        unstaged=["docs/privacy.md", "src-tauri/tauri.conf.json"])
        self.assertEqual(plan["blocked"], [])

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

    def write_echo_stub(self):
        """替身只回显它看到的暂存区，用来验证钩子把 Git 准备的 index 传下去了。"""
        (self.root / "scripts" / "pre_commit_checks.py").write_text(
            "import subprocess, sys\n"
            "result = subprocess.run(['git', 'diff', '--cached', '--name-only', '-z',\n"
            "                         '--find-renames'], capture_output=True)\n"
            "staged = result.stdout.decode('utf-8', 'replace').replace(chr(0), '|')\n"
            "print('STAGED=[' + staged + ']')\n"
            "sys.exit(0)\n", encoding="utf-8")

    def test_only_commit_exposes_the_paths_git_prepared_for_that_commit(self):
        """`git commit --only <path>` 用临时 index；钩子必须读到它，不能读默认 index。"""
        self.write_stub(0)  # 初始提交也需要一个放行的替身
        (self.root / "Cargo.toml").write_text("内容 = 1\n", encoding="utf-8")
        self.assertEqual(self.git("add", "Cargo.toml").returncode, 0)
        self.assertEqual(self.git("commit", "-qm", "初始").returncode, 0)
        self.write_echo_stub()
        (self.root / "Cargo.toml").write_text("内容 = 2\n", encoding="utf-8")

        result = self.git("commit", "--only", "Cargo.toml", "-m", "only 提交")
        combined = result.stdout + result.stderr
        self.assertIn("STAGED=[Cargo.toml|]", combined,
                      f"钩子没读到 --only 提交的路径：{combined!r}")
        self.assertEqual(result.returncode, 0, combined)

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


class FirstCommitCase(unittest.TestCase):
    """还没有 HEAD 的仓库：暂存区与空树比较，不能当作 Git 查询失败。"""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-firstcommit-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "首次 仓库"
        (self.root / "docs").mkdir(parents=True)
        self.git("init", "-q", "-b", "main")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args],
                              capture_output=True, encoding="utf-8", errors="replace",
                              env=git_env())

    def test_first_commit_is_read_from_the_empty_tree(self):
        (self.root / "docs" / "notes.md").write_text("# 说明\n", encoding="utf-8")
        self.assertEqual(self.git("add", "docs/notes.md").returncode, 0)
        changes = build_changes(self.root)
        self.assertEqual(changes, [{"status": "A", "path": "docs/notes.md",
                                    "old_path": None}])
        plan = classify(changes, manifest=MANIFEST)
        self.assertEqual(plan["checks"], ["docs"])
        self.assertEqual(plan["mode"], "selected")


class UserDataIsolationCase(unittest.TestCase):
    """被选中的检查不得写用户数据目录（本机 `~/.tokenscope`）。"""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-userdata-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "仓库"
        (self.root / "docs").mkdir(parents=True)
        subprocess.run(["git", "-C", str(self.root), "init", "-q", "-b", "main"],
                       capture_output=True, env=git_env())
        (self.root / "docs" / "notes.md").write_text("# 说明\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.root), "add", "docs/notes.md"],
                       capture_output=True, env=git_env())

    @property
    def data_dir(self):
        home = os.environ.get("USERPROFILE") or os.environ.get("HOME") or ""
        return Path(home) / ".tokenscope"

    def snapshot(self):
        if not self.data_dir.exists():
            return None
        return sorted(str(path.relative_to(self.data_dir))
                      for path in self.data_dir.rglob("*"))

    def test_selected_checks_do_not_touch_user_data_directory(self):
        before = self.snapshot()
        status = run([["python", "-B", str(ROOT / "scripts/check_staged_docs.py"),
                       "--root", str(self.root)]], cwd=self.root)
        self.assertEqual(status, 0)
        self.assertEqual(before, self.snapshot())


class EndToEndOnlyCommitCase(unittest.TestCase):
    """真实选择器 + 真实文档检查 + `git commit --only`：坏链接必须真的阻止提交。

    与 HookWiringCase 的区别：这里不放替身，临时仓库里是仓库中受版本管理的
    `pre_commit_checks.py` / `build_inputs.py` / `check_staged_docs.py` 本体，
    因此验证的是「选中了检查」进而「检查确实执行并失败」。
    """

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-e2e-only-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "端到端 仓库"
        (self.root / "scripts").mkdir(parents=True)
        (self.root / "docs").mkdir()
        for name in ("pre_commit_checks.py", "build_inputs.py", "check_staged_docs.py"):
            shutil.copyfile(ROOT / "scripts" / name, self.root / "scripts" / name)
        (self.root / "scripts" / "build-inputs.json").write_text(
            json.dumps({"schema_version": 1, "generated": [], "manual": []}),
            encoding="utf-8")
        (self.root / "docs" / "ok.md").write_text("# 正常\n", encoding="utf-8")
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "TokenScope fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        # 基线提交在挂上钩子之前完成，避免基线本身触发完整门禁。
        self.assertEqual(self.git("add", "-A").returncode, 0)
        self.assertEqual(self.git("commit", "-qm", "baseline").returncode, 0)
        self.git("config", "core.hooksPath", str(ROOT / ".githooks"))

    def git(self, *args):
        environment = git_env()
        return subprocess.run(["git", "-C", str(self.root), *args],
                              capture_output=True, encoding="utf-8", errors="replace",
                              env=environment)

    def test_broken_link_in_only_commit_runs_the_checker_and_blocks(self):
        target = self.root / "docs" / "tracked.md"
        target.write_text("# 初始\n", encoding="utf-8")
        self.assertEqual(self.git("add", "docs/tracked.md").returncode, 0)
        self.assertEqual(self.git("commit", "-qm", "跟踪一个文档").returncode, 0)

        # 工作区改坏但不 add：`--only` 会把工作区版本放进临时 index，
        # 检查器必须读到它，否则会看到"没有暂存文件"而放行。
        target.write_text("# 坏链接\n\n[缺失](nope.md)\n", encoding="utf-8")
        result = self.git("commit", "--only", "docs/tracked.md", "-m", "应被阻止")
        combined = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, combined)
        self.assertIn("nope.md", combined, f"文档检查没有真正执行：{combined!r}")
        self.assertEqual(len(self.git("log", "--oneline").stdout.strip().splitlines()), 2,
                         "检查失败时不得产生提交")

    def test_clean_change_in_only_commit_is_allowed(self):
        target = self.root / "docs" / "tracked-ok.md"
        target.write_text("# 初始\n", encoding="utf-8")
        self.assertEqual(self.git("add", "docs/tracked-ok.md").returncode, 0)
        self.assertEqual(self.git("commit", "-qm", "跟踪另一个文档").returncode, 0)

        target.write_text("# 正常改动\n\n[正常](ok.md)\n", encoding="utf-8")
        result = self.git("commit", "--only", "docs/tracked-ok.md", "-m", "应成功")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(len(self.git("log", "--oneline").stdout.strip().splitlines()), 3)


    def test_deleting_a_referenced_file_is_blocked_by_the_real_checker(self):
        """分类器选中文档检查 + 检查器真的发现失效引用 → 提交被阻止。"""
        (self.root / "docs" / "dep.md").write_text("# 依赖\n", encoding="utf-8")
        (self.root / "docs" / "referrer.md").write_text(
            "# 引用者\n\n[依赖](dep.md)\n", encoding="utf-8")
        self.assertEqual(self.git("add", "-A").returncode, 0)
        self.assertEqual(self.git("commit", "-qm", "加入引用").returncode, 0)

        self.assertEqual(self.git("rm", "-q", "docs/dep.md").returncode, 0)
        result = self.git("commit", "-m", "删除被引用的文件")
        combined = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, combined)
        self.assertIn("referrer.md", combined, f"未检查未修改文档的引用：{combined!r}")
        # baseline + "加入引用" 两个提交；被阻止的这次不得产生提交。
        self.assertEqual(len(self.git("log", "--oneline").stdout.strip().splitlines()), 2,
                         "检查失败时不得产生提交")

    def test_deleting_an_unreferenced_file_is_allowed(self):
        (self.root / "docs" / "lonely.md").write_text("# 无人引用\n", encoding="utf-8")
        self.assertEqual(self.git("add", "-A").returncode, 0)
        self.assertEqual(self.git("commit", "-qm", "加入孤立文档").returncode, 0)

        self.assertEqual(self.git("rm", "-q", "docs/lonely.md").returncode, 0)
        result = self.git("commit", "-m", "删除无人引用的文件")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
