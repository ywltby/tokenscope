"""构建输入清单生成器的纯函数契约（不编译真实工程）。

覆盖：dep-info 解析（多目标、续行、转义、Windows 盘符、中文）、路径规范化
与仓库边界、Git 跟踪过滤、固定/生成物排除、构建脚本输入、记录合并与
schema 校验。
"""

from pathlib import Path
import json
import tempfile
import unittest

from build_inputs import (
    ManifestError,
    collect_records,
    filter_tracked,
    is_excluded_input,
    load_manifest,
    merge_records,
    normalize_repo_path,
    parse_build_script_output,
    parse_dep_info,
    workflow_coverage,
)

REPO_POSIX = "/repo"


def posix_repo():
    return Path(REPO_POSIX)


class DepInfoCase(unittest.TestCase):
    def test_single_rule_returns_prerequisites_without_target(self):
        text = ("C:\\repo\\target\\debug\\deps\\libtokenscope-1a.rlib: "
                "crates\\tokenscope-core\\src\\lib.rs crates\\tokenscope-core\\src\\model.rs\n")
        self.assertEqual(parse_dep_info(text), [
            "crates\\tokenscope-core\\src\\lib.rs",
            "crates\\tokenscope-core\\src\\model.rs",
        ])

    def test_multiple_rules_in_one_file_are_all_parsed(self):
        text = ("target\\debug\\deps\\x.dll: src\\lib.rs\n"
                "\n"
                "target\\debug\\deps\\x.exe: src\\lib.rs src\\main.rs\n")
        self.assertEqual(parse_dep_info(text),
                         ["src\\lib.rs", "src\\lib.rs", "src\\main.rs"])

    def test_line_continuation_and_escaped_spaces_and_hashes(self):
        text = ("C:\\repo\\target\\x.rlib: C:\\repo\\crates\\a\\src\\lib.rs \\\n"
                " C:\\repo\\crates\\a\\with\\ space.rs C:\\repo\\crates\\a\\hash\\#name.rs\n")
        self.assertEqual(parse_dep_info(text), [
            "C:\\repo\\crates\\a\\src\\lib.rs",
            "C:\\repo\\crates\\a\\with space.rs",
            "C:\\repo\\crates\\a\\hash\\#name.rs",
        ])

    def test_colon_inside_drive_and_path_is_not_a_rule_separator(self):
        text = "C:\\repo\\target\\deps\\a.dll: C:\\repo\\src\\a.rs\n"
        self.assertEqual(parse_dep_info(text), ["C:\\repo\\src\\a.rs"])

    def test_blank_and_target_only_lines_are_ignored(self):
        self.assertEqual(parse_dep_info("\n\n  \n"), [])
        self.assertEqual(parse_dep_info("target\\x.rlib:\n"), [])


class NormalizeCase(unittest.TestCase):
    def test_relative_path_is_resolved_against_base(self):
        value = normalize_repo_path("crates\\tokenscope-core\\src\\lib.rs",
                                    base=Path("/repo"), repo=posix_repo())
        self.assertEqual(value, "crates/tokenscope-core/src/lib.rs")

    def test_literal_dot_dot_is_folded_before_boundary_check(self):
        value = normalize_repo_path("src-tauri\\src\\..\\..\\docs\\privacy.md",
                                    base=Path("/repo"), repo=posix_repo())
        self.assertEqual(value, "docs/privacy.md")

    def test_paths_outside_repository_are_dropped(self):
        self.assertIsNone(normalize_repo_path("..\\outside\\file.rs",
                                              base=Path("/repo"), repo=posix_repo()))
        self.assertIsNone(normalize_repo_path("C:\\other\\file.rs",
                                              base=Path("C:\\repo"),
                                              repo=Path("C:/repo")))

    def test_absolute_path_inside_repository_is_kept(self):
        value = normalize_repo_path("/repo/frontend/src/main.ts",
                                    base=Path("/elsewhere"), repo=posix_repo())
        self.assertEqual(value, "frontend/src/main.ts")

    def test_unc_and_drive_paths_are_handled(self):
        self.assertIsNone(normalize_repo_path("\\\\server\\share\\file.rs",
                                              base=Path("C:\\repo"),
                                              repo=Path("C:/repo")))
        value = normalize_repo_path("c:/repo/Cargo.toml", base=Path("C:/repo"),
                                    repo=Path("C:/repo"))
        self.assertEqual(value, "Cargo.toml")

    def test_directory_style_value_keeps_trailing_marker_off(self):
        self.assertEqual(normalize_repo_path("frontend/dist/", base=Path("/repo"),
                                             repo=posix_repo()),
                         "frontend/dist")


class ExclusionCase(unittest.TestCase):
    def test_rust_sources_and_manifests_are_covered_by_fixed_rules(self):
        for rel in ("crates/tokenscope-core/src/lib.rs", "src-tauri/src/main.rs",
                    "Cargo.toml", "crates/tokenscope-core/Cargo.toml"):
            with self.subTest(rel=rel):
                self.assertTrue(is_excluded_input(rel, "file"))

    def test_generated_outputs_are_excluded(self):
        for rel in ("target/debug/build/x/out/generated.rs",
                    "target/debug/build/x/out/tauri-codegen-assets/abc.png",
                    "frontend/dist/assets/index-4f2a.js",
                    "src-tauri/gen/schemas/acl-manifests.json"):
            with self.subTest(rel=rel):
                self.assertTrue(is_excluded_input(rel, "file"))

    def test_compiled_non_rust_inputs_are_not_exempted_by_extension(self):
        for rel in ("docs/privacy.md", "src-tauri/src/acceptance_csp.js",
                    "crates/tokenscope-core/tests/fixtures/cache/schema-v3.sql"):
            with self.subTest(rel=rel):
                self.assertFalse(is_excluded_input(rel, "file"))

    def test_directories_are_never_filtered_by_extension_rules(self):
        self.assertFalse(is_excluded_input("frontend/public/assets", "directory"))


class TrackedCase(unittest.TestCase):
    def test_untracked_files_and_directories_without_tracked_descendants_drop_out(self):
        kept, dropped = filter_tracked(
            ["src-tauri/tauri.conf.json", "notes/scratch.txt", "empty-dir"],
            tracked_files={"src-tauri/tauri.conf.json"},
            tracked_dirs=set(),
        )
        self.assertEqual(kept, ["src-tauri/tauri.conf.json"])
        self.assertEqual(sorted(dropped), ["empty-dir", "notes/scratch.txt"])

    def test_tracked_file_that_matches_ignore_patterns_is_still_kept(self):
        kept, dropped = filter_tracked(
            ["docs/privacy.md"], tracked_files={"docs/privacy.md"},
            tracked_dirs=set(),
        )
        self.assertEqual(kept, ["docs/privacy.md"])
        self.assertEqual(dropped, [])

    def test_directory_input_is_kept_when_it_has_tracked_descendants(self):
        kept, dropped = filter_tracked(
            ["frontend/public"], tracked_files=set(),
            tracked_dirs={"frontend/public"},
        )
        self.assertEqual(kept, ["frontend/public"])
        self.assertEqual(dropped, [])


class BuildScriptCase(unittest.TestCase):
    def test_both_prefixes_are_recognised_and_other_lines_ignored(self):
        text = ("cargo:rerun-if-changed=tauri.conf.json\n"
                "cargo::rerun-if-changed=icons/icon.ico\n"
                "cargo:rerun-if-env-changed=SOME_VAR\n"
                "cargo:rustc-link-lib=foo\n")
        self.assertEqual(parse_build_script_output(text), [
            ("rerun-if-changed", "tauri.conf.json"),
            ("rerun-if-changed", "icons/icon.ico"),
        ])

    def test_rerun_if_changed_supports_directory_and_relative_paths(self):
        self.assertEqual(parse_build_script_output("cargo:rerun-if-changed=capabilities\n"),
                         [("rerun-if-changed", "capabilities")])


class MergeCase(unittest.TestCase):
    def record(self, path, kind, package, mode):
        return {"path": path, "kind": kind,
                "consumers": [{"package": package, "mode": mode}]}

    def test_same_path_and_kind_merge_consumers_sorted_and_deduplicated(self):
        merged = merge_records([
            self.record("a.js", "file", "tokenscope-app", "acceptance"),
            self.record("a.js", "file", "tokenscope-app", "default"),
            self.record("a.js", "file", "tokenscope-app", "default"),
        ])
        self.assertEqual(merged, [{
            "path": "a.js", "kind": "file",
            "consumers": [
                {"package": "tokenscope-app", "mode": "acceptance"},
                {"package": "tokenscope-app", "mode": "default"},
            ],
        }])

    def test_same_path_with_different_kind_stays_separate(self):
        merged = merge_records([self.record("x", "file", "tokenscope", "default"),
                                self.record("x", "directory", "tokenscope", "default")])
        self.assertEqual([r["kind"] for r in merged], ["directory", "file"])

    def test_unknown_mode_is_rejected(self):
        with self.assertRaises(ManifestError):
            merge_records([self.record("a", "file", "tokenscope", "release")])


class ManifestCase(unittest.TestCase):
    def test_load_rejects_broken_schema_and_unknown_fields(self):
        with tempfile.TemporaryDirectory() as parent:
            path = Path(parent) / "build-inputs.json"
            for value in ({"schema_version": 2, "generated": [], "manual": []},
                          {"schema_version": 1, "generated": []},
                          {"schema_version": 1, "generated": [
                              {"path": "a", "kind": "symlink",
                               "consumers": [{"package": "tokenscope", "mode": "default"}]}],
                           "manual": []}):
                with self.subTest(value=value):
                    path.write_text(json.dumps(value), encoding="utf-8")
                    with self.assertRaises(ManifestError):
                        load_manifest(path)

    def test_load_returns_sorted_records(self):
        with tempfile.TemporaryDirectory() as parent:
            path = Path(parent) / "build-inputs.json"
            path.write_text(json.dumps({
                "schema_version": 1,
                "generated": [{"path": "b", "kind": "file", "consumers": [
                    {"package": "tokenscope", "mode": "default"}]},
                    {"path": "a", "kind": "file", "consumers": [
                        {"package": "tokenscope", "mode": "default"}]}],
                "manual": [],
            }), encoding="utf-8")
            loaded = load_manifest(path)
            self.assertEqual([r["path"] for r in loaded["generated"]], ["a", "b"])


class WorkflowCoverageCase(unittest.TestCase):
    """CI/安装/Native UI 的 paths 过滤必须覆盖清单里的全部路径。"""

    def write(self, parent, name, text):
        path = Path(parent) / name
        path.write_text(text, encoding="utf-8")
        return path

    def test_paths_filter_must_cover_every_manifest_path(self):
        with tempfile.TemporaryDirectory() as parent:
            workflow = self.write(parent, "install-test.yml", """
on:
  pull_request:
    branches: [main]
    paths:
      - 'src-tauri/**'
      - 'crates/tokenscope-core/**'

permissions:
  contents: read
""")
            missing = workflow_coverage(
                ["src-tauri/src/acceptance_csp.js", "docs/privacy.md"], [workflow])
            self.assertEqual(missing, {str(workflow): ["docs/privacy.md"]})

    def test_workflow_without_paths_is_conservatively_covered(self):
        with tempfile.TemporaryDirectory() as parent:
            workflow = self.write(parent, "ci.yml",
                                  "on:\n  push:\n    branches: [main]\n")
            self.assertEqual(workflow_coverage(["docs/privacy.md"], [workflow]), {})

    def test_negated_pattern_after_wildcard_removes_coverage(self):
        with tempfile.TemporaryDirectory() as parent:
            workflow = self.write(parent, "w.yml", """
on:
  pull_request:
    paths:
      - '**'
      - '!docs/**'
""")
            missing = workflow_coverage(["docs/privacy.md", "src-tauri/tauri.conf.json"],
                                        [workflow])
            self.assertEqual(list(missing.values()), [["docs/privacy.md"]])

    def test_directory_pattern_covers_descendants(self):
        with tempfile.TemporaryDirectory() as parent:
            workflow = self.write(parent, "w.yml", """
on:
  pull_request:
    paths:
      - 'src-tauri/capabilities/**'
""")
            self.assertEqual(
                workflow_coverage(["src-tauri/capabilities"], [workflow]),
                {str(workflow): ["src-tauri/capabilities"]})


class CollectCase(unittest.TestCase):
    """记录收集的合并语义：同一文件被两模式消费时两条记录都保留。"""

    def test_two_modes_and_two_packages_are_not_collapsed(self):
        records = collect_records({
            ("tokenscope", "default"): ["src-tauri/src/acceptance_csp.js"],
            ("tokenscope-app", "acceptance"): ["src-tauri/src/acceptance_csp.js"],
        })
        self.assertEqual(records, [{
            "path": "src-tauri/src/acceptance_csp.js", "kind": "file",
            "consumers": [
                {"package": "tokenscope", "mode": "default"},
                {"package": "tokenscope-app", "mode": "acceptance"},
            ],
        }])

    def test_directory_records_are_marked_as_directories(self):
        records = collect_records({("tokenscope-app", "default"): ["frontend/public/"]})
        self.assertEqual(records[0]["kind"], "directory")
        self.assertEqual(records[0]["path"], "frontend/public")


if __name__ == "__main__":
    unittest.main()
