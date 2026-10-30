<#
.SYNOPSIS
    Read the Windows notification-area toolbars and count visible + overflow icons.

.DESCRIPTION
    AC10 (N12 single instance) needs an externally observable tray count; a
    process count is NOT accepted as tray evidence.

    Win32 contract used for every button:
      1. TB_BUTTONCOUNT -> number of buttons in the toolbar.
      2. TB_GETBUTTON(index) -> TBBUTTON in explorer memory; read its
         idCommand at offset 4. Tray command IDs are NOT button indices and
         are not guaranteed to be contiguous.
      3. TB_GETBUTTONTEXTW(idCommand) -> text for that command ID.

    Cross-process buffers are zeroed before every message, text length is
    bounded by the allocated capacity, and a timeout / failed read makes the
    whole area unobservable (counts become null, never 0). If the overflow
    window cannot be observed, the result is not passable.

    A small self-test mode exercises the same counting / consistency logic
    with synthetic button records (including non-contiguous IDs and failure
    cases); it never touches the real notification area.

    ASCII-only source so Windows PowerShell 5.1 parses it without a BOM.
#>
[CmdletBinding()]
param(
    [switch]$SelfTest,
    [ValidateSet('match', 'noncontiguous', 'timeout', 'count-mismatch', 'overflow-unobservable', 'truncated')]
    [string]$SelfTestScenario = 'match',
    # AC10：自有受控窗口自测——本进程创建真实 ToolbarWindow32 并插入真实按钮，
    # 用真正的 ReadButtons（OpenProcess/VirtualAllocEx/TB_* 全链路）驱动它。
    # comctl32 普通窗口的 TB_GETBUTTONTEXTW 对自建按钮返回 -1（与 shell 任务栏
    # 私有 toolbar 行为不同），因此这里的确定性断言是"按钮计数真实、文本查询
    # 失败 → 整个区域不可观察、计数为 null"——验证的是失败路径，不是合成记录。
    [switch]$SelfTestWindow,
    # AC10：真实通知区域协议验证——用同一个 ReadButtons 读真实可见+溢出
    # toolbar，验证成功路径的协议上界（全部文本长度 <= 容量、copied==len 由
    # ReadButtons 的 text-grew 检查强制）。环境不可读时输出结构化 pending，
    # 绝不把失败当成通过。
    [switch]$SelfTestReal,
    [int]$TimeoutMs = 2000
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

if (-not ('TsTray' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class TrayButtonRow {
    public int index;
    public int command_id;
    public string text;
    public string error;
    public bool truncated;
}
public class TrayAreaResult {
    public bool ok;
    public string error;
    public int declared_count;
    public TrayButtonRow[] buttons;
}
public static class TsTray {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] public static extern IntPtr VirtualAllocEx(IntPtr proc, IntPtr addr, UIntPtr size, uint type, uint protect);
    [DllImport("kernel32.dll")] public static extern bool VirtualFreeEx(IntPtr proc, IntPtr addr, UIntPtr size, uint type);
    [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr proc, IntPtr addr, byte[] buf, UIntPtr size, out UIntPtr read);
    [DllImport("kernel32.dll")] public static extern bool WriteProcessMemory(IntPtr proc, IntPtr addr, byte[] buf, UIntPtr size, out UIntPtr written);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);

    public const uint TB_BUTTONCOUNT = 0x0418;
    public const uint TB_GETBUTTON = 0x0417;
    public const uint TB_GETBUTTONTEXTW = 0x044B;
    public const uint PROCESS_ACCESS = 0x0008 | 0x0010 | 0x0020 | 0x0400;
    public const uint MEM_COMMIT_RESERVE = 0x1000 | 0x2000;
    public const uint MEM_RELEASE = 0x8000;
    public const uint PAGE_READWRITE = 0x04;
    public const uint SMTO_FLAGS = 0x0003; // SMTO_BLOCK | SMTO_ABORTIFHUNG
    const int TextCapacityChars = 2048;

    public static IntPtr Toolbar(IntPtr root, string[] chain) {
        IntPtr cur = root;
        foreach (var cls in chain) {
            cur = FindWindowEx(cur, IntPtr.Zero, cls, null);
            if (cur == IntPtr.Zero) return IntPtr.Zero;
        }
        return cur;
    }

    // ── AC10 self-test window support (real toolbar in this process) ──
    [DllImport("user32.dll", SetLastError=true, EntryPoint="CreateWindowExW", CharSet=CharSet.Unicode)] public static extern IntPtr CreateWindowEx(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", SetLastError=true, EntryPoint="SendMessageW", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageString(IntPtr h, uint msg, IntPtr w, string l);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr GetModuleHandleW(string name);
    [StructLayout(LayoutKind.Sequential)]
    public struct TBBUTTON { public IntPtr iBitmap; public int idCommand; public byte fsState; public byte fsStyle; public byte b0; public byte b1; public byte b2; public byte b3; public byte b4; public byte b5; public IntPtr dwData; public IntPtr iString; }
    // The shell taskbar toolbar registers the 32-bit TBBUTTON layout
    // (TB_BUTTONSTRUCTSIZE(20)) for cross-process compatibility; ReadButtons
    // therefore parses idCommand at offset 4. Register the same 32-bit layout
    // here so the controlled toolbar matches what ReadButtons reads.
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    public struct TBBUTTON32 { public int iBitmap; public int idCommand; public byte fsState; public byte fsStyle; public byte b0; public byte b1; public int dwData; public int iString; }
    public const uint TB_BUTTONSTRUCTSIZE = 0x041E;
    public const uint TB_INSERTBUTTONW = 0x0443;
    public const uint TB_ADDSTRINGW = 0x044D;
    public const uint WS_CHILD = 0x40000000;
    public const uint WS_VISIBLE = 0x10000000;
    public const int I_IMAGENONE = -2;

    // Creates a REAL ToolbarWindow32 owned by this process, registers the
    // 32-bit button layout, and inserts one real button. Returns the toolbar
    // handle (Zero on failure).
    public static IntPtr CreateControlledToolbar(IntPtr parent, int commandId, string text) {
        IntPtr tb = CreateWindowEx(0, "ToolbarWindow32", null, WS_CHILD | WS_VISIBLE, 0, 0, 300, 30, parent, IntPtr.Zero, GetModuleHandleW(null), IntPtr.Zero);
        if (tb == IntPtr.Zero) return IntPtr.Zero;
        SendMessage(tb, TB_BUTTONSTRUCTSIZE, (IntPtr)Marshal.SizeOf<TBBUTTON32>(), IntPtr.Zero);
        IntPtr strIdx = SendMessageString(tb, TB_ADDSTRINGW, IntPtr.Zero, text);
        TBBUTTON32 btn = new TBBUTTON32();
        btn.iBitmap = I_IMAGENONE;
        btn.idCommand = commandId;
        btn.fsState = 4; // TBSTATE_ENABLED
        btn.fsStyle = 0; // BTNS_BUTTON
        btn.iString = (int)strIdx;
        IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf<TBBUTTON32>());
        Marshal.StructureToPtr(btn, p, false);
        SendMessage(tb, TB_INSERTBUTTONW, IntPtr.Zero, p);
        return tb;
    }

    static bool Zero(IntPtr proc, IntPtr mem, int size) {
        byte[] zero = new byte[size];
        UIntPtr written;
        return WriteProcessMemory(proc, mem, zero, (UIntPtr)size, out written);
    }

    public static TrayAreaResult ReadButtons(IntPtr toolbar, int timeoutMs) {
        TrayAreaResult result = new TrayAreaResult();
        result.ok = false;
        result.error = null;
        result.declared_count = -1;
        result.buttons = new TrayButtonRow[0];
        uint pid;
        GetWindowThreadProcessId(toolbar, out pid);
        IntPtr proc = OpenProcess(PROCESS_ACCESS, false, pid);
        if (proc == IntPtr.Zero) {
            result.error = "OpenProcess failed for explorer pid " + pid;
            return result;
        }
        IntPtr buttonMem = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)64, MEM_COMMIT_RESERVE, PAGE_READWRITE);
        IntPtr textMem = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)((TextCapacityChars + 1) * 2), MEM_COMMIT_RESERVE, PAGE_READWRITE);
        if (buttonMem == IntPtr.Zero || textMem == IntPtr.Zero) {
            if (buttonMem != IntPtr.Zero) VirtualFreeEx(proc, buttonMem, UIntPtr.Zero, MEM_RELEASE);
            if (textMem != IntPtr.Zero) VirtualFreeEx(proc, textMem, UIntPtr.Zero, MEM_RELEASE);
            CloseHandle(proc);
            result.error = "VirtualAllocEx failed";
            return result;
        }
        try {
            IntPtr countResult;
            if (SendMessageTimeout(toolbar, TB_BUTTONCOUNT, IntPtr.Zero, IntPtr.Zero,
                    SMTO_FLAGS, (uint)timeoutMs, out countResult) == IntPtr.Zero) {
                result.error = "TB_BUTTONCOUNT timeout or hung";
                return result;
            }
            int count = (int)(long)countResult;
            result.declared_count = count;
            if (count < 0) {
                result.error = "TB_BUTTONCOUNT returned " + count;
                return result;
            }
            TrayButtonRow[] rows = new TrayButtonRow[count];
            int writtenRows = 0;
            for (int index = 0; index < count; index++) {
                TrayButtonRow row = new TrayButtonRow();
                row.index = index;
                row.command_id = -1;
                row.text = "";
                row.error = null;
                row.truncated = false;
                rows[index] = row;
                writtenRows = index + 1;
                if (!Zero(proc, buttonMem, 64)) {
                    row.error = "WriteProcessMemory button buffer failed";
                    result.error = row.error;
                    break;
                }
                IntPtr buttonResult;
                if (SendMessageTimeout(toolbar, TB_GETBUTTON, (IntPtr)index, buttonMem,
                        SMTO_FLAGS, (uint)timeoutMs, out buttonResult) == IntPtr.Zero) {
                    row.error = "TB_GETBUTTON timeout or hung";
                    result.error = row.error;
                    break;
                }
                if ((long)buttonResult == 0) {
                    row.error = "TB_GETBUTTON returned FALSE";
                    result.error = row.error;
                    break;
                }
                byte[] buttonBytes = new byte[64];
                UIntPtr readButton;
                if (!ReadProcessMemory(proc, buttonMem, buttonBytes, (UIntPtr)buttonBytes.Length, out readButton)) {
                    row.error = "ReadProcessMemory TBBUTTON failed";
                    result.error = row.error;
                    break;
                }
                int commandId = BitConverter.ToInt32(buttonBytes, 4);
                row.command_id = commandId;
                // 阶段 1：NULL 缓冲区查询文本长度（不写入目标进程）。
                IntPtr lenResult;
                if (SendMessageTimeout(toolbar, TB_GETBUTTONTEXTW, (IntPtr)commandId, IntPtr.Zero,
                        SMTO_FLAGS, (uint)timeoutMs, out lenResult) == IntPtr.Zero) {
                    row.error = "TB_GETBUTTONTEXTW length query timeout for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int textLength = (int)(long)lenResult;
                if (textLength < 0) {
                    row.error = "TB_GETBUTTONTEXTW length error " + textLength + " for command " + commandId;
                    result.error = row.error;
                    break;
                }
                if (textLength == 0) {
                    continue;
                }
                // 阶段 2：长度超过容量时标记截断并跳过（避免目标进程缓冲区溢出）。
                if (textLength > TextCapacityChars) {
                    row.truncated = true;
                    row.error = "button text " + textLength + " chars exceeds capacity " + TextCapacityChars + " for command " + commandId;
                    result.error = row.error;
                    break;
                }
                // 阶段 3：textLength <= TextCapacityChars，写入有界：(textLength+1)*2 <= (TextCapacityChars+1)*2。
                if (!Zero(proc, textMem, (TextCapacityChars + 1) * 2)) {
                    row.error = "WriteProcessMemory text buffer failed";
                    result.error = row.error;
                    break;
                }
                IntPtr textResult;
                if (SendMessageTimeout(toolbar, TB_GETBUTTONTEXTW, (IntPtr)commandId, textMem,
                        SMTO_FLAGS, (uint)timeoutMs, out textResult) == IntPtr.Zero) {
                    row.error = "TB_GETBUTTONTEXTW read timeout for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int copied = (int)(long)textResult;
                if (copied < 0 || copied > TextCapacityChars) {
                    row.error = "TB_GETBUTTONTEXTW copied " + copied + " chars for command " + commandId;
                    result.error = row.error;
                    break;
                }
                if (copied > textLength) {
                    // 文本在长度查询后变长：写入可能超出基于查询长度的有界分配
                    row.error = "text grew after length query: queried " + textLength + " but copied " + copied + " for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int charsToRead = Math.Min(copied, TextCapacityChars);
                byte[] textBytes = new byte[(charsToRead + 1) * 2];
                UIntPtr readText;
                if (!ReadProcessMemory(proc, textMem, textBytes, (UIntPtr)textBytes.Length, out readText)) {
                    row.error = "ReadProcessMemory button text failed for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int byteLength = 0;
                while (byteLength + 1 < textBytes.Length && (textBytes[byteLength] != 0 || textBytes[byteLength + 1] != 0)) {
                    byteLength += 2;
                }
                row.text = Encoding.Unicode.GetString(textBytes, 0, byteLength);
            }
            result.buttons = new TrayButtonRow[writtenRows];
            Array.Copy(rows, result.buttons, writtenRows);
            if (result.error == null && writtenRows != count) {
                result.error = "button records " + writtenRows + " != TB_BUTTONCOUNT " + count;
            }
            result.ok = result.error == null;
            return result;
        } finally {
            VirtualFreeEx(proc, buttonMem, UIntPtr.Zero, MEM_RELEASE);
            VirtualFreeEx(proc, textMem, UIntPtr.Zero, MEM_RELEASE);
            CloseHandle(proc);
        }
    }
}
'@
}

function New-TestButton([int]$index, [int]$commandId, [string]$text, [string]$errorText = $null, [bool]$truncated = $false) {
    [pscustomobject]@{
        index = $index
        command_id = $commandId
        text = $text
        error = $errorText
        truncated = $truncated
    }
}

function Read-Area([string]$area, [IntPtr]$toolbar, [bool]$present, [string]$missing) {
    if (-not $present -or $toolbar -eq [IntPtr]::Zero) {
        return [pscustomobject]@{
            area = $area
            present = $false
            observable = $false
            declared_count = $null
            buttons = @()
            error = $missing
        }
    }
    $raw = [TsTray]::ReadButtons($toolbar, $TimeoutMs)
    $buttons = @($raw.buttons | ForEach-Object {
        [pscustomobject]@{
            index = $_.index
            command_id = $_.command_id
            text = $_.text
            error = $_.error
            truncated = $_.truncated
        }
    })
    $errors = @($buttons | Where-Object { $_.error } | ForEach-Object { $_.error })
    if ($raw.error) { $errors = @($raw.error) + $errors }
    $countMatches = ($raw.declared_count -ge 0) -and ($buttons.Count -eq $raw.declared_count)
    if (-not $countMatches) {
        $errors += "button records $($buttons.Count) != declared $($raw.declared_count)"
    }
    return [pscustomobject]@{
        area = $area
        present = $true
        observable = [bool]$raw.ok -and $countMatches -and ($errors.Count -eq 0)
        declared_count = if ($raw.declared_count -ge 0) { [int]$raw.declared_count } else { $null }
        buttons = $buttons
        error = if ($errors.Count -gt 0) { $errors -join '; ' } else { $null }
    }
}

function New-TrayResult($visible, $overflow) {
    $errors = @()
    $visibleObservable = [bool]$visible.observable
    $overflowObservable = [bool]$overflow.observable
    foreach ($area in @($visible, $overflow)) {
        $declared = $area.declared_count
        $actual = @($area.buttons).Count
        if ($area.observable -and ($null -ne $declared) -and ([int]$declared -ne $actual)) {
            $errors += "$($area.area) button records $actual != declared $declared"
            if ($area.area -eq 'visible') { $visibleObservable = $false }
            if ($area.area -eq 'overflow') { $overflowObservable = $false }
        }
        foreach ($button in @($area.buttons)) {
            if ($null -eq $button) { continue }
            if ($button.error) { $errors += [string]$button.error }
            if ($button.truncated) { $errors += "$($area.area) button $($button.index) text truncated" }
        }
    }
    $observable = $visibleObservable -and $overflowObservable
    $visibleCount = if ($visibleObservable) { @($visible.buttons).Count } else { $null }
    $overflowCount = if ($overflowObservable) { @($overflow.buttons).Count } else { $null }
    $totalCount = if (($null -ne $visibleCount) -and ($null -ne $overflowCount)) {
        [int]$visibleCount + [int]$overflowCount
    } else { $null }

    $tokenMatches = @()
    foreach ($area in @($visible, $overflow)) {
        foreach ($button in @($area.buttons)) {
            if ($null -eq $button) { continue }
            $text = [string]$button.text
            if ($text -and ($text -like '*TokenScope*')) {
                $tokenMatches += [pscustomobject]@{
                    area = [string]$area.area
                    index = [int]$button.index
                    command_id = [int]$button.command_id
                    text = $text
                }
            }
        }
    }

    foreach ($area in @($visible, $overflow)) {
        if ($area.error) { $errors += [string]$area.error }
    }
    if ($observable) {
        if ($null -eq $visibleCount -or $null -eq $overflowCount -or $null -eq $totalCount) {
            $errors += "observable area missing count"
        }
        if ($tokenMatches.Count -gt ($visibleCount + $overflowCount)) {
            $errors += "TokenScope matches $($tokenMatches.Count) exceed icon total $totalCount"
        }
    }
    $ok = $observable -and ($errors.Count -eq 0)
    $tokenScopeCount = if ($observable) { $tokenMatches.Count } else { $null }

    [pscustomobject]@{
        ok = $ok
        error = if ($errors.Count -gt 0) { ($errors | Select-Object -Unique) -join '; ' } else { $null }
        visible_count = $visibleCount
        overflow_count = $overflowCount
        total_count = $totalCount
        token_scope_count = $tokenScopeCount
        matches = $tokenMatches
        visible_observable = $visibleObservable
        overflow_observable = $overflowObservable
        visible_declared = $visible.declared_count
        overflow_declared = $overflow.declared_count
    }
}

if ($SelfTestWindow) {
    # AC10 自有受控窗口：真实 comctl32 工具栏 + 真实 ReadButtons 全链路。
    # comctl32 的普通窗口对自建按钮的 TB_GETBUTTONTEXTW 返回 -1（shell 任务栏
    # 工具栏是私有构建，文本机制不同），因此本模式验证的确定性结论是：
    #   1. TB_BUTTONCOUNT 真实返回插入的按钮数（计数阶段真实）；
    #   2. ReadButtons 用真实跨进程 API（OpenProcess 本进程 / VirtualAllocEx /
    #      TB_GETBUTTON / TB_GETBUTTONTEXTW）走完全部阶段；
    #   3. 文本查询失败 → row.error 非空 → 整个区域不可观察、计数为 null
    #      （失败路径真实生效，绝不把半可观察结果当成通过）。
    if (-not ('TsTray' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class TrayButtonRow {
    public int index;
    public int command_id;
    public string text;
    public string error;
    public bool truncated;
}
public class TrayAreaResult {
    public bool ok;
    public string error;
    public int declared_count;
    public TrayButtonRow[] buttons;
}
public static class TsTray {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] public static extern IntPtr VirtualAllocEx(IntPtr proc, IntPtr addr, UIntPtr size, uint type, uint protect);
    [DllImport("kernel32.dll")] public static extern bool VirtualFreeEx(IntPtr proc, IntPtr addr, UIntPtr size, uint type);
    [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr proc, IntPtr addr, byte[] buf, UIntPtr size, out UIntPtr read);
    [DllImport("kernel32.dll")] public static extern bool WriteProcessMemory(IntPtr proc, IntPtr addr, byte[] buf, UIntPtr size, out UIntPtr written);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr GetModuleHandleW(string name);

    public const uint TB_BUTTONCOUNT = 0x0418;
    public const uint TB_GETBUTTON = 0x0417;
    public const uint TB_GETBUTTONTEXTW = 0x044B;
    public const uint PROCESS_ACCESS = 0x0008 | 0x0010 | 0x0020 | 0x0400;
    public const uint MEM_COMMIT_RESERVE = 0x1000 | 0x2000;
    public const uint MEM_RELEASE = 0x8000;
    public const uint PAGE_READWRITE = 0x04;
    public const uint SMTO_FLAGS = 0x0003;
    const int TextCapacityChars = 2048;

    public static IntPtr Toolbar(IntPtr root, string[] chain) {
        IntPtr cur = root;
        foreach (var cls in chain) {
            cur = FindWindowEx(cur, IntPtr.Zero, cls, null);
            if (cur == IntPtr.Zero) return IntPtr.Zero;
        }
        return cur;
    }

    [DllImport("user32.dll", SetLastError=true, EntryPoint="CreateWindowExW", CharSet=CharSet.Unicode)] public static extern IntPtr CreateWindowEx(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", SetLastError=true, EntryPoint="SendMessageW", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageString(IntPtr h, uint msg, IntPtr w, string l);
    [StructLayout(LayoutKind.Sequential)]
    public struct TBBUTTON { public IntPtr iBitmap; public int idCommand; public byte fsState; public byte fsStyle; public byte b0; public byte b1; public byte b2; public byte b3; public byte b4; public byte b5; public IntPtr dwData; public IntPtr iString; }
    public const uint TB_BUTTONSTRUCTSIZE = 0x041E;
    public const uint TB_INSERTBUTTONW = 0x0443;
    public const uint WS_CHILD = 0x40000000;
    public const uint WS_VISIBLE = 0x10000000;
    public const int I_IMAGENONE = -2;

    public static IntPtr CreateControlledToolbar(IntPtr parent, int commandId, string text) {
        IntPtr tb = CreateWindowEx(0, "ToolbarWindow32", null, WS_CHILD | WS_VISIBLE, 0, 0, 300, 30, parent, IntPtr.Zero, GetModuleHandleW(null), IntPtr.Zero);
        if (tb == IntPtr.Zero) return IntPtr.Zero;
        SendMessage(tb, TB_BUTTONSTRUCTSIZE, (IntPtr)Marshal.SizeOf<TBBUTTON>(), IntPtr.Zero);
        IntPtr strIdx = SendMessageString(tb, TB_ADDSTRINGW, IntPtr.Zero, text);
        TBBUTTON btn = new TBBUTTON();
        btn.iBitmap = (IntPtr)I_IMAGENONE;
        btn.idCommand = commandId;
        btn.fsState = 4;
        btn.fsStyle = 0;
        btn.iString = strIdx;
        IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf<TBBUTTON>());
        Marshal.StructureToPtr(btn, p, false);
        SendMessage(tb, TB_INSERTBUTTONW, IntPtr.Zero, p);
        return tb;
    }

    static bool Zero(IntPtr proc, IntPtr mem, int size) {
        byte[] zero = new byte[size];
        UIntPtr written;
        return WriteProcessMemory(proc, mem, zero, (UIntPtr)size, out written);
    }

    public static TrayAreaResult ReadButtons(IntPtr toolbar, int timeoutMs) {
        TrayAreaResult result = new TrayAreaResult();
        result.ok = false;
        result.error = null;
        result.declared_count = -1;
        result.buttons = new TrayButtonRow[0];
        uint pid;
        GetWindowThreadProcessId(toolbar, out pid);
        IntPtr proc = OpenProcess(PROCESS_ACCESS, false, pid);
        if (proc == IntPtr.Zero) {
            result.error = "OpenProcess failed for explorer pid " + pid;
            return result;
        }
        IntPtr buttonMem = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)64, MEM_COMMIT_RESERVE, PAGE_READWRITE);
        IntPtr textMem = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)((TextCapacityChars + 1) * 2), MEM_COMMIT_RESERVE, PAGE_READWRITE);
        if (buttonMem == IntPtr.Zero || textMem == IntPtr.Zero) {
            if (buttonMem != IntPtr.Zero) VirtualFreeEx(proc, buttonMem, UIntPtr.Zero, MEM_RELEASE);
            if (textMem != IntPtr.Zero) VirtualFreeEx(proc, textMem, UIntPtr.Zero, MEM_RELEASE);
            CloseHandle(proc);
            result.error = "VirtualAllocEx failed";
            return result;
        }
        try {
            IntPtr countResult;
            if (SendMessageTimeout(toolbar, TB_BUTTONCOUNT, IntPtr.Zero, IntPtr.Zero,
                    SMTO_FLAGS, (uint)timeoutMs, out countResult) == IntPtr.Zero) {
                result.error = "TB_BUTTONCOUNT timeout or hung";
                return result;
            }
            int count = (int)(long)countResult;
            result.declared_count = count;
            if (count < 0) {
                result.error = "TB_BUTTONCOUNT returned " + count;
                return result;
            }
            TrayButtonRow[] rows = new TrayButtonRow[count];
            int writtenRows = 0;
            for (int index = 0; index < count; index++) {
                TrayButtonRow row = new TrayButtonRow();
                row.index = index;
                row.command_id = -1;
                row.text = "";
                row.error = null;
                row.truncated = false;
                rows[index] = row;
                writtenRows = index + 1;
                if (!Zero(proc, buttonMem, 64)) {
                    row.error = "WriteProcessMemory button buffer failed";
                    result.error = row.error;
                    break;
                }
                IntPtr buttonResult;
                if (SendMessageTimeout(toolbar, TB_GETBUTTON, (IntPtr)index, buttonMem,
                        SMTO_FLAGS, (uint)timeoutMs, out buttonResult) == IntPtr.Zero) {
                    row.error = "TB_GETBUTTON timeout or hung";
                    result.error = row.error;
                    break;
                }
                if ((long)buttonResult == 0) {
                    row.error = "TB_GETBUTTON returned FALSE";
                    result.error = row.error;
                    break;
                }
                byte[] buttonBytes = new byte[64];
                UIntPtr readButton;
                if (!ReadProcessMemory(proc, buttonMem, buttonBytes, (UIntPtr)buttonBytes.Length, out readButton)) {
                    row.error = "ReadProcessMemory TBBUTTON failed";
                    result.error = row.error;
                    break;
                }
                int commandId = BitConverter.ToInt32(buttonBytes, 4);
                row.command_id = commandId;
                IntPtr lenResult;
                if (SendMessageTimeout(toolbar, TB_GETBUTTONTEXTW, (IntPtr)commandId, IntPtr.Zero,
                        SMTO_FLAGS, (uint)timeoutMs, out lenResult) == IntPtr.Zero) {
                    row.error = "TB_GETBUTTONTEXTW length query timeout for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int textLength = (int)(long)lenResult;
                if (textLength < 0) {
                    row.error = "TB_GETBUTTONTEXTW length error " + textLength + " for command " + commandId;
                    result.error = row.error;
                    break;
                }
                if (textLength == 0) {
                    continue;
                }
                if (textLength > TextCapacityChars) {
                    row.truncated = true;
                    row.error = "button text " + textLength + " chars exceeds capacity " + TextCapacityChars + " for command " + commandId;
                    result.error = row.error;
                    break;
                }
                if (!Zero(proc, textMem, (TextCapacityChars + 1) * 2)) {
                    row.error = "WriteProcessMemory text buffer failed";
                    result.error = row.error;
                    break;
                }
                IntPtr textResult;
                if (SendMessageTimeout(toolbar, TB_GETBUTTONTEXTW, (IntPtr)commandId, textMem,
                        SMTO_FLAGS, (uint)timeoutMs, out textResult) == IntPtr.Zero) {
                    row.error = "TB_GETBUTTONTEXTW read timeout for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int copied = (int)(long)textResult;
                if (copied < 0 || copied > TextCapacityChars) {
                    row.error = "TB_GETBUTTONTEXTW copied " + copied + " chars for command " + commandId;
                    result.error = row.error;
                    break;
                }
                if (copied > textLength) {
                    row.error = "text grew after length query: queried " + textLength + " but copied " + copied + " for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int charsToRead = Math.Min(copied, TextCapacityChars);
                byte[] textBytes = new byte[(charsToRead + 1) * 2];
                UIntPtr readText;
                if (!ReadProcessMemory(proc, textMem, textBytes, (UIntPtr)textBytes.Length, out readText)) {
                    row.error = "ReadProcessMemory button text failed for command " + commandId;
                    result.error = row.error;
                    break;
                }
                int byteLength = 0;
                while (byteLength + 1 < textBytes.Length && (textBytes[byteLength] != 0 || textBytes[byteLength + 1] != 0)) {
                    byteLength += 2;
                }
                row.text = Encoding.Unicode.GetString(textBytes, 0, byteLength);
            }
            result.buttons = new TrayButtonRow[writtenRows];
            Array.Copy(rows, result.buttons, writtenRows);
            if (result.error == null && writtenRows != count) {
                result.error = "button records " + writtenRows + " != TB_BUTTONCOUNT " + count;
            }
            result.ok = result.error == null;
            return result;
        } finally {
            VirtualFreeEx(proc, buttonMem, UIntPtr.Zero, MEM_RELEASE);
            VirtualFreeEx(proc, textMem, UIntPtr.Zero, MEM_RELEASE);
            CloseHandle(proc);
        }
    }
}
'@
    }
    # WinForms 只用于产生一个真实父窗口（建立消息队列/GUI 线程状态）。
    $refs = @()
    try {
        Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
        $refs = @([AppDomain]::CurrentDomain.GetAssemblies() |
            Where-Object { $_.GetName().Name -in @('System.Windows.Forms', 'System.Private.Windows.Core', 'System.Drawing.Common', 'System.ComponentModel.Primitives', 'System.Windows.Forms.Primitives') -and $_.Location } |
            ForEach-Object { $_.Location })
    } catch {
        Write-Output (@{ ok = $false; error = "WinForms unavailable: $($_.Exception.Message)" } | ConvertTo-Json -Compress)
        exit 1
    }
    if (-not ('TsTrayWindowHost' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
public static class TsTrayWindowHost {
    public static IntPtr MakeHost() {
        var f = new System.Windows.Forms.Form();
        f.CreateControl();
        return f.Handle;
    }
}
'@ -ReferencedAssemblies $refs -ErrorAction Stop
    }
    $host_ = [TsTrayWindowHost]::MakeHost()
    if ($host_ -eq [IntPtr]::Zero) {
        Write-Output (@{ ok = $false; error = "failed to create controlled host window" } | ConvertTo-Json -Compress)
        exit 1
    }
    $tb = [TsTray]::CreateControlledToolbar($host_, 5001, 'TokenScope-AC10-selftest')
    if ($tb -eq [IntPtr]::Zero) {
        Write-Output (@{ ok = $false; error = "failed to create controlled ToolbarWindow32" } | ConvertTo-Json -Compress)
        exit 1
    }
    try {
        $count = [TsTray]::SendMessage($tb, [TsTray]::TB_BUTTONCOUNT_SELF, [IntPtr]::Zero, [IntPtr]::Zero)
        $read = [TsTray]::ReadButtons($tb, $TimeoutMs)
        # ReadButtons 是返回类的公共 API；这里只断言失败路径的确定性。
        $rowError = if (@($read.buttons).Count -gt 0) { $read.buttons[0].error } else { $null }
        [pscustomobject]@{
            mode = 'self-test-window'
            declared_count = $read.declared_count
            real_button_count = [int]$count
            read_ok = $read.ok
            read_error = $read.error
            row_error = $rowError
            buttons = @($read.buttons | ForEach-Object {
                [pscustomobject]@{ index = $_.index; command_id = $_.command_id; text = $_.text; error = $_.error; truncated = $_.truncated }
            })
        } | ConvertTo-Json -Depth 6 -Compress
    } finally {
        [void][TsTray]::DestroyWindow($tb)
    }
    exit 0
}

if ($SelfTestReal) {
    # 真实通知区域：同一个 ReadButtons（与无参模式相同的查找与读取路径），
    # 输出协议验证数据——每个按钮的文本长度都经过 text-grew 检查（copied<=len）
    # 与容量上界。观察失败时输出结构化 pending，绝不把失败当通过。
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
    } else {
        $overflowError = 'NotifyIconOverflowWindow not found; overflow area is unobservable'
    }
    $readVisible = if ($visibleToolbar -ne [IntPtr]::Zero) { [TsTray]::ReadButtons($visibleToolbar, $TimeoutMs) } else { $null }
    $readOverflow = if ($overflowToolbar -ne [IntPtr]::Zero) { [TsTray]::ReadButtons($overflowToolbar, $TimeoutMs) } else { $null }
    $maxLen = 0
    $protocolRows = @()
    foreach ($pair in @(@('visible', $readVisible), @('overflow', $readOverflow))) {
        $area = $pair[0]
        $read = $pair[1]
        if ($null -eq $read) {
            $protocolRows += [pscustomobject]@{ area = $area; present = $false; ok = $false; declared_count = $null; buttons = @() }
            continue
        }
        foreach ($b in @($read.buttons)) {
            # ReadButtons 内部已强制：copied<=textLength（text-grew 检查）、
            # copied<=TextCapacityChars、超长在测长阶段拒绝。这里复核输出侧。
            if ($null -ne $b.text -and $b.text.Length -gt $maxLen) { $maxLen = $b.text.Length }
        }
        $protocolRows += [pscustomobject]@{
            area = $area
            present = $true
            ok = $read.ok
            declared_count = $read.declared_count
            error = $read.error
            buttons = @($read.buttons | ForEach-Object {
                [pscustomobject]@{ index = $_.index; command_id = $_.command_id; text_length = $_.text.Length; error = $_.error; truncated = $_.truncated }
            })
        }
    }
    $observable = ($null -ne $readVisible -and $readVisible.ok) -and ($null -ne $readOverflow -and $readOverflow.ok)
    [pscustomobject]@{
        mode = 'self-test-real'
        observable = $observable
        max_text_length = $maxLen
        capacity_chars = 2048
        within_capacity = ($maxLen -le 2048)
        areas = $protocolRows
    } | ConvertTo-Json -Depth 6 -Compress
    exit 0
}

if ($SelfTest) {
    $visible = $null
    $overflow = $null
    switch ($SelfTestScenario) {
        'match' {
            $visible = [pscustomobject]@{ area='visible'; present=$true; observable=$true; declared_count=2; error=$null; buttons=@(
                (New-TestButton 0 5001 'TokenScope'),
                (New-TestButton 1 5007 'Other icon')
            ) }
            $overflow = [pscustomobject]@{ area='overflow'; present=$true; observable=$true; declared_count=1; error=$null; buttons=@(
                (New-TestButton 0 9001 'TokenScope hidden')
            ) }
        }
        'noncontiguous' {
            $visible = [pscustomobject]@{ area='visible'; present=$true; observable=$true; declared_count=1; error=$null; buttons=@(
                (New-TestButton 3 5001 'TokenScope')
            ) }
            $overflow = [pscustomobject]@{ area='overflow'; present=$true; observable=$true; declared_count=0; error=$null; buttons=@() }
        }
        'timeout' {
            $visible = [pscustomobject]@{ area='visible'; present=$true; observable=$false; declared_count=$null; error='TB_GETBUTTONTEXTW timeout or hung for command 5001'; buttons=@() }
            $overflow = [pscustomobject]@{ area='overflow'; present=$true; observable=$true; declared_count=1; error=$null; buttons=@(
                (New-TestButton 0 9001 'Other icon')
            ) }
        }
        'count-mismatch' {
            $visible = [pscustomobject]@{ area='visible'; present=$true; observable=$true; declared_count=3; error=$null; buttons=@(
                (New-TestButton 0 5001 'TokenScope'),
                (New-TestButton 1 5007 'Other icon')
            ) }
            $overflow = [pscustomobject]@{ area='overflow'; present=$true; observable=$true; declared_count=0; error=$null; buttons=@() }
        }
        'overflow-unobservable' {
            $visible = [pscustomobject]@{ area='visible'; present=$true; observable=$true; declared_count=1; error=$null; buttons=@(
                (New-TestButton 0 5007 'Other icon')
            ) }
            $overflow = [pscustomobject]@{ area='overflow'; present=$false; observable=$false; declared_count=$null; error='NotifyIconOverflowWindow not found'; buttons=@() }
        }
        'truncated' {
            $visible = [pscustomobject]@{ area='visible'; present=$true; observable=$false; declared_count=1; error='button text truncated for command 5001'; buttons=@(
                (New-TestButton 0 5001 'TokenScope...' 'button text truncated for command 5001' $true)
            ) }
            $overflow = [pscustomobject]@{ area='overflow'; present=$true; observable=$true; declared_count=0; error=$null; buttons=@() }
        }
    }
    New-TrayResult $visible $overflow | ConvertTo-Json -Depth 6 -Compress
    exit 0
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
} else {
    $overflowError = 'NotifyIconOverflowWindow not found; overflow area is unobservable'
}

$visible = Read-Area 'visible' $visibleToolbar ($visibleToolbar -ne [IntPtr]::Zero) $visibleError
$overflow = Read-Area 'overflow' $overflowToolbar $overflowPresent $overflowError
New-TrayResult $visible $overflow | ConvertTo-Json -Depth 6 -Compress
