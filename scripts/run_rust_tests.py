"""在独立临时根运行 Rust 测试，子进程退出后严格检查清理结果。

进程退出会释放测试全局查询表中的 SQLite 读快照，因此不依赖每个用例都能在
函数末尾删除仍被打开的文件。仅删除本次创建的目录，不扫描或清扫历史 Temp。
测试失败/中断时保留现场；清理失败同样返回非零，不把测试通过当作收尾成功。
"""

import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

_FO_DELETE = 0x0003
_FOF_ALLOWUNDO = 0x0040
_FOF_NOCONFIRMATION = 0x0010
_FOF_NOERRORUI = 0x0400
_FOF_SILENT = 0x0004


class _ShFileOpStructW(ctypes.Structure):
    _fields_ = [
        ("hwnd", wintypes.HWND),
        ("wFunc", ctypes.c_uint),
        ("pFrom", ctypes.c_wchar_p),
        ("pTo", ctypes.c_wchar_p),
        ("fFlags", ctypes.c_ushort),
        ("fAnyOperationsAborted", wintypes.BOOL),
        ("hNameMappings", ctypes.c_void_p),
        ("lpszProgressTitle", ctypes.c_wchar_p),
    ]




def _recycle_bin_holds(path):
    """核对回收站里是否确有该路径的条目（Vista+ 的 `$I` 元数据）。

    返回 True/False；一个回收站目录都读不到时返回 None —— 无法确认就不能算
    成功。这条核对用来发现「FOF_ALLOWUNDO 被组策略降级成永久删除」这种退化。
    """
    absolute = os.path.abspath(path)
    drive = os.path.splitdrive(absolute)[0]
    base = os.path.join(drive + os.sep, "$Recycle.Bin")
    target = os.path.normcase(absolute)
    cutoff = time.time() - 600
    try:
        bins = [entry.path for entry in os.scandir(base) if entry.is_dir(follow_symlinks=False)]
    except OSError:
        return None
    readable = False
    for bin_dir in bins:
        try:
            entries = list(os.scandir(bin_dir))
        except OSError:
            continue
        readable = True
        for entry in entries:
            if not entry.name.startswith("$I"):
                continue
            try:
                if entry.stat().st_mtime < cutoff:
                    continue
            except OSError:
                continue
            try:
                with open(entry.path, "rb") as handle:
                    header = handle.read(28)
                    if len(header) < 28:
                        continue
                    if int.from_bytes(header[0:8], "little") != 2:
                        continue
                    length = int.from_bytes(header[24:28], "little")
                    if length <= 0 or length > 32768:
                        continue
                    raw = handle.read(length * 2)
                    original = raw.decode("utf-16-le").rstrip("\x00")
            except (OSError, UnicodeDecodeError):
                continue
            if os.path.normcase(original) == target:
                return True
    return False if readable else None


def _send_to_recycle_bin(path):
    """把整个路径送入回收站（FOF_ALLOWUNDO）。返回 None 表示调用成功。

    只做显式回收：不删除、不改名、不搬运。回收站被组策略禁用时
    SHFileOperationW 会静默退化成永久删除，调用方必须另行核对回收站条目
    才能发现这种退化。
    """
    buf = ctypes.create_unicode_buffer(path, len(path) + 2)  # pFrom 需双 null 结尾
    op = _ShFileOpStructW()
    op.hwnd = None
    op.wFunc = _FO_DELETE
    op.pFrom = ctypes.cast(buf, ctypes.c_wchar_p)
    op.pTo = None
    op.fFlags = _FOF_ALLOWUNDO | _FOF_NOCONFIRMATION | _FOF_NOERRORUI | _FOF_SILENT
    result = ctypes.windll.shell32.SHFileOperationW(ctypes.byref(op))
    if result != 0:
        return f"SHFileOperationW 返回 0x{result:04x}"
    if op.fAnyOperationsAborted:
        return "回收操作被中止"
    return None


def _has_overlong_entry(root, limit=240):
    """临时根内部是否存在超出回收站接口能力的路径。

    SHFileOperationW 遇到超长路径会**先删掉一部分再失败**，做不到「失败即保持
    原样」。所以在调用前先探测：命中就直接拒绝，现场一点不动。
    枚举本身失败（例如已经无法访问的超长目录）同样按命中处理。
    """
    stack = [str(root)]
    while stack:
        current = stack.pop()
        if len(current) > limit:
            return True
        try:
            with os.scandir(current) as entries:
                for entry in entries:
                    try:
                        if entry.is_dir(follow_symlinks=False):
                            stack.append(entry.path)
                    except OSError:
                        return True
        except OSError:
            return True
    return False


def _cleanup(root, parent):
    # 删除前重新核对绝对边界与根的类型，拒绝被替换成链接/联接的根。
    if (root.parent != parent or root.is_symlink() or root.is_junction()
            or root.resolve() != root):
        raise OSError(f"拒绝清理越界或被替换的临时根：{root}")
    if not root.exists():
        return
    if os.name != "nt":
        shutil.rmtree(str(root))
        if root.exists():
            raise OSError(f"清理后临时根仍存在：{root}")
        return
    # 显式回收：不依赖宿主对 Temp 的放行策略，也不以任何形式的删除作为回退。
    if _has_overlong_entry(root):
        raise OSError(
            f"临时根内含超过回收站接口能力的路径，未做任何删除，保留现场：{root}")
    problem = _send_to_recycle_bin(str(root))
    if problem is not None:
        raise OSError(f"送入回收站失败（{problem}），保留现场：{root}")
    if root.exists():
        raise OSError(f"回收后临时根仍存在，保留现场：{root}")
    # 原路径消失不等于已回收：还要在回收站里找到对应条目。
    held = _recycle_bin_holds(str(root))
    if held is None:
        raise OSError(f"无法核对回收站条目，不能确认已回收：{root}")
    if not held:
        raise OSError(f"回收站中没有该路径的条目，可能已被永久删除：{root}")


def run(command, *, cwd, temp_parent=None):
    parent = Path(temp_parent or tempfile.gettempdir()).resolve()
    root = Path(tempfile.mkdtemp(prefix="tokenscope-test-", dir=parent))
    env = dict(os.environ)
    env.update({key: str(root) for key in ("TMP", "TEMP", "TMPDIR")})
    print(f"[rust-tests] 本轮临时根：{root}", file=sys.stderr, flush=True)
    try:
        status = subprocess.run(command, cwd=cwd, env=env, check=False).returncode
    except KeyboardInterrupt:
        print(f"[rust-tests] 运行中断，保留临时根：{root}", file=sys.stderr)
        return 130
    except OSError as error:
        print(f"[rust-tests] 启动失败：{error}；保留临时根：{root}", file=sys.stderr)
        return 127
    if status != 0:
        print(f"[rust-tests] 测试退出码 {status}，保留临时根：{root}", file=sys.stderr)
        return status
    try:
        _cleanup(root, parent)
    except OSError as error:
        print(f"[rust-tests] 测试已通过但回收失败：{error}；保留现场：{root}",
              file=sys.stderr)
        return 1
    print(f"[rust-tests] 回收已核实（临时根已进入回收站）：{root}",
          file=sys.stderr, flush=True)
    return 0


def main(argv=None):
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    command = list(sys.argv[1:] if argv is None else argv)
    if command[:1] == ["--"]:
        command.pop(0)
    if command[:2] != ["cargo", "test"]:
        print("用法：python -B scripts/run_rust_tests.py -- cargo test <原有参数>",
              file=sys.stderr)
        return 2
    return run(command, cwd=Path.cwd())


if __name__ == "__main__":
    sys.exit(main())
