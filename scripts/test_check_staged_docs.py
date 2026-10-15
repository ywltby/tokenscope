"""暂存文档检查的契约：临时 Git 仓库里的真实 index 行为。"""

from __future__ import annotations

from pathlib import Path
import os
import subprocess
import tempfile
import unittest

from check_staged_docs import (
    added_line_numbers,
    check_document,
    documents_referencing,
    extract_targets,
    git_show,
    heading_slugs,
    resolve_target,
    staged_documents,
    strip_code_fences,
    tree_has,
    whitespace_problems,
)


def git_env():
    """Git 钩子会注入 GIT_DIR 等变量；测试仓库必须与调用者环境隔离。"""
    return {key: value for key, value in os.environ.items()
            if not key.startswith("GIT_")}


def setUpModule():
    """实现本身保留 GIT_*（它们决定本次提交的内容），隔离只能发生在测试侧。"""
    for key in [name for name in os.environ if name.startswith("GIT_")]:
        del os.environ[key]


class DocumentCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tokenscope-docs-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "中文 仓库"
        (self.root / "docs").mkdir(parents=True)
        (self.root / "docs" / "target.md").write_text("# 目标标题\n", encoding="utf-8")
        (self.root / "docs" / "中文 目标.md").write_text("# 中文标题\n", encoding="utf-8")
        (self.root / "docs" / "source.md").write_text(
            "# 源\n\n[历史坏链接](missing-file.md)\n", encoding="utf-8")
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "TokenScope fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "baseline")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args],
                                       encoding="utf-8", stderr=subprocess.STDOUT,
                                       env=git_env())

    def stage(self, relative):
        self.git("add", "--", relative)

    def test_valid_relative_link_passes(self):
        text = "[目标](target.md)\n"
        self.assertEqual(check_document(self.root, "docs/source.md", text), [])

    def test_missing_target_is_reported(self):
        problems = check_document(self.root, "docs/source.md", "[缺失](nope.md)\n")
        self.assertEqual(len(problems), 1)
        self.assertEqual(problems[0]["reason"], "暂存树中不存在")

    def test_historical_broken_link_is_not_reported_without_change(self):
        text = (self.root / "docs" / "source.md").read_text(encoding="utf-8")
        self.assertEqual(check_document(self.root, "docs/source.md", text, baseline_text=text), [])

    def test_historical_link_is_reported_when_its_target_is_deleted_now(self):
        text = "[目标](target.md)\n[历史坏链接](missing-file.md)\n"
        baseline = "[历史坏链接](missing-file.md)\n"
        problems = check_document(self.root, "docs/source.md", text,
                                  changed_targets={"docs/missing-file.md"},
                                  baseline_text=baseline)
        self.assertEqual([p["target"] for p in problems], ["missing-file.md"])

    def test_fenced_code_block_links_are_ignored(self):
        text = "```\n[示例](does-not-exist.md)\n```\n正文没有链接\n"
        self.assertEqual(check_document(self.root, "docs/source.md", text), [])
        self.assertEqual(extract_targets(text), [])

    def test_chinese_and_space_paths_resolve(self):
        text = "[中文](%E4%B8%AD%E6%96%87%20%E7%9B%AE%E6%A0%87.md)\n"
        self.assertEqual(check_document(self.root, "docs/source.md", text), [])
        self.assertEqual(resolve_target("docs/source.md", "../docs/target.md"), "docs/target.md")

    def test_image_links_are_checked_like_other_targets(self):
        text = "![图](missing.png)\n"
        problems = check_document(self.root, "docs/source.md", text)
        self.assertEqual(len(problems), 1)
        self.assertTrue(problems[0]["image"])

    def test_anchor_must_exist_in_staged_target(self):
        good = "[标题](target.md#目标标题)\n"
        bad = "[标题](target.md#不存在的标题)\n"
        self.assertEqual(check_document(self.root, "docs/source.md", good), [])
        problems = check_document(self.root, "docs/source.md", bad)
        self.assertEqual([p["reason"] for p in problems], ["标题锚点不存在"])

    def test_staged_tree_wins_over_working_tree(self):
        (self.root / "docs" / "target.md").unlink()
        self.assertTrue(tree_has(self.root, "docs/target.md"))
        text = "[目标](target.md)\n"
        self.assertEqual(check_document(self.root, "docs/source.md", text), [])

    def test_external_and_protocol_links_are_not_verified(self):
        text = "[站点](https://example.invalid/x) [邮件](mailto:a@example.invalid)\n"
        self.assertEqual(check_document(self.root, "docs/source.md", text), [])

    def test_staged_documents_only_lists_markdown_changes(self):
        (self.root / "docs" / "source.md").write_text("# 源\n改动\n", encoding="utf-8")
        (self.root / "docs" / "data.json").write_text("{}\n", encoding="utf-8")
        self.stage("docs/source.md")
        self.stage("docs/data.json")
        staged = staged_documents(self.root)
        self.assertEqual(staged, [(None, "docs/source.md")])

    def test_staged_documents_reports_rename_source(self):
        self.git("mv", "docs/source.md", "docs/renamed.md")
        staged = staged_documents(self.root)
        self.assertEqual(staged, [("docs/source.md", "docs/renamed.md")])

    def test_heading_slugs_ignore_code_fences(self):
        slugs = heading_slugs("# 一级\n```\n# 代码里的井号\n```\n## 二级\n")
        self.assertEqual(slugs, {"一级", "二级"})

    def test_strip_code_fences_keeps_surrounding_prose(self):
        stripped = strip_code_fences("前\n~~~\n丢掉\n~~~\n后\n")
        self.assertEqual(stripped, "前\n后\n")

    def test_unsupported_shorthand_reference_links_are_out_of_scope(self):
        # 简写引用式（[text][ref]）不在解析范围：明确记录边界而不是假报通过。
        text = "[文本][ref]\n"
        self.assertEqual(extract_targets(text), [])

    def test_removing_a_linked_target_is_reported_from_unchanged_documents(self):
        """删掉被引用的文件时，引用它的文档本身没改也必须被检查出来。"""
        referrer = self.root / "docs" / "referrer.md"
        referrer.write_text("# 引用者\n\n[目标](target.md)\n", encoding="utf-8")
        self.stage("docs/referrer.md")
        self.git("commit", "-qm", "加入引用文档")
        self.git("rm", "-q", "docs/target.md")
        problems = documents_referencing(self.root, ["docs/target.md"])
        self.assertEqual([problem["document"] for problem in problems],
                         ["docs/referrer.md"])
        self.assertEqual(problems[0]["reason"], "引用的目标已在本提交删除或改名")

    def test_renaming_a_target_is_reported_from_unchanged_documents(self):
        referrer = self.root / "docs" / "referrer.md"
        referrer.write_text("# 引用者\n\n[目标](target.md)\n", encoding="utf-8")
        self.stage("docs/referrer.md")
        self.git("commit", "-qm", "加入引用文档")
        self.git("mv", "docs/target.md", "docs/moved.md")
        problems = documents_referencing(self.root, ["docs/target.md"])
        self.assertEqual([problem["document"] for problem in problems],
                         ["docs/referrer.md"])

    def test_removed_target_without_references_reports_nothing(self):
        (self.root / "docs" / "lonely.md").write_text("# 无人引用\n", encoding="utf-8")
        self.stage("docs/lonely.md")
        self.git("commit", "-qm", "加入孤立文档")
        self.git("rm", "-q", "docs/lonely.md")
        self.assertEqual(documents_referencing(self.root, ["docs/lonely.md"]), [])

    def test_new_trailing_whitespace_is_reported(self):
        (self.root / "docs" / "source.md").write_text("# 源\n带空白 \n干净\n", encoding="utf-8")
        self.stage("docs/source.md")
        problems = whitespace_problems(
            "docs/source.md", git_show(self.root, ":docs/source.md"),
            added_line_numbers(self.root, "docs/source.md"))
        self.assertEqual([problem["reason"] for problem in problems], ["行尾空白"])

    def test_historical_trailing_whitespace_is_not_reported(self):
        legacy = self.root / "docs" / "legacy.md"
        legacy.write_text("# 旧\n带空白 \n", encoding="utf-8")
        self.stage("docs/legacy.md")
        self.git("commit", "-qm", "含历史空白的基线")
        legacy.write_text("# 旧\n带空白 \n新增行\n", encoding="utf-8")
        self.stage("docs/legacy.md")
        self.assertEqual(whitespace_problems(
            "docs/legacy.md", git_show(self.root, ":docs/legacy.md"),
            added_line_numbers(self.root, "docs/legacy.md")), [])

    def test_carriage_return_in_added_line_is_reported(self):
        problems = whitespace_problems("docs/x.md", "# 标题\r\n正文\n", {1})
        self.assertEqual([problem["reason"] for problem in problems],
                         ["行尾为 CR（仓库钉 LF）"])


if __name__ == "__main__":
    unittest.main()
