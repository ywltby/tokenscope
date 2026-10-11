"""读取历史库的验收口径（安装/升级/卸载断言用）。

用法：
  python scripts/inspect_history.py <history.db>
      输出 `<requests> <input> <output> <projects>` 或 `absent`。
  python scripts/inspect_history.py <history.db> --projects
      逐行输出去重后的 project_key（排序），供路径级断言使用。
  python scripts/inspect_history.py <history.db> --cwd
      逐行输出 `session_initial_cwd=<路径>`（去重非空、排序）与
      `event_cwd_nonnull=<n>`，供 cwd 元数据级断言使用（M06：三个
      cwd 字段都要验到，不只 project_key）。
  python scripts/inspect_history.py <history.db> --records
      按 record_id 排序逐行输出
      `record_id=..|project=..|session_initial_cwd=..|event_cwd=..|input=..|output=..`
      （缺值输出 NULL）——供**按请求身份逐条核对** cwd 字段与 NULL
      语义的断言使用（M06：DISTINCT 集合相等验不出单行改 NULL 或
      两条互换）。
"""

from __future__ import annotations

import sqlite3
import sys


def open_readonly(db: str):
    try:
        return sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        return None


def main() -> int:
    args = [a for a in sys.argv[1:] if a not in ("--projects", "--cwd", "--records")]
    modes = [a for a in sys.argv[1:] if a in ("--projects", "--cwd", "--records")]
    records_mode = "--records" in modes
    cwd_mode = "--cwd" in modes
    projects_mode = "--projects" in modes
    if len(args) != 1:
        print(__doc__)
        return 2
    con = open_readonly(args[0])
    if con is None:
        print("absent" if modes == [] else "")
        return 0
    try:
        tables = {
            r[0]
            for r in con.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        }
        if "usage_events" not in tables:
            print("absent" if modes == [] else "")
            return 0
        if records_mode:
            rows = con.execute(
                "SELECT record_id, project_key, session_initial_cwd, event_cwd, "
                "input_tokens, output_tokens FROM usage_events "
                "WHERE record_id IS NOT NULL AND record_id <> '' ORDER BY record_id"
            ).fetchall()
            for rid, proj, sic, ecwd, inp, outp in rows:
                print(
                    f"record_id={rid}|project={proj or 'NULL'}"
                    f"|session_initial_cwd={sic or 'NULL'}"
                    f"|event_cwd={ecwd or 'NULL'}"
                    f"|input={inp}|output={outp}"
                )
            return 0
        if cwd_mode:
            sics = sorted(
                {
                    r[0]
                    for r in con.execute(
                        "SELECT DISTINCT session_initial_cwd FROM usage_events "
                        "WHERE session_initial_cwd IS NOT NULL"
                    ).fetchall()
                }
            )
            for v in sics:
                print(f"session_initial_cwd={v}")
            ecwd_nonnull = con.execute(
                "SELECT COUNT(*) FROM usage_events WHERE event_cwd IS NOT NULL"
            ).fetchone()[0]
            print(f"event_cwd_nonnull={ecwd_nonnull}")
            return 0
        if projects_mode:
            for (project,) in con.execute(
                "SELECT DISTINCT project_key FROM usage_events "
                "WHERE project_key IS NOT NULL ORDER BY project_key"
            ).fetchall():
                print(project)
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
