"""原生场景契约临时目录清理的外层回归（测试临时目录清理计划 TL02）。

测试自身通过不代表临时目录被清理：泄漏只能由真实 Vitest 子进程退出后的
外层文件系统核实。本契约在独立私有根中运行仓库现有的
`scripts/lib/native-scenario.test.mjs`「父进程设上限」用例，覆盖三条路径：

1. 正常通过：不注入任何错误，私有根不得残留 `native-scenario-test-*`。
2. 测试中途失败：仅对该用例的 `evidence.json` 读取注入带固定标记的抛错，
   目录仍必须被清理。
3. 清理失败：仅对该用例目录的 `fs.rmSync` 注入带固定标记的删除错误，
   Vitest 必须非零退出且输出标记；残留只允许出现在外层私有根。

全部子进程 stdout/stderr 与 JSON 报告落盘保存；核对原用例确实执行且只有
该用例执行，全跳过、报告缺失或配置加载失败一律判失败。每次外层同步调用
有 30 秒上限，超时即失败，不增加等待补偿；找不到 pnpm 也明确失败。
"""

import json
import os
import shutil
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

    def _run_vitest(self, case_name, setup_template=None, mark=None):
        setup_files = []
        if setup_template is not None:
            setup_path = self.root / f"setup-{case_name}.mjs"
            setup_path.write_text(
                setup_template.format(mark=json.dumps(mark),
                                      prefix=json.dumps(TEMP_DIR_PREFIX)),
                encoding="utf-8")
            setup_files = [setup_path]
        config_path = self.root / f"vitest-{case_name}.config.mjs"
        config_path.write_text(CONFIG_TEMPLATE.format(
            setup_files=json.dumps([str(p) for p in setup_files]),
            frontend_root=json.dumps(str(ROOT / FRONTEND_DIR)),
            test_file=json.dumps(TEST_FILE)), encoding="utf-8")
        report_path = self.evidence / f"report-{case_name}.json"

        # 只向实际 Vitest 子进程注入三个临时环境变量；父环境原样保留。
        env = {**os.environ,
               "TMP": str(self.root), "TEMP": str(self.root),
               "TMPDIR": str(self.root)}
        command = ["pnpm", "--dir", FRONTEND_DIR, "exec", "vitest", "--run",
                   "--config", str(config_path),
                   "--reporter=default", "--reporter=json",
                   f"--outputFile.json={report_path}",
                   "-t", TEST_FILTER]
        started = time.monotonic()
        try:
            completed = subprocess.run(
                self._argv(command), cwd=ROOT, env=env,
                capture_output=True, timeout=CALL_TIMEOUT_S)
        except subprocess.TimeoutExpired as exc:
            self.fail(f"外层调用超过 {CALL_TIMEOUT_S}s 上限（不等待补偿）：{exc}")
        elapsed = time.monotonic() - started

        stdout_text = _decode(completed.stdout)
        stderr_text = _decode(completed.stderr)
        (self.evidence / f"stdout-{case_name}.log").write_text(
            stdout_text, encoding="utf-8")
        (self.evidence / f"stderr-{case_name}.log").write_text(
            stderr_text, encoding="utf-8")
        report = None
        if report_path.exists():
            try:
                report = json.loads(report_path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                report = None
        return SimpleNamespace(returncode=completed.returncode,
                               stdout_text=stdout_text,
                               stderr_text=stderr_text,
                               report=report, elapsed_s=elapsed)

    # —— 断言辅助 ——

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

    # —— 三条路径 ——

    def test_normal_pass_still_cleans_own_temp_dir(self):
        outcome = self._run_vitest("normal")
        self.assertEqual(
            outcome.returncode, 0,
            f"Vitest 应正常通过（{outcome.elapsed_s:.1f}s）：\n"
            f"{outcome.stdout_text}\n{outcome.stderr_text}")
        self._require_single_executed(outcome, "passed")
        self._assert_no_residue()

    def test_midtest_failure_still_cleans_own_temp_dir(self):
        outcome = self._run_vitest("midfail", setup_template=SETUP_READ_FAIL_TEMPLATE,
                                   mark=MARK_READ)
        self.assertNotEqual(
            outcome.returncode, 0,
            "注入的读取失败必须让 Vitest 非零退出，注入失败路径不能假通过")
        combined = outcome.stdout_text + outcome.stderr_text
        self.assertIn(MARK_READ, combined,
                      "失败输出必须包含注入标记，证明失败来自注入而非配置/依赖问题")
        self._require_single_executed(outcome, "failed")
        self._assert_no_residue()

    def test_cleanup_failure_surfaces_and_keeps_scene(self):
        outcome = self._run_vitest("rmfail", setup_template=SETUP_RM_FAIL_TEMPLATE,
                                   mark=MARK_RM)
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


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    unittest.main()
