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

    def test_record_carries_builder_grade_and_lib_gates_on_ci(self):
        # 七轮复核：builder/ci 段是自报字段，不再授予 verified——lib 无
        # verified 出口（可核验绑定 = GitHub API artifact 核验，未实现）。
        # 登记侧保留 builder/ci 段作为未来核验的证据链。
        self.assertIn("$builder = if ($isCi) { 'ci' } else { 'local' }", self.recorder)
        self.assertIn("builder         = $builder", self.recorder)
        self.assertIn("run_id      = $env:GITHUB_RUN_ID", self.recorder)
        self.assertIn("repository  = $env:GITHUB_REPOSITORY", self.recorder)
        lib_identity = self.lib[self.lib.index("export function verifyAcceptanceBuildIdentity"):]
        lib_identity = lib_identity[:lib_identity.index("export function verifyBuildOriginBinding")]
        # 无 verified 出口：身份函数不再返回 build_identity: "verified"
        self.assertNotIn('build_identity: "verified"', lib_identity)
        # 结论口径：无法独立证明来源 → unverified
        self.assertIn("无法独立证明产物来源", lib_identity)
        # 不再以自报环境变量/字段作为许可条件（注释里可提及，许可逻辑不得引用）
        code_only = "\n".join(line for line in lib_identity.splitlines()
                              if not line.strip().startswith("//"))
        self.assertNotIn("GITHUB_ACTIONS", code_only)
        # 受控绑定核验核心存在（GitHub run 事实 + artifact digest），但未接线
        # 到预检——预检仍无 verified 出口；接线留待显式受控步骤（AC10 开放）。
        # 按 GitHub 实际响应形态：run.name 是工作流名、repository 是嵌套
        # full_name、构建是 native-ui job 内的 step（不是独立 job）、核验构建
        # step 而非整个 run、四方哈希（归档/归档内 EXE/记录/本地）。
        self.assertIn("export function verifyBuildOriginBinding", self.lib)
        binding = self.lib[self.lib.index("export function verifyBuildOriginBinding"):]
        self.assertIn("full_name", binding)
        self.assertIn('buildStepName = "Build embedded frontend and isolated acceptance executable"',
                      binding)
        self.assertIn('trustedBuildJobName = "native-ui"', binding)
        self.assertIn("archiveExeSha256", binding)
        self.assertIn("归档内容与记录不符", binding)
        self.assertIn("startsWith(\"sha256:\")", binding)
        # 合成测试覆盖可信与伪造路径（离线 GitHub 实际响应形态）
        test_text = (ROOT / "frontend/scripts/lib/native-single-instance.test.mjs").read_text(
            encoding="utf-8")
        self.assertIn("受控绑定（按 GitHub 实际响应形态）", test_text)
        self.assertIn("受控绑定反例（真实响应形态）", test_text)

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
