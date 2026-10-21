"""版本工具的实际文件、Git 基线与失败回滚契约；全部使用临时仓库。

布局契约（PR1）：仓库根是虚拟 workspace，应用版本只有一个权威核心 manifest
`crates/tokenscope-core/Cargo.toml`；受版本管理的锁只有根 `Cargo.lock`，
其中恰好包含两个无 source 的本地包。历史 ref 的版本必须按该 ref 自身的
布局读取，当前布局不能决定历史布局。
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import build_inputs
import bump_version as policy

ROOT = Path(__file__).resolve().parent.parent
CORE_MANIFEST = "crates/tokenscope-core/Cargo.toml"
SHELL_MANIFEST = "src-tauri/Cargo.toml"
LOCK = "Cargo.lock"
LEGACY_SHELL_LOCK = "src-tauri/Cargo.lock"
CONFIGS = (CORE_MANIFEST, SHELL_MANIFEST, "frontend/package.json",
           "src-tauri/tauri.conf.json")
EXTERNAL_PACKAGE = ('[[package]]\nname = "other"\nversion = "0.1.0"\n'
                    'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                    'checksum = "' + "0" * 64 + '"\n')
# 第三方同名包：带 source，不是本地 package，版本工具不得改写。
EXTERNAL_NAMESAKE = ('[[package]]\nname = "tokenscope"\nversion = "9.9.9"\n'
                     'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                     'checksum = "' + "1" * 64 + '"\n')


def manifest(package, version):
    return (f'[package]\nname = "{package}"\nversion = "{version}"\n'
            '[dependencies]\nother = "0.1.0"\n')


def virtual_manifest():
    return ('[workspace]\nmembers = ["crates/tokenscope-core", "src-tauri"]\n'
            'default-members = ["crates/tokenscope-core", "src-tauri"]\n'
            'resolver = "3"\n')


def lock_text(packages, version="0.1.0"):
    local = "".join(f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
                    for name in packages)
    return "version = 4\n" + local + EXTERNAL_PACKAGE + EXTERNAL_NAMESAKE


def write_configs(root, version):
    for name in ("frontend/package.json", "src-tauri/tauri.conf.json"):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"version": version, "label": "中文"},
                                   ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


BUILD_INPUTS = "scripts/build-inputs.json"


def write_build_inputs(root):
    """夹具仓库与真实仓库同构：`docs/privacy.md` 编译进程序，属产品输入。"""
    path = root / BUILD_INPUTS
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({
        "schema_version": 1,
        "generated": [{"path": "docs/privacy.md", "kind": "file",
                       "consumers": [{"package": "tokenscope-app", "mode": "default"}]}],
        "manual": [],
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def seed(root, version="0.1.0"):
    """当前布局：根虚拟 workspace + 核心 manifest + 单根锁。"""
    for name, package in ((CORE_MANIFEST, "tokenscope"), (SHELL_MANIFEST, "tokenscope-app")):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(manifest(package, version), encoding="utf-8")
    (root / "Cargo.toml").write_text(virtual_manifest(), encoding="utf-8")
    write_configs(root, version)
    write_build_inputs(root)
    (root / LOCK).write_text(lock_text(("tokenscope", "tokenscope-app"), version),
                             encoding="utf-8")


def seed_legacy(root, version="0.1.0"):
    """迁移前布局：根 package + 壳子锁。"""
    (root / "Cargo.toml").write_text(manifest("tokenscope", version), encoding="utf-8")
    shell = root / SHELL_MANIFEST
    shell.parent.mkdir(parents=True, exist_ok=True)
    shell.write_text(manifest("tokenscope-app", version), encoding="utf-8")
    write_configs(root, version)
    write_build_inputs(root)
    (root / LOCK).write_text(lock_text(("tokenscope",), version), encoding="utf-8")
    (root / LEGACY_SHELL_LOCK).write_text(lock_text(("tokenscope", "tokenscope-app"), version),
                                          encoding="utf-8")


def migrate_layout(root, version="0.1.0"):
    """把旧布局工作区就地改造成新布局（模拟迁移提交）。"""
    core = root / CORE_MANIFEST
    core.parent.mkdir(parents=True, exist_ok=True)
    core.write_text((root / "Cargo.toml").read_text(encoding="utf-8"), encoding="utf-8")
    (root / "Cargo.toml").write_text(virtual_manifest(), encoding="utf-8")
    (root / LOCK).write_text(lock_text(("tokenscope", "tokenscope-app"), version),
                             encoding="utf-8")
    (root / LEGACY_SHELL_LOCK).unlink()


def git_env():
    """Git 钩子会注入 GIT_DIR 等变量；测试仓库必须与调用者环境隔离。"""
    return {key: value for key, value in os.environ.items()
            if not key.startswith("GIT_")}


def setUpModule():
    """实现本身保留 GIT_*（它们决定本次提交的内容），隔离只能发生在测试侧。"""
    for key in [name for name in os.environ if name.startswith("GIT_")]:
        del os.environ[key]


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   encoding="utf-8", stderr=subprocess.PIPE,
                                   env=git_env()).strip()


class RepoCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-version-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "中文 仓库"
        self.root.mkdir()
        git(self.root, "init", "-q", "-b", "main")
        git(self.root, "config", "user.name", "TokenScope fixture")
        git(self.root, "config", "user.email", "fixture@example.invalid")
        # 真实仓库用 .gitattributes 钉 eol=lf；夹具关掉自动转换，让工作区与
        # 提交内容逐字节相同，声明一致性才可验证。
        git(self.root, "config", "core.autocrlf", "false")
        self.origin = Path(self.temp.name) / "origin.git"

    def commit_all(self, message="baseline"):
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", message)
        return git(self.root, "rev-parse", "HEAD")

    def publish(self):
        subprocess.run(["git", "clone", "--bare", "--quiet", str(self.root),
                        str(self.origin)], check=True, capture_output=True, env=git_env())
        git(self.root, "remote", "add", "origin", str(self.origin))

    def snapshot(self):
        return {p.relative_to(self.root).as_posix(): p.read_bytes()
                for p in self.root.rglob("*") if p.is_file() and ".git" not in p.parts}


class VersionPolicyCase(RepoCase):
    def setUp(self):
        super().setUp()
        seed(self.root)
        self.base = self.commit_all()
        self.publish()
        git(self.root, "fetch", "--quiet", "origin", "main")
        git(self.root, "switch", "-qc", "feature")

    def bump(self, kind="minor"):
        return policy.bump(self.root, kind, "origin/main", "历史用量与版本递增")

    def test_minor_and_patch_follow_zero_major_policy(self):
        for before, kind, after in (("0.1.0", "minor", "0.2.0"),
                                    ("0.2.9", "minor", "0.3.0"),
                                    ("0.2.9", "patch", "0.2.10")):
            with self.subTest(before=before, kind=kind):
                self.assertEqual(policy.next_version(before, kind), after)
        for version in ("1.0.0", "0.01.0", "0.2", "0.2.0-rc.1", "0.2.0+build"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                policy.next_version(version, "patch")
        with self.assertRaises(ValueError):
            policy.next_version("0.1.0", "major")

    def test_four_configs_one_lock_and_declaration_change_without_dependency_drift(self):
        self.assertEqual(self.bump(), "0.2.0")
        result = policy.check(self.root, self.base)
        self.assertEqual(result["version"], "0.2.0")
        self.assertEqual(result["kind"], "minor")
        for name in (CORE_MANIFEST, SHELL_MANIFEST):
            self.assertIn('other = "0.1.0"', (self.root / name).read_text(encoding="utf-8"))
        lock = (self.root / LOCK).read_text(encoding="utf-8")
        self.assertIn('name = "tokenscope"\nversion = "0.2.0"', lock)
        self.assertIn('name = "tokenscope-app"\nversion = "0.2.0"', lock)
        self.assertIn('name = "other"\nversion = "0.1.0"', lock)
        self.assertIn("中文", (self.root / "frontend/package.json").read_text(encoding="utf-8"))
        self.assertFalse((self.root / LEGACY_SHELL_LOCK).exists())
        self.assertNotIn("version", (self.root / "Cargo.toml").read_text(encoding="utf-8"))

    def test_external_package_with_same_name_and_source_is_never_rewritten(self):
        self.bump()
        lock = (self.root / LOCK).read_text(encoding="utf-8")
        self.assertIn('name = "tokenscope"\nversion = "9.9.9"\nsource =', lock)

    def test_lock_missing_shell_package_duplicate_or_wrong_version_is_rejected(self):
        cases = {
            "missing-shell": lock_text(("tokenscope",)),
            "duplicate-core": lock_text(("tokenscope", "tokenscope", "tokenscope-app")),
            "stale-version": lock_text(("tokenscope", "tokenscope-app"), "0.0.9"),
        }
        for name, text in cases.items():
            with self.subTest(case=name):
                (self.root / LOCK).write_text(text, encoding="utf-8")
                with self.assertRaises(ValueError):
                    policy.check(self.root, self.base)
        (self.root / LOCK).write_text(lock_text(("tokenscope", "tokenscope-app")),
                                      encoding="utf-8")
        (self.root / LOCK).write_text(
            lock_text(("tokenscope", "tokenscope-app")).replace(
                'name = "tokenscope-app"\nversion = "0.1.0"',
                'name = "tokenscope-app"\nversion = "0.1.0"\nsource = "git+https://example.invalid/x"'),
            encoding="utf-8")
        with self.assertRaises(ValueError):
            policy.check(self.root, self.base)

    def test_repeated_bump_is_idempotent_for_the_same_main_baseline(self):
        self.bump()
        before = self.snapshot()
        self.bump()
        self.assertEqual(before, self.snapshot())

    def test_missing_bump_mismatched_config_lock_and_kind_are_rejected(self):
        with self.assertRaises(ValueError):
            policy.check(self.root, self.base)
        self.bump()
        good = self.snapshot()
        mutations = ((CORE_MANIFEST, 'version = "0.2.0"', 'version = "0.1.1"'),
                     (LOCK, 'name = "tokenscope"\nversion = "0.2.0"',
                      'name = "tokenscope"\nversion = "0.1.0"'),
                     ("release-change.json", '"minor"', '"patch"'))
        for name, old, new in mutations:
            with self.subTest(file=name):
                path = self.root / name
                path.write_text(path.read_text(encoding="utf-8").replace(old, new),
                                encoding="utf-8")
                with self.assertRaises(ValueError):
                    policy.check(self.root, self.base)
                path.write_bytes(good[name])

    def test_version_rollback_skip_and_new_main_baseline_are_rejected(self):
        self.bump()
        declaration = (self.root / "release-change.json").read_text(encoding="utf-8")
        for version in ("0.1.0", "0.0.9", "0.3.0", "0.2.1", "1.0.0"):
            with self.subTest(version=version):
                seed(self.root, version)
                (self.root / "release-change.json").write_text(declaration, encoding="utf-8")
                with self.assertRaises(ValueError):
                    policy.check(self.root, self.base)
        seed(self.root, "0.2.0")
        (self.root / "release-change.json").write_text(declaration, encoding="utf-8")
        self.commit_all("another main version")
        # 版本没变又没有产品改动时，自比不构成一次合入：允许且不发布。
        self.assertFalse(policy.check(self.root, "HEAD")["release"])
        # 已含递增的基线上再改产品输入、版本不动，仍然必须被拒。
        product = self.root / "crates/tokenscope-core/src/lib.rs"
        product.parent.mkdir(parents=True, exist_ok=True)
        product.write_text("产品改动\n", encoding="utf-8")
        self.commit_all("product change without a bump")
        with self.assertRaises(ValueError):
            policy.check(self.root, "HEAD~1")

    def test_mid_write_failure_rolls_back_all_files_and_new_declaration(self):
        before = self.snapshot()
        replace = os.replace
        count = 0

        def fail_once(source, destination):
            nonlocal count
            count += 1
            if count == 3:
                raise OSError("injected replace failure")
            return replace(source, destination)

        with patch.object(policy.os, "replace", side_effect=fail_once):
            with self.assertRaises(OSError):
                self.bump()
        self.assertEqual(before, self.snapshot())

    def test_actual_event_before_supports_fast_forward_with_multiple_commits_and_merge(self):
        self.bump()
        self.commit_all("version")
        (self.root / "note.txt").write_text("第二个提交\n", encoding="utf-8")
        self.commit_all("feature")
        event = {"ref": "refs/heads/main", "before": self.base}
        self.assertEqual(policy.check_event(self.root, event, "push")["version"], "0.2.0")
        git(self.root, "switch", "-q", "main")
        git(self.root, "merge", "--no-ff", "-qm", "merge", "feature")
        self.assertEqual(policy.check_event(self.root, event, "push")["version"], "0.2.0")

    def test_pr_base_is_fetched_and_requires_main_to_be_in_tested_checkout(self):
        self.bump()
        event = {"pull_request": {"base": {"ref": "main", "sha": self.base}}}
        self.assertEqual(policy.check_event(self.root, event, "pull_request")["version"], "0.2.0")
        event["pull_request"]["base"]["ref"] = "other"
        with self.assertRaises(ValueError):
            policy.check_event(self.root, event, "pull_request")

    def test_remote_main_advancement_is_detected_even_when_minor_number_still_matches(self):
        self.bump()
        updater = Path(self.temp.name) / "updater"
        subprocess.run(["git","clone","--quiet",str(self.origin),str(updater)],
                       check=True,capture_output=True, env=git_env())
        git(updater,"config","user.name","TokenScope fixture")
        git(updater,"config","user.email","fixture@example.invalid")
        policy.bump(updater,"patch","origin/main","patch")
        git(updater,"add","-A")
        git(updater,"commit","-qm","advance main")
        git(updater,"push","--quiet","origin","main")
        event={"pull_request":{"base":{"ref":"main","sha":self.base}}}
        with self.assertRaisesRegex(ValueError,"main 已推进"):
            policy.check_event(self.root,event,"pull_request")


class HistoricalLayoutCase(RepoCase):
    """迁移前 main → 新布局分支；两种布局的历史基线都必须按自身布局读取。"""

    def setUp(self):
        super().setUp()
        seed_legacy(self.root)
        self.legacy_base = self.commit_all("legacy main")
        self.publish()
        migrate_layout(self.root)
        self.new_base = self.commit_all("workspace migration")
        git(self.root, "fetch", "--quiet", "origin", "main")
        git(self.root, "switch", "-qc", "feature")

    def test_bump_from_legacy_main_reads_that_ref_layout_and_updates_current_layout(self):
        self.assertEqual(policy.bump(self.root, "minor", self.legacy_base, "迁移后递增"), "0.2.0")
        result = policy.check(self.root, self.legacy_base)
        self.assertEqual(result["previous"], "0.1.0")
        self.assertEqual(result["version"], "0.2.0")
        self.assertIn('version = "0.2.0"',
                      (self.root / CORE_MANIFEST).read_text(encoding="utf-8"))
        self.assertFalse((self.root / LEGACY_SHELL_LOCK).exists())

    def test_bump_from_new_layout_main_also_reads_that_ref_layout(self):
        self.assertEqual(policy.bump(self.root, "patch", self.new_base, "后续补修"), "0.1.1")

    def test_corrupt_new_layout_manifest_never_falls_back_to_root_package(self):
        (self.root / CORE_MANIFEST).write_text(
            manifest("tokenscope", "0.2.0").replace('version = "0.2.0"', 'version = "unclosed'),
            encoding="utf-8")
        (self.root / SHELL_MANIFEST).write_text(manifest("tokenscope-app", "0.2.0"),
                                                encoding="utf-8")
        write_configs(self.root, "0.2.0")
        (self.root / LOCK).write_text(lock_text(("tokenscope", "tokenscope-app"), "0.2.0"),
                                      encoding="utf-8")
        # 同时在根 manifest 伪造旧式 package，证明损坏不会静默回退旧布局。
        (self.root / "Cargo.toml").write_text(manifest("tokenscope", "0.2.0"), encoding="utf-8")
        with self.assertRaises(ValueError):
            policy.check(self.root, self.new_base)

    def test_legacy_ref_without_core_manifest_is_distinguished_from_git_errors(self):
        legacy = self.legacy_base
        with self.assertRaises(ValueError):
            policy.historical_versions(self.root, "no-such-ref")
        self.assertEqual(policy.historical_versions(self.root, legacy)[1], "0.1.0")

    def test_shell_specific_version_drift_across_layouts_is_rejected(self):
        (self.root / SHELL_MANIFEST).write_text(manifest("tokenscope-app", "0.1.5"),
                                                encoding="utf-8")
        with self.assertRaises(ValueError):
            policy.check(self.root, self.new_base)


def build_manifest(*records):
    """最小 build-inputs 清单：records 为 (path, kind, modes)。"""
    return {"schema_version": 1,
            "generated": [{"path": path, "kind": kind,
                           "consumers": [{"package": "tokenscope", "mode": mode}
                                         for mode in modes]}
                          for path, kind, modes in records],
            "manual": []}


class ReleaseRelevanceCase(unittest.TestCase):
    """只有会进安装包的改动才要求递增版本。

    判定数据源只有 `scripts/build-inputs.json`：登记了 default 消费方的输入
    永远按产品处理，其余路径按明确的非产品清单排除，未知路径一律按产品。
    """

    def manifest(self):
        return build_inputs.load_manifest(ROOT / "scripts/build-inputs.json")

    def test_documents_ci_scripts_and_tests_do_not_need_a_release(self):
        paths = ["docs/plans/active/2026-10-21-release-on-version-change.md",
                 "README.md", "CLAUDE.md", "DESIGN.md",
                 ".github/workflows/release.yml",
                 ".github/actions/checks/action.yml",
                 ".githooks/pre-commit", "scripts/bump_version.py",
                 "scripts/test_bump_version.py",
                 "scripts/fixtures/upgrade_baselines.json",
                 "frontend/scripts/native-acceptance.mjs",
                 "frontend/src/usage.test.ts", "frontend/src/usage.test.mjs",
                 "crates/tokenscope-core/tests/usage_event.rs",
                 "crates/tokenscope-core/examples/print_persistent_formats.rs",
                 "src-tauri/tests/shell.rs",
                 ".gitignore", ".gitattributes", ".editorconfig",
                 "release-change.json"]
        self.assertEqual(policy.release_relevant(paths, self.manifest()), [])

    def test_empty_change_set_needs_no_release(self):
        self.assertEqual(policy.release_relevant([], self.manifest()), [])

    def test_registered_default_inputs_stay_relevant_even_under_docs_or_tests(self):
        manifest = self.manifest()
        for path in ("docs/privacy.md",
                     "crates/tokenscope-core/tests/fixtures/cache/schema-v3.sql",
                     "src-tauri/capabilities/default.json",
                     "src-tauri/tauri.conf.json"):
            with self.subTest(path=path):
                self.assertEqual(policy.release_relevant([path], manifest), [path])

    def test_acceptance_only_registration_does_not_make_a_docs_path_relevant(self):
        manifest = build_manifest(("docs/privacy.md", "file", ("acceptance",)))
        self.assertEqual(policy.release_relevant(["docs/privacy.md"], manifest), [])

    def test_acceptance_only_source_stays_relevant_by_path(self):
        manifest = build_manifest(("src-tauri/src/acceptance_csp.js", "file",
                                   ("acceptance",)))
        self.assertEqual(policy.release_relevant(["src-tauri/src/acceptance_csp.js"],
                                                 manifest),
                         ["src-tauri/src/acceptance_csp.js"])

    def test_markdown_registered_as_a_default_input_is_relevant(self):
        manifest = build_manifest(("README.md", "file", ("default",)))
        self.assertEqual(policy.release_relevant(["README.md"], manifest), ["README.md"])

    def test_directory_inputs_cover_children_but_not_sibling_prefixes(self):
        manifest = build_manifest(("assets/brand", "directory", ("default",)))
        self.assertEqual(policy.release_relevant(["assets/brand/logo.svg"], manifest),
                         ["assets/brand/logo.svg"])
        manifest = build_manifest(("docs/privacy", "directory", ("default",)))
        self.assertEqual(policy.release_relevant(["docs/privacy/policy.md",
                                                  "docs/privacy-extra/policy.md"],
                                                 manifest),
                         ["docs/privacy/policy.md"])

    def test_unknown_and_product_paths_are_relevant(self):
        manifest = self.manifest()
        paths = ["Cargo.toml", "Cargo.lock", "NOTES.txt", "toolchain.toml",
                 ".cargo/config.toml", "frontend/src/App.vue",
                 "frontend/package.json", "src-tauri/src/main.rs",
                 "crates/tokenscope-core/src/lib.rs"]
        self.assertEqual(policy.release_relevant(paths, manifest), paths)

    def test_rename_participates_on_both_sides(self):
        manifest = self.manifest()
        paths = ["docs/old.md", "crates/tokenscope-core/src/new.rs"]
        self.assertEqual(policy.release_relevant(paths, manifest),
                         ["crates/tokenscope-core/src/new.rs"])


def seed_declaration(root, kind="patch", summary="基线版本声明"):
    (root / "release-change.json").write_text(
        json.dumps({"schema_version": 1, "kind": kind, "summary": summary},
                   ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


class ReleaseOnChangeCase(RepoCase):
    """版本没变的合入：只有产品输入改动才要求递增，其余只校验声明成对。"""

    def setUp(self):
        super().setUp()
        seed(self.root)
        seed_declaration(self.root)
        self.base = self.commit_all()
        self.publish()
        git(self.root, "fetch", "--quiet", "origin", "main")
        git(self.root, "switch", "-qc", "feature")

    def touch(self, name, text="改动\n"):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return self.commit_all(name)

    def test_documentation_only_change_skips_the_release(self):
        self.touch("docs/plans/note.md")
        result = policy.check(self.root, self.base)
        self.assertEqual(result["version"], "0.1.0")
        self.assertFalse(result["release"])

    def test_main_push_and_pull_request_events_both_skip_documentation_changes(self):
        self.touch("docs/plans/note.md")
        push = policy.check_event(self.root, {"ref": "refs/heads/main",
                                              "before": self.base}, "push")
        self.assertFalse(push["release"])
        pull = policy.check_event(
            self.root,
            {"pull_request": {"base": {"ref": "main", "sha": self.base}}},
            "pull_request")
        self.assertFalse(pull["release"])

    def test_product_change_without_a_version_bump_is_rejected_naming_the_paths(self):
        self.touch("crates/tokenscope-core/src/lib.rs")
        with self.assertRaises(ValueError) as raised:
            policy.check(self.root, self.base)
        self.assertIn("crates/tokenscope-core/src/lib.rs", str(raised.exception))
        with self.assertRaises(ValueError):
            policy.check_event(self.root, {"ref": "refs/heads/main", "before": self.base},
                               "push")
        with self.assertRaises(ValueError):
            policy.check_event(
                self.root,
                {"pull_request": {"base": {"ref": "main", "sha": self.base}}},
                "pull_request")

    def test_privacy_document_is_a_product_input_because_it_is_compiled_in(self):
        self.touch("docs/privacy.md", "# 隐私说明\n")
        with self.assertRaises(ValueError):
            policy.check(self.root, self.base)

    def test_unknown_root_file_counts_as_product(self):
        self.touch("NOTES.txt")
        with self.assertRaises(ValueError):
            policy.check(self.root, self.base)

    def test_documentation_change_with_a_legal_patch_bump_still_releases(self):
        self.touch("docs/plans/note.md")
        self.assertEqual(policy.bump(self.root, "patch", self.base, "文档整理"), "0.1.1")
        result = policy.check(self.root, self.base)
        self.assertTrue(result["release"])
        self.assertEqual(result["kind"], "patch")

    def test_declaration_must_match_the_baseline_when_the_version_is_unchanged(self):
        self.touch("docs/plans/note.md")
        seed_declaration(self.root, summary="换了个说明")
        self.commit_all("声明")
        with self.assertRaises(ValueError) as raised:
            policy.check(self.root, self.base)
        self.assertIn("release-change.json", str(raised.exception))

    def test_tag_event_keeps_releasing(self):
        result = policy.check_event(self.root, {"ref": "refs/tags/v0.1.0"}, "push")
        self.assertTrue(result["release"])
        self.assertEqual(result["kind"], "existing-tag")


if __name__ == "__main__":
    unittest.main()
