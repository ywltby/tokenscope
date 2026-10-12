"""执行实际升级 workflow 的调用块，只记录参数，安装器永不执行。"""

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


class UpgradeInvocationCase(unittest.TestCase):
    def test_both_workflows_bind_exact_artifact_source_state_and_propagate_failure(self):
        pwsh = shutil.which("pwsh")
        self.assertIsNotNone(pwsh, "升级契约要求 PowerShell 7")
        script = (ROOT / "scripts/test_upgrade.ps1").read_text(encoding="utf-8-sig")
        declaration = re.search(r"^param\(\n.*?^\)", script, re.M | re.S)
        self.assertIsNotNone(declaration)
        for workflow, dependency in (("install-test.yml", "install-test"), ("release.yml", "gate")):
            text = (ROOT / ".github/workflows" / workflow).read_text(encoding="utf-8")
            marker = "      - name: Run real 0.1.0 upgrade\n"
            self.assertEqual(text.count(marker), 1, workflow)
            match = re.search(r"^        run: \|\n((?:          .*\n|\n)+)",
                              text.split(marker)[1], re.M)
            self.assertIsNotNone(match)
            template = "\n".join(line[10:] for line in match[1].splitlines()) + "\n"
            for state in ("present", "deleted"):
                block = template
                for expression, value in ((f"needs.{dependency}.outputs.version", "0.2.0"),
                                          (f"needs.{dependency}.outputs.sha", SHA),
                                          ("matrix.source_state", state)):
                    placeholder = "${{ " + expression + " }}"
                    self.assertEqual(block.count(placeholder), 1)
                    block = block.replace(placeholder, value)
                self.assertNotIn("${{", block)
                for exit_code in (0, 37):
                    with self.subTest(workflow=workflow, state=state, exit=exit_code), tempfile.TemporaryDirectory() as temporary:
                        workspace = Path(temporary) / "中文 影子"
                        scripts = workspace / "scripts"
                        scripts.mkdir(parents=True)
                        installer = workspace / "installer/TokenScope 中文-setup.exe"
                        installer.parent.mkdir()
                        installer.write_bytes(b"inert, never executed")
                        stub = declaration[0] + r'''
$bound = @{ installer=$InstallerPath; version=$AppVersion; commit=$ExpectedCommit;
    state=$SourceState; extra_arguments=@($args) }
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
                        self.assertEqual(bound,{"version":"0.2.0","commit":SHA,"state":state,"extra_arguments":[]})


if __name__ == "__main__":
    unittest.main()
