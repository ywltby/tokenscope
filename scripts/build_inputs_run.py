"""构建输入清单的编排层：真实运行 Cargo、收集证据、生成或比较清单。

归属方式：只用 Cargo 自己的 JSON `compiler-artifact` 消息里的产物路径与
`package_id` 把 dep-info 精确映射到本地包（`libtokenscope-<hash>.rlib` →
`tokenscope-<hash>.d`），不根据 `.d` 文件名猜包名。
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
from datetime import datetime, timezone

from build_inputs import (
    ManifestError,
    MODES,
    PACKAGE_ROOTS,
    collect_records,
    filter_tracked,
    git_tracked,
    is_excluded_input,
    load_manifest,
    normalize_repo_path,
    parse_build_script_output,
    parse_dep_info,
    workflow_coverage,
    write_manifest,
)

LOCAL_PACKAGES = ("tokenscope", "tokenscope-app")
ARTIFACT_SUFFIXES = (".rlib", ".rmeta", ".exe", ".dll", ".lib", ".so", ".d", ".pdb")
EVIDENCE_ROOT = "qa-artifacts/build-inputs"
# 会被清单路径改动影响、且用 paths 过滤控制是否运行的工作流。
WORKFLOW_PATHS = (".github/workflows/install-test.yml", ".github/workflows/ui-e2e.yml")


def run_id():
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def evidence_dir(root, name=None):
    path = Path(root) / EVIDENCE_ROOT / (name or run_id())
    path.mkdir(parents=True, exist_ok=True)
    return path


def _remove_local_package_artifacts(root, package):
    """删除本地包的构建目录与指纹（两种 Cargo 布局）。

    2026-11-13（CI 阻断修复）：`cargo clean -p` 在缓存恢复/新布局下可能
    留下构建脚本指纹，Cargo 因「fresh」只回放而不写 `output`，检查器读不到
    声明输入。显式删除 `target/*/build/<pkg>`、`<pkg>-*` 与 `.fingerprint`
    对应目录，保证本轮构建脚本真实执行且输出可读。
    """
    targets = []
    for base in Path(root).glob("target/*/build"):
        targets.extend(
            p for p in base.iterdir() if p.name == package or p.name.startswith(package + "-")
        )
    for base in Path(root).glob("target/*/.fingerprint"):
        targets.extend(
            p for p in base.iterdir() if p.name == package or p.name.startswith(package + "-")
        )
    for path in targets:
        shutil.rmtree(path, ignore_errors=True)


def clean_local_packages(root):
    """清理两个本地 package（不是整仓 clean），建立明确的干净构建区。"""
    command = ["cargo", "clean", "-p", "tokenscope", "-p", "tokenscope-app"]
    result = subprocess.run(command, cwd=str(root), capture_output=True,
                            text=True, encoding="utf-8", errors="replace")
    if result.returncode != 0:
        raise ManifestError(f"cargo clean -p 失败，无法建立干净构建区：{result.stderr.strip()}")
    for package in LOCAL_PACKAGES:
        _remove_local_package_artifacts(root, package)


def run_cargo(root, mode, evidence):
    """运行一种构建模式并把 Cargo JSON 原样留证；失败立即中止。"""
    command = ["cargo", "test", "--locked", "--workspace", "--no-run", "--message-format=json"]
    if mode == "acceptance":
        command += ["--features", "acceptance"]
    result = subprocess.run(command, cwd=str(root), capture_output=True,
                            text=True, encoding="utf-8", errors="replace")
    (evidence / f"cargo-{mode}.json").write_text(result.stdout or "", encoding="utf-8")
    if result.stderr:
        (evidence / f"cargo-{mode}.stderr.txt").write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        raise ManifestError(f"cargo 构建失败（{mode}），退出码 {result.returncode}")
    messages = []
    for line in (result.stdout or "").splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                messages.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    if not messages:
        raise ManifestError(f"cargo 未输出可解析的 JSON 消息（{mode}）")
    return messages


def local_package(package_id):
    if not isinstance(package_id, str) or not package_id.startswith("path+"):
        return None
    name = package_id.rsplit("#", 1)[-1].rsplit("@", 1)[0]
    return name if name in LOCAL_PACKAGES else None


def dep_files_from_messages(messages):
    """`{package: {dep-info 路径}}`：由产物路径精确映射，不猜包名。"""
    found = {}
    for message in messages:
        if message.get("reason") != "compiler-artifact":
            continue
        package = local_package(message.get("package_id"))
        if package is None:
            continue
        for filename in message.get("filenames", []):
            path = Path(filename)
            if path.suffix not in ARTIFACT_SUFFIXES:
                continue
            candidate = path.with_suffix(".d")
            if path.suffix in (".rlib", ".rmeta") and path.name.startswith("lib"):
                candidate = candidate.with_name(candidate.name[3:])
            if candidate.is_file():
                found.setdefault(package, set()).add(candidate)
    return found


def snapshot_build_script_output(messages, evidence):
    """把本次构建脚本输出留证：`output` 位于会被后续清理的构建目录内。"""
    if evidence is None:
        return
    for message in messages:
        if message.get("reason") != "build-script-executed":
            continue
        package = local_package(message.get("package_id"))
        out_dir = message.get("out_dir")
        if package is None or not out_dir:
            continue
        output = Path(out_dir).parent / "output"
        if output.is_file():
            (Path(evidence) / f"build-script-output-{package}-{output.parent.name}.txt").write_text(
                output.read_text(encoding="utf-8", errors="replace"), encoding="utf-8")


def _target_build_output_candidates(root, package):
    """target 下同包构建脚本 `output` 的候选（兼容两种 Cargo 布局）。

    旧布局 `build/<pkg>-<hash>/output`；较新 Cargo 使用
    `build/<pkg>/<hash>/output`（2026-11-13 CI 证据中的 out_dir 即后者）。
    只匹配包名边界，避免 tokenscope 误取 tokenscope-app 的输出。
    """
    candidates = []
    for build in Path(root).glob("target/*/build"):
        candidates.extend(build.glob(f"{package}-*/output"))
        candidates.extend(build.glob(f"{package}/*/output"))
        candidates.extend(build.glob(f"{package}/output"))
    return candidates


def _fingerprint_build_inputs_text(root, package):
    """从 Cargo 指纹 JSON 读取构建脚本声明输入。

    2026-11-13（CI 证据）：缓存恢复/新布局下 `output` 文件可能不存在，但
    `.fingerprint/<pkg>-<hash>/run-build-script-build-script-build.json` 的
    `local[].RerunIfChanged.paths` 保存了同一组 `rerun-if-changed` 声明。
    该指纹是 Cargo 自己的增量依据，和 output 文件同等权威。
    """
    candidates = []
    for base in Path(root).glob("target/*/.fingerprint"):
        candidates.extend(base.glob(f"{package}-*/run-build-script-build-script-build.json"))
        candidates.extend(base.glob(f"{package}/*/run-build-script-build-script-build.json"))
        candidates.extend(base.glob(f"{package}/run-build-script-build-script-build.json"))
    paths = []
    for candidate in sorted(
        candidates, key=lambda p: p.stat().st_mtime if p.is_file() else 0.0, reverse=True
    ):
        try:
            value = json.loads(candidate.read_text(encoding="utf-8", errors="replace"))
        except (OSError, json.JSONDecodeError):
            continue
        for entry in value.get("local") or []:
            changed = entry.get("RerunIfChanged") if isinstance(entry, dict) else None
            if isinstance(changed, dict):
                paths.extend(str(p) for p in changed.get("paths") or [])
        if paths:
            break
    if not paths:
        return None
    return "".join(f"cargo:rerun-if-changed={path}\n" for path in paths)


def _build_script_output_text(root, package, out_dir, evidence):
    """读取构建脚本输出：消息 out_dir → 证据快照 → target 下同包构建目录。

    2026-11-13（CI 阻断修复）：rust-cache 恢复的构建可能让
    `build-script-executed` 的 out_dir 缺少 `output` 文件。缺输出时若直接
    按「无声明输入」回退整包目录，会得到与提交清单不同的粒度（`src-tauri`
    对 `src-tauri/capabilities`），把缓存状态伪装成清单变更。这里先在
    target 下查找同包构建脚本输出；仍找不到才由调用方保守回退，并落证据。
    """
    candidates = []
    if out_dir:
        candidates.append(Path(out_dir).parent / "output")
    if evidence is not None:
        candidates.extend(sorted(Path(evidence).glob(f"build-script-output-{package}-*.txt")))
    candidates.extend(
        sorted(
            _target_build_output_candidates(root, package),
            key=lambda p: p.stat().st_mtime if p.is_file() else 0.0,
            reverse=True,
        )
    )
    # 最后兜底：在 build 树下按包名边界搜索任意层级的 output（覆盖 Cargo
    # 新布局与缓存恢复后的目录结构差异；不跨包误取 tokenscope 的输出）。
    for build in Path(root).glob("target/*/build"):
        for output in build.rglob("output"):
            if not output.is_file():
                continue
            parts = output.relative_to(build).parts
            if any(part == package or part.startswith(package + "-") for part in parts):
                candidates.append(output)
    for candidate in candidates:
        if candidate.is_file():
            return candidate.read_text(encoding="utf-8", errors="replace")
    fingerprint_text = _fingerprint_build_inputs_text(root, package)
    if fingerprint_text is not None:
        return fingerprint_text
    if evidence is not None:
        (Path(evidence) / f"build-script-output-missing-{package}.txt").write_text(
            f"package={package}\nout_dir={out_dir}\ntarget_candidates={len(candidates)}\n",
            encoding="utf-8",
        )
    return None


def build_script_inputs(root, messages, evidence=None):
    """`({package: [输入]}, {package: 包目录})`：后者用于无声明输入的保守登记。"""
    declared, undeclared = {}, {}
    for message in messages:
        if message.get("reason") != "build-script-executed":
            continue
        package = local_package(message.get("package_id"))
        if package is None:
            continue
        out_dir = message.get("out_dir")
        text = _build_script_output_text(root, package, out_dir, evidence)
        entries = parse_build_script_output(text) if text is not None else []
        collected = []
        for _, value in entries:
            rel = normalize_repo_path(value, base=root / PACKAGE_ROOTS[package], repo=root)
            if rel is None or is_excluded_input(rel, "file"):
                continue
            collected.append(rel)
        if collected:
            declared.setdefault(package, set()).update(collected)
        else:
            undeclared[package] = PACKAGE_ROOTS[package]
    # 2026-11-13（CI 阻断修复）：rust-cache 恢复时 Cargo 可能不再发出
    # `build-script-executed`。对确有 build.rs 的本地包，从 target 读取最近
    # 一次构建脚本输出，避免把「缺消息」误当成「无声明输入」而回退整包目录。
    for package, package_root in PACKAGE_ROOTS.items():
        if package in declared or package in undeclared:
            continue
        if not (root / package_root / "build.rs").is_file():
            continue
        text = _build_script_output_text(root, package, None, evidence)
        entries = parse_build_script_output(text) if text is not None else []
        collected = []
        for _, value in entries:
            rel = normalize_repo_path(value, base=root / package_root, repo=root)
            if rel is None or is_excluded_input(rel, "file"):
                continue
            collected.append(rel)
        if collected:
            declared[package] = set(collected)
        else:
            undeclared[package] = package_root
    return declared, undeclared


def _mark(root, rel):
    """目录输入保留斜杠标记，用于与文件输入区分。"""
    return rel + "/" if (root / rel).is_dir() else rel


def collect_mode_inputs(root, mode, evidence, tracked_files, tracked_dirs):
    return collect_from_messages(root, run_cargo(root, mode, evidence),
                                 evidence, mode, tracked_files, tracked_dirs)


def collect_from_messages(root, messages, evidence, mode, tracked_files, tracked_dirs):
    """把一次 Cargo 构建的 JSON 消息解码为 `{package: [输入]}`（已过滤跟踪范围）。

    与 `run_cargo` 分离，便于用已保存的证据复核或测试，不重复编译。"""
    snapshot_build_script_output(messages, evidence)
    by_package = {}
    for package, dep_files in dep_files_from_messages(messages).items():
        for dep_file in sorted(dep_files):
            text = dep_file.read_text(encoding="utf-8", errors="replace")
            for raw in parse_dep_info(text):
                rel = normalize_repo_path(raw, base=root, repo=root)
                if rel is None:
                    continue
                marked = _mark(root, rel)
                kind = "directory" if marked.endswith("/") else "file"
                if is_excluded_input(marked.rstrip("/"), kind):
                    continue
                by_package.setdefault(package, set()).add(marked)
    declared, undeclared = build_script_inputs(root, messages, evidence)
    for package, values in declared.items():
        for rel in values:
            by_package.setdefault(package, set()).add(_mark(root, rel))
    for package, directory in undeclared.items():
        by_package.setdefault(package, set()).add(directory + "/")
    filtered = {}
    for package, values in by_package.items():
        kept, dropped = filter_tracked(sorted(values), tracked_files, tracked_dirs)
        filtered[package] = kept
        if dropped:
            (evidence / f"excluded-{package}-{mode}.txt").write_text(
                "\n".join(dropped) + "\n", encoding="utf-8")
    return filtered


def collect_both_modes(root, evidence):
    tracked_files, tracked_dirs = git_tracked(root)
    inputs_by_consumer = {}
    for mode in MODES:
        collected = collect_mode_inputs(root, mode, evidence, tracked_files, tracked_dirs)
        for package in LOCAL_PACKAGES:
            if package not in collected:
                raise ManifestError(f"{mode} 模式缺少本地包 {package} 的依赖证据")
            inputs_by_consumer[(package, mode)] = collected[package]
    return collect_records(inputs_by_consumer)


def workflow_paths_check(root, manifest_paths):
    workflows = [root / name for name in WORKFLOW_PATHS if (root / name).is_file()]
    return workflow_coverage(manifest_paths, workflows)


def update(root, manifest_path, evidence=None, *, clean=True):
    evidence = evidence or evidence_dir(root)
    (evidence / "begin.json").write_text(json.dumps({
        "run_id": evidence.name,
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "clean": bool(clean),
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if clean:
        clean_local_packages(root)
    records = collect_both_modes(root, evidence)
    previous = load_manifest(manifest_path) if Path(manifest_path).is_file() else {"manual": []}
    validate_manual(root, previous["manual"])
    manifest = {"schema_version": 1, "generated": records, "manual": previous["manual"]}
    write_manifest(manifest_path, manifest)
    return {"manifest": str(manifest_path), "generated": len(records),
            "evidence": str(evidence),
            "packages": sorted({c["package"] for r in records for c in r["consumers"]})}


def diff_records(expected, actual):
    def key(record):
        return (record["path"], record["kind"])

    expected_map = {key(record): record for record in expected}
    actual_map = {key(record): record for record in actual}
    missing = [expected_map[item] for item in sorted(set(expected_map) - set(actual_map))]
    stale = [actual_map[item] for item in sorted(set(actual_map) - set(expected_map))]
    changed = []
    for item in sorted(set(expected_map) & set(actual_map)):
        if expected_map[item]["consumers"] != actual_map[item]["consumers"]:
            changed.append({"path": item[0], "kind": item[1],
                            "expected": expected_map[item]["consumers"],
                            "manifest": actual_map[item]["consumers"]})
    return missing, stale, changed


def validate_manual(root, records):
    """人工输入独立校验，不能替代或掩盖生成项。"""
    files, directories = git_tracked(root)
    for record in records:
        rel, kind = record["path"], record["kind"]
        if normalize_repo_path(rel, base=root, repo=root) != rel:
            raise ManifestError(f"人工输入不是规范仓库相对路径：{rel}")
        path = root / rel
        valid = (rel in files and path.is_file()) if kind == "file" else (
            rel in directories and path.is_dir())
        if not valid:
            raise ManifestError(f"人工输入不存在、类型不符或未受版本管理：{rel}")
        if any(c["package"] not in LOCAL_PACKAGES or c["mode"] not in MODES
               for c in record["consumers"]):
            raise ManifestError(f"人工输入消费方非法：{rel}")


def check(root, manifest_path, evidence=None, *, clean=True):
    manifest_path = Path(manifest_path)
    if not manifest_path.is_file():
        raise ManifestError(f"缺少构建输入清单：{manifest_path}")
    evidence = evidence or evidence_dir(root)
    if clean:
        clean_local_packages(root)
    expected = collect_both_modes(root, evidence)
    manifest = load_manifest(manifest_path)
    validate_manual(root, manifest["manual"])
    registered = manifest["generated"] + manifest["manual"]
    missing, stale, changed = diff_records(expected, manifest["generated"])
    report = {"generated": len(expected), "missing": missing, "stale": stale,
              "changed": changed, "evidence": str(evidence)}
    uncovered = workflow_paths_check(root, sorted({record["path"] for record in
                                                 expected + manifest["manual"]}))
    report["workflow_uncovered"] = uncovered
    # 登记路径必须仍然存在于仓库；不存在即失败，不允许静默留下失效登记。
    absent = sorted(record["path"] for record in registered
                    if not (root / record["path"].rstrip("/")).exists())
    report["absent"] = absent
    if missing or stale or changed or uncovered or absent:
        raise ManifestError("构建输入清单与本次构建不一致："
                            + json.dumps({k: report[k] for k in
                                          ("missing", "stale", "changed", "workflow_uncovered",
                                           "absent")}, ensure_ascii=False))
    return report
