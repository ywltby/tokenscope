#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AC10 单实例 evidence 的 CI 放行判定（不把 pending 当通过）。

用途：`ui-single-instance` 场景在 GitHub-hosted Runner 上运行时，托盘区域可能
整体不可观测（Explorer 环境差异）。场景本身对不可观测项记 pending 且整体
`verification_complete=false`；本脚本负责把「可接受的已知未验证项」与「真失败」
分开，供 CI 步骤决定是否放行：

  - `failed` 必须为空（任何失败都阻断）；
  - `pending` 必须全部落在允许集合内（默认托盘三项），集合外的 pending 阻断；
  - 关键项必须显式通过（第二实例退出/耗时/归属、窗口恢复与前台、隔离根、
    真实数据未动、构建身份 assumed）。

放行不代表 AC10 关闭：evidence 的 `pending` 原样保留，报告里明确列出未验证项。

退出码：0 = 放行（failed 为空且 pending ⊆ 允许集合且关键项通过）；1 = 阻断。
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
)


def load_evidence(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def evaluate(evidence: dict, allow: frozenset[str]) -> dict:
    checks = evidence.get("checks") or []
    by_name = {row.get("name"): row for row in checks}
    failed = list(evidence.get("failed") or [])
    pending = list(evidence.get("pending") or [])
    unexpected_pending = [name for name in pending if name not in allow]
    missing_required = [name for name in REQUIRED_OK if name not in by_name]
    failed_required = [
        name
        for name in REQUIRED_OK
        if name in by_name and by_name[name].get("ok") is not True
    ]
    reasons: list[str] = []
    if failed:
        reasons.append(f"存在失败项：{', '.join(failed)}")
    if unexpected_pending:
        reasons.append(f"存在集合外的未验证项：{', '.join(unexpected_pending)}")
    if missing_required:
        reasons.append(f"缺少关键项：{', '.join(missing_required)}")
    if failed_required:
        reasons.append(f"关键项未通过：{', '.join(failed_required)}")
    return {
        "allowed": not reasons,
        "failed": failed,
        "pending": pending,
        "accepted_pending": [name for name in pending if name in allow],
        "unexpected_pending": unexpected_pending,
        "missing_required": missing_required,
        "failed_required": failed_required,
        "verification_complete": evidence.get("verification_complete") is True,
        "real_untouched": by_name.get("real_data_untouched", {}).get("ok") is True,
        "reasons": reasons,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="AC10 单实例 evidence 的 CI 放行判定")
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
        state = "放行" if result["allowed"] else "阻断"
        print(f"[ac10-evidence] {state}：{path}")
        print(f"  failed={len(result['failed'])} pending={len(result['pending'])}")
        if result["accepted_pending"]:
            print(f"  已知未验证（CI 放行，不记为通过）：{', '.join(result['accepted_pending'])}")
        if result["unexpected_pending"]:
            print(f"  集合外未验证：{', '.join(result['unexpected_pending'])}")
        if result["reasons"]:
            for reason in result["reasons"]:
                print(f"  - {reason}")
        if result["allowed"]:
            print("  备注：放行不等于 AC10 关闭——托盘唯一性仍未验证，N12 保持开放。")
    return 0 if result["allowed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
