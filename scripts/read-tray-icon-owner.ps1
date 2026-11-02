# read-tray-icon-owner.ps1
#
# 只读探测：给定进程 PID，列出该进程通过 Shell_NotifyIcon 注册的托盘图标。
#
# 为什么需要它：Explorer 工具栏的按钮文本只含 tooltip（"TokenScope"），无法区分
# 正式实例与隔离验收实例。托盘图标在注册时绑定 (hwnd, uID)：本仓使用的
# tray-icon 0.25.1 为每个图标创建类名 "tray_icon_app" 的隐藏窗口，uID 为进程内
# 自增计数器。用 Shell_NotifyIconGetRect 对 (hwnd, uID) 探测即可确认：
#   - 图标是否属于目标进程（hwnd 归属该 PID）；
#   - 该进程注册了几个图标（唯一性）。
# 本脚本不读 Explorer 工具栏、不发消息、不点击、不修改任何状态。
#
# 输出：单行 JSON
#   { ok, pid, windows, count, icons: [{ hwnd, uid, left, top, right, bottom }],
#     error, failure_line }
# ok=false 表示探测本身不可用（进程不存在/API 失败），count 不做结论。
#
# 实现说明：探测与 JSON 组装全部在 C# 静态方法里完成，PowerShell 只传 PID 并
# 输出返回的字符串——结构体 ref/out 参数在 PowerShell 里直接绑定会抛
# "Argument types do not match"，放在 C# 内可彻底避免这类绑定问题。

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [int]$MaxUid = 64
)

$ErrorActionPreference = 'Stop'

if (-not ('TokenScopeTrayOwner.Probe' -as [type])) {
    $typeSource = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace TokenScopeTrayOwner {
    [StructLayout(LayoutKind.Sequential)]
    internal struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct NOTIFYICONIDENTIFIER {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uID;
        public Guid guidItem;
    }

    internal static class Native {
        [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string className, string windowName);

        [DllImport("user32.dll", SetLastError = true)]
        public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

        [DllImport("shell32.dll", SetLastError = false)]
        public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER identifier, out RECT iconLocation);
    }

    public static class Probe {
        public static string ProbeProcess(int pid, int maxUid) {
            var sb = new StringBuilder();
            bool ok = false;
            int windows = 0;
            string error = null;
            var icons = new List<string>();
            try {
                using (var process = System.Diagnostics.Process.GetProcessById(pid)) {
                    if (process.HasExited) throw new InvalidOperationException("process-exited");
                }
                var hwnds = new List<IntPtr>();
                IntPtr after = IntPtr.Zero;
                while (true) {
                    IntPtr hwnd = Native.FindWindowExW(IntPtr.Zero, after, "tray_icon_app", "");
                    if (hwnd == IntPtr.Zero) break;
                    after = hwnd;
                    uint ownerPid;
                    Native.GetWindowThreadProcessId(hwnd, out ownerPid);
                    if (ownerPid == (uint)pid) hwnds.Add(hwnd);
                }
                windows = hwnds.Count;
                foreach (IntPtr hwnd in hwnds) {
                    for (int uid = 0; uid <= maxUid; uid++) {
                        var ident = new NOTIFYICONIDENTIFIER();
                        ident.cbSize = (uint)Marshal.SizeOf(typeof(NOTIFYICONIDENTIFIER));
                        ident.hWnd = hwnd;
                        ident.uID = (uint)uid;
                        ident.guidItem = Guid.Empty;
                        RECT rect;
                        int hr = Native.Shell_NotifyIconGetRect(ref ident, out rect);
                        if (hr == 0) {
                            icons.Add("{\"hwnd\":\"" + hwnd.ToInt64() + "\",\"uid\":" + uid +
                                ",\"left\":" + rect.Left + ",\"top\":" + rect.Top +
                                ",\"right\":" + rect.Right + ",\"bottom\":" + rect.Bottom + "}");
                        }
                    }
                }
                ok = true;
            } catch (Exception ex) {
                error = ex.Message;
            }
            sb.Append("{\"ok\":").Append(ok ? "true" : "false");
            sb.Append(",\"pid\":").Append(pid);
            sb.Append(",\"windows\":").Append(windows);
            sb.Append(",\"count\":").Append(icons.Count);
            sb.Append(",\"icons\":[").Append(string.Join(",", icons)).Append("]");
            sb.Append(",\"error\":");
            sb.Append(error == null ? "null" : "\"" + error.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"");
            sb.Append(",\"failure_line\":null}");
            return sb.ToString();
        }
    }
}
'@
    Add-Type -TypeDefinition $typeSource
}

[TokenScopeTrayOwner.Probe]::ProbeProcess($ProcessId, $MaxUid)
