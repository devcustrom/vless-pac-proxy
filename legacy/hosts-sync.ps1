# Adds local domains from local-domains.txt to the Windows hosts file.
# Run local-domains-hosts.bat as Administrator (this script needs admin rights).
$ErrorActionPreference = 'Stop'
$dir = Split-Path -Parent $MyInvocation.MyCommand.Path
$file = Join-Path $dir 'local-domains.txt'
$hostsPath = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'

$domains = New-Object System.Collections.Generic.List[string]
if (Test-Path $file) {
    foreach ($line in Get-Content $file) {
        $t = $line.Trim()
        if (-not $t -or $t.StartsWith('#')) { continue }
        if ($t -match '^([^\s=]+)(?:\s*=\s*|\s+)(\d{1,5})$') {
            $d = $Matches[1].ToLower()
            if (-not $domains.Contains($d)) { $domains.Add($d) }
        }
    }
}

$begin = '# wireproxy-control local domains begin'
$end = '# wireproxy-control local domains end'

$lines = New-Object System.Collections.Generic.List[string]
if (Test-Path $hostsPath) {
    foreach ($l in Get-Content $hostsPath) { $lines.Add($l) }
}

$out = New-Object System.Collections.Generic.List[string]
$skip = $false
foreach ($l in $lines) {
    if ($l.Trim() -eq $begin) { $skip = $true; continue }
    if ($l.Trim() -eq $end) { $skip = $false; continue }
    if (-not $skip) { $out.Add($l) }
}

if ($domains.Count -gt 0) {
    $out.Add($begin)
    foreach ($d in $domains) { $out.Add('127.0.0.1 ' + $d) }
    $out.Add($end)
}

Set-Content -Path $hostsPath -Value $out -Encoding ASCII
Write-Output ("hosts updated: " + $domains.Count + " domain(s) -> 127.0.0.1")
Write-Output "Flush DNS cache..."
ipconfig /flushdns | Out-Null
Write-Output "Done."
