# Трей wireproxy (Windows): панель, сертификаты локальных доменов, списки, рестарт.
# Иконка в системном трее, меню обновляется при каждом открытии.
Add-Type -AssemblyName System.Windows.Forms | Out-Null
Add-Type -AssemblyName System.Drawing | Out-Null

$script:baseDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$bm = New-Object System.Drawing.Bitmap 16, 16
$g = [System.Drawing.Graphics]::FromImage($bm)
$g.SmoothingMode = 'AntiAlias'
$g.Clear([System.Drawing.Color]::Transparent)
$g.FillEllipse([System.Drawing.Brushes]::RoyalBlue, 0, 0, 15, 15)
$f = New-Object System.Drawing.Font('Segoe UI', 8, [System.Drawing.FontStyle]::Bold)
$g.DrawString('V', $f, [System.Drawing.Brushes]::White, 2.5, 0.5)
$g.Dispose()
$icon = [System.Drawing.Icon]::FromHandle($bm.GetHicon())

$script:ni = New-Object System.Windows.Forms.NotifyIcon
$script:ni.Icon = $icon
$script:ni.Text = 'wireproxy VPN'
$script:ni.Visible = $true

function Balloon($text, $err) {
    $script:ni.ShowBalloonTip(2500, 'wireproxy', $text,
        $(if ($err) { [System.Windows.Forms.ToolTipIcon]::Error } else { [System.Windows.Forms.ToolTipIcon]::Info }))
}

function Get-LocalDomainsInfo {
    try {
        return Invoke-RestMethod -Uri 'http://127.0.0.1:8080/api/local-domains' -TimeoutSec 3
    } catch { return $null }
}

function Test-PanelDomainMapped($info) {
    if (-not $info) { return $false }
    foreach ($d in $info.domains) {
        if ($d.domain -eq 'panel.localhost' -and [int]$d.port -eq 8080) { return $true }
    }
    return $false
}

# Доверие CA для браузеров = сертификат в LocalMachine\Root (Chrome/Edge его
# требуют; пользовательское хранилище они игнорируют).
function Test-CaTrusted {
    try {
        $store = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root', 'LocalMachine')
        $store.Open('ReadOnly')
        $hit = $store.Certificates | Where-Object { $_.Subject -like '*wireproxy-control local CA*' }
        $store.Close()
        if ($hit) { return $true }
    } catch { }
    return $false
}

function Open-Panel {
    $info = Get-LocalDomainsInfo
    if ((Test-CaTrusted) -and (Test-PanelDomainMapped $info)) {
        Start-Process 'https://panel.localhost/'
    } else {
        Start-Process 'http://127.0.0.1:8080/'
    }
}

function Add-LocalCerts {
    $bat = Join-Path $script:baseDir 'local-domains-trust.bat'
    if (-not (Test-Path $bat)) { Balloon 'Файл local-domains-trust.bat не найден' $true; return }
    if (-not (Test-Path (Join-Path $script:baseDir 'certs\ca.crt'))) {
        Balloon 'Сначала запустите стек — CA ещё не создан' $true
        return
    }
    Start-Process -FilePath $bat
}

function RefreshLists {
    try {
        Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:8080/api/refresh' -Headers @{ 'X-WPC' = '1' } -TimeoutSec 5 | Out-Null
        Balloon 'Обновление списка запущено'
    } catch { Balloon 'Панель не отвечает' $true }
}

function RestartProxy {
    try {
        Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:8080/api/restart-proxy' -Headers @{ 'X-WPC' = '1' } -TimeoutSec 5 | Out-Null
        Balloon 'Перезапуск VPN-прокси'
    } catch { Balloon 'Панель не отвечает' $true }
}

$menu = New-Object System.Windows.Forms.ContextMenuStrip
$menu.add_Opening({
    $menu.Items.Clear()
    $info = Get-LocalDomainsInfo
    $ready = (Test-CaTrusted) -and (Test-PanelDomainMapped $info)
    if ($ready) {
        $mi = $menu.Items.Add('Открыть панель (panel.localhost)')
    } else {
        $mi = $menu.Items.Add('Открыть панель (127.0.0.1:8080)')
    }
    $mi.add_Click({ Open-Panel })

    if (-not $ready) {
        $miCert = $menu.Items.Add('Добавить локальные сертификаты…')
        $miCert.add_Click({ Add-LocalCerts })
    }

    $miRefresh = $menu.Items.Add('Обновить списки доменов')
    $miRefresh.add_Click({ RefreshLists })

    $miRestart = $menu.Items.Add('Перезапустить VPN-прокси')
    $miRestart.add_Click({ RestartProxy })

    [void]$menu.Items.Add('-')

    $miQuit = $menu.Items.Add('Выход (остановить всё)')
    $miQuit.add_Click({
        Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -Name ProxyEnable -Value 0 -ErrorAction SilentlyContinue
        Stop-Process -Name wireproxy -Force -ErrorAction SilentlyContinue
        Stop-Process -Name xray -Force -ErrorAction SilentlyContinue
        Get-CimInstance Win32_Process |
            Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -match 'manager\.js' } |
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
        $script:ni.Visible = $false
        $script:ni.Dispose()
        [System.Windows.Forms.Application]::Exit()
    })
})

$script:ni.ContextMenuStrip = $menu
$script:ni.ShowBalloonTip(3000, 'wireproxy', 'Панель: http://127.0.0.1:8080 — меню в трее', [System.Windows.Forms.ToolTipIcon]::Info)

[System.Windows.Forms.Application]::Run((New-Object System.Windows.Forms.ApplicationContext))
