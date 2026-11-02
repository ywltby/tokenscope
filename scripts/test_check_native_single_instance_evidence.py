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
    """全部关键项与托盘三项都通过、无失败、无 pending 的基线 evidence。"""
    checks = [
        {"name": name, "ok": True, "detail": "ok"}
        for name in list(module.REQUIRED_OK) + sorted(module.TRAY_CHECKS)
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

    def test_tray_pending_is_only_partial_verification(self) -> None:
        # 真实驱动器形态：pending 项也在 checks 里，但 ok 为空（不是 true）
        evidence = base_evidence()
        for row in evidence["checks"]:
            if row["name"] in module.TRAY_CHECKS:
                row["ok"] = None
        evidence["pending"] = sorted(module.TRAY_CHECKS)
        evidence["verification_complete"] = False
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertTrue(result["allowed"])
        # 托盘六项（展示层 3 + 归属层 3）都允许 pending
        self.assertEqual(len(result["accepted_pending"]), 6)
        # 归属层全 pending → 唯一性未验证（不是通过）
        self.assertFalse(result["tray_uniqueness_verified"])
        # 放行只是部分验证：不宣称 AC10 通过
        self.assertEqual(result["verdict"], "partial")
        self.assertEqual(result["ac10_conclusion"], "none")
        self.assertFalse(result["verification_complete"])

    def test_tray_uniqueness_verified_only_when_owner_checks_pass(self) -> None:
        """归属层三项全部通过才算取得阳性观察；缺一项即未验证。"""
        full = base_evidence()
        self.assertTrue(module.evaluate(full, module.TRAY_CHECKS)["tray_uniqueness_verified"])

        partial = base_evidence()
        for row in partial["checks"]:
            if row["name"] == "tray_owner_single_icon_after_second":
                row["ok"] = None
        partial["pending"] = ["tray_owner_single_icon_after_second"]
        result = module.evaluate(partial, module.TRAY_CHECKS)
        self.assertTrue(result["allowed"])
        self.assertFalse(result["tray_uniqueness_verified"])

    def test_missing_tray_checks_entirely_blocks(self) -> None:
        """托盘三项既不在 checks 也不在 pending：不能当成"没有失败"放行。"""
        evidence = base_evidence()
        evidence["checks"] = [
            row for row in evidence["checks"] if row["name"] not in module.TRAY_CHECKS
        ]
        evidence["pending"] = []
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertFalse(result["allowed"])
        self.assertEqual(sorted(result["missing_tray"]), sorted(module.TRAY_CHECKS))
        self.assertIn("缺少托盘检查记录", " ".join(result["reasons"]))

    def test_check_detail_and_summary_mismatch_blocks(self) -> None:
        """checks 里托盘项 ok=false，但 failed 未列：明细与汇总不一致 → 阻断。"""
        evidence = base_evidence()
        evidence["checks"].append(
            {"name": "single_tray_icon_before_second", "ok": False, "detail": "0，期望 1"}
        )
        evidence["failed"] = []
        evidence["pending"] = [
            "single_tray_icon_after_second",
            "tray_count_unchanged_after_second",
        ]
        result = module.evaluate(evidence, module.TRAY_CHECKS)
        self.assertFalse(result["allowed"])
        self.assertEqual(
            result["checks_false_not_in_failed"], ["single_tray_icon_before_second"]
        )
        self.assertIn("明细与汇总不一致", " ".join(result["reasons"]))

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
        for row in evidence["checks"]:
            if row["name"] in module.TRAY_CHECKS:
                row["ok"] = None
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
