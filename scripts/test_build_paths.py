"""产物路径接线契约：发布、安装与原生验收三处必须都落在统一后的根 target。

只断言"从配置里解析出来的路径与参数"的语义（是否根 target、是否带显式目标三元组、
是否选中 GUI package），不把整段命令行当作字符串比对；真实构建与产物由 W06 的实跑
证据覆盖（生产 NSIS 收集、验收包与原生 E2E）。

刻意不依赖 PyYAML：CI 的 checks 步骤只保证标准库。
"""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parent.parent
TRIPLE = "x86_64-pc-windows-msvc"


def read(relative):
    return (ROOT / relative).read_text(encoding="utf-8")


def step_run_block(text, step_name):
    marker = f"      - name: {step_name}\n"
    if text.count(marker) != 1:
        raise AssertionError(f"步骤不是唯一命中：{step_name}")
    tail = text.split(marker, 1)[1]
    match = re.search(r"^        run: \|\n((?:          .*\n|\n)+)", tail, re.M)
    if match is None:
        raise AssertionError(f"步骤没有 run 块：{step_name}")
    return "\n".join(line[10:] for line in match[1].splitlines())


def collect_arguments(block):
    match = re.search(r"release_assets\.py collect(?P<rest>.*)", block)
    if match is None:
        raise AssertionError("步骤里没有 release_assets collect 调用")
    rest = match.group("rest")
    bundle = re.search(r"--bundle\s+(\S+)", rest)
    executable = re.search(r"--executable\s+(\S+)", rest)
    if bundle is None or executable is None:
        raise AssertionError("collect 调用缺少 --bundle 或 --executable")
    return bundle.group(1), executable.group(1)


def workflow_paths(relative):
    """提取 `on.<event>.paths` 列表（保持顺序，含否定模式）。"""
    patterns, inside, indent = [], False, None
    for line in read(relative).split("\n"):
        stripped = line.strip()
        if stripped == "paths:":
            inside, indent = True, len(line) - len(line.lstrip())
            continue
        if inside:
            if not stripped:
                continue
            if len(line) - len(line.lstrip()) <= indent:
                inside = False
                continue
            if stripped.startswith("- "):
                patterns.append(stripped[2:].strip().strip("'\""))
    return patterns


class ReleasePathCase(unittest.TestCase):
    def test_release_and_install_collect_the_same_root_target_layout(self):
        release = collect_arguments(step_run_block(
            read(".github/workflows/release.yml"), "Collect installer and checksums"))
        install = collect_arguments(step_run_block(
            read(".github/workflows/install-test.yml"),
            "Collect current installer and build identity"))
        self.assertEqual(release, install)
        bundle, executable = release
        self.assertTrue(bundle.startswith(f"target/{TRIPLE}/release/bundle/nsis"),
                        bundle)
        self.assertEqual(executable, f"target/{TRIPLE}/release/tokenscope.exe")
        for path in (bundle, executable):
            self.assertNotIn("src-tauri", path)

    def test_release_assets_script_has_no_repository_layout_assumption(self):
        script = read("scripts/release_assets.py")
        self.assertNotIn("src-tauri/target", script)
        self.assertNotIn("target/x86_64", script)
        self.assertIn("src-tauri/tauri.conf.json", script)  # 壳配置未迁移，路径仍正确


class AcceptancePathCase(unittest.TestCase):
    def test_native_acceptance_default_exe_is_root_target_release(self):
        script = read("frontend/scripts/native-acceptance.mjs")
        match = re.search(r'const EXE_DEFAULT = path\.join\(([^)]*)\)', script)
        self.assertIsNotNone(match, "缺少 EXE_DEFAULT 定义")
        parts = [part.strip() for part in match.group(1).split(",")]
        self.assertEqual(parts, ['REPO', '"target"', '"release"', '"tokenscope.exe"'])
        self.assertNotIn("src-tauri", match.group(0))
        self.assertIn('arg("exe", EXE_DEFAULT)', script)  # 用户仍可用 --exe 覆盖

    def test_ui_e2e_builds_gui_package_without_target_triple(self):
        block = step_run_block(read(".github/workflows/ui-e2e.yml"),
                               "Build embedded frontend and isolated acceptance executable")
        build = next(line for line in block.splitlines() if line.startswith("cargo build"))
        self.assertIn("-p tokenscope-app", build)
        self.assertIn("--release", build)
        self.assertIn("acceptance,tauri/custom-protocol", build)
        self.assertIn("--bin tokenscope", build)
        # 不带 --target 才会落在脚本默认读取的根 target/release。
        self.assertNotIn("--target", build)


class CiLayoutCase(unittest.TestCase):
    def test_rust_cache_maps_only_the_root_target(self):
        setup = read(".github/actions/setup/action.yml")
        block = re.search(r"workspaces: \|\n((?:          .*\n)+)", setup)
        self.assertIsNotNone(block, "缺少 rust-cache workspaces 配置")
        entries = [line.strip() for line in block.group(1).splitlines() if line.strip()]
        self.assertEqual(entries, [". -> target"])

    def test_checks_action_runs_workspace_rounds_once(self):
        checks = read(".github/actions/checks/action.yml")
        self.assertEqual(checks.count("cargo fmt --all -- --check"), 1)
        self.assertEqual(checks.count("cargo test --locked --workspace\n"), 1)
        self.assertEqual(checks.count("--features acceptance"), 2)
        self.assertNotIn("--manifest-path src-tauri", checks)
        self.assertIn("python -B scripts/build_inputs.py --check", checks)
        self.assertIn('test_build_paths.py', checks)

    def test_install_test_paths_cover_core_crate_and_not_legacy_root(self):
        patterns = workflow_paths(".github/workflows/install-test.yml")
        self.assertIn("crates/tokenscope-core/**", patterns)
        self.assertIn("src-tauri/**", patterns)
        self.assertIn("Cargo.lock", patterns)
        self.assertIn("docs/privacy.md", patterns)
        for legacy in ("src/**", "tests/**"):
            self.assertNotIn(legacy, patterns)

    def test_gitignore_ignores_target_directories_at_every_level(self):
        entries = [line.strip() for line in read(".gitignore").splitlines()]
        self.assertIn("target/", entries)
        self.assertNotIn("/target", entries)


class LegacyReferenceCase(unittest.TestCase):
    """有效入口不得残留旧布局路径；历史文档不在检查范围。"""

    ACTIVE_GLOBS = (".github", "scripts", "frontend/scripts", "src-tauri/src")

    def test_no_active_entry_point_references_legacy_shell_target(self):
        offenders = []
        for folder in self.ACTIVE_GLOBS:
            for path in sorted((ROOT / folder).rglob("*")):
                if not path.is_file() or path.name == Path(__file__).name:
                    continue
                if path.suffix not in (".yml", ".yaml", ".mjs", ".js", ".py", ".ps1", ".rs"):
                    continue
                if "src-tauri/target" in path.read_text(encoding="utf-8", errors="replace"):
                    offenders.append(str(path.relative_to(ROOT)))
        self.assertEqual(offenders, [])

    def test_only_one_tracked_lockfile_remains(self):
        locks = [str(path.relative_to(ROOT))
                 for path in ROOT.rglob("Cargo.lock")
                 if ".git" not in path.parts and "target" not in path.parts
                 and "node_modules" not in path.parts]
        self.assertEqual(locks, ["Cargo.lock"])


if __name__ == "__main__":
    unittest.main()
