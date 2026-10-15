# 校验仓库内 PowerShell 脚本的语法（只解析，不执行脚本内容）。
# 由提交钩子的脚本规则与 CI 调用；发现解析错误即非零退出。
param(
    [string]$Root
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
if (-not $Root) {
    $Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}
$folders = @("scripts", "frontend/scripts")
$checked = 0
$failures = @()

foreach ($folder in $folders) {
    $path = Join-Path $Root $folder
    if (-not (Test-Path -LiteralPath $path)) { continue }
    foreach ($file in Get-ChildItem -LiteralPath $path -Filter *.ps1 -File) {
        $errors = $null
        [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$null, [ref]$errors) | Out-Null
        $checked++
        foreach ($parseError in @($errors)) {
            $failures += "$($file.FullName):$($parseError.Extent.StartLineNumber): $($parseError.Message)"
        }
    }
}

if ($checked -eq 0) {
    [Console]::Error.WriteLine("[powershell-syntax] 检查范围内有 0 个脚本，拒绝空检查：$Root")
    exit 1
}
if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        [Console]::Error.WriteLine("[powershell-syntax] $failure")
    }
    exit 1
}

Write-Output "[powershell-syntax] 通过：$checked 个脚本"
exit 0
