"""读取历史库的验收口径（安装/升级/卸载断言用）。

用法：python scripts/inspect_history.py <history.db>
输出：`<requests> <input> <output> <projects>` 或 `absent`（库不存在 /
尚未创建 usage_events 表）。
"""

from __future__ import annotations

import sqlite3
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    db = sys.argv[1]
    try:
        con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        print("absent")
        return 0
    try:
        tables = {
            r[0]
            for r in con.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        }
        if "usage_events" not in tables:
            print("absent")
            return 0
        count, total_in, total_out = con.execute(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), "
            "COALESCE(SUM(output_tokens), 0) FROM usage_events"
        ).fetchone()
        projects = con.execute(
            "SELECT COUNT(DISTINCT project_key) FROM usage_events "
            "WHERE project_key IS NOT NULL"
        ).fetchone()[0]
        print(f"{count} {total_in} {total_out} {projects}")
        return 0
    finally:
        con.close()


if __name__ == "__main__":
    sys.exit(main())
