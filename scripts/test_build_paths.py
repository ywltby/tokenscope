"""产物路径接线契约：发布、安装与原生验收三处必须都落在统一后的根 target。

只断言"从配置里解析出来的路径与参数"的语义（是否根 target、是否带显式目标三元组、
是否选中 GUI package），不把整段命令行当作字符串比对；真实构建与产物由 W06 的实跑
证据覆盖（生产 NSIS 收集、验收包与原生 E2E）。

刻意不依赖 PyYAML：CI 的 checks 步骤只保证标准库。
"""

from pathlib import Path
import json
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent
TRIPLE = "x86_64-pc-windows-msvc"
# CI 拆分后的四个检查组：每个检查步骤都必须归属其中之一，且各组都有作业在跑。
CHECK_GROUPS = ("rust", "rust-acceptance", "contracts", "frontend")
GROUP_CONDITION = re.compile(r"(?:always\(\) && )?inputs\.group == ['\"]([a-z-]+)['\"]")


def read(relative):
    return (ROOT / relative).read_text(encoding="utf-8")


def read_toml(relative):
    return tomllib.loads(read(relative))


def core_version():
    return read_toml("crates/tokenscope-core/Cargo.toml")["package"]["version"]


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


def job_block(text, name):
    """截取工作流中顶层作业块（写法同 test_upgrade_workflow.py）。"""
    match = re.search(rf"^  {re.escape(name)}:\n((?:    .*\n|\n)+)", text, re.M)
    if match is None:
        raise AssertionError(f"workflow 里没有作业 {name}")
    return match[1]


def job_names(text):
    """`jobs:` 下的顶层作业名（缩进恰好 2 空格、行尾无值）。"""
    tail = text.split("\njobs:\n", 1)[1]
    return re.findall(r"^  ([A-Za-z0-9_-]+): *$", tail, re.M)


def needs_of(block):
    """作业的 `needs` 列表，支持内联与块序列两种写法。"""
    match = re.search(r"^    needs: *(\S.*)?$", block, re.M)
    if match is None:
        raise AssertionError("作业没有 needs")
    inline = match.group(1)
    if inline:
        return [item.strip().strip("'\"")
                for item in inline.strip().strip("[]").split(",") if item.strip()]
    items = []
    for line in block[match.end():].lstrip("\r\n").splitlines():
        stripped = line.strip()
        if not stripped:
            break
        if stripped.startswith("#"):
            continue
        if not stripped.startswith("- "):
            break
        items.append(stripped[2:].strip().strip("'\""))
    return items


def action_steps(relative):
    """按出现顺序提取复合动作的步骤：名字、`if` 条件与整块原始文本。"""
    steps = []
    for line in read(relative).split("\n"):
        if line.startswith("    - "):
            name = line[len("    - name: "):].strip() if line.startswith("    - name: ") else ""
            steps.append({"name": name, "condition": "", "text": [line]})
            continue
        if not steps:
            continue
        steps[-1]["text"].append(line)
        stripped = line.strip()
        if stripped.startswith("if: "):
            steps[-1]["condition"] = stripped[len("if: "):]
    return steps


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


class WorkspaceInvariantCase(unittest.TestCase):
    """计划 §2 的目标配置：根为虚拟 workspace、共享依赖继承、依赖方向单向。"""

    def test_root_manifest_is_a_virtual_workspace_without_exclude(self):
        manifest = read_toml("Cargo.toml")
        self.assertNotIn("package", manifest)
        self.assertNotIn("dependencies", manifest)
        workspace = manifest["workspace"]
        self.assertEqual(workspace["members"], ["crates/tokenscope-core", "src-tauri"])
        self.assertEqual(workspace["default-members"],
                         ["crates/tokenscope-core", "src-tauri"])
        self.assertEqual(workspace["resolver"], "3")
        self.assertNotIn("exclude", workspace)
        self.assertNotIn("version", workspace)

    def test_shell_manifest_has_no_own_workspace_and_points_at_the_core_crate(self):
        shell = read_toml("src-tauri/Cargo.toml")
        self.assertNotIn("workspace", shell)
        self.assertEqual(shell["package"]["name"], "tokenscope-app")
        self.assertEqual(shell["dependencies"]["tokenscope"]["path"],
                         "../crates/tokenscope-core")
        self.assertEqual(shell["features"]["acceptance"], ["tokenscope/acceptance"])

    def test_shared_dependencies_are_inherited_with_explicit_features(self):
        root = read_toml("Cargo.toml")["workspace"]["dependencies"]
        core = read_toml("crates/tokenscope-core/Cargo.toml")
        shell = read_toml("src-tauri/Cargo.toml")
        for name in ("anyhow", "jiff", "log", "serde", "serde_json"):
            self.assertIn(name, root, f"共享依赖 {name} 未在根声明")
        self.assertEqual(core["dependencies"]["serde"],
                         {"workspace": True, "features": ["derive"]})
        self.assertEqual(shell["dependencies"]["serde"],
                         {"workspace": True, "features": ["derive"]})
        self.assertEqual(core["dependencies"]["anyhow"], {"workspace": True})
        self.assertEqual(shell["dependencies"]["jiff"], {"workspace": True})

    def test_core_library_never_depends_on_tauri(self):
        core = read_toml("crates/tokenscope-core/Cargo.toml")
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            names = set(core.get(section, {}))
            offenders = sorted(name for name in names
                               if name == "tauri" or name.startswith("tauri-"))
            self.assertEqual(offenders, [], f"{section} 引入了 Tauri 依赖")

    def test_root_lock_holds_exactly_the_two_local_packages_at_the_app_version(self):
        lock = read_toml("Cargo.lock")
        version = core_version()
        local = [(package["name"], package["version"])
                 for package in lock["package"] if "source" not in package]
        self.assertEqual(sorted(local), [("tokenscope", version),
                                         ("tokenscope-app", version)])
        for name in ("crates/tokenscope-core/Cargo.toml", "src-tauri/Cargo.toml"):
            self.assertEqual(read_toml(name)["package"]["version"], version, name)
        self.assertEqual(json.loads(read("src-tauri/tauri.conf.json"))["version"], version)
        self.assertEqual(json.loads(read("frontend/package.json"))["version"], version)

    def test_manifest_records_are_repository_relative_without_run_metadata(self):
        text = read("scripts/build-inputs.json")
        value = json.loads(text)
        self.assertEqual(set(value), {"schema_version", "generated", "manual"})
        for record in value["generated"] + value["manual"]:
            path = record["path"]
            self.assertFalse(path.startswith("/"), path)
            self.assertNotIn("\\", path, path)
            self.assertNotIn(":", path, path)
            self.assertNotIn("..", path.split("/"), path)
        lowered = text.lower()
        for banned in ("run_id", "mtime", "c:\\\\"):
            self.assertNotIn(banned, lowered)


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

    def test_every_checks_step_belongs_to_exactly_one_group(self):
        """拆分后不得有检查掉出所有组：每步恰属一组，且四个组都有步骤在跑。"""
        steps = action_steps(".github/actions/checks/action.yml")
        self.assertGreater(len(steps), 1, "checks 动作没有步骤")
        self.assertEqual(steps[0]["name"], "Validate the check group")
        self.assertEqual(steps[0]["condition"], "", "组校验自身不挂在任何组上")
        body = "\n".join(steps[0]["text"])
        for group in CHECK_GROUPS:
            self.assertIn(group, body, f"组校验没有列出 {group}")
        groups = []
        for step in steps[1:]:
            match = GROUP_CONDITION.fullmatch(step["condition"])
            self.assertIsNotNone(match, f"步骤没有归属组：{step['name']}")
            self.assertIn(match.group(1), CHECK_GROUPS, step["name"])
            groups.append(match.group(1))
        self.assertEqual(sorted(set(groups)), sorted(CHECK_GROUPS), "有组没有步骤在跑")

    def test_ci_runs_each_check_group_once_and_gate_covers_them(self):
        """四个组各被一个作业调用一次；gate 汇总这四者且自身不跑检查。"""
        ci = read(".github/workflows/ci.yml")
        called = {}
        for name in job_names(ci):
            block = job_block(ci, name)
            if "./.github/actions/checks" not in block:
                continue
            match = re.search(r"^ *group: *([a-z-]+) *$", block, re.M)
            self.assertIsNotNone(match, f"作业 {name} 没有传给 checks 的 group")
            called[name] = match.group(1)
        self.assertEqual(sorted(called.values()), sorted(CHECK_GROUPS),
                         "四个组必须各被调用恰好一次")
        for name, group in called.items():
            self.assertEqual(name, group, "作业名与组名一致（gate 的 needs 依赖它）")
        gate = job_block(ci, "gate")
        self.assertNotIn("./.github/actions/checks", gate, "gate 不再直接跑检查")
        self.assertEqual(sorted(needs_of(gate)), sorted(CHECK_GROUPS))

    def test_release_gate_keeps_identity_checks_without_full_quality_gate(self):
        # 完整质量门禁由 PR 必需检查（ci.yml 的 gate）保证；Release 的 gate 只保留
        # 发布身份校验。防止日后删测试时连身份校验一起删掉，或把 CI 的完整门禁误删。
        release_gate = job_block(read(".github/workflows/release.yml"), "gate")
        self.assertIn("bump_version.py --check", release_gate)
        self.assertIn("release_metadata.py", release_gate)
        self.assertIn("id: metadata", release_gate)
        for duplicated in ("./.github/actions/setup", "./.github/actions/checks"):
            self.assertNotIn(duplicated, release_gate, duplicated)
        # CI 侧只要求工作流仍在调用该动作；每个步骤的归属与作业调用由
        # CheckGroupCase 覆盖（gate 本身是汇总作业，不再直接跑检查）。
        self.assertIn("./.github/actions/checks", read(".github/workflows/ci.yml"))

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
