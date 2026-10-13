# Controlled windows owned by this test process; no user application is touched.
#Requires -Version 7
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class WindowFixture {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern IntPtr CreateWindowEx(int ex, string cls, string title, uint style,
        int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
}
'@
[void][WindowFixture]::SetProcessDPIAware()
$mainWindow = [WindowFixture]::CreateWindowEx(0, 'STATIC', 'TokenScope', 0x10CF0000, 0, 0, 400, 300, 0, 0, 0, 0)
$auxWindow = [WindowFixture]::CreateWindowEx(0, 'STATIC', 'Auxiliary fixture', 0x10000000, 0, 0, 0, 0, 0, 0, 0, 0)
try {
    if ($mainWindow -eq 0 -or $auxWindow -eq 0) { throw 'Could not create fixture windows' }
    [void][WindowFixture]::SetWindowPos($auxWindow, [IntPtr]::Zero, 0, 0, 0, 0, 0x0013)
    $geometry = & "$PSScriptRoot/native-window-action.ps1" -ProcessId $PID -Action geometry | ConvertFrom-Json
    if ($geometry.width -le 0 -or $geometry.height -le 0) {
        throw "Selected auxiliary window instead of main: $($geometry | ConvertTo-Json -Compress)"
    }
    $resized = & "$PSScriptRoot/native-window-action.ps1" -ProcessId $PID -Action resize -Width 320 -Height 200 | ConvertFrom-Json
    if ($resized.width -ne 320 -or $resized.height -ne 200) { throw "Unexpected main geometry: $($resized | ConvertTo-Json -Compress)" }
    Write-Output 'PASS owned main window selected despite zero-size auxiliary window'
} finally {
    if ($auxWindow -ne 0) { [void][WindowFixture]::DestroyWindow($auxWindow) }
    if ($mainWindow -ne 0) { [void][WindowFixture]::DestroyWindow($mainWindow) }
}
