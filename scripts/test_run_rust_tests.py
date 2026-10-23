"""用真实子进程复现遗漏收尾和 Windows 活连接清理失败，不读取用户数据。"""

import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest import mock

import run_rust_tests as runner
import pre_commit_checks as checks

ROOT = Path(__file__).resolve().parent.parent


class RustTempCleanupCase(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory(prefix="tokenscope-cleanup-contract-")
        self.addCleanup(self.sandbox.cleanup)
        self.root = Path(self.sandbox.name)
        if os.name == "nt":
            self.addCleanup(lambda: shutil.rmtree("\\\\?\\" + str(self.root))
                            if self.root.exists() else None)
        self.parent = self.root / "临时 目录"
        self.parent.mkdir()
        self.observation = self.root / "observation.json"

    def child(self, body):
        program = "\n".join([
            "import json, os, pathlib, sqlite3, sys, tempfile",
            "root = pathlib.Path(tempfile.gettempdir())",
            "work = root / 'tokenscope-fixture'",
            "work.mkdir()",
            "(work / 'fixture.txt').write_text('synthetic', encoding='utf-8')",
            "observation = pathlib.Path(sys.argv[1])",
            "observed = {'root': str(root), 'temp_env': {k: os.environ[k] for k in ('TMP', 'TEMP', 'TMPDIR')}}",
            textwrap.dedent(body),
            "observation.write_text(json.dumps(observed), encoding='utf-8')",
            "sys.exit(observed.get('exit', 0))",
        ])
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            status = runner.run([sys.executable, "-c", program, str(self.observation)],
                                cwd=self.root, temp_parent=self.parent)
        observed = json.loads(self.observation.read_text(encoding="utf-8"))
        return status, observed, output.getvalue()

    def test_success_cleans_fixture_without_any_test_teardown(self):
        status, observed, _ = self.child("")
        self.assertEqual(status, 0)
        self.assertFalse((Path(observed["root"]) / "tokenscope-fixture").exists(),
                         "子进程没有清理逻辑时，入口仍必须收尾")
        self.assertEqual(list(self.parent.iterdir()), [])

    @unittest.skipUnless(os.name == "nt", "Windows 文件共享冲突回归")
    def test_cleanup_waits_until_sqlite_handle_is_released_by_process_exit(self):
        status, observed, _ = self.child("""
            import shutil
            connection = sqlite3.connect(work / 'history.db')
            connection.execute('CREATE TABLE fixture(value INTEGER)')
            connection.commit()
            try:
                shutil.rmtree(work)
            except OSError as error:
                observed['early_cleanup_error'] = error.winerror
            observed['database_still_exists'] = (work / 'history.db').exists()
            # 故意不 close：复现旧测试吞掉删除错误，直到进程退出才释放句柄。
        """)
        self.assertEqual(status, 0)
        self.assertIn(observed["early_cleanup_error"], (5, 32))
        self.assertTrue(observed["database_still_exists"])
        self.assertFalse(Path(observed["root"]).exists(), "进程退出后的删除必须真正成功")

    def test_temp_is_private_and_parent_environment_and_sibling_are_unchanged(self):
        before = {key: os.environ.get(key) for key in ("TMP", "TEMP", "TMPDIR")}
        sibling = self.parent / "tokenscope-existing-evidence"
        sibling.mkdir()
        marker = sibling / "keep.txt"
        marker.write_text("keep", encoding="utf-8")
        status, observed, _ = self.child("")
        self.assertEqual(status, 0)
        actual = Path(observed["root"])
        self.assertEqual(actual.parent, self.parent.resolve())
        self.assertNotEqual(actual, self.parent)
        self.assertEqual(set(observed["temp_env"].values()), {str(actual)})
        self.assertEqual({key: os.environ.get(key) for key in before}, before)
        self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
        self.assertEqual(list(self.parent.iterdir()), [sibling])

    def test_failed_test_retains_only_its_private_root_and_original_status(self):
        status, observed, output = self.child("observed['exit'] = 7")
        self.assertEqual(status, 7)
        actual = Path(observed["root"])
        self.assertEqual(actual.parent, self.parent.resolve())
        self.assertTrue((actual / "tokenscope-fixture/fixture.txt").exists())
        self.assertIn(str(actual), output)
        self.assertIn("保留", output)

    def test_cleanup_failure_cannot_be_reported_as_success(self):
        with mock.patch.object(runner.shutil, "rmtree", side_effect=PermissionError("synthetic lock")):
            status, observed, output = self.child("")
        self.assertNotEqual(status, 0, "子测试通过但清理失败时，门禁必须失败")
        self.assertTrue(Path(observed["root"]).exists())
        self.assertIn("synthetic lock", output)
        self.assertIn(str(Path(observed["root"])), output)

    @unittest.skipUnless(os.name == "nt", "Windows 长路径回归")
    def test_cleanup_removes_deep_paths_without_silently_skipping_them(self):
        status, observed, _ = self.child("""
            deep = work.joinpath(*(['segment'] * 40))
            extended = pathlib.Path(chr(92) * 2 + '?' + chr(92) + str(deep))
            extended.mkdir(parents=True)
            (extended / 'fixture.txt').write_text('synthetic', encoding='utf-8')
            observed['deep_length'] = len(str(deep))
        """)
        self.assertEqual(status, 0)
        self.assertGreater(observed["deep_length"], 260)
        self.assertFalse(Path(observed["root"]).exists())

    def test_launch_error_fails_and_reports_the_private_root(self):
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            status = runner.run([str(self.root / "missing-command")],
                                cwd=self.root, temp_parent=self.parent)
        self.assertEqual(status, 127)
        retained = list(self.parent.iterdir())
        self.assertEqual(len(retained), 1)
        # runner 打印的是 resolve 后的临时根（Windows 上展开 8.3 短名；
        # GitHub Runner 的 TEMP 即短名形式），比较前同样 resolve。
        self.assertIn(str(retained[0].resolve()), output.getvalue())

    def test_interruption_preserves_evidence_and_is_not_success(self):
        output = io.StringIO()
        with (contextlib.redirect_stderr(output),
              mock.patch.object(runner.subprocess, "run", side_effect=KeyboardInterrupt)):
            status = runner.run([sys.executable], cwd=self.root, temp_parent=self.parent)
        self.assertEqual(status, 130)
        retained = list(self.parent.iterdir())
        self.assertEqual(len(retained), 1)
        self.assertIn(str(retained[0].resolve()), output.getvalue())

    @unittest.skipUnless(os.name == "nt", "Windows 目录联接边界")
    def test_cleanup_does_not_follow_junction_outside_owned_root(self):
        outside = self.root / "outside"
        outside.mkdir()
        sentinel = outside / "keep.txt"
        sentinel.write_text("keep", encoding="utf-8")
        status, _, _ = self.child(f"""
            import subprocess
            subprocess.run([os.environ.get('COMSPEC', 'cmd.exe'), '/c', 'mklink', '/J',
                            str(work / 'link'), {str(outside)!r}],
                           check=True, stdout=subprocess.DEVNULL)
        """)
        self.assertEqual(status, 0)
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")
        self.assertEqual(list(self.parent.iterdir()), [])


class WiringCase(unittest.TestCase):
    def test_cli_forwards_every_cargo_argument_and_exit_status(self):
        command = ["cargo", "test", "--locked", "-p", "tokenscope", "--test",
                   "codex_parse_revision", "--", "--nocapture", "--test-threads=1"]
        with mock.patch.object(runner, "run", return_value=7) as execute:
            status = runner.main(["--", *command])
        self.assertEqual(status, 7)
        execute.assert_called_once_with(command, cwd=Path.cwd())

    def test_cli_rejects_unrelated_commands_without_running_them(self):
        for command in ([], ["cargo", "clean"], ["python", "-c", "pass"]):
            with (self.subTest(command=command),
                  contextlib.redirect_stderr(io.StringIO()),
                  mock.patch.object(runner, "run") as execute):
                self.assertNotEqual(runner.main(command), 0)
                execute.assert_not_called()

    def test_every_local_rust_test_command_uses_cleanup_without_changing_argv(self):
        for group in ("rust-core-default", "rust-core-acceptance",
                      "rust-shell-default", "rust-shell-acceptance"):
            command = next(c for c in checks.expand_commands([group]) if c[:2] == ["cargo", "test"])
            with (self.subTest(group=group),
                  mock.patch.object(runner, "run", return_value=7) as execute,
                  mock.patch.object(checks.subprocess, "run", return_value=subprocess.CompletedProcess(command, 0))):
                status = checks._execute(command, ROOT)
            self.assertEqual(status, 7)
            execute.assert_called_once_with(command, cwd=ROOT)

    def test_runner_and_its_contracts_require_full_gate(self):
        for path in ("scripts/run_rust_tests.py", "scripts/test_run_rust_tests.py"):
            with self.subTest(path=path):
                self.assertTrue(checks.is_full_trigger(path))

    def test_unstaged_runner_is_an_input_of_rust_checks(self):
        self.assertEqual(checks._blocked_inputs(["rust-core-default"],
                                               ["scripts/run_rust_tests.py"]),
                         ["scripts/run_rust_tests.py"])

    def test_ci_wraps_both_existing_workspace_rounds_and_runs_cleanup_contracts(self):
        action = (ROOT / ".github/actions/checks/action.yml").read_text(encoding="utf-8")
        self.assertIn("run: python -B scripts/run_rust_tests.py -- cargo test --locked --workspace\n", action)
        self.assertIn("run: python -B scripts/run_rust_tests.py -- cargo test --locked --workspace --features acceptance\n", action)
        self.assertIn('python -B -m unittest discover -s scripts -p "test_run_rust_tests.py" -v', action)


if __name__ == "__main__":
    unittest.main()
