$ErrorActionPreference = "Stop"
$d = "C:\Users\admin\Desktop\项目\tokenscope\docs\research\ccusage-draft"
$targets = @("S1-main-and-claude.md", "S2-codex.md", "S5-other-adapters.md", "S4-aggregation.md")

foreach ($name in $targets) {
    $path = Join-Path $d $name
    $lines = Get-Content $path -Encoding UTF8

    $h2 = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^##\s') { $h2 = $i; break }
    }
    if ($h2 -le 0) { Write-Output ("{0}: skip (h2 index {1})" -f $name, $h2); continue }

    $intro = @()
    for ($i = 0; $i -lt $h2; $i++) {
        if ($lines[$i] -match '^#\s') { continue }
        $intro += $lines[$i]
    }
    while ($intro.Count -gt 0 -and $intro[0].Trim() -eq "") { $intro = $intro[1..($intro.Count - 1)] }
    while ($intro.Count -gt 0 -and $intro[-1].Trim() -eq "") { $intro = $intro[0..($intro.Count - 2)] }
    if ($intro.Count -eq 0) { Write-Output ("{0}: no intro, skip" -f $name); continue }

    $body = if ($h2 + 1 -lt $lines.Count) { $lines[($h2 + 1)..($lines.Count - 1)] } else { @() }
    while ($body.Count -gt 0 -and $body[0].Trim() -eq "") { $body = $body[1..($body.Count - 1)] }

    $new = New-Object System.Collections.Generic.List[string]
    $new.Add($lines[$h2])
    $new.Add("")
    foreach ($l in $intro) { $new.Add($l) }
    $new.Add("")
    foreach ($l in $body) { $new.Add($l) }

    $text = ($new -join "`n") + "`n"
    [System.IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
    Write-Output ("{0}: intro {1} lines moved after chapter title" -f $name, $intro.Count)
}
