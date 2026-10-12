"""同步应用版本；PR/主分支按实际 Git 基线只读校验一次递增。"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib

from release_metadata import read_versions

ROOT = Path(__file__).resolve().parent.parent
ZERO_VERSION = re.compile(r"0\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
CONFIGS = ("Cargo.toml", "src-tauri/Cargo.toml", "frontend/package.json",
           "src-tauri/tauri.conf.json")
LOCKS = ("Cargo.lock", "src-tauri/Cargo.lock")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   encoding="utf-8", stderr=subprocess.PIPE).strip()


def resolve(root, ref):
    if not isinstance(ref, str) or not ref or ref.startswith("-"):
        raise ValueError("版本基线引用非法")
    sha = git(root, "rev-parse", "--verify", "--end-of-options", ref + "^{commit}")
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("版本基线不是完整提交 SHA")
    return sha


def next_version(version, kind):
    match = ZERO_VERSION.fullmatch(version) if isinstance(version, str) else None
    if match is None or kind not in ("minor", "patch"):
        raise ValueError("只允许 0.x.y 与 minor/patch 递增")
    minor, patch = map(int, match.groups())
    return f"0.{minor + 1}.0" if kind == "minor" else f"0.{minor}.{patch + 1}"


def uniform(versions):
    if not versions or len(set(versions.values())) != 1:
        raise ValueError(f"应用版本不一致：{versions}")
    version = next(iter(versions.values()))
    next_version(version, "patch")  # 同时校验当前 0.x.y 策略。
    return version


def historical_versions(root, ref):
    sha = resolve(root, ref)
    versions = {}
    for name in CONFIGS:
        text = git(root, "show", f"{sha}:{name}")
        versions[name] = (tomllib.loads(text)["package"]["version"] if name.endswith(".toml")
                          else json.loads(text)["version"])
    return sha, uniform(versions)


def local_names(root):
    return [tomllib.loads((root / name).read_text(encoding="utf-8"))["package"]["name"]
            for name in CONFIGS[:2]]


def validate_locks(root, version):
    names = local_names(root)
    for index, name in enumerate(LOCKS):
        packages = tomllib.loads((root / name).read_text(encoding="utf-8"))["package"]
        expected_names = names[:1] if index == 0 else names
        for package_name in expected_names:
            rows = [p for p in packages if p["name"] == package_name and "source" not in p]
            if len(rows) != 1 or rows[0]["version"] != version:
                raise ValueError(f"{name} 的本项目包 {package_name} 版本不一致")


def declaration(root):
    path = root / "release-change.json"
    if not path.is_file():
        raise ValueError("缺少本轮 release-change.json")
    value = json.loads(path.read_text(encoding="utf-8"))
    if (not isinstance(value, dict) or type(value.get("schema_version")) is not int
            or value["schema_version"] != 1 or value.get("kind") not in ("minor", "patch")
            or not isinstance(value.get("summary"), str) or not value["summary"].strip()
            or set(value) != {"schema_version", "kind", "summary"}):
        raise ValueError("版本声明必须包含 schema_version=1、minor/patch 和非空 summary")
    return value


def check(root, base_ref):
    root = Path(root)
    sha, previous = historical_versions(root, base_ref)
    current = uniform(read_versions(root))
    validate_locks(root, current)
    change = declaration(root)
    expected = next_version(previous, change["kind"])
    if current != expected:
        raise ValueError(f"版本应从 {previous} 按 {change['kind']} 升至 {expected}，实际 {current}")
    return {"version": current, "previous": previous, "kind": change["kind"], "base_sha": sha}


def check_event(root, event, event_name):
    if event_name == "pull_request":
        if event.get("pull_request", {}).get("base", {}).get("ref") != "main":
            raise ValueError("版本门禁只接受目标 main 的 PR")
        git(root, "fetch", "--no-tags", "origin", "+refs/heads/main:refs/remotes/origin/main")
        base = resolve(root, "origin/main")
        ancestor = subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor",
                                   base, "HEAD"], capture_output=True, check=False)
        if ancestor.returncode != 0:
            raise ValueError("main 已推进，当前被测提交未包含最新基线；请更新 PR 并重新检查")
    elif event_name == "push" and event.get("ref") == "refs/heads/main":
        base = event.get("before", "")
        if not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
            raise ValueError("main push 缺少真实 before 提交")
    elif event_name == "push" and str(event.get("ref", "")).startswith("refs/tags/v"):
        current = uniform(read_versions(Path(root)))
        validate_locks(Path(root), current)
        if event["ref"] != "refs/tags/v" + current:
            raise ValueError("版本标签与应用版本不一致")
        return {"version": current, "kind": "existing-tag"}
    else:
        raise ValueError("不支持的版本校验事件")
    return check(root, base)


def replace_version(text, version):
    updated, count = re.subn(r'^version\s*=\s*"[^"]+"', f'version = "{version}"',
                            text, count=1, flags=re.M)
    if count != 1:
        raise ValueError("缺少可更新的 version 字段")
    return updated


def atomic_write(changes):
    """先暂存全部 UTF-8 内容；任一替换失败则恢复已替换文件，不留半次更新。"""
    originals = {}
    for path in changes:
        if path.exists():
            with path.open(encoding="utf-8", newline="") as fh:
                originals[path] = fh.read()
        else:
            originals[path] = None
    staged, committed = {}, []
    try:
        for path, text in changes.items():
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n",
                                             dir=path.parent, prefix=".tokenscope-version-",
                                             delete=False) as fh:
                staged[path] = Path(fh.name)
                fh.write(text)
        for path, temporary in staged.items():
            os.replace(temporary, path)
            committed.append(path)
    except BaseException:
        for path in reversed(committed):
            original = originals[path]
            if original is None:
                path.unlink(missing_ok=True)
            else:
                with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="",
                                                 dir=path.parent, prefix=".tokenscope-version-",
                                                 delete=False) as fh:
                    rollback = Path(fh.name)
                    fh.write(original)
                try:
                    os.replace(rollback, path)
                finally:
                    rollback.unlink(missing_ok=True)
        raise
    finally:
        for temporary in staged.values():
            temporary.unlink(missing_ok=True)


def bump(root, kind, base_ref="origin/main", summary="版本更新"):
    root = Path(root)
    _, previous = historical_versions(root, base_ref)
    current = uniform(read_versions(root))
    validate_locks(root, current)
    if current not in {previous, next_version(previous, "minor"), next_version(previous, "patch")}:
        raise ValueError("工作区版本偏离当前 main 基线，请先核对已有版本改动")
    version = next_version(previous, kind)
    if not isinstance(summary, str) or not summary.strip():
        raise ValueError("本轮版本摘要不能为空")
    changes = {}
    for name in CONFIGS:
        path = root / name
        text = path.read_text(encoding="utf-8")
        if name.endswith(".json"):
            value = json.loads(text)
            value["version"] = version
            updated = json.dumps(value, ensure_ascii=False, indent=2) + "\n"
        else:
            pattern = r"(?ms)(^\[package\][^\n]*\n)(.*?)(?=^\[|\Z)"
            updated, count = re.subn(pattern, lambda m: m[1] + replace_version(m[2], version), text)
            if count != 1:
                raise ValueError(f"{name} 缺少唯一 package 表")
        changes[path] = updated
    names = set(local_names(root))
    for name in LOCKS:
        path = root / name
        text = path.read_text(encoding="utf-8")

        def update_package(match):
            package = tomllib.loads(match[0])["package"][0]
            return (replace_version(match[0], version)
                    if package["name"] in names and "source" not in package else match[0])

        changes[path] = re.sub(r"(?ms)^\[\[package\]\]\n.*?(?=^\[\[package\]\]|\Z)",
                               update_package, text)
    changes[root / "release-change.json"] = json.dumps(
        {"schema_version": 1, "kind": kind, "summary": summary},
        ensure_ascii=False, indent=2) + "\n"
    atomic_write(changes)
    check(root, base_ref)
    return version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--bump", choices=("minor", "patch"))
    mode.add_argument("--check", action="store_true")
    parser.add_argument("--base-ref", default="origin/main")
    parser.add_argument("--summary", default="版本更新")
    parser.add_argument("--event-file", type=Path)
    args = parser.parse_args()
    try:
        if args.bump:
            result = {"version": bump(ROOT, args.bump, args.base_ref, args.summary)}
        elif args.event_file:
            result = check_event(ROOT, json.loads(args.event_file.read_text(encoding="utf-8")),
                                 os.environ.get("GITHUB_EVENT_NAME"))
        else:
            result = check(ROOT, args.base_ref)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as exc:
        print(f"版本校验失败：{exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
