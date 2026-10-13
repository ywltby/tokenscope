<#
.SYNOPSIS
    Native acceptance window helper (RC11): read the real window rect or ask
    the window to close, from outside the application.

.DESCRIPTION
    -Action rect  -> prints "PID <id> RECT x,y,w,h" (physical pixels)
    -Action close -> posts WM_CLOSE to the window (the same request the title
                     bar button makes), prints "CLOSED true|false"

    Only the process resolved by -ProcessId (or -Name) is touched. ASCII-only
    source so Windows PowerShell 5.1 parses it without a BOM.
#>
[CmdletBinding()]
param(
    [int]$ProcessId = 0,
    [string]$Name = 'tokenscope',
    [ValidateSet('rect', 'close', 'mainrect', 'dpi', 'minimize', 'restore', 'geometry', 'resize')]
    [string]$Action = 'rect',
    [int]$Width = 0,
    [int]$Height = 0
)

$ErrorActionPreference = 'Stop'
if (-not ('TsActWin' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TsActWin {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
    public static string Geometry(int pid) {
        IntPtr h = Find(pid); RECT r;
        if (h == IntPtr.Zero || !GetClientRect(h, out r)) throw new Exception("Missing owned window");
        return string.Format("{{\"width\":{0},\"height\":{1},\"dpi\":{2}}}", r.Right-r.Left, r.Bottom-r.Top, GetDpiForWindow(h));
    }
    public static bool Resize(int pid, int width, int height) {
        IntPtr h = Find(pid); RECT outer, client;
        if (h == IntPtr.Zero || width <= 0 || height <= 0) return false;
        ShowWindowAsync(h, 9);
        if (!GetWindowRect(h, out outer) || !GetClientRect(h, out client)) return false;
        return SetWindowPos(h, IntPtr.Zero, 0, 0,
            width + (outer.Right-outer.Left) - (client.Right-client.Left),
            height + (outer.Bottom-outer.Top) - (client.Bottom-client.Top), 0x0006);
    }
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    public static bool ChangeState(int pid, bool minimized) {
        IntPtr h = Find(pid);
        if (h == IntPtr.Zero) return false;
        ShowWindowAsync(h, minimized ? 6 : 9);
        for (int i = 0; i < 40 && IsIconic(h) != minimized; i++) System.Threading.Thread.Sleep(50);
        if (!minimized) SetForegroundWindow(h);
        return IsIconic(h) == minimized;
    }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    static IntPtr _found = IntPtr.Zero;
    public static IntPtr Find(int pid) {
        _found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint wp; GetWindowThreadProcessId(h, out wp);
            if ((int)wp == pid && IsWindowVisible(h) && GetParent(h) == IntPtr.Zero) { _found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return _found;
    }
    public static string RectString(int pid) {
        IntPtr h = Find(pid);
        if (h == IntPtr.Zero) return "none";
        RECT r; if (!GetWindowRect(h, out r)) return "none";
        return string.Format("{0},{1},{2},{3}", r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top);
    }
    public static string LargestRect(int pid) {
        int bestW = 0, bestH = 0, bestX = 0, bestY = 0, bestArea = 0, count = 0;
        EnumWindows((h, l) => {
            uint wp; GetWindowThreadProcessId(h, out wp);
            if ((int)wp != pid || !IsWindowVisible(h) || GetParent(h) != IntPtr.Zero) return true;
            RECT r; if (!GetWindowRect(h, out r)) return true;
            int w = r.Right - r.Left, hh = r.Bottom - r.Top;
            count++;
            if (w * hh > bestArea) { bestArea = w * hh; bestW = w; bestH = hh; bestX = r.Left; bestY = r.Top; }
            return true;
        }, IntPtr.Zero);
        if (count == 0) return "none count=0";
        return string.Format("{0},{1},{2},{3} count={4}", bestX, bestY, bestW, bestH, count);
    }
    public static bool Close(int pid) {
        IntPtr h = Find(pid);
        if (h == IntPtr.Zero) return false;
        const uint WM_CLOSE = 0x0010;
        return PostMessage(h, WM_CLOSE, IntPtr.Zero, IntPtr.Zero);
    }
}
'@
}
[void][TsActWin]::SetProcessDPIAware()

$target = $ProcessId
if ($Action -in @('minimize', 'restore', 'resize', 'geometry') -and $target -le 0) {
    throw 'State changes require an explicit owned ProcessId'
}
if ($target -le 0) {
    $cand = @(Get-Process -Name $Name -ErrorAction SilentlyContinue | Sort-Object StartTime -Descending)
    if ($cand.Count -eq 0) { Write-Output "PID none"; exit 4 }
    $target = $cand[0].Id
}
if ($Action -eq 'geometry') {
    Write-Output ([TsActWin]::Geometry($target))
}
elseif ($Action -eq 'resize') {
    if (-not [TsActWin]::Resize($target, $Width, $Height)) { throw 'Native resize failed' }
    Write-Output ([TsActWin]::Geometry($target))
}
elseif ($Action -in @('minimize', 'restore')) {
    $changed = [TsActWin]::ChangeState($target, $Action -eq 'minimize')
    Write-Output ("PID {0} STATE_CHANGED {1}" -f $target, $changed)
    if (-not $changed) { exit 5 }
}
elseif ($Action -eq 'dpi') {
    Write-Output ("PID {0} DPI {1}" -f $target, [TsActWin]::GetDpiForWindow([TsActWin]::Find($target)))
}
elseif ($Action -eq 'mainrect') {
    Write-Output ("PID {0} MAINRECT {1}" -f $target, [TsActWin]::LargestRect($target))
}
elseif ($Action -eq 'rect') {
    Write-Output ("PID {0} RECT {1}" -f $target, [TsActWin]::RectString($target))
}
else {
    Write-Output ("PID {0} CLOSED {1}" -f $target, [TsActWin]::Close($target))
}
