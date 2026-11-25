#Requires -Version 7
# M05 生命周期契约测试：用受控进程替身 + 临时目录验证
# scripts/install_test_helpers.ps1 的等待/观察/关闭判定。
#
# 反例先于实现（先红后绿）：旧日志借用、快启动、仅写库、提前退出 0/23、
# 关闭失败/超时/退出非 0 全部必须失败；单实例正例必须记录同一 PID 与
# 本次 query_id。全部使用临时目录，不触碰默认用户目录与注册表。

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "install_test_helpers.ps1")

$Failures = New-Object System.Collections.Generic.List[string]
$Checks = 0

function Test-True {
    param([bool]$Condition, [string]$Message)
    $script:Checks++
    if ($Condition) {
        Write-Host "PASS $Message"
    }
    else {
        $script:Failures.Add($Message) | Out-Null
        Write-Host "FAIL $Message"
    }
}

function Test-Throws {
    param([scriptblock]$Action, [string]$Message, [string]$ExpectedText = "")
    $script:Checks++
    try {
        & $Action
        Write-Host "FAIL $Message（未抛出）"
        $script:Failures.Add($Message) | Out-Null
    }
    catch {
        if ($ExpectedText -ne "" -and -not ($_.Exception.Message -like "*$ExpectedText*")) {
            Write-Host "FAIL $Message（抛出但信息不符：$($_.Exception.Message)）"
            $script:Failures.Add("$Message（信息不符）") | Out-Null
        }
        else {
            Write-Host "PASS $Message"
        }
    }
}

function New-Scenario {
    param([string]$Tag)
    # 模拟真实布局：<root>/home/.tokenscope/logs（与 DataDir 派生一致，
    # 使 Move-LaunchLogsToEvidence 的 ".tokenscope\logs" 校验可用）。
    $root = Join-Path ([System.IO.Path]::GetTempPath()) "tokenscope-helper-test-$Tag-$([System.Guid]::NewGuid().ToString('N'))"
    $dataDir = Join-Path $root "home" | Join-Path -ChildPath ".tokenscope"
    New-Item -ItemType Directory -Path (Join-Path $dataDir "logs") -Force | Out-Null
    return [pscustomobject]@{
        Root    = $root
        DataDir = $dataDir
        LogsDir = Join-Path $dataDir "logs"
        Context = [pscustomobject]@{
            Process     = $null
            Pid         = $null
            ExePath     = "stand-in"
            Scenario    = $Tag
            RunId       = "run-$Tag"
            EvidenceDir = $root
            LiveLogsDir = Join-Path $dataDir "logs"
            QueryId     = $null
            Requests    = $null
            Stage       = "launched"
        }
    }
}

function Write-LogLine {
    param([string]$LogsDir, [string]$FileName, [string]$Line, [bool]$NoNewline = $false)
    if ($NoNewline) {
        Add-Content -Path (Join-Path $LogsDir $FileName) -Value $Line -NoNewline -Encoding utf8NoBOM
    }
    else {
        Add-Content -Path (Join-Path $LogsDir $FileName) -Value $Line -Encoding utf8NoBOM
    }
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class InstallHelperTestWindowProbe {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool IsWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);
}
"@

function New-GuiStandInProcess {
    # 最小受控 GUI 替身子进程：
    # - 进程启动后先对全线程调用 ImmDisableIME(-1) 并对窗口解除 IME 上下文，
    #   避免本机桌面中文 IME 浮窗（如 UAC_InputIndicatorOverlayWnd）抢占
    #   Process.MainWindowHandle；
    # - 创建可见但不抢焦点的顶层 Win32 主窗口（SW_SHOWNOACTIVATE），消息循环
    #   就绪后经 WM_APP 原子写出 {pid, hwnd, ime_disabled} 就绪信号；
    # - 收到 CloseMainWindow（WM_CLOSE）后 DestroyWindow → PostQuitMessage(0)，
    #   以退出码 0 干净退出；超时定时器兜底防孤儿窗口残留。
    param([Parameter(Mandatory = $true)][string]$Root)
    $readyPath = Join-Path $Root "gui-standin.ready.json"
    $childPath = Join-Path $Root "gui-standin.ps1"
    $quote = { param($path) "'" + $path.Replace("'", "''") + "'" }
    $childScript = @"
`$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class TokenScopeGuiStandIn {
    private const uint WM_DESTROY = 0x0002;
    private const uint WM_CLOSE = 0x0010;
    private const uint WM_TIMER = 0x0113;
    private const uint WM_APP_READY = 0x8001;
    private const uint WS_OVERLAPPEDWINDOW = 0x00CF0000;
    private const int SW_SHOWNOACTIVATE = 4;
    private const uint TIMER_TIMEOUT_ID = 1;

    private delegate IntPtr WndProcDelegate(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WNDCLASSEX {
        public uint cbSize;
        public uint style;
        public WndProcDelegate lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT {
        public int x;
        public int y;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MSG {
        public IntPtr hwnd;
        public uint message;
        public IntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public POINT pt;
    }

    [DllImport("imm32.dll")]
    private static extern bool ImmDisableIME(uint idThread);

    [DllImport("imm32.dll")]
    private static extern IntPtr ImmAssociateContext(IntPtr hWnd, IntPtr hIMC);

    [DllImport("imm32.dll")]
    private static extern IntPtr ImmGetContext(IntPtr hWnd);

    [DllImport("imm32.dll")]
    private static extern bool ImmReleaseContext(IntPtr hWnd, IntPtr hIMC);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string lpModuleName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClassEx(ref WNDCLASSEX lpwcx);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(
        uint dwExStyle, string lpClassName, string lpWindowName, uint dwStyle,
        int x, int y, int nWidth, int nHeight,
        IntPtr hWndParent, IntPtr hMenu, IntPtr hInstance, IntPtr lpParam);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    private static extern bool UpdateWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern void PostQuitMessage(int nExitCode);

    [DllImport("user32.dll")]
    private static extern int GetMessage(out MSG lpMsg, IntPtr hWnd, uint wMsgFilterMin, uint wMsgFilterMax);

    [DllImport("user32.dll")]
    private static extern bool TranslateMessage(ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage(ref MSG lpMsg);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr DefWindowProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern IntPtr SetTimer(IntPtr hWnd, IntPtr nIDEvent, uint uElapse, IntPtr lpTimerFunc);

    private static string _readyPath;
    private static int _exitCode = 0;
    private static WndProcDelegate _pinnedProc;

    private static IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam) {
        if (msg == WM_APP_READY) {
            if (!string.IsNullOrEmpty(_readyPath)) {
                IntPtr imc = ImmGetContext(hWnd);
                bool imeDisabled = (imc == IntPtr.Zero);
                if (imc != IntPtr.Zero) {
                    ImmReleaseContext(hWnd, imc);
                }
                string tmp = _readyPath + ".tmp";
                string payload = "{\"pid\":" + Environment.ProcessId +
                    ",\"hwnd\":" + hWnd.ToInt64() +
                    ",\"ime_disabled\":" + (imeDisabled ? "true" : "false") + "}";
                File.WriteAllText(tmp, payload, new UTF8Encoding(false));
                File.Move(tmp, _readyPath, true);
            }
            return IntPtr.Zero;
        }
        if (msg == WM_TIMER && wParam == (IntPtr)TIMER_TIMEOUT_ID) {
            _exitCode = 124;
            DestroyWindow(hWnd);
            return IntPtr.Zero;
        }
        if (msg == WM_CLOSE) {
            _exitCode = 0;
            DestroyWindow(hWnd);
            return IntPtr.Zero;
        }
        if (msg == WM_DESTROY) {
            PostQuitMessage(_exitCode);
            return IntPtr.Zero;
        }
        return DefWindowProc(hWnd, msg, wParam, lParam);
    }

    public static int Run(string readyPath, uint maxLifetimeMs) {
        ImmDisableIME(0xFFFFFFFFu);
        _readyPath = readyPath;
        _pinnedProc = new WndProcDelegate(WndProc);
        IntPtr hInstance = GetModuleHandle(null);
        string className = "TokenScopeInstallContractGui_" + Environment.ProcessId;
        var wc = new WNDCLASSEX {
            cbSize = (uint)Marshal.SizeOf<WNDCLASSEX>(),
            lpfnWndProc = _pinnedProc,
            hInstance = hInstance,
            lpszClassName = className
        };
        if (RegisterClassEx(ref wc) == 0) {
            return 2;
        }
        IntPtr hwnd = CreateWindowEx(
            0, className, "TokenScope Install Contract Stand-In", WS_OVERLAPPEDWINDOW,
            0, 0, 240, 120,
            IntPtr.Zero, IntPtr.Zero, hInstance, IntPtr.Zero);
        if (hwnd == IntPtr.Zero) {
            return 3;
        }
        ImmAssociateContext(hwnd, IntPtr.Zero);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        UpdateWindow(hwnd);
        SetTimer(hwnd, (IntPtr)TIMER_TIMEOUT_ID, maxLifetimeMs, IntPtr.Zero);
        PostMessage(hwnd, WM_APP_READY, IntPtr.Zero, IntPtr.Zero);
        MSG msg;
        while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) {
            TranslateMessage(ref msg);
            DispatchMessage(ref msg);
        }
        GC.KeepAlive(_pinnedProc);
        return _exitCode;
    }
}
'@
exit ([TokenScopeGuiStandIn]::Run($(& $quote $readyPath), 60000u))
"@
    Set-Content -LiteralPath $childPath -Value $childScript -Encoding utf8NoBOM
    $launcher = "& $(& $quote $childPath)"
    $encoded = [System.Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($launcher))
    $proc = Start-Process -FilePath "pwsh" -ArgumentList "-NoProfile", "-EncodedCommand", $encoded -WindowStyle Hidden -PassThru
    try {
        # 等待子进程通过就绪信号回报目标 HWND，并核验：
        # 1) 目标 HWND 真实可见且属于该子进程 PID；
        # 2) Process.MainWindowHandle 明确指向该目标 HWND。
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        while ($watch.Elapsed.TotalSeconds -lt 30) {
            $proc.Refresh()
            if ($proc.HasExited) { throw "GUI 替身在窗口就绪前退出：$($proc.ExitCode)" }
            if (Test-Path -LiteralPath $readyPath) {
                $signal = Get-Content -LiteralPath $readyPath -Raw -Encoding utf8 | ConvertFrom-Json
                if ([int]$signal.pid -ne $proc.Id) {
                    throw "GUI 替身就绪信号 PID 不匹配：信号 $($signal.pid) != 进程 $($proc.Id)"
                }
                $targetHwnd = [System.IntPtr]::new([long]$signal.hwnd)
                if ($targetHwnd -eq [System.IntPtr]::Zero) {
                    throw "GUI 替身就绪信号返回空 HWND"
                }
                [uint32]$ownerPid = 0
                [void][InstallHelperTestWindowProbe]::GetWindowThreadProcessId($targetHwnd, [ref]$ownerPid)
                if ([InstallHelperTestWindowProbe]::IsWindow($targetHwnd) -and
                    [InstallHelperTestWindowProbe]::IsWindowVisible($targetHwnd) -and
                    $ownerPid -eq [uint32]$proc.Id -and
                    $proc.MainWindowHandle -eq $targetHwnd) {
                    return [pscustomobject]@{
                        Process     = $proc
                        TargetHwnd  = $targetHwnd
                        OwnerPid    = $ownerPid
                        ImeDisabled = [bool]$signal.ime_disabled
                    }
                }
            }
            Start-Sleep -Milliseconds 50
        }
        $proc.Refresh()
        throw "GUI 替身未在 30 秒内就绪（MainWindowHandle=$($proc.MainWindowHandle)）"
    }
    catch {
        if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
        throw
    }
}

function New-StandInProcess {
    # 存活替身：一个安静存活 60 秒的 pwsh 子进程（等待期/观察期用，
    # 不涉及 CloseMainWindow）。
    Start-Process -FilePath "pwsh" -ArgumentList "-NoProfile", "-Command", "Start-Sleep -Seconds 60" -WindowStyle Hidden -PassThru
}

function New-ExitProcess {
    # 受控退出替身：子进程先写就绪文件，等到放行文件出现才以指定退出码退出。
    # pwsh 冷启动可能超过观察期，不能让 Start-Process 返回充当"已运行"信号；
    # 调用方在进入等待/观察前调用 Release-ExitProcess，退出即落在窗口内。
    param([int]$Code, [string]$Root)
    $ready = Join-Path $Root "exit-$Code.ready"
    $go = Join-Path $Root "exit-$Code.go"
    $quote = { param($path) "'" + $path.Replace("'", "''") + "'" }
    $script = @"
New-Item -ItemType File -Path $(& $quote $ready) | Out-Null
`$watch = [System.Diagnostics.Stopwatch]::StartNew()
while (-not (Test-Path -LiteralPath $(& $quote $go)) -and `$watch.Elapsed.TotalSeconds -lt 60) { Start-Sleep -Milliseconds 20 }
exit $Code
"@
    $encoded = [System.Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($script))
    $proc = Start-Process -FilePath "pwsh" -ArgumentList "-NoProfile", "-EncodedCommand", $encoded -WindowStyle Hidden -PassThru
    try {
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        while ($watch.Elapsed.TotalSeconds -lt 30) {
            if (Test-Path -LiteralPath $ready) {
                return [pscustomobject]@{ Process = $proc; GoFile = $go }
            }
            if ($proc.HasExited) { throw "退出替身在就绪前退出：$($proc.ExitCode)" }
            Start-Sleep -Milliseconds 50
        }
        throw "退出替身未在 30 秒内就绪"
    }
    catch {
        if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
        throw
    }
}

function Release-ExitProcess {
    param($Handle)
    New-Item -ItemType File -Path $Handle.GoFile | Out-Null
}

function Cleanup {
    param($Scenario)
    Stop-ScenarioProcess -Context $Scenario.Context
    if (Test-Path $Scenario.Root) {
        Remove-Item $Scenario.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$SummaryOk = "汇总完成（会话 q-alpha-1）：by=day 组=3 请求=3，12 ms"

# ---------- 反例 1：旧日志借用 ----------

Write-Host "=== 反例：旧日志已有成功汇总，新实例无新日志 ==="
$s = New-Scenario "old-log-borrow"
# 旧日志写入活动目录 → Prepare-LaunchEvidence 归档移走（真实流程）→
# 活动目录变空。归档里的旧汇总不得满足本次启动的等待判定。
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log.2026-10-10" -Line $SummaryOk
# 证据根沿用生产命名（tokenscope-install-evidence），触发 Move-LaunchLogsToEvidence
# 的目的地守卫——契约必须走与安装验收完全相同的校验路径。
$evidence = Prepare-LaunchEvidence -DataDir $s.DataDir -EvidenceRoot (
    Join-Path $s.Root "tokenscope-install-evidence") -Scenario "old-log-borrow" -RunId "run-1"
Test-True ((Test-Path (Join-Path $evidence "tokenscope.log.2026-10-10")) -and
    -not (Test-Path $s.LogsDir)) "旧日志已归档移出活动目录"
New-Item -ItemType Directory -Path $s.LogsDir -Force | Out-Null
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
$deadline = 2
Test-Throws {
    Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds $deadline
} "旧日志借用必须等待超时失败（归档后的新日志没有汇总）" "超时"
Cleanup $s

# ---------- 反例 2：仅写"历史库写入"没有"汇总完成" ----------

Write-Host "=== 反例：仅写历史库写入 ==="
$s = New-Scenario "write-only"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line "历史库写入：新增 3，更新 0，已存在 0，冲突 0，陈旧 0；别名 +0，来源 +0"
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
Test-Throws {
    Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
} "仅写库没有汇总完成必须失败" "超时"
Cleanup $s

# ---------- 反例 3：本次汇总请求数 0 或 2 ----------

Write-Host "=== 反例：汇总请求数 0 / 2 ==="
foreach ($wrong in 0, 2) {
    $s = New-Scenario "wrong-count-$wrong"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line (
        "汇总完成（会话 q-wrong）：by=day 组=1 请求=$wrong，5 ms"
    )
    $proc = New-StandInProcess
    $s.Context.Process = $proc
    $s.Context.Pid = $proc.Id
    Test-Throws {
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
    } "汇总请求数 $wrong != 3 必须失败" "本次汇总请求数 $wrong != 期望 3"
    Cleanup $s
}

# ---------- 反例 4：等待期提前退出（退出码 0 与 23） ----------

Write-Host "=== 反例：等待期提前退出 0 / 23 ==="
foreach ($code in 0, 23) {
    $s = New-Scenario "early-exit-$code"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line "价格加载：重建，3 ms"
    $exit = New-ExitProcess -Code $code -Root $s.Root
    $proc = $exit.Process
    $s.Context.Process = $proc
    $s.Context.Pid = $proc.Id
    Test-Throws {
        Release-ExitProcess $exit
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 10
    } "等待期提前退出（退出码 $code）必须失败" "进程在场景进行中退出"
    Cleanup $s
}

# ---------- 反例 5：观察期提前退出 ----------

Write-Host "=== 反例：观察期提前退出 ==="
foreach ($code in 0, 23) {
    $s = New-Scenario "observe-exit-$code"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk
    $exit = New-ExitProcess -Code $code -Root $s.Root
    $proc = $exit.Process
    $s.Context.Process = $proc
    $s.Context.Pid = $proc.Id
    $s.Context.QueryId = "q-alpha-1"
    Test-Throws {
        Release-ExitProcess $exit
        Observe-ProcessAlive -Context $s.Context -DurationSeconds 1 -PollMilliseconds 100
    } "观察期提前退出（退出码 $code）必须失败" "进程在场景进行中退出"
    Cleanup $s
}

# ---------- 反例 6：关闭失败 / 超时 / 退出非 0 ----------

Write-Host "=== 反例：关闭语义 ==="
function New-FakeContext {
    param([bool]$CloseResult, [bool]$WaitResult, $ExitCode)
    $proc = [pscustomobject]@{ HasExited = $false; ExitCode = $ExitCode }
    $proc | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { return $script:CloseResult }.GetNewClosure()
    $proc | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param([int]$ms) return $script:WaitResult }.GetNewClosure()
    return [pscustomobject]@{ Process = $proc; Pid = 0; ExePath = "fake"; Scenario = "fake"; RunId = "fake"; EvidenceDir = $null; QueryId = $null; Requests = $null; Stage = "launched" }
}
Test-Throws { Close-AppGracefully -Context (New-FakeContext -CloseResult $false -WaitResult $true -ExitCode $null) } "CloseMainWindow=false 必须失败" "CloseMainWindow"
Test-Throws { Close-AppGracefully -Context (New-FakeContext -CloseResult $true -WaitResult $false -ExitCode $null) } "关闭等待超时必须失败" "30s"
Test-Throws { Close-AppGracefully -Context (New-FakeContext -CloseResult $true -WaitResult $true -ExitCode 3) } "退出码非 0 必须失败" "退出码 3"

# ---------- 反例 7：半行日志（UTF-8/写入边界）不算完整记录 ----------

Write-Host "=== 反例：半行日志不算完成 ==="
$s = New-Scenario "partial-line"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk -NoNewline $true
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
Test-Throws {
    Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
} "无换行的半行汇总不得参与匹配" "超时"
Cleanup $s

# ---------- 正例 1：快启动完整汇总并存活 ----------

Write-Host "=== 正例：快启动完整汇总 ==="
$s = New-Scenario "fast-start"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
try {
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 10
    Test-True ($summary.QueryId -eq "q-alpha-1") "记录本次 query_id"
    Test-True ($summary.Requests -eq 3) "记录本次请求数"
    Test-True ($s.Context.Pid -eq $proc.Id) "全程同一进程句柄"
    Observe-ProcessAlive -Context $s.Context -DurationSeconds 1 -PollMilliseconds 100
    Test-True $true "观察期存活"
}
finally {
    Cleanup $s
}

# ---------- 正例 2：分多次写完 + 跨文件（跨日） ----------

Write-Host "=== 正例：分次写入与跨文件 ==="
$s = New-Scenario "split-write"
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
try {
    # 先写半行（无换行），再在第二个文件里补完整行——半行不匹配，
    # 完整行匹配后成功。
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk -NoNewline $true
    Test-Throws {
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
    } "半行阶段必须继续等待" "超时"
    Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log.1" -Line $SummaryOk
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 10
    Test-True ($summary.QueryId -eq "q-alpha-1") "跨文件完整行匹配"
}
finally {
    Cleanup $s
}

# ---------- 正例 3：真实日志 writer 持续打开，完整行仍须可读 ----------

Write-Host "=== 正例：读取活动写入日志 ==="
$s = New-Scenario "live-writer"
$proc = New-StandInProcess
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
$logPath = Join-Path $s.LogsDir "tokenscope.log"
# 与 Rust 日志文件一致：允许共享读写/删除，写入句柄整个等待期保持打开。
$writer = [System.IO.File]::Open($logPath, [System.IO.FileMode]::Create,
    [System.IO.FileAccess]::Write,
    [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete)
try {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($SummaryOk)
    $writer.Write($bytes, 0, $bytes.Length)
    $writer.Flush()
    Test-Throws {
        Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 1
    } "活动写入的半行不算完成" "超时"
    $newline = [System.Text.Encoding]::UTF8.GetBytes("`n")
    $writer.Write($newline, 0, $newline.Length)
    $writer.Flush()
    Test-True (@(Get-CompleteLogLines -LogsDir $s.LogsDir).Count -eq 1) "writer 未关闭时读取完整行"
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 2
    Test-True ($summary.QueryId -eq "q-alpha-1") "活动写入日志记录本次 query_id"
    Test-True ($summary.Requests -eq 3) "活动写入日志记录请求数 3"
}
finally {
    $writer.Dispose()
    Cleanup $s
}

# ---------- 正例 4：完整生命周期（单实例 + 观察 + 优雅关闭） ----------

Write-Host "=== 正例：完整生命周期 ==="
$s = New-Scenario "full-lifecycle"
Write-LogLine -LogsDir $s.LogsDir -FileName "tokenscope.log" -Line $SummaryOk
$gui = New-GuiStandInProcess -Root $s.Root
$proc = $gui.Process
$s.Context.Process = $proc
$s.Context.Pid = $proc.Id
try {
    Test-True $gui.ImeDisabled "GUI 替身主窗口已禁用 IME"
    Test-True ($gui.TargetHwnd -ne [System.IntPtr]::Zero -and $gui.OwnerPid -eq [uint32]$proc.Id) "就绪信号目标 HWND 属于替身 PID"
    $proc.Refresh()
    Test-True ($proc.MainWindowHandle -eq $gui.TargetHwnd) "Process.MainWindowHandle 指向替身目标 HWND"
    $summary = Wait-CurrentSummary -Context $s.Context -ExpectedRequests 3 -TimeoutSeconds 15
    Observe-ProcessAlive -Context $s.Context -DurationSeconds 1 -PollMilliseconds 100
    Close-AppGracefully -Context $s.Context
    Test-True ($s.Context.Stage -eq "closed") "生命周期到 closed"
    Test-True ($s.Context.Pid -eq $proc.Id) "PID 全程一致"
    Test-True ($proc.HasExited -and $proc.ExitCode -eq 0) "替身进程经 CloseMainWindow 在时限内退出且退出码为 0"
    Test-True ($s.Context.QueryId -eq "q-alpha-1") "query_id 全程一致"
}
catch {
    Stop-ScenarioProcess -Context $s.Context
    throw
}
finally {
    if (Test-Path $s.Root) {
        Remove-Item $s.Root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "契约断言通过 $Checks 项，失败 $($Failures.Count) 项"
if ($Failures.Count -gt 0) {
    foreach ($f in $Failures) {
        Write-Host "  - $f"
    }
    exit 1
}
exit 0
