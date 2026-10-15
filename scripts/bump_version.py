"""同步应用版本；PR/主分支按实际 Git 基线只读校验一次递增。

布局契约（PR1）：仓库根是虚拟 workspace，应用版本写在核心 manifest
`crates/tokenscope-core/Cargo.toml`；根 `Cargo.toml` 只有 workspace 表，
禁止向其写入 version。受版本管理的锁只有根 `Cargo.lock`，其中两个本地包
（`tokenscope`、`tokenscope-app`）必须各有一条无 source 记录且版本一致。

历史 ref 的版本按该 ref 自身的布局读取：该提交存在核心 manifest 就从核心
manifest 取，不存在才按旧布局读根 manifest 的 `[package]`；核心 manifest
损坏或缺失 `package.version` 一律失败，不静默回退旧布局。
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
import tomllib

from release_metadata import read_versions

ROOT = Path(__file__).resolve().parent.parent
ZERO_VERSION = re.compile(r"0\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
CORE_MANIFEST = "crates/tokenscope-core/Cargo.toml"
SHELL_MANIFEST = "src-tauri/Cargo.toml"
LEGACY_MANIFEST = "Cargo.toml"
JSON_CONFIGS = ("frontend/package.json", "src-tauri/tauri.conf.json")
CONFIGS = (CORE_MANIFEST, SHELL_MANIFEST, *JSON_CONFIGS)
LOCK = "Cargo.lock"


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   encoding="utf-8", stderr=subprocess.PIPE).strip()


def git_error_text(exc):
    detail = exc.stderr if exc.stderr is not None else exc.output
    if isinstance(detail, bytes):
        detail = detail.decode("utf-8", "replace")
    return (detail or "").strip()


def resolve(root, ref):
    if not isinstance(ref, str) or not ref or ref.startswith("-"):
        raise ValueError("版本基线引用非法")
    try:
        sha = git(root, "rev-parse", "--verify", "--end-of-options", ref + "^{commit}")
    except (subprocess.CalledProcessError, OSError) as exc:
        raise ValueError(f"版本基线引用无法解析：{ref}") from exc
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


def ref_has(root, sha, name):
    """该提交的树里是否存在该路径。

    不存在返回 False；Git 查询本身失败（非仓库、对象损坏、命令错误）抛
    ValueError——这与“该 ref 采用旧布局”是两件事，不能混淆后回退。
    """
    try:
        output = git(root, "ls-tree", "--name-only", sha, "--", name)
    except (subprocess.CalledProcessError, OSError) as exc:
        detail = git_error_text(exc) if isinstance(exc, subprocess.CalledProcessError) else str(exc)
        raise ValueError(f"读取历史版本布局失败：{detail or exc}") from exc
    return bool(output.strip())


def file_at(root, sha, name):
    try:
        return git(root, "show", f"{sha}:{name}")
    except (subprocess.CalledProcessError, OSError) as exc:
        detail = git_error_text(exc) if isinstance(exc, subprocess.CalledProcessError) else str(exc)
        raise ValueError(f"读取 {name}（{sha[:12]}）失败：{detail or exc}") from exc


def manifest_version(root, sha, name):
    try:
        data = tomllib.loads(file_at(root, sha, name))
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(f"{name}（{sha[:12]}）的 TOML 无法解析") from exc
    package = data.get("package")
    if not isinstance(package, dict) or not isinstance(package.get("version"), str):
        raise ValueError(f"{name}（{sha[:12]}）缺少 package.version")
    return package["version"]


def json_version(root, sha, name):
    try:
        value = json.loads(file_at(root, sha, name))
    except json.JSONDecodeError as exc:
        raise ValueError(f"{name}（{sha[:12]}）的 JSON 无法解析") from exc
    if not isinstance(value, dict) or not isinstance(value.get("version"), str):
        raise ValueError(f"{name}（{sha[:12]}）缺少 version")
    return value["version"]


def historical_versions(root, ref):
    sha = resolve(root, ref)
    manifest = CORE_MANIFEST if ref_has(root, sha, CORE_MANIFEST) else LEGACY_MANIFEST
    versions = {manifest: manifest_version(root, sha, manifest),
                SHELL_MANIFEST: manifest_version(root, sha, SHELL_MANIFEST)}
    for name in JSON_CONFIGS:
        versions[name] = json_version(root, sha, name)
    return sha, uniform(versions)


def local_names(root):
    return [tomllib.loads((root / name).read_text(encoding="utf-8"))["package"]["name"]
            for name in (CORE_MANIFEST, SHELL_MANIFEST)]


def validate_locks(root, version):
    """唯一根锁必须恰好含两个无 source 的本地包，版本与应用版本一致。

    第三方同名但带 source 的记录不是本项目包，既不参与校验也不被改写。
    """
    packages = tomllib.loads((root / LOCK).read_text(encoding="utf-8")).get("package")
    if not isinstance(packages, list):
        raise ValueError(f"{LOCK} 缺少 package 记录")
    for package_name in local_names(root):
        rows = [p for p in packages
                if p.get("name") == package_name and "source" not in p]
        if len(rows) != 1 or rows[0].get("version") != version:
            raise ValueError(f"{LOCK} 的本项目包 {package_name} 版本不一致")


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
    path = root / LOCK
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
