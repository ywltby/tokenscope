# -*- coding: utf-8 -*-
"""`check_native_single_instance_evidence.py` 的契约测试（合成 evidence，不读真实运行）。"""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "check_native_single_instance_evidence.py"

spec = importlib.util.spec_from_file_location("ac10_evidence_check", SCRIPT)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)


def base_evidence() -> dict:
    """全部关键项通过、无失败、无 pending 的基线 evidence。"""
    checks = [
        {"name": name, "ok": True, "detail": "ok"} for name in module.REQUIRED_OK
    ]
    return {
        "checks": checks,
        "failed": [],
        "pending": [],
        "verification_complete": True,
    }


class EvidenceCheckCase(unittest.TestCase):
    def test_all_required_ok_and_no_failures_is_allowed(self) -> None:
        result = module.evaluate(base_evidence(), module.TRAY_CHECKS)
        self.assertTrue(result["allowed"])
        self.assertEqual(result["reasons"], [])

    def test_tray_pending_is_accepted_without_marking_complete(self) -> None:
        evidence = base_evidence()
        evidence["pending"] = sorted(module.TRAY_CHECKS)
        evidence["verification_complete"] = False
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertTrue(result["allowed"])
        self.assertEqual(len(result["accepted_pending"]), 3)
        # 放行不等于完成：verification_complete 原样带出
        self.assertFalse(result["verification_complete"])
        self.assertIn("不等于 AC10 关闭", "放行不等于 AC10 关闭")

    def test_tray_pending_blocks_when_not_allowed(self) -> None:
        evidence = base_evidence()
        evidence["pending"] = sorted(module.TRAY_CHECKS)
        result = module.evaluate(evidence, frozenset())
        self.assertFalse(result["allowed"])
        self.assertIn("集合外", " ".join(result["reasons"]))

    def test_unexpected_pending_blocks(self) -> None:
        evidence = base_evidence()
        evidence["pending"] = ["second_process_exit_code_zero"]
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertFalse(result["allowed"])
        self.assertEqual(result["unexpected_pending"], ["second_process_exit_code_zero"])

    def test_any_failure_blocks(self) -> None:
        evidence = base_evidence()
        evidence["failed"] = ["first_window_foreground"]
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertFalse(result["allowed"])
        self.assertIn("存在失败项", " ".join(result["reasons"]))

    def test_missing_required_check_blocks(self) -> None:
        evidence = base_evidence()
        evidence["checks"] = [
            row for row in evidence["checks"] if row["name"] != "real_data_untouched"
        ]
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertFalse(result["allowed"])
        self.assertEqual(result["missing_required"], ["real_data_untouched"])

    def test_required_check_not_ok_blocks(self) -> None:
        evidence = base_evidence()
        for row in evidence["checks"]:
            if row["name"] == "second_process_exit_code_zero":
                row["ok"] = False
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertFalse(result["allowed"])
        self.assertEqual(result["failed_required"], ["second_process_exit_code_zero"])

    def test_cli_reads_evidence_file(self) -> None:
        evidence = base_evidence()
        evidence["pending"] = sorted(module.TRAY_CHECKS)
        evidence["verification_complete"] = False
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "evidence.json"
            path.write_text(json.dumps(evidence, ensure_ascii=False), encoding="utf-8")
            code = module.main(["--evidence", str(path), "--allow-pending", "tray"])
            self.assertEqual(code, 0)
            strict = module.main(["--evidence", str(path), "--allow-pending", "none"])
            self.assertEqual(strict, 1)

    def test_cli_missing_or_broken_evidence_returns_1(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "nope.json"
            self.assertEqual(module.main(["--evidence", str(missing)]), 1)
            broken = Path(tmp) / "broken.json"
            broken.write_text("{not json", encoding="utf-8")
            self.assertEqual(module.main(["--evidence", str(broken)]), 1)

    def test_repository_script_exists_and_ui_e2e_is_not_wired_yet(self) -> None:
        # 本轮只准备接线，不改工作流触发面：ui-e2e.yml 仍不含 ui-single-instance
        workflow = (ROOT / ".github" / "workflows" / "ui-e2e.yml").read_text(encoding="utf-8")
        self.assertIn("ui-refresh", workflow)
        self.assertNotIn("ui-single-instance", workflow)


if __name__ == "__main__":
    unittest.main()
