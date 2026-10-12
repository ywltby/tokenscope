"""用实际 workflow 的 PowerShell 调用块验证安装参数与退出码契约。

只在临时影子工作区运行：test_install.ps1 替身采用生产脚本的参数声明，
记录绑定结果后退出，不执行安装、注册表操作或默认用户目录写入。
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = (
    ("install-test.yml",
     "Run install / first-launch migration / reinstall / uninstall scenarios",
     "src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis",
     "${{ steps.app_version.outputs.version }}"),
    ("release.yml",
     "Install / first-launch migration / reinstall / uninstall scenarios",
     "installer", "${{ needs.gate.outputs.version }}"),
)
VERSION = "1.2.3"
OLD_URL = "https://example.invalid/旧包/old-setup.exe"
OLD_HASH = "a" * 64


def installation_block(workflow: str, step: str, version_expression: str) -> str:
    """提取唯一的安装步骤 run 块，保留实际调用语句，不重写其实现。"""
    text = (ROOT / ".github/workflows" / workflow).read_text(encoding="utf-8")
    marker = f"      - name: {step}\n"
    if text.count(marker) != 1:
        raise ValueError(f"{workflow}: 安装步骤不是唯一命中")
    tail = text.split(marker, 1)[1]
    match = re.search(r"^        run: \|\n((?:          .*\n|\n)+)", tail, re.M)
    if match is None:
        raise ValueError(f"{workflow}: 没有安装 run 块")
    block = "\n".join(line[10:] for line in match[1].splitlines()) + "\n"
    if block.count(version_expression) != 1:
        raise ValueError(f"{workflow}: 缺少唯一版本输出引用")
    block = block.replace(version_expression, VERSION)
    if "${{" in block:
        raise ValueError(f"{workflow}: 安装 run 块含未替换的表达式")
    return block


def parameter_probe() -> str:
    production = (ROOT / "scripts/test_install.ps1").read_text(encoding="utf-8-sig")
    declaration = re.search(r"^param\(\n.*?^\)", production, re.M | re.S)
    if declaration is None:
        raise ValueError("生产安装脚本缺少参数声明")
    return declaration[0] + r'''
$payload = @{
    installer = $InstallerPath
    version = $AppVersion
    old_url = $OldInstallerUrl
    old_hash = $OldInstallerSha256
    extra_arguments = @($args)
}
[System.IO.File]::WriteAllText(
    $env:TOKENSCOPE_BINDING_RESULT,
    ($payload | ConvertTo-Json -Depth 4),
    [System.Text.UTF8Encoding]::new($false)
)
exit ([int]$env:TOKENSCOPE_BINDING_EXIT)
'''


class InstallWorkflowCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pwsh = shutil.which("pwsh")
        if cls.pwsh is None:
            raise RuntimeError("安装调用契约要求 PowerShell 7（pwsh）")
        cls.probe = parameter_probe()

    def invoke(self, workflow, step, artifact_dir, version_expression,
               old_url, old_hash, exit_code):
        with tempfile.TemporaryDirectory(prefix="tokenscope-workflow-") as parent:
            workspace = Path(parent) / "中文 影子工作区"
            scripts = workspace / "scripts"
            scripts.mkdir(parents=True)
            (scripts / "test_install.ps1").write_text(self.probe, encoding="utf-8")
            installer = workspace / artifact_dir / "TokenScope 中文-setup.exe"
            installer.parent.mkdir(parents=True)
            installer.write_bytes(b"inert fixture, never executed")
            runner = workspace / "invoke.ps1"
            runner.write_text(
                '$ErrorActionPreference = "Stop"\n'
                + installation_block(workflow, step, version_expression),
                encoding="utf-8",
            )
            result_path = workspace / "bound.json"
            env = os.environ.copy()
            env.update(OLD_INSTALLER_URL=old_url, OLD_INSTALLER_SHA256=old_hash,
                       TOKENSCOPE_BINDING_RESULT=str(result_path),
                       TOKENSCOPE_BINDING_EXIT=str(exit_code))
            result = subprocess.run(
                [self.pwsh, "-NoProfile", "-NonInteractive", "-File", str(runner)],
                cwd=workspace, env=env, capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, exit_code,
                             result.stderr.decode("utf-8", errors="replace"))
            self.assertTrue(result_path.is_file(), "安装替身未被调用")
            bound = json.loads(result_path.read_text(encoding="utf-8"))
            # Runner 的 TEMP 可用 8.3 短名，Get-ChildItem.FullName 会展开长名。
            # 必须指向同一个实际产物；字符串不同不代表安装器不同。
            actual_installer = Path(bound.pop("installer"))
            self.assertTrue(actual_installer.is_absolute(), "安装器路径必须是绝对路径")
            self.assertTrue(actual_installer.is_file(), "安装器路径必须存在")
            self.assertTrue(actual_installer.samefile(installer), "必须调用预期安装产物")
            self.assertEqual(bound, {
                "version": VERSION,
                "old_url": old_url, "old_hash": old_hash,
                "extra_arguments": [],
            })

    def check_workflows(self, exit_code):
        for workflow in WORKFLOWS:
            for old_url, old_hash in (("", ""), (OLD_URL, ""),
                                      ("", OLD_HASH), (OLD_URL, OLD_HASH)):
                with self.subTest(workflow=workflow[0], old_url=old_url,
                                  old_hash=old_hash, exit_code=exit_code):
                    self.invoke(*workflow, old_url, old_hash, exit_code)

    def test_required_and_optional_arguments_bind_by_name(self):
        self.check_workflows(exit_code=0)

    def test_installer_failure_exit_code_is_propagated(self):
        self.check_workflows(exit_code=37)


if __name__ == "__main__":
    unittest.main()
