#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AC10 单实例 evidence 的放行判定（只做部分验证判定，不宣布 AC10 通过）。

用途：`ui-single-instance` 场景在 GitHub-hosted Runner 上运行时，托盘区域可能
整体不可观测（Explorer 环境差异）。场景本身对不可观测项记 pending 且整体
`verification_complete=false`；本脚本把「可接受的已知未验证项」与「真失败」
分开，供 CI 步骤决定是否放行：

  - `failed` 必须为空，且每一项都必须在 `checks` 里以 `ok=false` 出现；
  - `pending` 必须全部落在允许集合内，且不得同时出现在 `checks` 里；
  - `checks` 里 `ok=false` 的项必须全部出现在 `failed`（明细与汇总一致）；
  - 托盘三项必须**明确出现在 `checks` 或 `pending` 之一**（缺项即阻断，
    防止"三项都不存在"被当成"没有失败"）；
  - 关键项必须出现在 `checks` 且 `ok=true`。

放行只表示"未验证项在允许集合内"，**不表示 AC10 通过**：输出固定
`ac10_conclusion=none`、`verdict=partial`，并列出未验证项。

退出码：0 = 放行（部分验证）；1 = 阻断。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

TRAY_CHECKS = frozenset(
    {
        "single_tray_icon_before_second",
        "single_tray_icon_after_second",
        "tray_count_unchanged_after_second",
    }
)

REQUIRED_OK = (
    "acceptance_root_isolated",
    "acceptance_build_identity_assumed",
    "second_process_same_isolated_root",
    "secondary_pid_differs_from_primary",
    "second_process_exit_confirmed",
    "second_process_exited_within_budget",
    "second_process_exit_code_zero",
    "first_window_restored_visible",
    "first_window_foreground",
    "first_instance_alive_after_second",
    "real_data_untouched",
    # 托盘唯一性改由归属观察判定（按进程 + (hwnd, uID)），不再依赖文本计数
    "tray_owner_single_icon_before_second",
    "tray_owner_single_icon_after_second",
    "tray_owner_unchanged_after_second",
)

# 必须留下明确记录的项：托盘三项（见 checks 或 pending）+ 全部关键项（见 checks）
REQUIRED_PRESENT = TRAY_CHECKS | frozenset(REQUIRED_OK)


def load_evidence(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def evaluate(evidence: dict, allow: frozenset[str]) -> dict:
    checks = evidence.get("checks") or []
    by_name = {row.get("name"): row for row in checks}
    failed = list(evidence.get("failed") or [])
    pending = list(evidence.get("pending") or [])

    reasons: list[str] = []

    duplicate_checks = sorted(
        name for name, count in _count(checks).items() if count > 1
    )
    if duplicate_checks:
        reasons.append(f"checks 中存在重复项：{', '.join(duplicate_checks)}")

    both = sorted(set(failed) & set(pending))
    if both:
        reasons.append(f"同一项同时出现在 failed 与 pending：{', '.join(both)}")

    # 托盘三项必须明确出现（checks 或 pending），缺失即阻断
    missing_tray = sorted(name for name in TRAY_CHECKS if name not in by_name and name not in pending)
    if missing_tray:
        reasons.append(f"缺少托盘检查记录（checks/pending 都没有）：{', '.join(missing_tray)}")

    # failed 明细一致性：每项必须在 checks 且 ok=false
    failed_not_in_checks = [name for name in failed if name not in by_name]
    if failed_not_in_checks:
        reasons.append(f"failed 中的项未出现在 checks：{', '.join(failed_not_in_checks)}")
    failed_marked_ok = [
        name for name in failed if name in by_name and by_name[name].get("ok") is not False
    ]
    if failed_marked_ok:
        reasons.append(f"failed 中的项在 checks 里不是 ok=false：{', '.join(failed_marked_ok)}")

    # checks 汇总一致性：ok=false 的项必须都在 failed
    checks_false = [name for name, row in by_name.items() if row.get("ok") is False]
    false_not_failed = [name for name in checks_false if name not in failed]
    if false_not_failed:
        reasons.append(
            f"checks 里 ok=false 但未计入 failed（明细与汇总不一致）：{', '.join(false_not_failed)}"
        )

    # checks 汇总一致性：ok=true 的项不得出现在 failed/pending
    checks_true = [name for name, row in by_name.items() if row.get("ok") is True]
    true_in_failed = [name for name in checks_true if name in failed]
    true_in_pending = [name for name in checks_true if name in pending]
    if true_in_failed:
        reasons.append(f"checks 里 ok=true 但同时计入 failed：{', '.join(true_in_failed)}")
    if true_in_pending:
        reasons.append(f"checks 里 ok=true 但同时计入 pending：{', '.join(true_in_pending)}")

    # checks 里无结论（ok 为空）的项必须计入 pending——真实驱动器对 pending 项
    # 会写 checks 条目但不带 ok；这类项不得悄悄消失，也不得计入 failed。
    checks_null = [
        name for name, row in by_name.items() if row.get("ok") not in (True, False)
    ]
    null_not_pending = [name for name in checks_null if name not in pending]
    if null_not_pending:
        reasons.append(
            f"checks 里无结论（ok 为空）但未计入 pending：{', '.join(null_not_pending)}"
        )

    unexpected_pending = [name for name in pending if name not in allow]
    if unexpected_pending:
        reasons.append(f"存在集合外的未验证项：{', '.join(unexpected_pending)}")

    missing_required = [name for name in REQUIRED_OK if name not in by_name]
    if missing_required:
        reasons.append(f"缺少关键项：{', '.join(missing_required)}")
    failed_required = [
        name for name in REQUIRED_OK if name in by_name and by_name[name].get("ok") is not True
    ]
    if failed_required:
        reasons.append(f"关键项未通过：{', '.join(failed_required)}")

    if failed:
        reasons.append(f"存在失败项：{', '.join(failed)}")

    present = set(by_name) | set(pending)
    missing_present = sorted(name for name in REQUIRED_PRESENT if name not in present)
    if missing_present:
        reasons.append(f"缺少必须记录的检查项：{', '.join(missing_present)}")

    allowed = not reasons
    accepted_pending = [name for name in pending if name in allow]
    return {
        "allowed": allowed,
        # 放行只是"部分验证"；本轮不宣布 AC10 通过
        "verdict": "partial" if allowed else "blocked",
        "ac10_conclusion": "none",
        "failed": failed,
        "pending": pending,
        "accepted_pending": accepted_pending,
        "unexpected_pending": unexpected_pending,
        "missing_tray": missing_tray,
        "missing_required": missing_required,
        "failed_required": failed_required,
        "checks_false_not_in_failed": false_not_failed,
        "verification_complete": evidence.get("verification_complete") is True,
        "real_untouched": by_name.get("real_data_untouched", {}).get("ok") is True,
        "reasons": reasons,
    }


def _count(checks: list[dict]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for row in checks:
        name = row.get("name")
        if isinstance(name, str):
            counts[name] = counts.get(name, 0) + 1
    return counts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="AC10 单实例 evidence 的放行判定（部分验证）")
    parser.add_argument("--evidence", required=True, help="evidence.json 路径")
    parser.add_argument(
        "--allow-pending",
        default="tray",
        help="允许 pending 的检查项集合：tray（默认托盘三项）/none/逗号分隔的项名",
    )
    parser.add_argument("--json", action="store_true", help="以 JSON 输出结果")
    args = parser.parse_args(argv)

    raw = args.allow_pending.strip()
    if raw == "tray":
        allow = TRAY_CHECKS
    elif raw in ("", "none"):
        allow = frozenset()
    else:
        allow = frozenset(part.strip() for part in raw.split(",") if part.strip())

    path = Path(args.evidence)
    if not path.is_file():
        print(f"[ac10-evidence] 找不到 evidence：{path}", file=sys.stderr)
        return 1
    try:
        evidence = load_evidence(path)
    except (OSError, ValueError) as error:
        print(f"[ac10-evidence] evidence 解析失败：{error}", file=sys.stderr)
        return 1

    result = evaluate(evidence, allow)
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        state = "部分验证（放行）" if result["allowed"] else "阻断"
        print(f"[ac10-evidence] {state}：{path}")
        print(f"  failed={len(result['failed'])} pending={len(result['pending'])}")
        if result["accepted_pending"]:
            print(f"  已知未验证项（不计为通过）：{', '.join(result['accepted_pending'])}")
        if result["unexpected_pending"]:
            print(f"  集合外未验证：{', '.join(result['unexpected_pending'])}")
        if result["reasons"]:
            for reason in result["reasons"]:
                print(f"  - {reason}")
        if result["allowed"]:
            print(
                "  结论：部分验证（ac10_conclusion=none）——存在未验证项，"
                "不得用作 AC10 已通过的结论；AC10 与 N12 保持开放。"
            )
    return 0 if result["allowed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
