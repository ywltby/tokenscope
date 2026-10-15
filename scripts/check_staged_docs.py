"""暂存文档检查：只读 index 内容，校验暂存版本的本地链接与格式。

边界（PR1 §W05a）：
- 只检查本次新增/变更的链接，以及本次删除/重命名明确影响到的本地引用；
  历史文档已有的失效链接不突然全仓阻断。
- 链接目标以暂存树为准（`git show :<path>`），而不是工作区文件。
- 网络链接不在 hook 内联网验证；不支持的语法明确报告检查边界，不假报通过。
- 只检查，不修改任何文件。
"""

from __future__ import annotations

import argparse
import os
import posixpath
import re
import subprocess
import sys
import unicodedata
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent
FENCE = re.compile(r"^\s*(```+|~~~+)")
MARKDOWN_LINK = re.compile(r"(!?)\[([^\]\n]*)\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
REFERENCE_LINK = re.compile(r"^\s*\[([^\]\n]+)\]:\s*(\S+)")
HTML_LINK = re.compile(r"(?:href|src)\s*=\s*[\"']([^\"']+)[\"']")
ANCHOR = re.compile(r"^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$")
SLUG_STRIP = re.compile(r"[^\w\- ]", re.UNICODE)


class DocumentError(ValueError):
    """暂存文档读取或判定失败（区别于"发现坏链接"）。"""


def git_env():
    """剥离 Git 钩子注入的仓库变量，保证子进程只操作显式指定的仓库。"""
    return {key: value for key, value in os.environ.items()
            if not key.startswith("GIT_")}


def git_show(root, spec):
    result = subprocess.run(["git", "-C", str(root), "show", spec],
                            capture_output=True, check=False, env=git_env())
    if result.returncode != 0:
        return None
    return result.stdout.decode("utf-8", "replace")


def git_lines(root, *args):
    result = subprocess.run(["git", "-C", str(root), *args],
                            capture_output=True, check=False, env=git_env())
    if result.returncode != 0:
        raise DocumentError(f"git {' '.join(args)} 失败")
    return [line for line in result.stdout.decode("utf-8", "replace").split("\n") if line]


def staged_documents(root):
    """暂存区里新增/修改/重命名后的 Markdown 路径。"""
    result = subprocess.run(["git", "-C", str(root), "diff", "--cached", "--name-status",
                             "-z", "--find-renames", "--diff-filter=ACMR"],
                            capture_output=True, check=False, env=git_env())
    if result.returncode != 0:
        raise DocumentError("读取暂存区失败")
    fields = [item.decode("utf-8") for item in result.stdout.split(b"\0") if item]
    documents, index = [], 0
    while index < len(fields):
        status = fields[index]
        if status[:1] in ("R", "C"):
            documents.append((fields[index + 1], fields[index + 2]))
            index += 3
            continue
        documents.append((None, fields[index + 1]))
        index += 2
    return [(old, new) for old, new in documents if new.endswith(".md")]


def strip_code_fences(text):
    """去掉围栏代码块内容，避免把示例代码里的方括号当链接。"""
    lines, fenced, marker = [], False, ""
    for line in text.split("\n"):
        match = FENCE.match(line)
        if match:
            token = match.group(1)[0]
            if not fenced:
                fenced, marker = True, token
            elif token == marker:
                fenced = False
            continue
        if not fenced:
            lines.append(line)
    return "\n".join(lines)


def heading_slugs(text):
    slugs = set()
    for line in strip_code_fences(text).split("\n"):
        match = ANCHOR.match(line)
        if not match:
            continue
        title = match.group(1).strip()
        slug = SLUG_STRIP.sub("", title).strip().lower().replace(" ", "-")
        if slug:
            slugs.add(slug)
    return slugs


def extract_targets(text):
    """返回 (链接目标, 行内是否图片) 列表，含行内链接、引用式与 HTML。"""
    body = strip_code_fences(text)
    targets = [(match.group(3), bool(match.group(1))) for match in MARKDOWN_LINK.finditer(body)]
    for match in REFERENCE_LINK.finditer(body):
        targets.append((match.group(2), False))
    for match in HTML_LINK.finditer(body):
        targets.append((match.group(1), False))
    return targets


def is_external(target):
    return bool(re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target)) or target.startswith("//")


def resolve_target(source, target):
    """把链接解析为仓库相对路径（POSIX）；纯片段返回 None，图片路径可能带锚点。"""
    stripped = target.split("#", 1)[0]
    if not stripped:
        return None
    decoded = unquote(stripped)
    if decoded.startswith("/"):
        return decoded.lstrip("/")
    base = posixpath.dirname(source)
    return posixpath.normpath(posixpath.join(base, decoded)).lstrip("./") or None


def tree_has(root, path):
    """暂存树（index）里是否存在该路径（文件或目录）。"""
    result = subprocess.run(["git", "-C", str(root), "cat-file", "-e", f":{path}"],
                            capture_output=True, check=False, env=git_env())
    if result.returncode == 0:
        return True
    listing = subprocess.run(["git", "-C", str(root), "ls-files", "-z"],
                             capture_output=True, check=False, env=git_env())
    if listing.returncode != 0:
        raise DocumentError("读取暂存树失败")
    prefix = path.rstrip("/") + "/"
    return any(name.startswith(prefix) for name in
               listing.stdout.decode("utf-8", "replace").split("\0"))


def added_line_numbers(root, path):
    """暂存版本中新增/修改的行号集合（1 基），用于只检查本次引入的空白问题。"""
    result = subprocess.run(["git", "-C", str(root), "diff", "--cached", "-U0", "--", path],
                            capture_output=True, check=False, env=git_env())
    if result.returncode != 0:
        raise DocumentError(f"读取 {path} 的暂存差异失败")
    numbers = set()
    for match in re.finditer(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@",
                             result.stdout.decode("utf-8", "replace"), re.M):
        start = int(match.group(1))
        count = 1 if match.group(2) is None else int(match.group(2))
        numbers.update(range(start, start + count))
    return numbers


def whitespace_problems(source, text, added_lines):
    """只对本次新增/修改的行报告行尾空白与 CR，历史行不追溯。"""
    problems = []
    lines = text.split("\n")
    for number in sorted(added_lines):
        if number - 1 >= len(lines):
            continue
        line = lines[number - 1]
        if line.endswith("\r"):
            problems.append({"document": source, "target": f"第 {number} 行",
                             "reason": "行尾为 CR（仓库钉 LF）"})
            line = line[:-1]
        if line != line.rstrip():
            problems.append({"document": source, "target": f"第 {number} 行",
                             "reason": "行尾空白"})
    return problems


def check_document(root, source, text, changed_targets=frozenset(), baseline_text=""):
    """只校验本次新增/变更的链接，以及本次删除/重命名影响到的历史链接。

    返回问题列表；`changed_targets` 为本次删除/重命名的仓库相对路径。"""
    problems = []
    baseline = {target for target, _ in extract_targets(baseline_text)}
    for target, is_image in extract_targets(text):
        if not target or is_external(target) or target.startswith("#"):
            continue
        path = resolve_target(source, target)
        if path is None:
            continue
        if target in baseline and not _affected_by_change(path, changed_targets):
            continue  # 历史链接且本次未触及该目标：不阻断本次提交。
        if not tree_has(root, path):
            problems.append({"document": source, "target": target,
                             "reason": "暂存树中不存在", "image": is_image})
            continue
        if "#" in target and path.endswith(".md"):
            fragment = unquote(target.split("#", 1)[1])
            if fragment:
                body = git_show(root, f":{path}")
                if body is not None and fragment not in heading_slugs(body):
                    problems.append({"document": source, "target": target,
                                     "reason": "标题锚点不存在", "image": is_image})
    return problems


def _affected_by_change(path, changed_targets):
    if path in changed_targets:
        return True
    return any(name.startswith(path.rstrip("/") + "/") for name in changed_targets)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    root = args.root
    try:
        documents = staged_documents(root)
        if not documents:
            print("[docs] 暂存区没有 Markdown 变更")
            return 0
        # 本次删除/重命名的目标：只有这些目标的旧链接才追究。
        changed = set()
        deleted = subprocess.run(["git", "-C", str(root), "diff", "--cached", "--name-only",
                                  "-z", "--diff-filter=DR"], capture_output=True, check=False,
                                 env=git_env())
        if deleted.returncode == 0:
            changed.update(item.decode("utf-8") for item in deleted.stdout.split(b"\0") if item)
        problems = []
        for old, new in documents:
            text = git_show(root, f":{new}")
            if text is None:
                problems.append({"document": new, "target": "", "reason": "暂存内容不可读"})
                continue
            if "\ufffd" in text:
                problems.append({"document": new, "target": "", "reason": "不是合法 UTF-8"})
            baseline = git_show(root, f"HEAD:{new}") if old is None else git_show(root, f"HEAD:{old}")
            problems.extend(check_document(root, new, text, changed, baseline or ""))
            problems.extend(whitespace_problems(new, text, added_line_numbers(root, new)))
        for problem in problems:
            print(f"[docs] {problem['document']} → {problem['target']}：{problem['reason']}",
                  file=sys.stderr)
        return 1 if problems else 0
    except DocumentError as exc:
        print(f"[docs] 文档检查失败：{exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    sys.exit(main())
