"""在独立临时根运行 Rust 测试，子进程退出后严格检查清理结果。

进程退出会释放测试全局查询表中的 SQLite 读快照，因此不依赖每个用例都能在
函数末尾删除仍被打开的文件。仅删除本次创建的目录，不扫描或清扫历史 Temp。
测试失败/中断时保留现场；清理失败同样返回非零，不把测试通过当作收尾成功。
"""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def _extended(path):
    """把路径规范成扩展长度形式，保证 `\\\\?\\` 只出现一次。

    普通绝对路径加普通前缀，UNC 走 `\\\\?\\UNC\\`，已带前缀的原样返回。
    """
    if path.startswith("\\\\?\\"):
        return path
    if path.startswith("\\\\"):
        return "\\\\?\\UNC\\" + path[2:]
    return "\\\\?\\" + path


def _path_too_long(error):
    """判断错误是否属于「普通路径受 MAX_PATH 限制」这一种。

    WinError 206 = ERROR_FILENAME_EXCED_RANGE；145 = ERROR_DIR_NOT_EMPTY，是
    长链中途删除失败后的收尾症状。123（保护层拒绝）、5/32（占用）、13（权限）
    都不属于这一类，必须原样抛出并保留现场。
    """
    codes = {error.errno, getattr(error, "winerror", None)}
    return bool(codes & {145, 206})


def _cleanup(root, parent):
    # 删除前重新核对绝对边界与根的类型，拒绝被替换成链接/联接的根。
    if (root.parent != parent or root.is_symlink() or root.is_junction()
            or root.resolve() != root):
        raise OSError(f"拒绝清理越界或被替换的临时根：{root}")
    if not root.exists():
        return
    # 先用普通路径：宿主删除保护层按绝对路径判定接管方式，扩展长度前缀会让
    # 该判定失效并被拒绝（ERROR_INVALID_NAME 123）。深目录由用例自身清理，
    # 这里只剩浅层路径与已释放句柄的数据库文件。
    try:
        shutil.rmtree(str(root))
    except OSError as error:
        if not _path_too_long(error):
            raise
        # 只有超长类失败才值得再试一次扩展长度路径，且只加一次前缀。
        # Python >= 3.12 的 rmtree 不进入目录符号链接/Windows junction 的目标；
        # 不忽略错误、不加等待补偿，仍删不掉就让门禁失败并保留现场。
        shutil.rmtree(_extended(str(root)))
    if root.exists():
        raise OSError(f"清理后临时根仍存在：{root}")


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
        print(f"[rust-tests] 测试已通过但清理失败：{error}；残留位置：{root}",
              file=sys.stderr)
        return 1
    print(f"[rust-tests] 清理已核实，临时根不存在：{root}", file=sys.stderr, flush=True)
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
