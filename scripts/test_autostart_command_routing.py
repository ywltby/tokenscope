"""自启命令层路由源码契约（AC06 方案 A / PR52 复盘补证）。

替身 helper 单测只证明状态机；这里证明**命令层接线**本身：
- ``autostart_status``/``autostart_set`` 函数体内 ``privacy::require_ready``
  先于 ``run_blocking``（隐私闸门先于一切重活）；
- acceptance cfg 分支调用 ``autostart_shim``（隔离替身）且位于
  ``#[cfg(not(feature = "acceptance"))]`` 之前；真实插件 ``app.autolaunch()``
  只出现在 not(acceptance) 区域内；
- 全文件所有 ``autostart_shim`` 引用都落在 acceptance cfg 区域内——
  普通构建编译不到任何替身代码，真实插件行为保持。

纯源码分析：不启动应用、不触真实注册表（反例不得调用真实自启后端）。
契约自检：对突变样本（闸门缺失 / 替身引用越出 acceptance 区 / 真实插件
调用越区）必须报失败——先证契约能揭缺陷，再对真实源码放行。
"""

from pathlib import Path
import re
import unittest

SOURCE = Path(__file__).resolve().parent.parent / "src-tauri/src/commands.rs"
ACCEPTANCE_ATTR = '#[cfg(feature = "acceptance")]'
NOT_ACCEPTANCE_ATTR = '#[cfg(not(feature = "acceptance"))]'


def load_lines(text=None):
    return (text if text is not None else SOURCE.read_text(encoding="utf-8")).splitlines()


def fn_span(lines, name):
    """顶层函数体行区间 [start, end)：rustfmt 保证函数结束 `}` 在第 0 列。"""
    start = next(i for i, line in enumerate(lines) if f"pub async fn {name}(" in line)
    end = next(i for i in range(start + 1, len(lines)) if lines[i] == "}")
    return start, end


def index_of(lines, span, needle, occurrence=1):
    hits = [i for i in range(*span) if needle in lines[i]]
    if len(hits) < occurrence:
        raise AssertionError(f"区间内找不到 {needle!r}（第 {occurrence} 次）")
    return hits[occurrence - 1]


def acceptance_spans(lines):
    """acceptance cfg 覆盖的行区间集合：模块（含测试模块）与命令内部分支。"""
    spans = []
    for i, line in enumerate(lines):
        if line.strip() in (ACCEPTANCE_ATTR, '#[cfg(all(test, feature = "acceptance"))]'):
            # 模块形态：属性行之后是 `mod xxx {`，结束于第 0 列的 `}`
            if i + 1 < len(lines) and re.match(r"^mod \w+ \{$", lines[i + 1].strip()):
                end = next(j for j in range(i + 2, len(lines)) if lines[j] == "}")
                spans.append((i, end + 1))
            # 函数内裸块形态：属性行之后是缩进的 `{`，结束于同缩进的 `}`
            elif i + 1 < len(lines) and lines[i + 1].strip() == "{":
                indent = len(lines[i + 1]) - len(lines[i + 1].lstrip())
                end = next(
                    j
                    for j in range(i + 2, len(lines))
                    if lines[j].strip() == "}" and len(lines[j]) - len(lines[j].lstrip()) == indent
                )
                spans.append((i, end + 1))
    return spans


def check_routing(lines):
    """返回违例清单；空列表 = 契约成立。"""
    problems = []
    spans_by_command = {}
    for name in ("autostart_status", "autostart_set"):
        span = fn_span(lines, name)
        spans_by_command[name] = span
        try:
            gate = index_of(lines, span, "privacy::require_ready(")
            blocking = index_of(lines, span, "run_blocking(")
            acc = index_of(lines, span, ACCEPTANCE_ATTR)
            shim = index_of(lines, span, "autostart_shim::")
            not_acc = index_of(lines, span, NOT_ACCEPTANCE_ATTR)
            real = index_of(lines, span, "app.autolaunch()")
        except AssertionError as error:
            problems.append(f"{name}: {error}")
            continue
        if not gate < blocking:
            problems.append(f"{name}: 隐私闸门必须先于 run_blocking")
        if not acc < shim < not_acc < real:
            problems.append(
                f"{name}: 顺序应为 acceptance cfg → 替身 → not cfg → 真实插件，"
                f"实际行号 acc={acc} shim={shim} not={not_acc} real={real}"
            )
    covered = acceptance_spans(lines)
    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if "autostart_shim" in line and not any(a <= i < b for a, b in covered):
            problems.append(f"第 {i + 1} 行的 autostart_shim 引用不在 acceptance 区域内：{stripped}")
        if "app.autolaunch()" in line:
            command = next((n for n, s in spans_by_command.items() if s[0] <= i < s[1]), None)
            if command:
                span = spans_by_command[command]
                preceding = [j for j in range(span[0], i) if lines[j].strip() in (ACCEPTANCE_ATTR, NOT_ACCEPTANCE_ATTR)]
                if preceding and lines[preceding[-1]].strip() == ACCEPTANCE_ATTR:
                    problems.append(f"第 {i + 1} 行真实插件调用出现在 acceptance 区域内：{line.strip()}")
    if sum(1 for line in lines if line.strip() == "mod autostart_shim {") != 1:
        problems.append("autostart_shim 模块必须恰好定义一次")
    if not any(
        NOT_ACCEPTANCE_ATTR in lines[i] and "ManagerExt" in lines[i + 1]
        for i in range(len(lines) - 1)
        if lines[i].strip() == NOT_ACCEPTANCE_ATTR
    ) and not any(NOT_ACCEPTANCE_ATTR in line and "ManagerExt" in line for line in lines):
        problems.append("ManagerExt 导入必须按 not(acceptance) 门控（普通构建真实插件可用）")
    return problems


class AutostartCommandRoutingCase(unittest.TestCase):
    def test_real_source_satisfies_the_routing_contract(self):
        problems = check_routing(load_lines())
        self.assertEqual(problems, [])

    def test_contract_detects_gate_removal(self):
        lines = load_lines()
        span = fn_span(lines, "autostart_status")
        gate = index_of(lines, span, "privacy::require_ready(")
        del lines[gate]
        problems = check_routing(lines)
        self.assertTrue(
            any("privacy::require_ready" in p or "闸门" in p for p in problems),
            problems,
        )

    def test_contract_detects_shim_outside_acceptance_cfg(self):
        lines = load_lines()
        span = fn_span(lines, "autostart_status")
        acc = index_of(lines, span, ACCEPTANCE_ATTR)
        del lines[acc]
        self.assertTrue(any("不在 acceptance 区域" in p for p in check_routing(lines)))

    def test_contract_detects_real_plugin_call_in_acceptance_region(self):
        lines = load_lines()
        span = fn_span(lines, "autostart_set")
        acc = index_of(lines, span, ACCEPTANCE_ATTR)
        lines.insert(acc + 1, "            let _leak = app.autolaunch();")
        self.assertTrue(any("acceptance 区域" in p for p in check_routing(lines)))


if __name__ == "__main__":
    unittest.main()
