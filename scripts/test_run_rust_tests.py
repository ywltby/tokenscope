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
from types import SimpleNamespace
import unittest
from unittest import mock

import run_rust_tests as runner
import pre_commit_checks as checks

ROOT = Path(__file__).resolve().parent.parent


class RustTempCleanupCase(unittest.TestCase):
    def setUp(self):
        # sandbox 自身也走入口的回收链路，避免用例自己成为绕过入口的永久删除。
        self.sandbox_parent = Path(tempfile.gettempdir()).resolve()
        self.root = Path(tempfile.mkdtemp(
            prefix="tokenscope-cleanup-contract-", dir=self.sandbox_parent))
        self.addCleanup(self._discard_sandbox)
        self.parent = self.root / "临时 目录"
        self.parent.mkdir()
        self.observation = self.root / "observation.json"

    def _discard_sandbox(self):
        # 深路径回归会造出超过 MAX_PATH 的 sandbox；回收不了就保留现场并打印，
        # 这属于环境限制，由各用例自己断言「成功必须真回收、失败必须保留现场」。
        try:
            runner._cleanup(self.root, self.sandbox_parent)
        except OSError as error:
            sys.stderr.write(f"[cleanup-contract] sandbox 未被回收：{error}\n")

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
        with mock.patch.object(runner, "_send_to_recycle_bin",
                               return_value="synthetic recycle failure"):
            status, observed, output = self.child("")
        self.assertNotEqual(status, 0, "子测试通过但回收失败时，门禁必须失败")
        self.assertTrue(Path(observed["root"]).exists(), "回收失败必须保留现场")
        self.assertIn("synthetic recycle failure", output)
        self.assertIn(str(Path(observed["root"])), output)

    def test_unconfirmed_recycle_is_not_success(self):
        # 原路径消失不等于已回收：回收站里没有对应条目时（FOF_ALLOWUNDO 被降级成
        # 永久删除就是这样），必须报失败，不能把「目录不见了」当成功。
        with mock.patch.object(runner, "_recycle_bin_holds", return_value=False):
            status, observed, output = self.child("")
        self.assertNotEqual(status, 0, "无法确认已回收时门禁必须失败")
        self.assertIn("回收站", output)
        self.assertIn("永久删除", output)

    def test_unreadable_recycle_bin_is_not_success(self):
        with mock.patch.object(runner, "_recycle_bin_holds", return_value=None):
            status, _, output = self.child("")
        self.assertNotEqual(status, 0, "读不到回收站条目时不能算成功")
        self.assertIn("无法核对回收站条目", output)

    @unittest.skipUnless(os.name == "nt", "Windows 长路径回归")
    def test_cleanup_reports_deep_path_outcome_without_silently_skipping(self):
        # 契约：要么在支持扩展长度删除的环境真的删掉，要么如实报失败并保留现场，
        # 绝不允许静默跳过。宿主删除保护层拒绝扩展长度路径时属于后者。
        status, observed, output = self.child("""
            deep = work.joinpath(*(['segment'] * 40))
            extended = pathlib.Path(chr(92) * 2 + '?' + chr(92) + str(deep))
            extended.mkdir(parents=True)
            (extended / 'fixture.txt').write_text('synthetic', encoding='utf-8')
            observed['deep_length'] = len(str(deep))
        """)
        self.assertGreater(observed["deep_length"], 260)
        root = Path(observed["root"])
        if status == 0:
            self.assertFalse(root.exists(), "报告成功时临时根必须真的不存在")
            self.assertIn("回收已核实", output)
        else:
            self.assertTrue(root.exists(), "回收失败必须保留现场，不得删一半")
            self.assertIn("回收失败", output)
            self.assertIn(str(root.resolve()), output)

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


class OverlongEntryPreflightCase(unittest.TestCase):
    """回收前置条件：普通文件与目录的完整路径都必须在调用 Shell 前检查。"""

    def test_long_regular_file_is_checked_before_shell_operation(self):
        # 短根目录下放一个 254 字符合法文件名，完整路径远超 260；旧实现
        # 只检查目录栈里的路径，普通文件从不进栈，预检错误地返回 False。
        root = Path(r"C:\synthetic-root")
        entry = SimpleNamespace(
            path=str(root / ("x" * 250 + ".txt")),
            is_dir=lambda follow_symlinks=False: False,
        )
        self.assertGreater(len(entry.path), 260)
        with mock.patch.object(runner.os, "scandir",
                               return_value=contextlib.nullcontext([entry])):
            self.assertTrue(
                runner._has_overlong_entry(root),
                "预检必须覆盖普通文件的完整路径，否则 Shell 会删一半再失败",
            )

    def test_enumeration_failure_is_treated_as_overlong(self):
        root = Path(r"C:\synthetic-root")
        with mock.patch.object(runner.os, "scandir",
                               side_effect=OSError("synthetic scan failure")):
            self.assertTrue(
                runner._has_overlong_entry(root),
                "枚举失败无法证明可安全回收，必须按命中处理保留现场",
            )


class RecycleIntentCase(unittest.TestCase):
    """回收必须走明确要求回收、能拒绝永久删除的接口路径（注入模拟，不改系统策略）。"""

    def test_recycle_flags_require_recycle_and_reject_permanent_delete(self):
        flags = runner._RECYCLE_FLAGS
        self.assertTrue(flags & runner._FOFX_RECYCLEONDELETE,
                        "必须显式要求回收（FOFX_RECYCLEONDELETE）")
        self.assertTrue(flags & runner._FOFX_EARLYFAILURE,
                        "任一子操作失败必须整体停止")
        self.assertFalse(hasattr(runner, "_FOF_ALLOWUNDO"),
                         "不得依赖可静默降级成永久删除的 FOF_ALLOWUNDO")
        self.assertFalse(hasattr(runner, "_FO_DELETE"),
                         "不得保留 SHFileOperationW 删除意图")

    @unittest.skipUnless(os.name == "nt", "真实回收站核验仅 Windows")
    def test_real_recycle_places_synthetic_tree_in_recycle_bin(self):
        """正常回收：合成目录（中文/空格标记 + 深目录 + 长文件名）确实进入回收站。"""
        parent = Path(tempfile.gettempdir()).resolve()
        root = Path(tempfile.mkdtemp(prefix="tokenscope-real-recycle-", dir=parent))
        marker = root / "标记 文件 with space.txt"
        marker.write_text("synthetic", encoding="utf-8")
        deep = root / "目录 空格" / ("n" * 100 + ".txt")
        deep.parent.mkdir(parents=True)
        deep.write_text("deep-synthetic", encoding="utf-8")

        # 用真实 IFileOperation 回收；核验回收站确实记载了该原路径。
        problem = runner._send_to_recycle_bin(str(root))
        self.assertIsNone(problem, f"真实回收应成功：{problem}")
        self.assertFalse(root.exists(), "回收后原路径应消失")
        self.assertTrue(runner._recycle_bin_holds(str(root)),
                        "回收站 `$I` 必须记载本次合成目录原路径")
        # addCleanup 无需再清理：已真实进入回收站；避免二次回收不存在的路径。

    def test_disabled_recycle_preserves_source_and_fails(self):
        # 回收被禁用：PerformOperations 返回失败 HRESULT。源目录必须原样保留，
        # 且不发生任何永久删除回退。
        self._assert_refusal(perform_hr=0x80070005)  # E_ACCESSDENIED

    def test_cancelled_recycle_preserves_source_and_fails(self):
        self._assert_refusal(perform_hr=0, aborted=True)

    def test_noop_recycle_preserves_source_and_fails(self):
        self._assert_refusal(perform_hr=runner._S_FALSE)

    def test_interface_failure_preserves_source_and_fails(self):
        def broken_backend(path, flags):
            raise OSError("synthetic CoCreateInstance failure")

        root, _ = self._make_source()
        # 直接验证 backend 契约：接口失败 → 返回非零说明 → 源内容不变。
        problem = self._direct_send(str(root), broken_backend)
        self.assertIsNotNone(problem)
        self.assertIn("CoCreateInstance", problem)
        self.assertTrue((root / "标记 文件.txt").exists())

    def test_backend_receives_recycle_flags_and_path(self):
        seen = {}

        def spy(path, flags):
            seen["path"], seen["flags"] = path, flags
            return 0, False

        self.assertIsNone(runner._send_to_recycle_bin(r"C:\synthetic", _file_operation=spy))
        self.assertEqual(seen["path"], r"C:\synthetic")
        self.assertEqual(seen["flags"], runner._RECYCLE_FLAGS)

    def _direct_send(self, path, backend):
        return runner._send_to_recycle_bin(path, _file_operation=backend)

    def _make_source(self):
        parent = Path(tempfile.gettempdir()).resolve()
        root = Path(tempfile.mkdtemp(prefix="tokenscope-recycle-intent-", dir=parent))
        marker = root / "标记 文件.txt"
        marker.write_text("synthetic", encoding="utf-8")
        self.addCleanup(lambda: self._discard(root))
        return root, io.StringIO()

    def _discard(self, root):
        try:
            runner._cleanup(root, root.parent.resolve())
        except OSError as error:
            sys.stderr.write(f"[recycle-intent] sandbox 未回收：{error}\n")

    def _assert_refusal(self, *, perform_hr=0, aborted=False):
        calls = []

        def refusing_backend(path, flags):
            calls.append((path, flags))
            return perform_hr, aborted

        root, _ = self._make_source()
        problem = self._direct_send(str(root), refusing_backend)
        self.assertIsNotNone(problem, "不可回收时必须返回非零")
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1], runner._RECYCLE_FLAGS)
        self.assertTrue((root / "标记 文件.txt").exists(), "源内容必须保持")
        self.assertEqual((root / "标记 文件.txt").read_text(encoding="utf-8"), "synthetic")


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
