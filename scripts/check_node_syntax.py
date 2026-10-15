"""校验前端脚本的 Node 语法（`node --check`），只解析不执行。

对应计划 §W05a「`frontend/scripts/**` … E2E 编排并入脚本规则」：编排脚本改动时
必须能被选中检查，而前端四项（typecheck/format/test/build）并不校验它们能否运行。
真正的原生 E2E 仍由 PR CI 的 ui-e2e 工作流承担。
"""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
FOLDERS = ("frontend/scripts",)
SUFFIXES = (".mjs", ".js")


def node_files(root):
    for folder in FOLDERS:
        base = Path(root) / folder
        if not base.is_dir():
            continue
        for item in sorted(base.rglob("*")):
            if item.is_file() and item.suffix in SUFFIXES:
                yield item


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    node = shutil.which("node")
    if node is None:
        print("[node-syntax] 找不到 node，拒绝在无验证的情况下通过", file=sys.stderr)
        return 127
    files = list(node_files(args.root))
    failures = []
    for item in files:
        result = subprocess.run([node, "--check", str(item)], capture_output=True,
                                encoding="utf-8", errors="replace")
        if result.returncode != 0:
            detail = (result.stderr or result.stdout or "").strip().splitlines()
            location = detail[0] if detail else "语法检查失败"
            failures.append(f"{item.relative_to(args.root)}: {location}")
    if failures:
        for line in failures:
            print(f"[node-syntax] {line}", file=sys.stderr)
        return 1
    print(f"[node-syntax] 通过：{len(files)} 个脚本")
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
