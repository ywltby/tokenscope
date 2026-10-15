"""第 15 节构建清单与暂存边界回归；全部写入仅发生在临时目录。"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import build_inputs_run as runner
from build_inputs import ManifestError
from pre_commit_checks import classify, unstaged_paths

ROOT = Path(__file__).resolve().parent.parent


class ImportedRunnerEncodingCase(unittest.TestCase):
    def test_imported_runner_logs_unicode_under_english_windows_encoding(self):
        for missing in (False, True):
            with self.subTest(missing=missing):
                code = (
                    "import sys; from pre_commit_checks import run; "
                    "sys.exit(run([['missing-中文-tool']] if " + repr(missing) +
                    " else [[sys.executable, '-c', 'pass', '中文路径']], cwd='.'))"
                )
                env = {**os.environ, "PYTHONIOENCODING": "cp1252:strict"}
                result = subprocess.run(
                    [sys.executable, "-B", "-c", code], cwd=ROOT / "scripts",
                    env=env, capture_output=True,
                )
                self.assertEqual(result.returncode, 127 if missing else 0,
                                 result.stderr.decode("utf-8", errors="replace"))
                self.assertIn("中文", result.stdout.decode("utf-8"))
                if missing:
                    self.assertIn("找不到可执行文件", result.stderr.decode("utf-8"))


def record(path, package="tokenscope", kind="file"):
    return {"path": path, "kind": kind,
            "consumers": [{"package": package, "mode": "default"}]}


class LibraryDepInfoCase(unittest.TestCase):
    def test_rlib_only_reads_library_dependencies(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            dep = root / "tokenscope-abc.d"
            dep.write_text("x: docs/non-test.md\n", encoding="utf-8")
            messages = [{"reason": "compiler-artifact",
                         "package_id": "path+file:///repo#tokenscope@0.3.2",
                         "target": {"kind": ["lib"]},
                         "filenames": [str(root / "libtokenscope-abc.rlib"),
                                       str(root / "libtokenscope-abc.rmeta")]}]
            self.assertEqual(runner.dep_files_from_messages(messages), {"tokenscope": {dep}})

    def test_binary_lib_prefix_is_not_stripped(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            dep = root / "libexample-abc.d"
            dep.touch()
            messages = [{"reason": "compiler-artifact",
                         "package_id": "path+file:///repo#tokenscope@0.3.2",
                         "target": {"kind": ["bin"]},
                         "filenames": [str(root / "libexample-abc.exe")]}]
            self.assertEqual(runner.dep_files_from_messages(messages), {"tokenscope": {dep}})


class ManualManifestCase(unittest.TestCase):
    def check_manifest(self, generated, manual, expected, tracked):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in tracked:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture", encoding="utf-8")
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"schema_version": 1, "generated": generated,
                                            "manual": manual}), encoding="utf-8")
            with patch.object(runner, "collect_both_modes", return_value=expected), \
                    patch.object(runner, "git_tracked", return_value=(set(tracked), set())), \
                    patch.object(runner, "workflow_paths_check", return_value={}) as coverage:
                result = runner.check(root, manifest, root, clean=False)
                self.assertIn("docs/manual.md", coverage.call_args.args[1])
                return result

    def test_valid_manual_entry_is_not_stale(self):
        result = self.check_manifest([], [record("docs/manual.md")], [], ["docs/manual.md"])
        self.assertEqual(result["stale"], [])

    def test_manual_entry_cannot_hide_missing_generated_entry(self):
        with self.assertRaises(ManifestError):
            self.check_manifest([], [record("docs/manual.md")],
                                [record("docs/manual.md")], ["docs/manual.md"])

    def test_manual_untracked_or_unknown_consumer_fails(self):
        for entry, tracked in [(record("docs/manual.md"), []),
                               (record("docs/manual.md", "unknown"), ["docs/manual.md"])]:
            with self.subTest(entry=entry), self.assertRaises(ManifestError):
                self.check_manifest([], [entry], [], tracked)

    def test_cli_manifest_error_has_no_traceback(self):
        with tempfile.TemporaryDirectory() as temp:
            result = subprocess.run([sys.executable, "-B", str(ROOT / "scripts/build_inputs.py"),
                                     "--check", "--manifest", str(Path(temp) / "missing.json"),
                                     "--evidence", temp], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(b"Traceback", result.stderr)
            self.assertIn("构建输入清单失败".encode(), result.stderr)


class UntrackedInputCase(unittest.TestCase):
    def test_untracked_source_blocks_but_ignored_output_does_not(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
            subprocess.run(["git", "init", "-q", temp], check=True, env=env)
            (root / ".gitignore").write_text("target/\n", encoding="utf-8")
            source = root / "crates/tokenscope-core/src/foo.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub fn foo() {}", encoding="utf-8")
            (root / "target").mkdir()
            (root / "target/output").touch()
            with patch.dict(os.environ, env, clear=True):
                paths = unstaged_paths(root)
            relative = source.relative_to(root).as_posix()
            self.assertIn(relative, paths)
            self.assertNotIn("target/output", paths)
            manifest = {"schema_version": 1, "generated": [], "manual": []}
            changes = [{"path": "crates/tokenscope-core/src/lib.rs", "status": "M"}]
            self.assertIn(relative, classify(changes, manifest=manifest, unstaged=paths)["blocked"])
            self.assertEqual(classify([{"path": "docs/plan.md", "status": "M"}],
                                     manifest=manifest, unstaged=paths)["blocked"], [])


if __name__ == "__main__":
    unittest.main()
