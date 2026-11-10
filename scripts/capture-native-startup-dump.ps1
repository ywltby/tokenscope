# Minimal stack/module dump for one identity-checked acceptance host. No full-memory dump.
#Requires -Version 7
param(
    [Parameter(Mandatory)][ValidateRange(1, 2147483647)][int]$TargetPid,
    [Parameter(Mandatory)][ValidateRange(1, 2147483647)][int]$ExpectedParentPid,
    [Parameter(Mandatory)][string]$ExpectedExe,
    [Parameter(Mandatory)][long]$StartedAfterMs,
    [Parameter(Mandatory)][long]$StartedBeforeMs,
    [Parameter(Mandatory)][string]$OutputPath
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
try {
    if (-not [IO.Path]::IsPathFullyQualified($OutputPath) -or
        [IO.Path]::GetFileName($OutputPath) -notmatch '^startup-\d+\.dmp$') {
        throw 'Invalid startup dump output path'
    }
    $owner = Get-CimInstance Win32_Process -Filter "ProcessId = $TargetPid" -ErrorAction Stop
    if (-not $owner -or $owner.ParentProcessId -ne $ExpectedParentPid) {
        throw 'Host no longer belongs to this acceptance driver'
    }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
public static class StartupDump {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr process, out long created, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder name, ref int size);
    [DllImport("dbghelp.dll", SetLastError=true)]
    static extern bool MiniDumpWriteDump(IntPtr process, int pid, IntPtr file, uint type, IntPtr exception, IntPtr streams, IntPtr callback);
    public static long Capture(int pid, string expectedExe, long afterMs, long beforeMs, string output) {
        var process = OpenProcess(0x0410, false, pid); // QUERY_INFORMATION | VM_READ
        if (process == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            var name = new StringBuilder(32768);
            int size = name.Capacity;
            if (!QueryFullProcessImageName(process, 0, name, ref size)) throw new Win32Exception(Marshal.GetLastWin32Error());
            if (!String.Equals(Path.GetFullPath(name.ToString()), Path.GetFullPath(expectedExe), StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("Host executable identity changed");
            long created, exited, kernel, user;
            if (!GetProcessTimes(process, out created, out exited, out kernel, out user)) throw new Win32Exception(Marshal.GetLastWin32Error());
            long ms = new DateTimeOffset(DateTime.FromFileTimeUtc(created)).ToUnixTimeMilliseconds();
            if (ms < afterMs || ms > beforeMs) throw new InvalidOperationException("Host creation time does not match this launch");
            using (var file = new FileStream(output, FileMode.CreateNew, FileAccess.Write, FileShare.None)) {
                // MiniDumpNormal (0): stack/module information, never MiniDumpWithFullMemory.
                if (!MiniDumpWriteDump(process, pid, file.SafeFileHandle.DangerousGetHandle(), 0, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero))
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                return file.Length;
            }
        } finally { CloseHandle(process); }
    }
}
'@
    $bytes = [StartupDump]::Capture($TargetPid, $ExpectedExe, $StartedAfterMs, $StartedBeforeMs, $OutputPath)
    @{ status='captured'; file=[IO.Path]::GetFileName($OutputPath); bytes=$bytes; pid=$TargetPid } | ConvertTo-Json -Compress
    exit 0
} catch {
    @{ status='unavailable'; error=$_.Exception.Message; pid=$TargetPid } | ConvertTo-Json -Compress
    exit 1
}
