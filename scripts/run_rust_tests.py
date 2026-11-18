"""在独立临时根运行 Rust 测试，子进程退出后严格检查清理结果。

进程退出会释放测试全局查询表中的 SQLite 读快照，因此不依赖每个用例都能在
函数末尾删除仍被打开的文件。仅删除本次创建的目录，不扫描或清扫历史 Temp。
测试失败/中断时保留现场；清理失败同样返回非零，不把测试通过当作收尾成功。
"""

import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

# IFileOperation 操作标志（shobjidl_core.h / ShObjIdl.h）。FOFX_* 是 32 位扩展
# 标志，必须通过 IFileOperation::SetOperationFlags 设置；已废弃的
# SHFileOperationW 只认 16 位 FOF_*，无法表达「必须回收」的意图。
_FOF_NOERRORUI = 0x0400
_FOF_SILENT = 0x0004
_FOFX_EARLYFAILURE = 0x00100000
_FOFX_RECYCLEONDELETE = 0x00080000
# FOFX_RECYCLEONDELETE = "send it to the Recycle Bin rather than permanently
# deleting it"：接口不支持或回收被禁用时操作失败，绝不回退永久删除。
# FOFX_EARLYFAILURE 与 FOF_NOERRORUI 同设时任一子操作出错即整体停止并报错。
# 刻意不含 FOF_ALLOWUNDO（只是 "if possible"，可静默降级成永久删除），也不含
# "不显示确认框即继续删除" 的语义；取消/中止由 GetAnyOperationsAborted 反映。
_RECYCLE_FLAGS = (
    _FOFX_RECYCLEONDELETE | _FOFX_EARLYFAILURE | _FOF_NOERRORUI | _FOF_SILENT
)

_COINIT_APARTMENTTHREADED = 0x00000002  # IFileOperation 只能用于 STA
_CLSCTX_ALL = 0x00000017

# 已知 GUID（shobjidl_core.h），内存布局同 Windows GUID 结构。
_CLSID_FILE_OPERATION = (
    0x3AD05575, 0x8857, 0x4850, (0x92, 0x77, 0x11, 0xB8, 0x5B, 0xDB, 0x8E, 0x09)
)
_IID_IFILE_OPERATION = (
    0x947AAB5F, 0x0A5C, 0x4C13, (0xB4, 0xD6, 0x4B, 0xF7, 0x83, 0x6F, 0xC9, 0xF8)
)
_IID_ISHELL_ITEM = (
    0x43826D1E, 0xE718, 0x42EE, (0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE)
)

# IFileOperation vtable 槽位（IUnknown 0-2 之后按 shobjidl_core.h 声明顺序）。
_VTBL_RELEASE = 2
_VTBL_SET_OPERATION_FLAGS = 5
_VTBL_DELETE_ITEM = 18
_VTBL_PERFORM_OPERATIONS = 21
_VTBL_GET_ANY_OPERATIONS_ABORTED = 22
# IShellItem::Release（IUnknown 继承）。
_ISHELL_ITEM_VTBL_RELEASE = 2

_S_FALSE = 0x00000001  # PerformOperations 未执行任何操作（HRESULT，非失败）


class _Guid(ctypes.Structure):
    _fields_ = [
        ("Data1", wintypes.DWORD),
        ("Data2", wintypes.WORD),
        ("Data3", wintypes.WORD),
        ("Data4", ctypes.c_ubyte * 8),
    ]


def _make_guid(spec):
    d1, d2, d3, tail = spec
    return _Guid(d1, d2, d3, (ctypes.c_ubyte * 8)(*tail))


def _hr(value):
    """HRESULT 按无符号 32 位比较；失败码最高位为 1。"""
    return value & 0xFFFFFFFF


def _failed(value):
    return _hr(value) >= 0x80000000


def _vtable_method(ptr, index, restype, argtypes):
    """从 COM 接口指针取出第 index 个 vtable 方法。"""
    vtable = ctypes.cast(ctypes.cast(ptr, ctypes.POINTER(ctypes.c_void_p))[0],
                         ctypes.POINTER(ctypes.c_void_p))
    return ctypes.WINFUNCTYPE(restype, *argtypes)(vtable[index])


def _release(ptr, slot=_VTBL_RELEASE):
    if not ptr:
        return
    release = _vtable_method(ptr, slot, wintypes.ULONG, [ctypes.c_void_p])
    release(ptr)


def _default_file_operation(path, flags):
    """真实 IFileOperation 回收路径。仅做显式回收，不删除、不改名、不搬运。

    返回 (perform_hr, aborted)。任何前置步骤失败都以 OSError 上抛，由调用方
    保留现场并返回非零——绝不在失败时改用 DeleteFile / rmtree 等永久删除。
    """
    ole32 = ctypes.windll.ole32
    shell32 = ctypes.windll.shell32

    apartment = _hr(ole32.CoInitializeEx(None, _COINIT_APARTMENTTHREADED))
    # S_OK=0；S_FALSE=1 表示线程已在该套间初始化（仍是有效 STA，可用）。
    # 两者都是成功调用：每次成功调用都会增加线程的 COM 初始化计数，必须各自
    # 配对一次 CoUninitialize（微软 CoInitializeEx 文档）。
    if apartment not in (0x00000000, _S_FALSE):
        raise OSError(f"CoInitializeEx(STA) 失败 0x{apartment:08x}")

    op = ctypes.c_void_p()
    item = ctypes.c_void_p()
    try:
        clsid = _make_guid(_CLSID_FILE_OPERATION)
        iid_op = _make_guid(_IID_IFILE_OPERATION)
        hr = _hr(ole32.CoCreateInstance(
            ctypes.byref(clsid), None, _CLSCTX_ALL,
            ctypes.byref(iid_op), ctypes.byref(op)))
        if _failed(hr):
            raise OSError(f"CoCreateInstance(FileOperation) 失败 0x{hr:08x}")

        iid_item = _make_guid(_IID_ISHELL_ITEM)
        hr = _hr(shell32.SHCreateItemFromParsingName(
            ctypes.c_wchar_p(path), None, ctypes.byref(iid_item),
            ctypes.byref(item)))
        if _failed(hr):
            raise OSError(f"SHCreateItemFromParsingName 失败 0x{hr:08x}")

        set_flags = _vtable_method(
            op, _VTBL_SET_OPERATION_FLAGS, wintypes.LONG,
            [ctypes.c_void_p, wintypes.DWORD])
        hr = _hr(set_flags(op, flags))
        if _failed(hr):
            raise OSError(
                f"SetOperationFlags(FOFX_RECYCLEONDELETE) 失败 0x{hr:08x}")

        delete_item = _vtable_method(
            op, _VTBL_DELETE_ITEM, wintypes.LONG,
            [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p])
        # pfopsItem=NULL：取消仍由 GetAnyOperationsAborted 反映。
        hr = _hr(delete_item(op, item, None))
        if _failed(hr):
            raise OSError(f"DeleteItem(声明) 失败 0x{hr:08x}")

        perform = _vtable_method(
            op, _VTBL_PERFORM_OPERATIONS, wintypes.LONG, [ctypes.c_void_p])
        perform_hr = _hr(perform(op))

        aborted = wintypes.BOOL(False)
        get_aborted = _vtable_method(
            op, _VTBL_GET_ANY_OPERATIONS_ABORTED, wintypes.LONG,
            [ctypes.c_void_p, ctypes.POINTER(wintypes.BOOL)])
        hr = _hr(get_aborted(op, ctypes.byref(aborted)))
        if _failed(hr):
            raise OSError(f"GetAnyOperationsAborted 失败 0x{hr:08x}")
        return perform_hr, bool(aborted.value)
    finally:
        _release(item, _ISHELL_ITEM_VTBL_RELEASE)
        _release(op)
        # 成功的 CoInitializeEx（S_OK 或 S_FALSE）都要配对一次 CoUninitialize。
        # S_FALSE 只说明本次调用之前线程已在同一套间初始化过，不代表这次调用
        # 不需要释放：初始化计数已经加一，少释放一次就会泄漏。
        if apartment in (0x00000000, _S_FALSE):
            ole32.CoUninitialize()


def _send_to_recycle_bin(path, *, _file_operation=None):
    """把整个路径显式送入回收站。返回 None 表示成功，否则返回错误说明。

    走 IFileOperation + FOFX_RECYCLEONDELETE：明确要求回收，接口不支持或回收
    被禁用时操作失败而非永久删除。任何失败、取消、中止都返回非零说明并保留
    源内容，由调用方拒绝把门禁标记为成功。`_file_operation` 仅供测试注入模拟
    接口失败/取消，不修改用户系统策略。
    """
    backend = _default_file_operation if _file_operation is None else _file_operation
    try:
        perform_hr, aborted = backend(path, _RECYCLE_FLAGS)
    except OSError as error:
        return str(error)
    if aborted:
        return "回收操作被取消或中止"
    if _failed(perform_hr):
        return f"PerformOperations 回收失败 0x{perform_hr:08x}"
    if _hr(perform_hr) == _S_FALSE:
        return "PerformOperations 未执行任何操作（可能无可回收项）"
    return None


def _recycle_bin_holds(path):
    """附加证据：核对回收站里是否确有该路径的 `$I` 元数据条目。

    仅作为主路径（IFileOperation 明确要求回收）成功后的**附加**核验：读不到
    任何回收站目录返回 None（无法确认即不算成功），找到了返回 True。主保护
    来自 FOFX_RECYCLEONDELETE，本函数不承担「禁止永久删除」的唯一职责。
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


def _has_overlong_entry(root, limit=240):
    """临时根内部是否存在超出回收接口能力的路径（调用 Shell 前探测）。

    命中就直接拒绝、现场一点不动，避免任何 Shell 删除操作做到一半再失败。
    枚举出的**每个条目（含普通文件）**都按完整路径长度检查；不跟随链接进入
    外部目标（链接/junction 只按其自身路径判断，不展开）。枚举本身失败
    （例如已无法访问的超长目录）同样按命中处理。
    """
    stack = [str(root)]
    while stack:
        current = stack.pop()
        if len(current) > limit:
            return True
        try:
            with os.scandir(current) as entries:
                for entry in entries:
                    path = entry.path
                    if len(path) > limit:
                        return True
                    try:
                        if entry.is_dir(follow_symlinks=False):
                            stack.append(path)
                    except OSError:
                        return True
        except OSError:
            return True
    return False


def _cleanup(root, parent, *, _file_operation=None):
    # 删除前重新核对绝对边界与根的类型，拒绝被替换成链接/联接的根。
    if (root.parent != parent or root.is_symlink() or root.is_junction()
            or root.resolve() != root):
        raise OSError(f"拒绝清理越界或被替换的临时根：{root}")
    if not root.exists():
        return
    if os.name != "nt":
        # 本仓清理约定只允许「送入回收站」这一种可逆路径；非 Windows 无法
        # 满足时不永久删除，保留现场并让门禁失败（CI 一律 Windows）。
        raise OSError(f"非 Windows 平台无法保证回收站语义，保留现场：{root}")
    # 显式回收：不依赖宿主对 Temp 的放行策略，也不以任何形式的删除作为回退。
    if _has_overlong_entry(root):
        raise OSError(
            f"临时根内含超过回收接口能力的路径，未做任何删除，保留现场：{root}")
    problem = _send_to_recycle_bin(str(root), _file_operation=_file_operation)
    if problem is not None:
        raise OSError(f"送入回收站失败（{problem}），保留现场：{root}")
    if root.exists():
        raise OSError(f"回收后临时根仍存在，保留现场：{root}")
    # 附加证据：原路径消失不等于已回收，还要在回收站里核实 `$I` 条目。
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
