"""原生场景契约临时目录清理的外层回归（测试临时目录清理计划 TL02）。

测试自身通过不代表临时目录被清理：泄漏只能由真实 Vitest 子进程退出后的
外层文件系统核实。本契约在独立私有根中运行仓库现有的
`scripts/lib/native-scenario.test.mjs`「父进程设上限」用例，覆盖三条路径：

1. 正常通过：不注入任何错误，私有根不得残留 `native-scenario-test-*`。
2. 测试中途失败：仅对该用例的 `evidence.json` 读取注入带固定标记的抛错，
   目录仍必须被清理。
3. 清理失败：仅对该用例目录的 `fs.rmSync` 注入带固定标记的删除错误，
   Vitest 必须非零退出且输出标记；残留只允许出现在外层私有根。

另有第四条反例：合成子进程先产出一个继承输出管道的长寿后代再驻留，
用短预算证明外层超时会做有界进程树收尾、落盘已捕获输出、后代确实被
终止（PR #44 复审补修；三条真实路径仍保持 30 秒预算）。

全部子进程 stdout/stderr 与 JSON 报告落盘保存；外层真实失败时证据先移入
`qa-artifacts/test-temp-cleanup/contract-evidence/` 再上抛（CI 按失败产物
上传），成功时正常清理。核对原用例确实执行且只有该用例执行，全跳过、
报告缺失或配置加载失败一律判失败。每次外层同步调用不超过其预算，超时
判失败，不增加等待补偿；找不到 pnpm 也明确失败。
"""

import functools
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parent.parent
FRONTEND_DIR = "frontend"
TEST_FILE = "scripts/lib/native-scenario.test.mjs"
TEST_FILTER = "父进程设上限"
TEMP_DIR_PREFIX = "native-scenario-test-"
SENTINEL_NAME = "outer-sentinel.txt"
SENTINEL_BYTES = b"tokenscope temp-cleanup outer sentinel\n"
MARK_READ = "TOKENSCOPE_TEMP_CLEANUP_INJECTED_READ_FAILURE"
MARK_RM = "TOKENSCOPE_TEMP_CLEANUP_INJECTED_RM_FAILURE"
CALL_TIMEOUT_S = 30
TEARDOWN_GRACE_S = 10
EVIDENCE_DIR = ROOT / "qa-artifacts" / "test-temp-cleanup" / "contract-evidence"
DESCENDANT_PID_ENV = "TOKENSCOPE_DESCENDANT_PID_FILE"

# 复审补修反例（合成子进程，符合计划不变量 7 的注入边界）：父进程先产出一个
# 继承输出管道的长寿后代（寿命必须长于反例预算，才能暴露无界排空），把后代
# PID 写入指定文件后驻留。
HANG_DESCENDANT_SLEEP_S = 3
HANG_PARENT_PROGRAM = (
    "import os, subprocess, sys, time\n"
    "child = subprocess.Popen([sys.executable, '-c', "
    f"'import time; time.sleep({HANG_DESCENDANT_SLEEP_S}); print(\"descendant-ended\", flush=True)'])\n"
    "with open(os.environ[" + json.dumps(DESCENDANT_PID_ENV) + "], \"w\") as handle:\n"
    "    handle.write(str(child.pid))\n"
    "print('parent-ready', flush=True)\n"
    "time.sleep(10)\n"
)

SETUP_READ_FAIL_TEMPLATE = """\
// 外层契约注入：仅对被测用例的 evidence.json 读取抛带固定标记的错误，其余读取透传。
import fs from "node:fs";
const MARK = {mark};
const original = fs.readFileSync;
fs.readFileSync = function patched(target, ...rest) {{
  const text =
    typeof target === "string" ? target
    : target instanceof URL ? target.href
    : String(target && target.path !== undefined ? target.path : target);
  if (typeof text === "string" && text.includes({prefix}) && text.endsWith("evidence.json")) {{
    throw new Error(`${{MARK}}: ${{text}}`);
  }}
  return original.call(fs, target, ...rest);
}};
"""

SETUP_RM_FAIL_TEMPLATE = """\
// 外层契约注入：仅对被测用例目录的 rmSync 抛带固定标记的删除错误，其余删除透传。
import fs from "node:fs";
const MARK = {mark};
const original = fs.rmSync;
fs.rmSync = function patched(target, ...rest) {{
  const text =
    typeof target === "string" ? target
    : target instanceof URL ? target.href
    : String(target && target.path !== undefined ? target.path : target);
  if (typeof text === "string" && text.includes({prefix})) {{
    throw new Error(`${{MARK}}: ${{text}}`);
  }}
  return original.call(fs, target, ...rest);
}};
"""

CONFIG_TEMPLATE = """\
// 外层契约生成的临时 Vitest 配置：显式只限定原测试文件，使用项目内安装的 Vitest。
const setupFiles = {setup_files};
export default {{
  root: {frontend_root},
  test: {{
    environment: "node",
    include: [{test_file}],
    ...(setupFiles.length > 0 ? {{ setupFiles }} : {{}}),
  }},
}};
"""


def _decode(data):
    return data.decode("utf-8", errors="replace")


def _keep_evidence_on_failure(method):
    """外层真实失败时先把证据移出会被清理的私有根再上抛；成功时正常清理。"""
    @functools.wraps(method)
    def wrapper(self):
        try:
            method(self)
        except BaseException:
            preserved = self._preserve_evidence()
            print(f"[temp-cleanup] 外层真实失败，证据已保存：{preserved}",
                  file=sys.stderr)
            raise
        self._discard_evidence()
    return wrapper


class NativeScenarioTempCleanupCase(unittest.TestCase):
    def setUp(self):
        self._keeper = tempfile.TemporaryDirectory(
            prefix="tokenscope-temp-cleanup-contract-")
        self.addCleanup(self._keeper.cleanup)
        self.root = Path(self._keeper.name)
        self.evidence = self.root / "evidence"
        self.evidence.mkdir()
        self.sentinel = self.root / SENTINEL_NAME
        self.sentinel.write_bytes(SENTINEL_BYTES)

    # —— 失败证据保存 ——

    def _preserve_evidence(self):
        """把证据目录整体移入独立证据目录，并补一份私有根清单（残留名与大小）。"""
        stamp = time.strftime("%Y%m%d-%H%M%S", time.gmtime())
        target = EVIDENCE_DIR / f"{stamp}-{self._testMethodName}"
        target.mkdir(parents=True, exist_ok=True)
        if self.evidence.exists():
            shutil.move(str(self.evidence), str(target / self.evidence.name))
        lines = []
        for entry in sorted(self.root.rglob("*")):
            relative = entry.relative_to(self.root)
            if entry.is_dir():
                lines.append(f"dir  {relative}")
            else:
                try:
                    size = entry.stat().st_size
                except OSError:
                    size = -1
                lines.append(f"file {relative} {size}B")
        (target / "private-root-listing.txt").write_text(
            "\n".join(lines) or "(私有根为空)", encoding="utf-8")
        return str(target)

    def _discard_evidence(self):
        try:
            shutil.rmtree(self.evidence)
        except FileNotFoundError:
            pass

    # —— 外层调用（pnpm 可执行文件解析方式沿用 scripts/pre_commit_checks.py：Windows 上
    #    .cmd/.bat 显式经由 cmd /c，解析不到即明确失败）——

    def _argv(self, command):
        executable = shutil.which(command[0])
        if executable is None:
            self.fail(f"找不到可执行文件：{command[0]}（拒绝在无验证的情况下判通过）")
        if os.name == "nt" and executable.lower().endswith((".cmd", ".bat")):
            shell = os.environ.get("COMSPEC", "cmd.exe")
            return [shell, "/c", executable, *command[1:]]
        return [executable, *command[1:]]

    def _kill_process_tree(self, proc):
        """有界进程树收尾：Windows 用 taskkill /T /F，POSIX 杀整个进程组。

        尽力而为：进程可能已自行退出（竞态），失败不外抛，返回值由调用方
        写进证据，供反例核对后代确实终止。"""
        try:
            if os.name == "nt":
                done = subprocess.run(
                    ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                    capture_output=True, timeout=TEARDOWN_GRACE_S)
                if done.returncode != 0:
                    return (_decode(done.stderr).strip()
                            or f"taskkill 退出码 {done.returncode}")
                return None
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            return None
        except (OSError, subprocess.TimeoutExpired) as exc:
            return str(exc)

    def _pid_alive(self, pid):
        if os.name == "nt":
            listing = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV"],
                capture_output=True, timeout=TEARDOWN_GRACE_S)
            rows = [line for line in _decode(listing.stdout).splitlines()
                    if line.startswith('"')]
            return bool(rows)
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def _execute_case(self, case_name, argv, env, budget_s):
        """在预算内有界运行一次外层子进程；超时按进程树收尾并保存已捕获输出。

        `communicate` 超时后只杀直接进程是不够的：后代进程会继续持有输出
        管道，让管道排空和等待无限延长（PR #44 复审缺陷 2）。这里先整树
        强杀，再有界排空；仍超时才直关管道并改用超时异常里的部分输出。"""
        started = time.monotonic()
        try:
            proc = subprocess.Popen(argv, cwd=ROOT, env=env,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    start_new_session=(os.name != "nt"))
        except OSError as exc:
            self.fail(f"无法启动外层子进程 {argv[0]}：{exc}")
        kind = "completed"
        stdout_data = stderr_data = b""
        kill_error = None
        try:
            stdout_data, stderr_data = proc.communicate(timeout=budget_s)
        except subprocess.TimeoutExpired as expired:
            kind = "timeout"
            partial_stdout = expired.stdout or b""
            partial_stderr = expired.stderr or b""
            kill_error = self._kill_process_tree(proc)
            try:
                stdout_data, stderr_data = proc.communicate(timeout=TEARDOWN_GRACE_S)
            except subprocess.TimeoutExpired:
                for stream in (proc.stdout, proc.stderr):
                    if stream is not None:
                        try:
                            stream.close()
                        except OSError:
                            pass
                stdout_data, stderr_data = partial_stdout, partial_stderr
                try:
                    proc.wait(timeout=TEARDOWN_GRACE_S)
                except subprocess.TimeoutExpired:
                    proc.kill()
        elapsed = time.monotonic() - started

        stdout_text = _decode(stdout_data or b"")
        stderr_text = _decode(stderr_data or b"")
        (self.evidence / f"stdout-{case_name}.log").write_text(
            stdout_text, encoding="utf-8")
        (self.evidence / f"stderr-{case_name}.log").write_text(
            stderr_text, encoding="utf-8")
        if kill_error is not None:
            (self.evidence / f"kill-{case_name}.log").write_text(
                f"{kill_error}\n", encoding="utf-8")
        return SimpleNamespace(kind=kind, returncode=proc.returncode,
                               stdout_text=stdout_text,
                               stderr_text=stderr_text,
                               report=None, elapsed_s=elapsed)

    def _run_vitest(self, case_name, *, setup_template=None, mark=None,
                    budget_s=None, argv_override=None, env_extra=None):
        setup_files = []
        if setup_template is not None:
            setup_path = self.root / f"setup-{case_name}.mjs"
            setup_path.write_text(
                setup_template.format(mark=json.dumps(mark),
                                      prefix=json.dumps(TEMP_DIR_PREFIX)),
                encoding="utf-8")
            setup_files = [setup_path]

        # 只向实际子进程注入三个临时环境变量；父环境原样保留。
        env = {**os.environ,
               "TMP": str(self.root), "TEMP": str(self.root),
               "TMPDIR": str(self.root), **(env_extra or {})}
        if argv_override is not None:
            command = argv_override
        else:
            config_path = self.root / f"vitest-{case_name}.config.mjs"
            config_path.write_text(CONFIG_TEMPLATE.format(
                setup_files=json.dumps([str(p) for p in setup_files]),
                frontend_root=json.dumps(str(ROOT / FRONTEND_DIR)),
                test_file=json.dumps(TEST_FILE)), encoding="utf-8")
            command = self._argv([
                "pnpm", "--dir", FRONTEND_DIR, "exec", "vitest", "--run",
                "--config", str(config_path),
                "--reporter=default", "--reporter=json",
                f"--outputFile.json={self.evidence / f'report-{case_name}.json'}",
                "-t", TEST_FILTER])
        outcome = self._execute_case(case_name, command, env,
                                     budget_s if budget_s is not None else CALL_TIMEOUT_S)

        report_path = self.evidence / f"report-{case_name}.json"
        if report_path.exists():
            try:
                outcome.report = json.loads(report_path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                outcome.report = None
        return outcome

    # —— 断言辅助 ——

    def _require_completed(self, outcome):
        if outcome.kind != "completed":
            self.fail(
                f"外层调用超时（预算内未完成，实际 {outcome.elapsed_s:.1f}s），"
                f"已按进程树有界收尾；已捕获输出存于 {self.evidence}")

    def _require_single_executed(self, outcome, expected_status):
        if outcome.report is None:
            self.fail("JSON 报告缺失或不可解析：配置加载失败、全跳过或没有实际执行"
                      "用例，都不能当作清理成功（stdout/stderr 已存证据目录）")
        executed = []
        for suite in outcome.report.get("testResults", []):
            for assertion in suite.get("assertionResults", []):
                if assertion.get("status") in ("passed", "failed"):
                    executed.append(assertion)
        names = [item.get("fullName", "") for item in executed]
        self.assertEqual(len(executed), 1,
                         f"应恰好实际执行 1 个用例，实际：{names}")
        self.assertIn(TEST_FILTER, names[0],
                      f"实际执行的用例不是目标用例：{names[0]}")
        self.assertEqual(executed[0].get("status"), expected_status,
                         f"用例状态应为 {expected_status}：{names[0]}")

    def _residue(self):
        return sorted(entry.name for entry in self.root.iterdir()
                      if entry.is_dir() and entry.name.startswith(TEMP_DIR_PREFIX))

    def _assert_sentinel_intact(self):
        self.assertEqual(self.sentinel.read_bytes(), SENTINEL_BYTES,
                         "同级哨兵字节被改动：清理越出了自身私有根")

    def _assert_no_residue(self):
        residue = self._residue()
        self.assertEqual(
            residue, [],
            f"Vitest 退出后私有根仍残留 {TEMP_DIR_PREFIX}* 目录：{residue}")
        self._assert_sentinel_intact()

    # —— 三条真实路径（30 秒预算不变）——

    @_keep_evidence_on_failure
    def test_normal_pass_still_cleans_own_temp_dir(self):
        outcome = self._run_vitest("normal")
        self._require_completed(outcome)
        self.assertEqual(
            outcome.returncode, 0,
            f"Vitest 应正常通过（{outcome.elapsed_s:.1f}s）：\n"
            f"{outcome.stdout_text}\n{outcome.stderr_text}")
        self._require_single_executed(outcome, "passed")
        self._assert_no_residue()

    @_keep_evidence_on_failure
    def test_midtest_failure_still_cleans_own_temp_dir(self):
        outcome = self._run_vitest("midfail", setup_template=SETUP_READ_FAIL_TEMPLATE,
                                   mark=MARK_READ)
        self._require_completed(outcome)
        self.assertNotEqual(
            outcome.returncode, 0,
            "注入的读取失败必须让 Vitest 非零退出，注入失败路径不能假通过")
        combined = outcome.stdout_text + outcome.stderr_text
        self.assertIn(MARK_READ, combined,
                      "失败输出必须包含注入标记，证明失败来自注入而非配置/依赖问题")
        self._require_single_executed(outcome, "failed")
        self._assert_no_residue()

    @_keep_evidence_on_failure
    def test_cleanup_failure_surfaces_and_keeps_scene(self):
        outcome = self._run_vitest("rmfail", setup_template=SETUP_RM_FAIL_TEMPLATE,
                                   mark=MARK_RM)
        self._require_completed(outcome)
        self.assertNotEqual(
            outcome.returncode, 0,
            "注入的删除失败必须让 Vitest 非零退出；清理失败不能被吞成通过")
        combined = outcome.stdout_text + outcome.stderr_text
        self.assertIn(MARK_RM, combined, "失败输出必须包含删除注入标记")
        self.assertIn(TEMP_DIR_PREFIX, combined,
                      "删除失败输出应指向被清理的临时目录")
        self._require_single_executed(outcome, "failed")
        residue = self._residue()
        self.assertEqual(
            len(residue), 1,
            f"清理失败时应恰好保留一个现场目录（残留只在外层私有根）：{residue}")
        self._assert_sentinel_intact()

    # —— 超时收尾反例（短预算；不改变三条真实路径的 30 秒预算）——

    @_keep_evidence_on_failure
    def test_timeout_teardown_bounds_descendant_and_saves_partial_output(self):
        # 合成子进程反例：父进程先产出一个继承输出管道的长寿后代再驻留。
        # PR #44 复审缺陷 2：旧实现 subprocess.run(timeout=…) 只杀直接进程，
        # 管道被后代持有时会无界排空（预算 0.5s 实测 2.102s 才返回），且超时
        # 输出不落盘。这里用短预算证明：超时必有界、已捕获输出保留、后代
        # 确实被进程树收尾终止。
        budget_s = 1.0
        pid_path = self.root / "descendant.pid"
        outcome = self._run_vitest(
            "hang", budget_s=budget_s,
            argv_override=[sys.executable, "-c", HANG_PARENT_PROGRAM],
            env_extra={DESCENDANT_PID_ENV: str(pid_path)})
        self.assertEqual(outcome.kind, "timeout",
                         "短预算必须触发超时路径，否则反例无效")
        self.assertLess(
            outcome.elapsed_s, budget_s + TEARDOWN_GRACE_S + 10,
            f"超时收尾必须有界（预算 {budget_s}s），实际 {outcome.elapsed_s:.2f}s")
        self.assertIn("parent-ready", outcome.stdout_text,
                      "超时前已捕获的输出必须保留")
        self.assertNotIn(
            "descendant-ended", outcome.stdout_text,
            "后代寿命长于预算时不得等到它的输出（存在无界排空）")
        self.assertTrue(pid_path.exists(), "反例后代未按预期产出 PID 文件")
        descendant_pid = int(pid_path.read_text(encoding="utf-8").strip())
        self.assertFalse(self._pid_alive(descendant_pid),
                         f"后代进程 {descendant_pid} 未被进程树收尾终止")
        self._assert_sentinel_intact()


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    unittest.main()
