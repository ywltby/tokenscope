"""升级 workflow 的契约：起点矩阵、空矩阵、参数传递与失败传播。

执行实际 workflow 的调用块，只记录参数，安装器永不执行。
"""

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SHA = "a" * 40
WORKFLOWS = {"install-test.yml": "install-test", "release.yml": "gate"}
MARKER = "      - name: Run real upgrade from ${{ matrix.origin }}\n"
CHAIN_MARKER = "      - name: Run chained upgrades\n"


def job_block(text, name):
    match = re.search(rf"^  {re.escape(name)}:\n((?:    .*\n|\n)+)", text, re.M)
    if match is None:
        raise AssertionError(f"workflow 里没有作业 {name}")
    return match[1]


def run_block(text, marker):
    if text.count(marker) != 1:
        raise AssertionError(f"调用块必须唯一：{marker.strip()}")
    match = re.search(r"^        run: \|\n((?:          .*\n|\n)+)", text.split(marker)[1], re.M)
    if match is None:
        raise AssertionError(f"调用块缺少 run：{marker.strip()}")
    return "\n".join(line[10:] for line in match[1].splitlines()) + "\n"


class UpgradeMatrixCase(unittest.TestCase):
    def workflow(self, name):
        return (ROOT / ".github" / "workflows" / name).read_text(encoding="utf-8")

    def test_both_workflows_select_origins_offline_and_refuse_empty_matrix(self):
        for name in WORKFLOWS:
            with self.subTest(workflow=name):
                text = self.workflow(name)
                plan = job_block(text, "plan-upgrades")
                self.assertIn("scripts/upgrade_baselines.py select --current-version", plan)
                self.assertIn("起点集合为空", plan)
                self.assertIn("GITHUB_OUTPUT", plan)
                self.assertNotIn("gh ", plan)
                upgrade = job_block(text, "upgrade-test")
                self.assertIn("include: ${{ fromJSON(needs.plan-upgrades.outputs.include) }}", upgrade)
                self.assertIn("needs: [", upgrade)
                self.assertIn("plan-upgrades", upgrade)
                self.assertIn("Origin = '${{ matrix.origin }}'", upgrade)
                self.assertIn("SourceState = '${{ matrix.source_state }}'", upgrade)

    def test_no_origin_is_skipped_and_no_failure_is_hidden(self):
        for name in WORKFLOWS:
            with self.subTest(workflow=name):
                text = self.workflow(name)
                upgrade = job_block(text, "upgrade-test")
                self.assertNotIn("continue-on-error", upgrade)
                self.assertNotIn("exclude:", upgrade)
                self.assertNotIn("if: false", upgrade)
                # 起点只能来自矩阵，工作流不得写死某个版本或跳过某个起点。
                self.assertNotIn("0.1.0", upgrade)
                self.assertNotIn("0.3.", upgrade)
                # 只有证据上传允许带条件；调用本身与作业级不得跳过起点。
                self.assertEqual(upgrade.count("if:"), 1)
                self.assertNotIn("  upgrade-test:\n    if:", text)

    def test_install_test_chain_runs_only_on_manual_dispatch(self):
        text = self.workflow("install-test.yml")
        chain = job_block(text, "upgrade-chain")
        self.assertIn("if: github.event_name == 'workflow_dispatch'", chain)
        self.assertIn("scripts/test_upgrade_chain.ps1", chain)
        self.assertIn("SourceState = '${{ matrix.source_state }}'", chain)

    def test_release_chain_runs_before_publish_and_shares_the_same_selection(self):
        text = self.workflow("release.yml")
        chain = job_block(text, "upgrade-chain")
        self.assertIn("scripts/test_upgrade_chain.ps1", chain)
        self.assertIn("SourceState = '${{ matrix.source_state }}'", chain)
        self.assertNotIn("Origin =", chain)
        publish = job_block(text, "publish")
        self.assertIn("upgrade-chain", publish.split("steps:")[0])
        self.assertIn("upgrade-test", publish.split("steps:")[0])
        with tempfile.TemporaryDirectory() as temporary:
            for state in ("present", "deleted"):
                for exit_code in (0, 37):
                    with self.subTest(state=state, exit=exit_code):
                        bound = self.invoke_chain(text, temporary, state, exit_code)
                        self.assertEqual(bound["version"], "0.3.3")
                        self.assertEqual(bound["commit"], SHA)
                        self.assertEqual(bound["state"], state)
                        self.assertEqual(bound["extra_arguments"], [])

    def invoke_chain(self, workflow_text, temporary, state, exit_code):
        block = run_block(workflow_text, CHAIN_MARKER)
        workspace = Path(temporary) / f"中文 影子 {state} {exit_code}"
        scripts = workspace / "scripts"
        scripts.mkdir(parents=True, exist_ok=True)
        installer = workspace / "installer/TokenScope 中文-setup.exe"
        installer.parent.mkdir(parents=True, exist_ok=True)
        installer.write_bytes(b"inert, never executed")
        declaration = re.search(r"^param\(\n.*?^\)", (ROOT / "scripts/test_upgrade_chain.ps1")
                                .read_text(encoding="utf-8-sig"), re.M | re.S)
        self.assertIsNotNone(declaration)
        (scripts / "test_upgrade_chain.ps1").write_text(declaration[0] + r'''
$bound = @{ installer=$InstallerPath; version=$AppVersion; commit=$ExpectedCommit;
    state=$SourceState; extra_arguments=@($args) }
[IO.File]::WriteAllText($env:TOKENSCOPE_UPGRADE_BOUND,($bound|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
exit ([int]$env:TOKENSCOPE_UPGRADE_EXIT)
''', encoding="utf-8")
        for expression, value in (("needs.gate.outputs.version", "0.3.3"),
                                  ("needs.gate.outputs.sha", SHA),
                                  ("matrix.source_state", state)):
            placeholder = "${{ " + expression + " }}"
            self.assertEqual(block.count(placeholder), 1)
            block = block.replace(placeholder, value)
        self.assertNotIn("${{", block)
        runner = workspace / "invoke.ps1"
        runner.write_text('$ErrorActionPreference="Stop"\n' + block, encoding="utf-8")
        output = workspace / "bound.json"
        env = dict(os.environ, TOKENSCOPE_UPGRADE_BOUND=str(output),
                   TOKENSCOPE_UPGRADE_EXIT=str(exit_code))
        result = subprocess.run([shutil.which("pwsh"), "-NoProfile", "-File", str(runner)],
                                cwd=workspace, env=env, capture_output=True, check=False)
        self.assertEqual(result.returncode, exit_code,
                         result.stderr.decode("utf-8", errors="replace"))
        bound = json.loads(output.read_text(encoding="utf-8"))
        self.assertTrue(Path(bound.pop("installer")).samefile(installer))
        return bound

    def test_both_workflows_bind_exact_artifact_origin_source_state_and_propagate_failure(self):
        pwsh = shutil.which("pwsh")
        self.assertIsNotNone(pwsh, "升级契约要求 PowerShell 7")
        script = (ROOT / "scripts/test_upgrade.ps1").read_text(encoding="utf-8-sig")
        declaration = re.search(r"^param\(\n.*?^\)", script, re.M | re.S)
        self.assertIsNotNone(declaration)
        for workflow, dependency in WORKFLOWS.items():
            text = self.workflow(workflow)
            template = run_block(text, MARKER)
            for origin in ("0.1.0", "0.3.2"):
                for state in ("present", "deleted"):
                    block = template
                    for expression, value in ((f"needs.{dependency}.outputs.version", "0.3.3"),
                                              (f"needs.{dependency}.outputs.sha", SHA),
                                              ("matrix.origin", origin),
                                              ("matrix.source_state", state)):
                        placeholder = "${{ " + expression + " }}"
                        self.assertEqual(block.count(placeholder), 1)
                        block = block.replace(placeholder, value)
                    self.assertNotIn("${{", block)
                    for exit_code in (0, 37):
                        with self.subTest(workflow=workflow, origin=origin, state=state,
                                          exit=exit_code), tempfile.TemporaryDirectory() as temporary:
                            workspace = Path(temporary) / "中文 影子"
                            scripts = workspace / "scripts"
                            scripts.mkdir(parents=True)
                            installer = workspace / "installer/TokenScope 中文-setup.exe"
                            installer.parent.mkdir()
                            installer.write_bytes(b"inert, never executed")
                            stub = declaration[0] + r'''
$bound = @{ installer=$InstallerPath; version=$AppVersion; commit=$ExpectedCommit;
    state=$SourceState; origin=$Origin; extra_arguments=@($args) }
[IO.File]::WriteAllText($env:TOKENSCOPE_UPGRADE_BOUND,($bound|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
exit ([int]$env:TOKENSCOPE_UPGRADE_EXIT)
'''
                            (scripts / "test_upgrade.ps1").write_text(stub, encoding="utf-8")
                            runner = workspace / "invoke.ps1"
                            runner.write_text('$ErrorActionPreference="Stop"\n' + block, encoding="utf-8")
                            output = workspace / "bound.json"
                            env = dict(os.environ, TOKENSCOPE_UPGRADE_BOUND=str(output),
                                       TOKENSCOPE_UPGRADE_EXIT=str(exit_code))
                            result = subprocess.run([pwsh,"-NoProfile","-File",str(runner)], cwd=workspace,
                                                    env=env,capture_output=True,check=False)
                            self.assertEqual(result.returncode,exit_code,result.stderr.decode("utf-8",errors="replace"))
                            bound = json.loads(output.read_text(encoding="utf-8"))
                            self.assertTrue(Path(bound.pop("installer")).samefile(installer))
                            self.assertEqual(bound,{"version":"0.3.3","commit":SHA,"state":state,
                                                    "origin":origin,"extra_arguments":[]})


if __name__ == "__main__":
    unittest.main()
