"""AC03–AC05 原生工作流接线契约；只执行替身命令，不启动应用。"""

import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
ARTIFACT_ROOT = "qa-artifacts/ui-adjustment/native-e2e"
SCENARIOS = {
    "ui-identity": (9500, "identity"),
    "ui-ccs-import": (9510, "ccs-import"),
    "ui-pricing": (9520, "pricing"),
}


def steps(text):
    starts = list(re.finditer(r"^      - (?:name|uses):", text, re.M))
    return [text[start.start(): starts[index + 1].start() if index + 1 < len(starts) else len(text)]
            for index, start in enumerate(starts)]


class AcceptanceNativeWorkflowCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = (ROOT / ".github/workflows/ui-e2e.yml").read_text(encoding="utf-8")

    def native_job(self):
        job = re.search(r"(?ms)^  native-ui:\n(.*?)(?=^  [\w-]+:|\Z)", self.text)
        self.assertIsNotNone(job, "缺少 native-ui 作业")
        return job[1]

    def scenario_step(self, scenario):
        found = [step for step in steps(self.native_job())
                 if re.search(rf"--scenario\s+{re.escape(scenario)}(?:\s|$)", step)]
        self.assertEqual(len(found), 1, f"{scenario} 应有且只有一个独立工作流步骤")
        return found[0]

    def run_body(self, scenario):
        step = self.scenario_step(scenario)
        block = re.search(r"^        run: \|\n((?:          .*\n|\n)+)", step, re.M)
        self.assertIsNotNone(block, f"{scenario} 缺少独立的多行运行块")
        return "\n".join(line[10:] for line in block[1].splitlines()) + "\n"

    def test_three_scenarios_have_independent_pwsh_steps(self):
        selected = []
        for scenario in SCENARIOS:
            with self.subTest(scenario=scenario):
                step = self.scenario_step(scenario)
                selected.append(step)
                self.assertRegex(step, r"(?m)^        shell: pwsh$")
                self.assertNotRegex(step, r"(?m)^        (?:if|continue-on-error):")
                self.assertEqual(self.run_body(scenario).count("node frontend/scripts/native-acceptance.mjs"), 1)
        self.assertEqual(len(set(selected)), 3)

    def test_synthetic_event_and_evidence_arguments(self):
        for scenario, (port, directory) in SCENARIOS.items():
            with self.subTest(scenario=scenario):
                body = self.run_body(scenario)
                expected = (f"node frontend/scripts/native-acceptance.mjs --scenario {scenario} "
                            f"--events 8 --port {port} --out {ARTIFACT_ROOT}/{directory}")
                self.assertIn(expected, body)
                self.assertEqual(body.count("--scenario"), 1)
                self.assertEqual(body.count("--events"), 1)
                self.assertEqual(body.count("--port"), 1)
                self.assertEqual(body.count("--out"), 1)
                self.assertNotIn("--attach", body)

    def test_new_port_segments_are_disjoint_from_each_other_and_existing_steps(self):
        reserved = set()
        chosen_steps = []
        for scenario, (port, _) in SCENARIOS.items():
            step = self.scenario_step(scenario)
            chosen_steps.append(step)
            self.assertRegex(self.run_body(scenario), rf"--port\s+{port}\b")
            segment = set(range(port, port + 10))
            self.assertFalse(reserved & segment, f"{scenario} 与其它新增场景端口重叠")
            reserved |= segment
        # 同时读取旧步骤的字面 --port 与缩放轮使用的端口映射，不写死旧场景名单。
        for step in steps(self.native_job()):
            if step in chosen_steps:
                continue
            literals = re.findall(r"--port\s+(\d+)|['\"]\d+(?:\.\d+)?['\"]\s*=\s*(\d+)", step)
            for direct, mapped in literals:
                port = int(direct or mapped)
                self.assertFalse(reserved & set(range(port, port + 10)),
                                 f"已有步骤端口 {port} 的启动段与新增场景重叠")

    def test_real_step_bodies_propagate_native_failure_exit(self):
        powershell = shutil.which("pwsh")
        self.assertIsNotNone(powershell, "需要 PowerShell 验证真实步骤的退出码传播")
        with tempfile.TemporaryDirectory(prefix="native-workflow-中文-") as temporary:
            folder = Path(temporary)
            for scenario, (port, directory) in SCENARIOS.items():
                body = self.run_body(scenario)
                for code in (0, 37):
                    with self.subTest(scenario=scenario, exit_code=code):
                        record = folder / "record.json"
                        escaped_record = str(record).replace("'", "''")
                        stub = ("function node {\n"
                                f"  ConvertTo-Json -InputObject @($args | ForEach-Object {{ [string]$_ }}) -Compress | Set-Content -LiteralPath '{escaped_record}' -Encoding utf8\n"
                                f"  $global:LASTEXITCODE = {code}\n"
                                "}\n")
                        script = folder / "step.ps1"
                        script.write_text(stub + body + "\nexit 0\n", encoding="utf-8")
                        result = subprocess.run([powershell, "-NoProfile", "-File", str(script)],
                                                capture_output=True, timeout=30)
                        diagnostic = (result.stdout.decode("utf-8", errors="replace")
                                      + result.stderr.decode("utf-8", errors="replace"))
                        self.assertEqual(result.returncode, code, diagnostic)
                        self.assertEqual(json.loads(record.read_text(encoding="utf-8-sig")),
                                         ["frontend/scripts/native-acceptance.mjs", "--scenario", scenario,
                                          "--events", "8", "--port", str(port), "--out", f"{ARTIFACT_ROOT}/{directory}"])

    def test_job_deadline_and_unconditional_original_artifact_upload_are_retained(self):
        job = self.native_job()
        self.assertRegex(job, r"(?m)^    timeout-minutes: 45$")
        uploads = [step for step in steps(job) if "uses: actions/upload-artifact@" in step
                   and re.search(r"(?m)^          name: native-ui-evidence$", step)]
        self.assertEqual(len(uploads), 1)
        self.assertRegex(uploads[0], r"(?m)^        if: always\(\)$")
        self.assertRegex(uploads[0], rf"(?m)^          path: {re.escape(ARTIFACT_ROOT)}$")
        self.assertRegex(uploads[0], r"(?m)^          if-no-files-found: warn$")

    def test_required_contracts_group_runs_this_workflow_contract(self):
        action = (ROOT / ".github/actions/checks/action.yml").read_text(encoding="utf-8")
        starts = list(re.finditer(r"^    - name:", action, re.M))
        blocks = [action[start.start(): starts[index + 1].start()
                         if index + 1 < len(starts) else len(action)]
                  for index, start in enumerate(starts)]
        invocation = ('python -B -m unittest discover -s scripts '
                      '-p "test_acceptance_native_workflow.py" -v')
        selected = [block for block in blocks if invocation in block]
        self.assertEqual(len(selected), 1, "contracts 组必须运行新增的原生工作流契约")
        self.assertRegex(selected[0], r"(?m)^      if: inputs\.group == 'contracts'$")
        self.assertRegex(selected[0], r"(?m)^      shell: pwsh$")
        self.assertNotRegex(selected[0], r"(?m)^      continue-on-error:")
        self.assertIn(invocation + '\n        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }',
                      selected[0])


if __name__ == "__main__":
    unittest.main()
