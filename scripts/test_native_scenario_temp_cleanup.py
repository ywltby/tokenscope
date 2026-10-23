"""原生场景契约临时目录清理的外层回归（测试临时目录清理计划 TL02）。

测试自身通过不代表临时目录被清理：泄漏只能由真实 Vitest 子进程退出后的
外层文件系统核实。本契约在独立私有根中运行仓库现有的
`scripts/lib/native-scenario.test.mjs`「父进程设上限」用例，覆盖三条路径：

1. 正常通过：不注入任何错误，私有根不得残留 `native-scenario-test-*`。
2. 测试中途失败：仅对该用例的 `evidence.json` 读取注入带固定标记的抛错，
   目录仍必须被清理。
3. 清理失败：仅对该用例目录的 `fs.rmSync` 注入带固定标记的删除错误，
   Vitest 必须非零退出且输出标记；残留只允许出现在外层私有根。

另有三条短反例（合成子进程，符合计划不变量 7 的注入边界）：父进程产出
长寿后代后驻留、或正常退出留下孤儿后代、或受控时序强制「后代先于作业
分配创建」，用短预算证明外层超时受原预算内统一截止约束（含收尾预留）、
输出落盘、后代——包括孤儿与逃逸后代——确实被终止。三条真实路径仍保持
30 秒预算。

全部子进程 stdout/stderr 直接写入私有日志文件（不依赖管道 EOF，超时后
读取已有内容）；外层真实失败时证据先移入
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
from unittest import mock

_CONTRACT_MODULE = sys.modules[__name__]

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
# 收尾（终止进程树 + 等待退出）在原预算内部预留的时间：所有等待共用同一
# 截止时间（启动 + 预算），不追加任何预算外的等待或收尾。
TEARDOWN_RESERVE_S = 0.5
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


def _read_log(path):
    """读取子进程直写的日志文件；读不到时留空，不掩盖后续断言。"""
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


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
    kernel32.OpenThread.restype = ctypes.c_void_p
    kernel32.OpenThread.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
    kernel32.ResumeThread.restype = ctypes.c_uint32
    kernel32.ResumeThread.argtypes = [ctypes.c_void_p]
    kernel32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
    kernel32.CreateToolhelp32Snapshot.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
    kernel32.Thread32First.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    kernel32.Thread32Next.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
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


def _open_main_thread(proc):
    """打开子进程主线程的 THREAD_SUSPEND_RESUME 句柄（Popen 不暴露该句柄）。

    挂起创建的子进程此刻只有主线程一个线程，用工具帮助快照按进程枚举取得；
    若已有其他线程 id 可用则直接打开。"""
    tid = getattr(proc, "_tid", None)
    if tid is not None:
        return _kernel32().OpenThread(0x0002, False, tid) or None  # THREAD_SUSPEND_RESUME
    return _open_only_thread(proc.pid)


def _open_only_thread(pid):
    """枚举目标进程的线程并打开唯一线程；挂起创建的子进程只有主线程一个，
    出现多个线程说明进程已开始运行（违背挂起前提），宁可失败也不误恢复。"""
    from ctypes import wintypes

    class _ThreadEntry32(ctypes.Structure):
        _fields_ = [
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ThreadID", wintypes.DWORD),
            ("th32OwnerProcessID", wintypes.DWORD),
            ("tpBasePri", ctypes.c_long),
            ("tpDeltaPri", ctypes.c_long),
            ("dwFlags", wintypes.DWORD)]

    kernel32 = _kernel32()
    snapshot = kernel32.CreateToolhelp32Snapshot(0x4, 0)  # TH32CS_SNAPTHREAD
    if not snapshot or snapshot == ctypes.c_void_p(-1).value:
        return None
    try:
        entry = _ThreadEntry32()
        entry.dwSize = ctypes.sizeof(_ThreadEntry32)
        found = None
        alive = kernel32.Thread32First(snapshot, ctypes.byref(entry))
        while alive:
            if entry.th32OwnerProcessID == pid:
                if found is not None:
                    return None
                found = entry.th32ThreadID
            alive = kernel32.Thread32Next(snapshot, ctypes.byref(entry))
    finally:
        kernel32.CloseHandle(snapshot)
    if found is None:
        return None
    return kernel32.OpenThread(0x0002, False, found) or None  # THREAD_SUSPEND_RESUME


def _spawn(argv, cwd, env, stdout_file, stderr_file):
    """启动外层子进程并返回 (proc, job)；作业不可用时显式抛错，不降级。

    Windows：挂起创建（CREATE_SUSPENDED）→ 挂入 KILL_ON_JOB_CLOSE 作业 →
    恢复主线程。作业先于任何目标代码就位，后代自动入作业，父进程先退出的
    孤儿也随作业终止（复审第三轮缺陷 1 的启动竞态由此消除；作业只收编加入
    之后创建的后代，先启动再分配是不够的）。POSIX：新会话 + 进程组，组成员
    资格在创建时继承，结构上无入组竞态。"""
    if os.name == "nt":
        proc = subprocess.Popen(argv, cwd=cwd, env=env,
                                stdout=stdout_file, stderr=stderr_file,
                                creationflags=getattr(subprocess, "CREATE_SUSPENDED", 0x4))
        job = None
        thread = None
        try:
            job = _create_kill_on_close_job()
            if job is None:
                raise OSError("创建 KILL_ON_JOB_CLOSE 作业对象失败")
            thread = _open_main_thread(proc)
            if thread is None:
                raise OSError(f"OpenThread({getattr(proc, '_tid', '?')}) 失败"
                              f"（GetLastError={ctypes.get_last_error()}）")
            if not _assign_to_job(job, proc):
                raise OSError("AssignProcessToJobObject 失败"
                              f"（GetLastError={ctypes.get_last_error()}）")
            if _kernel32().ResumeThread(thread) == 0xFFFFFFFF:
                raise OSError(f"ResumeThread 失败（GetLastError={ctypes.get_last_error()}）")
            return proc, job
        except OSError:
            if job is not None:
                _kernel32().TerminateJobObject(job, 1)
                _close_job(job)
            if proc.poll() is None:
                proc.kill()
            raise
        finally:
            if thread is not None:
                _kernel32().CloseHandle(thread)
    return subprocess.Popen(argv, cwd=cwd, env=env, stdout=stdout_file, stderr=stderr_file,
                            start_new_session=True), None


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
        """有界进程树收尾：Windows 终止整个作业——挂起创建保证后代已随作业
        入组，父进程先退出的孤儿一并终止；POSIX 终止进程组。尽力而为：进程
        可能已自行退出（竞态），失败不外抛，返回值由调用方写进证据，供反例
        核对后代确实终止。"""
        try:
            if job is not None:
                if _kernel32().TerminateJobObject(job, 1):
                    return None
                return (f"TerminateJobObject 失败"
                        f"（GetLastError={ctypes.get_last_error()}）")
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            return None
        except (OSError, subprocess.TimeoutExpired) as exc:
            return str(exc)

    def _pid_alive(self, pid):
        if os.name == "nt":
            listing = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV"],
                capture_output=True, timeout=TEARDOWN_RESERVE_S)
            rows = [line for line in _decode(listing.stdout).splitlines()
                    if line.startswith('"')]
            return bool(rows)
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def _execute_case(self, case_name, argv, env, budget_s):
        """在原预算内统一截止运行一次外层子进程，超时按进程树收尾。

        stdout/stderr 直接写入私有日志文件——不依赖管道 EOF，超时后读取已有
        内容即可（复审第三轮建议）。所有等待共用同一截止时间（启动 + 预算）：
        首次等待提前预留 TEARDOWN_RESERVE_S 收尾时间；超时后终止整棵进程树
        （Windows 作业在挂起创建阶段已先于任何目标代码挂入，孤儿一并终止；
        POSIX 终止进程组），再在同一截止时间的剩余部分内等待退出。树终止仍
        失败时不追加任何等待，按超时失败返回，日志保留已写入内容。"""
        started = time.monotonic()
        deadline = started + budget_s
        reserve = min(TEARDOWN_RESERVE_S, budget_s / 4)
        stdout_path = self.evidence / f"stdout-{case_name}.log"
        stderr_path = self.evidence / f"stderr-{case_name}.log"
        with open(stdout_path, "wb") as stdout_file, open(stderr_path, "wb") as stderr_file:
            try:
                proc, job = _spawn(argv, cwd=ROOT, env=env,
                                   stdout_file=stdout_file, stderr_file=stderr_file)
            except OSError as exc:
                self.fail(f"无法在作业内启动外层子进程"
                          f"（不降级为无法处理孤儿的收尾方式）：{exc}")
            kind = "completed"
            kill_error = None
            try:
                try:
                    proc.communicate(
                        timeout=max(0.0, deadline - time.monotonic() - reserve))
                except subprocess.TimeoutExpired:
                    kind = "timeout"
                    kill_error = self._kill_process_tree(proc, job)
                    try:
                        proc.wait(timeout=max(0.0, deadline - time.monotonic()))
                    except subprocess.TimeoutExpired:
                        pass  # 树终止失败：不追加等待，按超时失败返回（日志保留已写入内容）
            finally:
                if job is not None:
                    # KILL_ON_JOB_CLOSE：关闭句柄终止作业内仍存活的进程，正常
                    # 完成时是对残留后代的最后兜底。
                    _close_job(job)
        elapsed = time.monotonic() - started
        stdout_text = _read_log(stdout_path)
        stderr_text = _read_log(stderr_path)
        if kill_error is not None:
            (self.evidence / f"kill-{case_name}.log").write_text(
                f"{kill_error}\n", encoding="utf-8")
        return SimpleNamespace(kind=kind, returncode=proc.returncode,
                               stdout_text=stdout_text, stderr_text=stderr_text,
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
                f"外层调用超时（原预算 {CALL_TIMEOUT_S}s 内统一截止，含收尾预留，"
                f"实际 {outcome.elapsed_s:.1f}s）；已捕获输出存于 {self.evidence}")

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
        # 反例（合成子进程）：父进程先产出一个长寿后代再驻留——预算内父进程
        # 不肯退出，证明等待受原预算约束、超时收尾终止整树、已产出输出保留。
        budget_s = 1.0
        pid_path = self.root / "descendant.pid"
        outcome = self._run_vitest(
            "hang", budget_s=budget_s,
            argv_override=[sys.executable, "-c", HANG_PARENT_PROGRAM],
            env_extra={DESCENDANT_PID_ENV: str(pid_path)})
        self.assertEqual(outcome.kind, "timeout",
                         "短预算必须触发超时路径，否则反例无效")
        self.assertLess(
            outcome.elapsed_s, budget_s + 1.0,
            f"统一截止时间必须约束在原预算内（含收尾预留，预算 {budget_s}s），"
            f"实际 {outcome.elapsed_s:.2f}s")
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
        # 父进程先退出的反例（复审第二轮缺陷 1）：父进程产出长寿后代后正常
        # 退出，调用在预算内正常完成——但作业关闭（KILL_ON_JOB_CLOSE）必须
        # 收割父进程已退出后仍存活的孤儿后代。旧机制（收尾依赖父 PID、管道
        # 排空依赖 EOF）实测预算 0.3s+宽限 0.2s 拖到 2.101s、后代自然跑完、
        # stdout 为空。这里断言：正常完成、输出保留、孤儿后代确认死亡。
        budget_s = 0.5
        pid_path = self.root / "orphan.pid"
        outcome = self._run_vitest(
            "early-parent", budget_s=budget_s,
            argv_override=[sys.executable, "-c", _pipe_holding_program(
                EARLY_PARENT_DESCENDANT_SLEEP_S, parent_lingers=False)],
            env_extra={DESCENDANT_PID_ENV: str(pid_path)})
        self.assertEqual(outcome.kind, "completed",
                         "父进程正常退出后调用应在预算内完成")
        self.assertLess(
            outcome.elapsed_s, budget_s + 1.0,
            f"统一截止时间必须约束在原预算内（含收尾预留，预算 {budget_s}s），"
            f"实际 {outcome.elapsed_s:.2f}s")
        self.assertEqual(outcome.returncode, 0, "父进程应按 0 退出")
        self.assertIn("parent-ready", outcome.stdout_text,
                      "父进程自然退出，其已产出的输出仍必须落盘保留")
        self.assertNotIn(
            "descendant-ended", outcome.stdout_text,
            "孤儿后代寿命长于调用时长时不得等到它的输出")
        self.assertTrue(pid_path.exists(), "反例后代未按预期产出 PID 文件")
        orphan_pid = int(pid_path.read_text(encoding="utf-8").strip())
        time.sleep(0.2)  # 给作业关闭后的终止留出落盘与调度余量
        self.assertFalse(self._pid_alive(orphan_pid),
                         f"父进程已退出，孤儿后代 {orphan_pid} 仍存活："
                         "作业关闭必须收割孤儿")
        self._assert_sentinel_intact()

    @_keep_evidence_on_failure
    def test_job_enrollment_precedes_descendant_creation(self):
        # 复审第三轮缺陷 1 的确定性回归：入作业必须先于任何目标代码运行。
        # 旧实现先启动再分配，控制调度（拖到后代已创建才分配）即可让后代逃出
        # 作业：分配成功、终止报成功、调用返回后后代仍存活（用户
        # review_probes_round3.json 实测 2.321s、stdout 为空）。这里用同样的
        # 受控时序——分配前等待 PID 文件出现；挂起创建下子进程尚未运行任何
        # 目标代码、PID 文件不可能出现，等待到点后才分配并恢复。调用随后在
        # 预算内正常完成，断言「分配时后代尚未创建」与「逃逸后代已被收割」。
        budget_s = 2.0
        pid_path = self.root / "escaped.pid"
        observations = {}
        real_assign = _assign_to_job

        def assign_as_late_as_possible(job, proc):
            deadline = time.monotonic() + 0.3
            while not pid_path.exists() and time.monotonic() < deadline:
                time.sleep(0.005)
            observations["enrolled_before_descendant_creation"] = not pid_path.exists()
            return real_assign(job, proc)

        program = (
            "import os, subprocess, sys\n"
            "child = subprocess.Popen([sys.executable, '-c', "
            "'import time; time.sleep(5); print(\"escaped-ended\", flush=True)'])\n"
            "with open(os.environ[" + json.dumps(DESCENDANT_PID_ENV) + "], \"w\") as handle:\n"
            "    handle.write(str(child.pid))\n"
            "print('parent-ready', flush=True)\n"
        )
        with mock.patch.object(_CONTRACT_MODULE, "_assign_to_job",
                               side_effect=assign_as_late_as_possible):
            outcome = self._run_vitest(
                "enrollment", budget_s=budget_s,
                argv_override=[sys.executable, "-c", program],
                env_extra={DESCENDANT_PID_ENV: str(pid_path)})
        self.assertTrue(
            observations.get("enrolled_before_descendant_creation"),
            "分配发生时子进程必须尚未运行任何目标代码（挂起创建先入作业）")
        self.assertEqual(outcome.kind, "completed",
                         "父进程随即正常退出后调用应在预算内完成")
        self.assertLess(
            outcome.elapsed_s, budget_s + 1.0,
            f"统一截止时间必须约束在原预算内（含收尾预留，预算 {budget_s}s），"
            f"实际 {outcome.elapsed_s:.2f}s")
        self.assertIn("parent-ready", outcome.stdout_text,
                      "超时前已产出的输出必须落盘保留")
        self.assertNotIn(
            "escaped-ended", outcome.stdout_text,
            "后代寿命长于调用时长时不得等到它的输出（不得无界排空）")
        self.assertTrue(pid_path.exists(), "反例后代未按预期产出 PID 文件")
        escaped_pid = int(pid_path.read_text(encoding="utf-8").strip())
        time.sleep(0.2)  # 给作业关闭后的终止留出落盘与调度余量
        self.assertFalse(self._pid_alive(escaped_pid),
                         f"受控时序下后代 {escaped_pid} 仍存活：入作业晚于目标代码")
        self._assert_sentinel_intact()


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    unittest.main()
