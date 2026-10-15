"""脚本语法与契约接线的契约测试。

覆盖：PowerShell 语法检查器能对坏脚本报错、对好脚本放行；提交门禁的脚本规则
把 Python 契约、PowerShell 契约与语法检查都纳入选择集合。
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from pre_commit_checks import expand_commands

ROOT = Path(__file__).resolve().parent.parent
PWSH = shutil.which("pwsh")


class PowerShellSyntaxCase(unittest.TestCase):
    def setUp(self):
        if PWSH is None:
            self.skipTest("需要 PowerShell 7（pwsh）")
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-ps1-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        (self.root / "frontend" / "scripts").mkdir(parents=True)

    def run_check(self):
        return subprocess.run(
            [PWSH, "-NoProfile", "-File", str(ROOT / "scripts/check_powershell_syntax.ps1"),
             "-Root", str(self.root)],
            capture_output=True, encoding="utf-8", errors="replace",
            env={key: value for key, value in os.environ.items()
                 if not key.startswith("GIT_")})

    def test_valid_scripts_pass(self):
        (self.root / "scripts" / "ok.ps1").write_text(
            "param([string]$Name)\nWrite-Output \"你好 $Name\"\n", encoding="utf-8")
        (self.root / "frontend" / "scripts" / "ok2.ps1").write_text(
            "Get-ChildItem | ForEach-Object { $_.Name }\n", encoding="utf-8")
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("通过：2 个脚本", result.stdout)

    def test_broken_script_fails_with_location(self):
        (self.root / "scripts" / "broken.ps1").write_text(
            "function Test- {\n  param(\n}\n", encoding="utf-8")
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("broken.ps1", result.stderr)
        self.assertIn("[powershell-syntax]", result.stderr)

    def test_repository_scripts_are_syntactically_valid(self):
        result = self.run_check()  # 默认以仓库根为范围
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertRegex(result.stdout, r"通过：\d+ 个脚本")


class ScriptCheckSelectionCase(unittest.TestCase):
    """脚本规则必须同时覆盖 Python 契约、PowerShell 契约与语法检查。"""

    def commands_for(self, check):
        return [" ".join(command) for command in expand_commands([check])]

    def test_scripts_check_runs_python_and_powershell_contracts(self):
        commands = self.commands_for("scripts-contracts")
        self.assertIn("python -B -m unittest discover -s scripts -p test_*.py", commands)
        self.assertIn("pwsh -NoProfile -File scripts/check_powershell_syntax.ps1", commands)
        self.assertIn("pwsh -NoProfile -File scripts/test_install_helpers.ps1", commands)
        self.assertIn("pwsh -NoProfile -File scripts/test_upgrade_helpers.ps1", commands)
        self.assertIn("pwsh -NoProfile -File scripts/test_native_window_action.ps1", commands)

    def test_commands_are_argv_arrays(self):
        for command in expand_commands(["scripts-contracts"]):
            self.assertIsInstance(command, list)
            self.assertTrue(all(isinstance(part, str) for part in command))


if __name__ == "__main__":
    unittest.main()
