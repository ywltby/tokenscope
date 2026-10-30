"""验收构建记录三处一致性：JS 常量、记录脚本与工作流。

只读源码文本，不构建、不启动 Native、不访问真实用户数据。
"""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parent.parent
LIB = ROOT / "frontend/scripts/lib/native-single-instance.mjs"
RECORDER = ROOT / "scripts/record-acceptance-build.ps1"
WORKFLOW = ROOT / ".github/workflows/ui-e2e.yml"
DRIVER = ROOT / "frontend/scripts/native-acceptance.mjs"

CARGO = ("cargo build --locked -p tokenscope-app --release "
         "--features acceptance,tauri/custom-protocol --bin tokenscope")


def js_constant(text, name):
    match = re.search(rf'export const {name} =\s*\n?\s*"([^"]+)"', text)
    if not match:
        match = re.search(rf'export const {name} = "([^"]+)"', text)
    assert match, f"{name} 未导出"
    return match[1]


class RecordAcceptanceBuildCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.lib = LIB.read_text(encoding="utf-8")
        cls.recorder = RECORDER.read_text(encoding="utf-8")
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.driver = DRIVER.read_text(encoding="utf-8")

    def test_build_command_matches_across_js_and_recorder(self):
        self.assertEqual(js_constant(self.lib, "ACCEPTANCE_BUILD_COMMAND"), CARGO)
        self.assertIn(f"$buildCommand = '{CARGO}'", self.recorder)

    def test_record_kind_matches_across_js_and_recorder(self):
        kind = js_constant(self.lib, "ACCEPTANCE_BUILD_RECORD_KIND")
        self.assertEqual(kind, "tokenscope-acceptance-build")
        self.assertIn(f"kind            = '{kind}'", self.recorder)

    def test_recorder_records_hash_commit_and_features(self):
        for required in ("Get-FileHash -Algorithm SHA256", "artifact_sha256",
                         "git -C $repo rev-parse HEAD", "source_commit",
                         "features        = @('acceptance', 'tauri/custom-protocol')"):
            with self.subTest(required=required):
                self.assertIn(required, self.recorder)

    def test_skipbuild_cannot_skip_the_controlled_command(self):
        # -SkipBuild 语义收窄：受控命令永远执行（增量 no-op），否则任何普通 PE
        # 的哈希都能被登记进记录并获得 verified。
        body = self.recorder[self.recorder.index("[switch]$SkipBuild"):]
        # 不再存在"if (-not $SkipBuild) { 构建 }"的跳过分支：命令调用必须无条件出现
        self.assertNotIn("if (-not $SkipBuild) {", body)
        self.assertIn("& $cargoProgram @cargoArguments", body)
        # 文档说明必须保留收窄语义，防止回归
        self.assertIn("不能跳过命令本身", self.recorder)

    def test_exe_parameter_is_pinned_to_the_controlled_artifact(self):
        # -Exe 收窄：登记路径必须是 target/release/tokenscope.exe，登记其他
        # 路径的哈希会让任意文件获得验收身份。
        self.assertIn("$controlledArtifact = Join-Path $repo 'target/release/tokenscope.exe'",
                      self.recorder)
        self.assertIn("只接受受控构建的实际产物", self.recorder)
        self.assertIn("拒绝登记", self.recorder)

    def test_record_carries_tree_state_and_lib_requires_it(self):
        # 登记侧写入 tree_clean / tree_dirty_count；lib 侧把 tree_clean 设为
        # 必填——旧格式/手写记录无法通过 parse。
        self.assertIn("tree_clean", self.recorder)
        self.assertIn("tree_dirty_count", self.recorder)
        self.assertIn("git -C $repo status --porcelain", self.recorder)
        lib_parse = self.lib[self.lib.index("export function parseAcceptanceBuildRecord"):]
        lib_parse = lib_parse[:lib_parse.index("export function verifyAcceptanceBuildIdentity")]
        self.assertIn("tree_clean", lib_parse)
        self.assertIn("必须由 record-acceptance-build.ps1 登记源树状态", lib_parse)

    def test_workflow_uses_recorder_instead_of_bare_cargo_build(self):
        self.assertIn("./scripts/record-acceptance-build.ps1", self.workflow)
        # 工作流里不得再出现"构建但不登记"的裸 cargo build acceptance 命令
        bare = [line for line in self.workflow.splitlines()
                if "--features acceptance" in line and "record-acceptance-build" not in line]
        self.assertEqual(bare, [], f"工作流仍有未登记的 acceptance 构建：{bare}")

    def test_driver_feeds_record_and_commit_into_preflight(self):
        preflight = self.driver[self.driver.index("evaluateAcceptanceLaunchPreflight({"):]
        preflight = preflight[:preflight.index("});")]
        self.assertIn("buildRecordPath: BUILD_RECORD", preflight)
        self.assertIn("sourceCommit: readGitHeadCommit(REPO)", preflight)
        self.assertIn("launchPreflight.paths.ok", self.driver)
        self.assertNotIn("launchPreflight.root.ok", self.driver)

    def test_marker_screening_alone_never_allows_launch(self):
        body = self.lib[self.lib.index("export function verifyAcceptanceExecutable"):]
        body = body[:body.index("export function evaluateAcceptanceLaunchPreflight")]
        # 标记预筛之后必须先过身份门；旧的"marker-screened 即放行"结论不得回来
        self.assertIn("verifyAcceptanceBuildIdentity({", body)
        self.assertIn("if (!identity.ok) {", body)
        self.assertNotIn('"marker-screened"', body)
        self.assertIn("launch_allowed: false", body)


if __name__ == "__main__":
    unittest.main()
