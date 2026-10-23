"""原生场景契约临时目录清理的外层回归（测试临时目录清理计划 TL02）。

测试自身通过不代表临时目录被清理：泄漏只能由真实 Vitest 子进程退出后的
外层文件系统核实。本契约在独立私有根中运行仓库现有的
`scripts/lib/native-scenario.test.mjs`「父进程设上限」用例，覆盖三条路径：

1. 正常通过：不注入任何错误，私有根不得残留 `native-scenario-test-*`。
2. 测试中途失败：仅对该用例的 `evidence.json` 读取注入带固定标记的抛错，
   目录仍必须被清理。
3. 清理失败：仅对该用例目录的 `fs.rmSync` 注入带固定标记的删除错误，
   Vitest 必须非零退出且输出标记；残留只允许出现在外层私有根。

另有两条短反例（合成子进程，符合计划不变量 7 的注入边界）：父进程产出
继承输出管道的长寿后代后，或驻留、或正常退出留下孤儿后代，用短预算证明
外层超时受统一截止时间约束（等待不超过预算，收尾只追加固定上限）、已捕获
输出落盘、后代——包括父进程已退出的孤儿——确实被终止。三条真实路径仍
保持 30 秒预算。

全部子进程 stdout/stderr 与 JSON 报告落盘保存；外层真实失败时证据先移入
`qa-artifacts/test-temp-cleanup/contract-evidence/` 再上抛（CI 按失败产物
上传），成功时正常清理。核对原用例确实执行且只有该用例执行，全跳过、
报告缺失或配置加载失败一律判失败。每次外层同步调用不超过其预算，超时
判失败，不增加等待补偿；找不到 pnpm 也明确失败。
"""

import ctypes
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
# 超时后收尾（终止进程树 + 排空缓冲）的固定上限：不随预算缩放、不串联多次。
# 等待阶段受预算统一截止，收尾只在预算耗尽后追加这至多 2 秒。
TEARDOWN_CAP_S = 2.0
EVIDENCE_DIR = ROOT / "qa-artifacts" / "test-temp-cleanup" / "contract-evidence"
DESCENDANT_PID_ENV = "TOKENSCOPE_DESCENDANT_PID_FILE"

# 复审补修反例（合成子进程，符合计划不变量 7 的注入边界）：父进程先产出一个
# 继承输出管道的长寿后代（寿命必须长于反例预算，才能暴露无界排空），把后代
# PID 写入指定文件。parent_lingers=False 时父进程随即正常退出——覆盖「父已
# 退出、孤儿后代持有管道」的边界（复审第二轮缺陷 1），收尾不得依赖父 PID 存活。
HANG_DESCENDANT_SLEEP_S = 3
EARLY_PARENT_DESCENDANT_SLEEP_S = 5


def _pipe_holding_program(descendant_sleep_s, *, parent_lingers):
    return (
        "import os, subprocess, sys, time\n"
        "child = subprocess.Popen([sys.executable, '-c', "
        f"'import time; time.sleep({descendant_sleep_s}); print(\"descendant-ended\", flush=True)'])\n"
        "with open(os.environ[" + json.dumps(DESCENDANT_PID_ENV) + "], \"w\") as handle:\n"
        "    handle.write(str(child.pid))\n"
        "print('parent-ready', flush=True)\n"
        + ("time.sleep(10)\n" if parent_lingers else "")
    )


HANG_PARENT_PROGRAM = _pipe_holding_program(HANG_DESCENDANT_SLEEP_S, parent_lingers=True)

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


def _kernel32():
    """配置好签名绑定的 kernel32（句柄用 c_void_p，避免 64 位截断）。"""
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateJobObjectW.restype = ctypes.c_void_p
    kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    kernel32.SetInformationJobObject.argtypes = [
        ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    kernel32.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    kernel32.TerminateJobObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    return kernel32


def _create_kill_on_close_job():
    """Windows：创建带 KILL_ON_JOB_CLOSE 的作业对象；失败返回 None，调用方回退。

    作业在 Popen 返回后立即挂入子进程，其后代自动入作业：即使父进程先退出，
    终止作业也能连带终止孤儿后代——收尾不依赖父 PID 存活（复审第二轮缺陷 1）。"""
    if os.name != "nt":
        return None
    from ctypes import wintypes

    kernel32 = _kernel32()
    job = kernel32.CreateJobObjectW(None, None)
    if not job:
        return None

    class _IoCounters(ctypes.Structure):
        _fields_ = [(name, ctypes.c_uint64) for name in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class _BasicLimits(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.c_size_t),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD)]

    class _ExtendedLimits(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", _BasicLimits),
            ("IoInfo", _IoCounters),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t)]

    limits = _ExtendedLimits()
    limits.BasicLimitInformation.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    if not kernel32.SetInformationJobObject(job, 9, ctypes.byref(limits),
                                            ctypes.sizeof(limits)):
        kernel32.CloseHandle(job)
        return None
    return job


def _assign_to_job(job, proc):
    """把子进程挂入作业；其后代自动入作业。返回是否成功（失败由调用方回退）。"""
    return bool(_kernel32().AssignProcessToJobObject(job, int(proc._handle)))


def _close_job(job):
    """关闭作业句柄；KILL_ON_JOB_CLOSE 会终止作业内仍存活的任何进程。"""
    _kernel32().CloseHandle(job)


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

    def _kill_process_tree(self, proc, job):
        """有界进程树收尾，不依赖父 PID 存活：优先终止整个作业（覆盖父进程已
        退出的孤儿后代）；无作业时 Windows 回退 taskkill /T /F，POSIX 终止
        进程组。尽力而为：进程可能已自行退出（竞态），失败不外抛，返回值由
        调用方写进证据，供反例核对后代确实终止。"""
        try:
            if job is not None:
                if _kernel32().TerminateJobObject(job, 1):
                    return None
                return (f"TerminateJobObject 失败"
                        f"（GetLastError={ctypes.get_last_error()}）")
            if os.name == "nt":
                done = subprocess.run(
                    ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                    capture_output=True, timeout=TEARDOWN_CAP_S)
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
                capture_output=True, timeout=TEARDOWN_CAP_S)
            rows = [line for line in _decode(listing.stdout).splitlines()
                    if line.startswith('"')]
            return bool(rows)
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def _execute_case(self, case_name, argv, env, budget_s):
        """在统一截止时间内运行一次外层子进程，超时按进程树收尾并保存输出。

        等待阶段受预算约束（deadline = 启动 + 预算）；超时后先终止整棵进程
        树——Windows 用 KILL_ON_JOB_CLOSE 作业（后代自动入作业，父进程先退出
        的孤儿也一并终止，收尾不依赖父 PID），POSIX 终止进程组——再以固定
        TEARDOWN_CAP_S 上限排空缓冲：整树终止后写端全部关闭，EOF 即到，通常
        立刻返回。排空仍超时则改用超时异常里的部分输出，且不再触碰管道对象：
        BufferedReader.close() 会阻塞在幸存读取线程持有的锁上（复审第二轮
        实测把 0.2s 宽限拖成 2.101s 的机制）。"""
        started = time.monotonic()
        deadline = started + budget_s
        try:
            proc = subprocess.Popen(argv, cwd=ROOT, env=env,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    start_new_session=(os.name != "nt"))
        except OSError as exc:
            self.fail(f"无法启动外层子进程 {argv[0]}：{exc}")
        job = _create_kill_on_close_job()
        if job is not None and not _assign_to_job(job, proc):
            _close_job(job)
            job = None
        kind = "completed"
        stdout_data = stderr_data = b""
        kill_error = None
        try:
            stdout_data, stderr_data = proc.communicate(
                timeout=max(0.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as expired:
            kind = "timeout"
            kill_error = self._kill_process_tree(proc, job)
            try:
                stdout_data, stderr_data = proc.communicate(timeout=TEARDOWN_CAP_S)
            except subprocess.TimeoutExpired:
                # 树终止失败时的兜底：不 close 管道对象（会阻塞在幸存读取线程
                # 的锁上），直接采用部分输出并放弃句柄，交由解释器退出回收。
                stdout_data = expired.stdout or b""
                stderr_data = expired.stderr or b""
        finally:
            if job is not None:
                # KILL_ON_JOB_CLOSE：关闭句柄会终止作业内仍存活的进程，正常
                # 完成时是对残留后代的最后兜底。
                _close_job(job)
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
                f"外层调用超时（预算 {CALL_TIMEOUT_S}s + 收尾上限 {TEARDOWN_CAP_S}s "
                f"内未完成，实际 {outcome.elapsed_s:.1f}s），已按统一截止时间收尾；"
                f"已捕获输出存于 {self.evidence}")

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
            outcome.elapsed_s, budget_s + TEARDOWN_CAP_S + 1.0,
            f"统一截止时间必须约束等待与收尾（预算 {budget_s}s + 固定收尾上限 "
            f"{TEARDOWN_CAP_S}s），实际 {outcome.elapsed_s:.2f}s")
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

    @_keep_evidence_on_failure
    def test_early_parent_exit_orphan_is_bounded_killed_and_recorded(self):
        # 父进程先退出的反例（复审第二轮缺陷 1）：父进程产出继承输出管道的
        # 长寿后代后正常退出——收尾不能依赖父 PID 存活。旧机制实测（用户
        # review_probes_round2.json）：taskkill 报父 PID 不存在，后代自然跑完，
        # 预算 0.3s + 宽限 0.2s 拖到 2.101s，保存的 stdout 为空。这里预算
        # 0.5s、后代 5s：断言超时触发、耗时有界、父进程已产出的输出保留、
        # 孤儿后代被终止、哨兵不变。
        budget_s = 0.5
        pid_path = self.root / "orphan.pid"
        outcome = self._run_vitest(
            "early-parent", budget_s=budget_s,
            argv_override=[sys.executable, "-c", _pipe_holding_program(
                EARLY_PARENT_DESCENDANT_SLEEP_S, parent_lingers=False)],
            env_extra={DESCENDANT_PID_ENV: str(pid_path)})
        self.assertEqual(outcome.kind, "timeout",
                         "短预算必须触发超时路径，否则反例无效")
        self.assertLess(
            outcome.elapsed_s, budget_s + TEARDOWN_CAP_S + 1.0,
            f"统一截止时间必须约束等待与收尾（预算 {budget_s}s + 固定收尾上限 "
            f"{TEARDOWN_CAP_S}s），实际 {outcome.elapsed_s:.2f}s")
        self.assertIn("parent-ready", outcome.stdout_text,
                      "父进程自然退出，其已产出的输出仍必须落盘保留")
        self.assertNotIn(
            "descendant-ended", outcome.stdout_text,
            "孤儿后代寿命长于预算时不得等到它的输出（不得无界排空）")
        self.assertTrue(pid_path.exists(), "反例后代未按预期产出 PID 文件")
        orphan_pid = int(pid_path.read_text(encoding="utf-8").strip())
        self.assertFalse(self._pid_alive(orphan_pid),
                         f"父进程已退出，孤儿后代 {orphan_pid} 仍存活："
                         "收尾不能依赖父 PID")
        self._assert_sentinel_intact()


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    unittest.main()
