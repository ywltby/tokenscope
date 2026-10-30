<#
.SYNOPSIS
    Read the Windows notification-area toolbars and count visible + overflow icons.

.DESCRIPTION
    AC10 (N12 single instance) needs an externally observable tray count; a
    process count is NOT accepted as tray evidence. This helper sends the
    classic toolbar messages (TB_BUTTONCOUNT / TB_GETBUTTONTEXTW) to the
    notification-area toolbars and returns a JSON object with the icon counts
    and every text that contains "TokenScope".

    - Visible icons: Shell_TrayWnd -> TrayNotifyWnd -> SysPager -> ToolbarWindow32
      (falls back to TrayNotifyWnd -> ToolbarWindow32).
    - Hidden icons: NotifyIconOverflowWindow -> ToolbarWindow32.

    If a toolbar cannot be read (for example a Windows 11 shell without the
    classic toolbar), ok=false and the caller must mark the observation as
    pending instead of guessing. ASCII-only source so Windows PowerShell 5.1
    parses it without a BOM.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

if (-not ('TsTray' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TsTray {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] public static extern IntPtr VirtualAllocEx(IntPtr proc, IntPtr addr, UIntPtr size, uint type, uint protect);
    [DllImport("kernel32.dll")] public static extern bool VirtualFreeEx(IntPtr proc, IntPtr addr, UIntPtr size, uint type);
    [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr proc, IntPtr addr, byte[] buf, UIntPtr size, out UIntPtr read);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
    public const uint TB_BUTTONCOUNT = 0x0418;
    public const uint TB_GETBUTTONTEXTW = 0x044B;
    public const uint PROCESS_VM_OPERATION = 0x0008;
    public const uint PROCESS_VM_READ = 0x0010;
    public const uint PROCESS_VM_WRITE = 0x0020;
    public const uint PROCESS_QUERY_INFORMATION = 0x0400;
    public const uint MEM_COMMIT = 0x1000;
    public const uint MEM_RESERVE = 0x2000;
    public const uint MEM_RELEASE = 0x8000;
    public const uint PAGE_READWRITE = 0x04;
    public static IntPtr Toolbar(IntPtr root, string[] chain) {
        IntPtr cur = root;
        foreach (var cls in chain) {
            cur = FindWindowEx(cur, IntPtr.Zero, cls, null);
            if (cur == IntPtr.Zero) return IntPtr.Zero;
        }
        return cur;
    }
    public static string[] Texts(IntPtr toolbar) {
        uint pid;
        GetWindowThreadProcessId(toolbar, out pid);
        IntPtr proc = OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION, false, pid);
        if (proc == IntPtr.Zero) throw new Exception("OpenProcess failed for explorer pid " + pid);
        IntPtr mem = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)2048, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (mem == IntPtr.Zero) { CloseHandle(proc); throw new Exception("VirtualAllocEx failed"); }
        try {
            int count = (int)(long)SendMessage(toolbar, TB_BUTTONCOUNT, IntPtr.Zero, IntPtr.Zero);
            var result = new string[count];
            for (int i = 0; i < count; i++) {
                IntPtr unused;
                SendMessageTimeout(toolbar, TB_GETBUTTONTEXTW, (IntPtr)i, mem, 0x0002, 2000, out unused);
                byte[] buf = new byte[2048];
                UIntPtr read;
                if (ReadProcessMemory(proc, mem, buf, (UIntPtr)buf.Length, out read) && (long)read >= 2) {
                    int len = 0;
                    while (len + 1 < buf.Length && (buf[len] != 0 || buf[len + 1] != 0)) len += 2;
                    result[i] = Encoding.Unicode.GetString(buf, 0, len);
                } else {
                    result[i] = "";
                }
            }
            return result;
        } finally {
            VirtualFreeEx(proc, mem, UIntPtr.Zero, MEM_RELEASE);
            CloseHandle(proc);
        }
    }
}
'@
}

function Read-Area([string]$area, [IntPtr]$toolbar, [string]$missing) {
    if ($toolbar -eq [IntPtr]::Zero) {
        return [pscustomobject]@{
            area = $area
            present = $false
            count = $null
            texts = @()
            error = $missing
        }
    }
    try {
        $texts = @([TsTray]::Texts($toolbar))
        return [pscustomobject]@{
            area = $area
            present = $true
            count = $texts.Count
            texts = $texts
            error = $null
        }
    } catch {
        return [pscustomobject]@{
            area = $area
            present = $true
            count = $null
            texts = @()
            error = $_.Exception.Message
        }
    }
}

$shell = [TsTray]::FindWindow('Shell_TrayWnd', $null)
$visibleToolbar = [IntPtr]::Zero
$visibleError = 'Shell_TrayWnd not found'
if ($shell -ne [IntPtr]::Zero) {
    $visibleToolbar = [TsTray]::Toolbar($shell, @('TrayNotifyWnd', 'SysPager', 'ToolbarWindow32'))
    if ($visibleToolbar -eq [IntPtr]::Zero) {
        $visibleToolbar = [TsTray]::Toolbar($shell, @('TrayNotifyWnd', 'ToolbarWindow32'))
    }
    if ($visibleToolbar -eq [IntPtr]::Zero) { $visibleError = 'visible tray ToolbarWindow32 not found' }
    else { $visibleError = $null }
}

$overflowWindow = [TsTray]::FindWindow('NotifyIconOverflowWindow', $null)
$overflowToolbar = [IntPtr]::Zero
$overflowPresent = $overflowWindow -ne [IntPtr]::Zero
$overflowError = $null
if ($overflowPresent) {
    $overflowToolbar = [TsTray]::Toolbar($overflowWindow, @('ToolbarWindow32'))
    if ($overflowToolbar -eq [IntPtr]::Zero) { $overflowError = 'overflow ToolbarWindow32 not found' }
}

$overflowMessage = $null
if ($overflowPresent) { $overflowMessage = $overflowError }
$visible = Read-Area 'visible' $visibleToolbar $visibleError
if ($overflowPresent) {
    $overflow = Read-Area 'overflow' $overflowToolbar $overflowMessage
} else {
    $overflow = [pscustomobject]@{
        area = 'overflow'
        present = $false
        count = 0
        texts = @()
        error = $null
    }
}

$matches = @()
foreach ($area in @($visible, $overflow)) {
    foreach ($text in @($area.texts)) {
        if ("$text" -match 'TokenScope') {
            $matches += [pscustomobject]@{ area = $area.area; text = "$text" }
        }
    }
}

$ok = ($null -eq $visible.error) -and ($null -eq $overflow.error)
$errors = @($visible.error, $overflow.error) | Where-Object { $_ }
$visibleCount = if ($null -ne $visible.count) { [int]$visible.count } else { $null }
$overflowCount = if ($null -ne $overflow.count) { [int]$overflow.count } else { $null }
$totalCount = if ($null -ne $visibleCount -and $null -ne $overflowCount) { [int]($visibleCount + $overflowCount) } else { $null }

[pscustomobject]@{
    ok = $ok
    error = if ($errors.Count -gt 0) { $errors -join '; ' } else { $null }
    visible_count = $visibleCount
    overflow_count = $overflowCount
    total_count = $totalCount
    token_scope_count = $matches.Count
    matches = $matches
    visible_toolbar = [string]$visibleToolbar
    overflow_toolbar = [string]$overflowToolbar
    overflow_present = $overflowPresent
} | ConvertTo-Json -Depth 4 -Compress
