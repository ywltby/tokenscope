"""从 Cargo 依赖信息生成受版本管理的构建输入清单（scripts/build-inputs.json）。

设计约束（PR1 §W05c）：
- 不做源码静态扫描；`--update` 用 Cargo 的 JSON 消息、dep-info（`.d`）与构建脚本
  输出实际记录编译输入，`--check` 只比较不修改受跟踪文件。
- 消费方按 `(package, mode)` 记录，`mode` 为 `default` / `acceptance`；同一文件
  被两种模式消费时保留两条消费方记录，不能合并成只有包名。
- 清单不含本机路径、mtime、run_id；记录稳定排序。
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = ROOT / "scripts" / "build-inputs.json"
SCHEMA_VERSION = 1
MODES = ("default", "acceptance")
KINDS = ("file", "directory")
CORE_PACKAGE = "tokenscope"
SHELL_PACKAGE = "tokenscope-app"
PACKAGE_ROOTS = {CORE_PACKAGE: "crates/tokenscope-core", SHELL_PACKAGE: "src-tauri"}

_RULE_SEP = re.compile(r":(?=\s|$)")
_DRIVE = re.compile(r"^[A-Za-z]:")


class ManifestError(ValueError):
    """清单、记录或证据非法（区别于"文件不存在"）。"""


# ── Makefile dep-info 解析 ──────────────────────────────────────────


def _split_make_words(text):
    """按未转义空白切分；`\\ ` 表示字面空格。

    Windows 路径里的单反斜杠是分隔符而不是转义前缀，因此只消费「反斜杠+空白」
    这一种转义；`\\#`、`\\:` 保留原字符，避免把真实目录分隔符吃掉。
    """
    words, current, index = [], [], 0
    while index < len(text):
        char = text[index]
        if char == "\\" and index + 1 < len(text) and text[index + 1] in " \t":
            current.append(text[index + 1])
            index += 2
            continue
        if char in " \t":
            if current:
                words.append("".join(current))
                current = []
            index += 1
            continue
        current.append(char)
        index += 1
    if current:
        words.append("".join(current))
    return words


def parse_dep_info(text):
    """解析一份 dep-info，返回全部 rule 的 prerequisites（保留重复与顺序）。

    规则以": "或行尾冒号分隔；同一文件可含多条 rule（多个 target），空行与
    只有 target 的行忽略。Makefile 续行（反斜杠+换行）先合并为单空格。
    """
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    text = re.sub(r"\\\n[ \t]*", " ", text)
    dependencies = []
    for line in text.split("\n"):
        line = line.strip()
        if not line:
            continue
        match = _RULE_SEP.search(line)
        if match is None:
            continue
        dependencies.extend(_split_make_words(line[match.end():]))
    return dependencies


def parse_build_script_output(text):
    """解析构建脚本输出里的 `cargo:rerun-if-changed=` / `cargo::rerun-if-changed=`。"""
    found = []
    for line in text.splitlines():
        line = line.strip()
        for prefix in ("cargo:rerun-if-changed=", "cargo::rerun-if-changed="):
            if line.startswith(prefix):
                value = line[len(prefix):].strip()
                if value:
                    found.append(("rerun-if-changed", value))
    return found


# ── 路径规范化与过滤 ────────────────────────────────────────────────


def _to_posix(value):
    return value.replace("\\", "/")


def _canonical(value):
    """Windows 盘符大小写归一，用于跨平台比较。"""
    if _DRIVE.match(value):
        return value[0].upper() + value[1:]
    return value


def _is_absolute(value):
    return value.startswith("/") or bool(_DRIVE.match(value))


def _join(base, value):
    base = base.rstrip("/")
    return f"{base}/{value}" if base else value


def _fold_segments(value):
    """字面折叠 `.` 与 `..`；越出根返回 None（不做文件系统访问）。"""
    value = _to_posix(value)
    if value.startswith("//"):
        head, rest = "//", value[2:]
    elif value.startswith("/"):
        head, rest = "/", value[1:]
    else:
        drive = re.match(r"^([A-Za-z]:)/?", value)
        if drive:
            head, rest = drive.group(1) + "/", value[drive.end():]
        else:
            head, rest = "", value
    parts = []
    for segment in rest.split("/"):
        if segment in ("", "."):
            continue
        if segment == "..":
            if not parts:
                return None
            parts.pop()
            continue
        parts.append(segment)
    return head + "/".join(parts)


def normalize_repo_path(raw, *, base, repo):
    """把依赖路径转成仓库相对 POSIX 路径；仓库外或无法解释返回 None。"""
    if not isinstance(raw, str):
        return None
    value = _to_posix(raw.strip())
    if not value:
        return None
    repo_norm = _canonical(_to_posix(str(repo)).rstrip("/"))
    absolute = value if _is_absolute(value) else _join(_to_posix(str(base)), value)
    folded = _fold_segments(absolute)
    if folded is None:
        return None
    folded = _canonical(folded)
    if folded == repo_norm:
        return None
    if not folded.startswith(repo_norm + "/"):
        return None
    return folded[len(repo_norm) + 1:]


def is_excluded_input(rel, kind):
    """固定规则覆盖的输入（Rust 源码、Cargo.toml）与构建生成物不登记。"""
    if kind == "directory":
        return False
    name = rel.rsplit("/", 1)[-1]
    if name == "Cargo.toml" or name.endswith(".rs"):
        return True
    if rel == "target" or rel.startswith("target/") or "/target/" in rel:
        return True
    if rel == "frontend/dist" or rel.startswith("frontend/dist/"):
        return True
    if rel == "src-tauri/gen" or rel.startswith("src-tauri/gen/"):
        return True
    return False


def filter_tracked(paths, tracked_files, tracked_dirs):
    """只保留 Git 跟踪的文件，或确有跟踪后代的目录，返回 (保留, 排除)。

    "位于含跟踪文件的目录下面"不构成成员资格：只跟踪了 `docs/privacy.md` 时，
    `docs/untracked.md` 与空的 `docs/empty/` 都不是受版本管理的输入。
    """
    kept, dropped = [], []
    for rel in paths:
        candidate = rel.rstrip("/")
        if candidate in tracked_files or candidate in tracked_dirs:
            kept.append(rel)
        else:
            dropped.append(rel)
    return kept, dropped


# ── 记录合并与 schema ───────────────────────────────────────────────


def merge_records(records):
    merged = {}
    for record in records:
        if not isinstance(record, dict):
            raise ManifestError(f"记录不是对象：{record!r}")
        path = record.get("path")
        kind = record.get("kind")
        if not isinstance(path, str) or not path:
            raise ManifestError(f"记录缺少 path：{record!r}")
        if kind not in KINDS:
            raise ManifestError(f"记录 kind 非法：{kind!r}")
        consumers = record.get("consumers")
        if not isinstance(consumers, list) or not consumers:
            raise ManifestError(f"记录缺少 consumers：{record!r}")
        entry = merged.setdefault((kind, path), set())
        for consumer in consumers:
            if not isinstance(consumer, dict):
                raise ManifestError(f"消费方不是对象：{consumer!r}")
            package, mode = consumer.get("package"), consumer.get("mode")
            if not isinstance(package, str) or not package:
                raise ManifestError(f"消费方缺少 package：{consumer!r}")
            if mode not in MODES:
                raise ManifestError(f"未知构建模式：{mode!r}")
            entry.add((package, mode))
    result = [
        {"path": path, "kind": kind,
         "consumers": [{"package": package, "mode": mode} for package, mode in sorted(pairs)]}
        for (kind, path), pairs in merged.items()
    ]
    result.sort(key=lambda record: (record["path"], record["kind"]))
    return result


def collect_records(inputs_by_consumer):
    """`{(package, mode): [输入, ...]}` → 稳定排序的清单记录。

    输入串以 `/` 结尾表示目录输入（保留目录类型），其余按文件登记。
    """
    records = []
    for (package, mode), paths in sorted(inputs_by_consumer.items()):
        if mode not in MODES:
            raise ManifestError(f"未知构建模式：{mode!r}")
        for raw in paths:
            if not isinstance(raw, str) or not raw.strip():
                raise ManifestError(f"输入路径非法：{raw!r}")
            path = raw.strip().rstrip("/")
            kind = "directory" if raw.strip().endswith("/") else "file"
            records.append({"path": path, "kind": kind,
                            "consumers": [{"package": package, "mode": mode}]})
    return merge_records(records)


def validate_manifest(value):
    if not isinstance(value, dict) or set(value) != {"schema_version", "generated", "manual"}:
        raise ManifestError("清单必须恰好包含 schema_version、generated、manual")
    if type(value["schema_version"]) is not int or value["schema_version"] != SCHEMA_VERSION:
        raise ManifestError("清单 schema_version 不受支持")
    for key in ("generated", "manual"):
        if not isinstance(value[key], list):
            raise ManifestError(f"清单 {key} 必须是数组")
    return {"schema_version": SCHEMA_VERSION,
            "generated": merge_records(value["generated"]),
            "manual": merge_records(value["manual"])}


def load_manifest(path):
    try:
        raw = Path(path).read_text(encoding="utf-8")
    except OSError as exc:
        raise ManifestError(f"构建输入清单不可读：{exc}") from exc
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ManifestError(f"构建输入清单不是合法 JSON：{exc}") from exc
    return validate_manifest(value)


def render_manifest(value):
    return json.dumps(validate_manifest(value), ensure_ascii=False, indent=2) + "\n"


def write_manifest(path, value, *, atomic=True):
    """原子替换清单；生成失败时调用方不得走到这里，避免用空清单覆盖旧清单。"""
    text = render_manifest(value)
    path = Path(path)
    if not atomic:
        path.write_text(text, encoding="utf-8")
        return path
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n",
                                     dir=path.parent, prefix=".build-inputs-",
                                     delete=False) as handle:
        temporary = Path(handle.name)
        handle.write(text)
    os.replace(temporary, path)
    return path


# ── Git 跟踪集合 ────────────────────────────────────────────────────


def git_tracked(root):
    """返回 (tracked_files, tracked_dirs)，均为仓库相对 POSIX 路径。"""
    result = subprocess.run(["git", "-C", str(root), "ls-files", "-z"],
                            capture_output=True, check=False)
    if result.returncode != 0:
        raise ManifestError("git ls-files 失败，无法判定跟踪范围")
    files = [item.decode("utf-8") for item in result.stdout.split(b"\0") if item]
    directories = set()
    for name in files:
        parts = name.split("/")[:-1]
        for index in range(len(parts)):
            directories.add("/".join(parts[:index + 1]))
    return set(files), directories


# ── CI workflow paths 覆盖校验 ──────────────────────────────────────


def _workflow_patterns(path):
    """提取 workflow `on.<event>.paths` 列表（含否定模式，保持顺序）。"""
    text = Path(path).read_text(encoding="utf-8")
    patterns = []
    inside = False
    indent = None
    for line in text.split("\n"):
        stripped = line.strip()
        if stripped == "paths:":
            inside = True
            indent = len(line) - len(line.lstrip())
            continue
        if inside:
            if not stripped:
                continue
            current = len(line) - len(line.lstrip())
            if current <= indent:
                inside = False
                continue
            if stripped.startswith("- "):
                patterns.append(stripped[2:].strip().strip("'\""))
            else:
                inside = False
    return patterns


def _pattern_matches(pattern, rel):
    if pattern == rel or pattern.rstrip("/") == rel:
        return True
    if pattern.endswith("/**"):
        return rel.startswith(pattern[:-3].rstrip("/") + "/")
    if pattern.endswith("**"):
        return rel.startswith(pattern[:-2])
    if pattern.endswith("*"):
        return rel.startswith(pattern[:-1]) if pattern.count("*") == 1 else False
    return False


def covers(patterns, rel):
    """按 GitHub paths 语义判断：后出现的匹配规则决定结果（含 `!` 否定）。"""
    decision = False
    for pattern in patterns:
        negate = pattern.startswith("!")
        candidate = pattern[1:] if negate else pattern
        if _pattern_matches(candidate, rel):
            decision = not negate
    return decision


def workflow_coverage(manifest_paths, workflow_files):
    """`workflow_files` 为 workflow 文件路径列表，返回 `{workflow: [未覆盖路径]}`。

    没有 `paths` 过滤的 workflow 视为"总是运行"，保守认为覆盖全部路径。"""
    missing = {}
    for workflow in workflow_files:
        patterns = _workflow_patterns(workflow)
        if not patterns:
            continue
        uncovered = sorted(rel for rel in manifest_paths if not covers(patterns, rel))
        if uncovered:
            missing[str(workflow)] = uncovered
    return missing


# ── CLI ─────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--update", action="store_true", help="重新生成并原子替换清单")
    mode.add_argument("--check", action="store_true", help="只比较，不修改清单")
    mode.add_argument("--begin", action="store_true", help="清理两个本地包并建立证据边界")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    try:
        # 编排层延迟导入：纯函数契约测试不触发 Cargo 与文件系统副作用。
        from build_inputs_run import check, clean_local_packages, evidence_dir, update
        evidence = Path(args.evidence) if args.evidence else evidence_dir(ROOT)
        if not evidence.is_absolute():
            evidence = ROOT / evidence
        evidence.mkdir(parents=True, exist_ok=True)
        if args.begin:
            (evidence / "begin.json").write_text(json.dumps({
                "run_id": evidence.name,
                "started_utc": datetime.now(timezone.utc).isoformat(),
                "clean": True,
            }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            clean_local_packages(ROOT)
            result = {"begin": str(evidence)}
        elif args.update:
            result = update(ROOT, args.manifest, evidence)
        else:
            result = check(ROOT, args.manifest, evidence)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0
    except ManifestError as exc:
        print(f"构建输入清单失败：{exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
