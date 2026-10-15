"""按暂存区选择本地提交前检查。

设计要点（PR1 §W05a/W05b）：
- 分类层只返回选择结果与原因，不执行 Cargo；执行层接收 argv 数组，禁止字符串
  拼接命令。构建/门禁入口优先于宽泛目录规则，多类改动取并集并按检查身份去重。
- 纯文档提交只跑文档检查；编译输入按清单消费方选择包与构建模式；未知类别、
  清单缺失或损坏、检查选择器自身变更一律回退完整模式。
- 运行代码检查时，被选检查读取的受版本管理输入若存在未暂存差异则明确阻止，
  不 stash、不 reset、不补暂存。
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from build_inputs import ManifestError, merge_records, validate_manifest

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = "scripts/build-inputs.json"
MODES = ("default", "acceptance")
MODE_FULL = "full"
MODE_SELECTED = "selected"

DOC_CHECK = "docs"
SCRIPTS_CHECK = "scripts-contracts"

FULL_TRIGGERS = (
    "Cargo.toml",
    "Cargo.lock",
    "rustfmt.toml",
    "rust-toolchain.toml",
    "release-change.json",
    "crates/tokenscope-core/Cargo.toml",
    "src-tauri/Cargo.toml",
    "src-tauri/tauri.conf.json",
    "src-tauri/build.rs",
    "src-tauri/capabilities/",
    "src-tauri/gen/",
    ".cargo/",
    ".github/",
    ".githooks/",
    "scripts/build-inputs.json",
    "scripts/build_inputs.py",
    "scripts/build_inputs_run.py",
    "scripts/test_build_inputs.py",
    "scripts/pre_commit_checks.py",
    "scripts/test_pre_commit_checks.py",
    "scripts/check_staged_docs.py",
    "scripts/test_check_staged_docs.py",
)

CORE_PREFIX = "crates/tokenscope-core/"
LEGACY_CORE_PREFIXES = ("src/", "tests/")
SHELL_PREFIX = "src-tauri/src/"
FIXTURE_PREFIXES = ("crates/tokenscope-core/tests/fixtures/", "tests/fixtures/")
FRONTEND_PREFIXES = ("frontend/src/", "frontend/public/", "frontend/scripts/")
FRONTEND_FILES = ("frontend/package.json", "frontend/pnpm-lock.yaml",
                  "frontend/tsconfig.json", "frontend/vite.config.ts",
                  "frontend/env.d.ts", "frontend/index.html",
                  "frontend/.prettierrc.json", "frontend/vitest.config.ts")

CHECK_COMMANDS = {
    "rust-fmt": [["cargo", "fmt", "--all", "--", "--check"]],
    "rust-core-default": [
        ["cargo", "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--locked", "--workspace"],
    ],
    "rust-core-acceptance": [
        ["cargo", "clippy", "--locked", "--workspace", "--all-targets",
         "--features", "acceptance", "--", "-D", "warnings"],
        ["cargo", "test", "--locked", "--workspace", "--features", "acceptance"],
    ],
    "rust-shell-default": [
        ["cargo", "clippy", "--locked", "-p", "tokenscope-app", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--locked", "-p", "tokenscope-app"],
    ],
    "rust-shell-acceptance": [
        ["cargo", "clippy", "--locked", "-p", "tokenscope-app", "--all-targets",
         "--features", "acceptance", "--", "-D", "warnings"],
        ["cargo", "test", "--locked", "-p", "tokenscope-app", "--features", "acceptance"],
    ],
    "frontend-typecheck": [["pnpm", "--dir", "frontend", "typecheck"]],
    "frontend-format": [["pnpm", "--dir", "frontend", "format:check"]],
    "frontend-test": [["pnpm", "--dir", "frontend", "test"]],
    "frontend-build": [["pnpm", "--dir", "frontend", "build"]],
    "node-syntax": [["python", "-B", "scripts/check_node_syntax.py"]],
    SCRIPTS_CHECK: [
        ["python", "-B", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"],
        ["pwsh", "-NoProfile", "-File", "scripts/check_powershell_syntax.ps1"],
        ["pwsh", "-NoProfile", "-File", "scripts/test_install_helpers.ps1"],
        ["pwsh", "-NoProfile", "-File", "scripts/test_upgrade_helpers.ps1"],
        ["pwsh", "-NoProfile", "-File", "scripts/test_native_window_action.ps1"],
    ],
    DOC_CHECK: [["python", "-B", "scripts/check_staged_docs.py"]],
}

# 完整模式合并为「一次 fmt + 一次 workspace 两模式 clippy/test + 前端门禁 + 脚本与
# 文档」：workspace 轮次已覆盖两个 package，不再叠加壳轮次（壳轮次只在壳改动被选中
# 时执行，见 CHECK_COMMANDS）。
FULL_CHECKS = ("rust-fmt", "rust-core-default", "rust-core-acceptance",
               "frontend-typecheck", "frontend-format", "frontend-test", "frontend-build",
               "node-syntax", SCRIPTS_CHECK, DOC_CHECK)

# 每个检查读取的作用域：未暂存差异落在这些前缀下会阻止提交。
# Rust 检查还会读取共享配置与整个 src-tauri（壳的构建脚本、Tauri 配置、
# capabilities 都参与编译），只写 src-tauri/src 会漏掉这些输入。
RUST_INPUT_SCOPES = ("crates/", "src-tauri/", "Cargo.toml", "Cargo.lock")

CHECK_SCOPES = {
    "rust-fmt": ("crates/", "src-tauri/src/"),
    "rust-core-default": ("crates/", "src-tauri/src/"),
    "rust-core-acceptance": ("crates/", "src-tauri/src/"),
    "rust-shell-default": ("crates/", "src-tauri/src/"),
    "rust-shell-acceptance": ("crates/", "src-tauri/src/"),
    "frontend-typecheck": ("frontend/",),
    "frontend-format": ("frontend/",),
    "frontend-test": ("frontend/",),
    "frontend-build": ("frontend/",),
    "node-syntax": ("frontend/scripts/",),
    SCRIPTS_CHECK: ("scripts/",),
    DOC_CHECK: (),
}


class CheckError(ValueError):
    """暂存区、清单或执行环境非法（不同于"选择到空集合"）。"""


def _matches(path, prefixes_or_files):
    for item in prefixes_or_files:
        if item.endswith("/"):
            if path.startswith(item):
                return True
        elif item.startswith(("*", ".")):
            if path.endswith(item.lstrip("*")):
                return True
        elif path == item:
            return True
    return False


def is_full_trigger(path):
    # 任意层级的 build.rs 都是构建入口（可能声明 rerun-if-changed 输入）。
    if path.rsplit("/", 1)[-1] == "build.rs":
        return True
    for trigger in FULL_TRIGGERS:
        if trigger.endswith("/"):
            if path.startswith(trigger) or path == trigger.rstrip("/"):
                return True
        elif path == trigger:
            return True
    return False


def _manifest_consumers(manifest, path):
    """清单里登记的消费方（generated 与 manual 都算）。"""
    if not manifest:
        return [], False
    found, registered = [], False
    for layer in ("generated", "manual"):
        for record in manifest.get(layer, []):
            if record["path"] == path or (record["kind"] == "directory"
                                          and path.startswith(record["path"] + "/")):
                registered = True
                for consumer in record["consumers"]:
                    found.append((consumer["package"], consumer["mode"]))
    return sorted(set(found)), registered


def _checks_for_consumers(consumers):
    checks = set()
    for package, mode in consumers:
        if package == "tokenscope":
            checks.add(f"rust-core-{mode}" if mode == "default" else "rust-core-acceptance")
        elif package == "tokenscope-app":
            checks.add(f"rust-shell-{mode}" if mode == "default" else "rust-shell-acceptance")
        else:
            raise CheckError(f"清单出现未知消费包：{package}")
    return checks


def classify(changes, *, manifest=None, unstaged=(), explain=False, force_full=False):
    """把暂存区变更映射为检查集合。

    `changes` 为 `{"status", "path", "old_path"}` 列表；`manifest` 为清单记录
    （None 表示缺失或损坏，直接完整兜底）；`unstaged` 为未暂存路径列表；
    `force_full` 强制完整模式，且与其他模式共用同一套未暂存保护。
    """
    result = {"mode": MODE_SELECTED, "checks": set(), "triggers": {}, "fallback": [],
              "blocked": [], "notes": []}
    if force_full:
        result["mode"] = MODE_FULL
        result["notes"].append("强制完整模式（TOKENSCOPE_PRECOMMIT_MODE=full）")
    if manifest is not None:
        try:
            manifest = validate_manifest(manifest)
        except ManifestError as exc:
            raise CheckError(f"构建输入清单非法：{exc}") from exc
    if manifest is None:
        result["mode"] = MODE_FULL
        result["notes"].append("构建输入清单缺失或损坏：完整模式兜底")
    for change in changes:
        for path in filter(None, (change.get("path"), change.get("old_path"))):
            reasons = _classify_path(path, manifest, result)
            result["triggers"].setdefault(path, []).extend(reasons)
    if result["mode"] == MODE_FULL:
        result["checks"] = set(FULL_CHECKS)
    checks = sorted(result["checks"])
    result["checks"] = checks
    result["blocked"] = _blocked_inputs(checks, unstaged, manifest)
    return result


def _classify_path(path, manifest, result):
    reasons = []
    if is_full_trigger(path):
        result["mode"] = MODE_FULL
        result["notes"].append(f"{path}：构建/门禁入口，完整模式")
        return ["full-trigger"]
    if _matches(path, FIXTURE_PREFIXES):
        # fixtures 专用规则先于文档豁免：目录内 Markdown 也不能按纯文档免检。
        result["checks"].update({"rust-core-default", "rust-core-acceptance"})
        return ["fixtures"]
    if path.startswith(SHELL_PREFIX):
        if path.endswith(".rs"):
            result["checks"].update({"rust-fmt", "rust-shell-default", "rust-shell-acceptance"})
            return ["shell-rust"]
        consumers, registered = _manifest_consumers(manifest, path)
        if registered:
            result["checks"].update(_checks_for_consumers(consumers))
            reasons.append("manifest")
            if path.endswith(".md"):
                result["checks"].add(DOC_CHECK)
            return reasons + ["manifest"]
        result["mode"] = MODE_FULL
        result["notes"].append(f"{path}：壳内未登记非 Rust 输入，完整模式兜底")
        return ["unregistered-shell-input"]
    if path.startswith(CORE_PREFIX) or path.startswith(LEGACY_CORE_PREFIXES):
        if path.endswith(".rs"):
            result["checks"].update({"rust-fmt", "rust-core-default", "rust-core-acceptance"})
            return ["core-rust"]
        consumers, registered = _manifest_consumers(manifest, path)
        if registered:
            result["checks"].update(_checks_for_consumers(consumers))
            reasons.append("manifest")
            if path.endswith(".md"):
                result["checks"].add(DOC_CHECK)
            return reasons + ["manifest"]
        result["mode"] = MODE_FULL
        result["notes"].append(f"{path}：核心库内未登记非 Rust 输入，完整模式兜底")
        return ["unregistered-core-input"]
    if path.startswith("frontend/scripts/"):
        # E2E 编排脚本属于脚本规则：前端四项并不校验它们能否运行。
        result["checks"].update({"frontend-typecheck", "frontend-format",
                                 "frontend-test", "frontend-build", "node-syntax"})
        return ["frontend-scripts"]
    if _matches(path, FRONTEND_PREFIXES) or path in FRONTEND_FILES:
        result["checks"].update({"frontend-typecheck", "frontend-format",
                                 "frontend-test", "frontend-build"})
        return ["frontend"]
    if path.startswith("scripts/"):
        if path.endswith((".py", ".ps1")):
            result["checks"].add(SCRIPTS_CHECK)
            return ["scripts"]
        result["mode"] = MODE_FULL
        result["notes"].append(f"{path}：未登记脚本输入，完整模式兜底")
        return ["unregistered-script"]
    if path.endswith(".md"):
        consumers, registered = _manifest_consumers(manifest, path)
        if registered:
            result["checks"].update(_checks_for_consumers(consumers))
        result["checks"].add(DOC_CHECK)
        return ["doc"] if not registered else ["manifest", "doc"]
    result["mode"] = MODE_FULL
    result["fallback"].append(path)
    result["notes"].append(f"{path}：未识别类别，完整模式兜底")
    return ["unrecognised"]


def _in_scope(path, scopes):
    for scope in scopes:
        if scope.endswith("/"):
            if path.startswith(scope):
                return True
        elif path == scope:
            return True
    return False


def _blocked_inputs(checks, unstaged, manifest=None):
    """被选检查会读取、却存在未暂存差异的受版本管理输入。

    范围包含共享配置与清单登记的编译输入：它们同样参与实际构建，若工作区版本
    与暂存版本不同，检查结果就不能代表将要提交的内容。
    """
    scopes = set()
    for check in checks:
        scopes.update(CHECK_SCOPES.get(check, ()))
        if check.startswith("rust-"):
            scopes.update(RUST_INPUT_SCOPES)
    if manifest:
        for layer in ("generated", "manual"):
            for record in manifest.get(layer, []):
                scopes.add(record["path"])
    if not scopes:
        return []
    blocked = {path for path in unstaged or () if _in_scope(path, scopes)}
    return sorted(blocked)


def expand_commands(checks):
    commands = []
    for check in checks:
        if check not in CHECK_COMMANDS:
            raise CheckError(f"未知检查项：{check}")
        commands.extend([list(command) for command in CHECK_COMMANDS[check]])
    return commands


def manifest_index(root):
    """HEAD 与暂存区清单的并集；都不可用时返回 None（调用方完整兜底）。"""
    layers = []
    for spec in (f"HEAD:{MANIFEST_PATH}", f":{MANIFEST_PATH}"):
        result = subprocess.run(["git", "-C", str(root), "show", spec],
                                capture_output=True, check=False)
        if result.returncode != 0:
            continue
        try:
            value = json.loads(result.stdout.decode("utf-8", "replace"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            return None
        try:
            layers.append(validate_manifest(value))
        except ManifestError:
            return None
    if not layers:
        return None
    merged = {"schema_version": 1, "generated": [], "manual": []}
    for layer in layers:
        merged["generated"].extend(layer["generated"])
        merged["manual"].extend(layer["manual"])
    return {"schema_version": 1,
            "generated": merge_records(merged["generated"]),
            "manual": merge_records(merged["manual"])}


def build_changes(root):
    """读取暂存区变更；Git 查询失败不能当作空提交。"""
    result = subprocess.run(["git", "-C", str(root), "diff", "--cached", "--name-status",
                             "-z", "--find-renames"], capture_output=True, check=False)
    if result.returncode != 0:
        raise CheckError("读取暂存区失败（git diff --cached），不能当作空提交")
    fields = [item.decode("utf-8") for item in result.stdout.split(b"\0") if item]
    changes, index = [], 0
    while index < len(fields):
        status = fields[index]
        if status[:1] in ("R", "C"):
            if index + 2 >= len(fields):
                raise CheckError("重命名记录不完整")
            changes.append({"status": "R", "path": fields[index + 2],
                            "old_path": fields[index + 1]})
            index += 3
            continue
        if index + 1 >= len(fields):
            raise CheckError("变更记录不完整")
        changes.append({"status": status[:1], "path": fields[index + 1], "old_path": None})
        index += 2
    return changes


def unstaged_paths(root):
    result = subprocess.run(["git", "-C", str(root), "diff", "--name-only", "-z"],
                            capture_output=True, check=False)
    if result.returncode != 0:
        raise CheckError("读取未暂存差异失败（git diff）")
    return [item.decode("utf-8") for item in result.stdout.split(b"\0") if item]


def run(commands, *, cwd, executor=None):
    """顺序执行；任一非零立即返回该退出码，后续命令不再运行。"""
    runner = executor or _execute
    for command in commands:
        status = runner(command, cwd)
        if status != 0:
            return status
    return 0


def _execute(argv, cwd):
    """解析可执行文件后以 argv 数组执行，不做字符串拼接。

    Windows 上 `pnpm` 实际是 `pnpm.cmd`，直接交给 CreateProcess 会报找不到文件；
    这里先用 shutil.which 解析，批处理再显式经由 cmd /c。解析不到即明确失败，
    不能静默当作通过。"""
    print(f"[pre-commit] {' '.join(argv)}", flush=True)
    executable = shutil.which(argv[0])
    if executable is None:
        print(f"[pre-commit] 找不到可执行文件：{argv[0]}（拒绝在无验证的情况下提交）",
              file=sys.stderr)
        return 127
    if os.name == "nt" and executable.lower().endswith((".cmd", ".bat")):
        shell = os.environ.get("COMSPEC", "cmd.exe")
        return subprocess.run([shell, "/c", executable, *argv[1:]],
                              cwd=cwd, check=False).returncode
    return subprocess.run([executable, *argv[1:]], cwd=cwd, check=False).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--explain", action="store_true", help="只打印选中检查，不执行")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    mode = os.environ.get("TOKENSCOPE_PRECOMMIT_MODE", "auto")
    if mode not in ("auto", "full"):
        print(f"[pre-commit] TOKENSCOPE_PRECOMMIT_MODE 非法：{mode}", file=sys.stderr)
        return 2
    try:
        changes = build_changes(args.root)
        plan = classify(changes, manifest=manifest_index(args.root),
                        unstaged=unstaged_paths(args.root), force_full=(mode == "full"))
    except CheckError as exc:
        print(f"[pre-commit] 选择检查失败：{exc}", file=sys.stderr)
        return 2
    if not changes:
        print("[pre-commit] 暂存区没有文件，无检查项")
        return 0
    print(f"[pre-commit] 模式={plan['mode']} 检查={'、'.join(plan['checks']) or '（无）'}")
    for path, reasons in sorted(plan["triggers"].items()):
        print(f"[pre-commit]   {path} ← {','.join(reasons)}")
    for note in plan["notes"]:
        print(f"[pre-commit] 说明：{note}")
    if plan["blocked"]:
        print("[pre-commit] 以下受版本管理输入存在未暂存差异，请先处理后再提交：", file=sys.stderr)
        for path in plan["blocked"]:
            print(f"[pre-commit]   {path}", file=sys.stderr)
        if not args.explain:
            return 3
    if not plan["checks"]:
        return 0
    commands = expand_commands(plan["checks"])
    if args.explain:
        print(f"[pre-commit] 选中 {len(commands)} 条命令：")
        for command in commands:
            print(" ".join(command))
        return 0
    status = run(commands, cwd=args.root)
    if status != 0:
        print(f"[pre-commit] 检查失败，退出码 {status}", file=sys.stderr)
    return status


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
