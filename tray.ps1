# Иконка wireproxy в системном трее (чистый PowerShell + WinForms, без зависимостей).
# Меню: открыть панель, обновить списки, перезапустить прокси, выход.
Add-Type -AssemblyName System.Windows.Forms | Out-Null
Add-Type -AssemblyName System.Drawing | Out-Null

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
$script:ni.Text = 'wireproxy VPN — панель: http://127.0.0.1:8080'
$script:ni.Visible = $true

function Balloon($text, $err) {
    $script:ni.ShowBalloonTip(2500, 'wireproxy', $text,
        $(if ($err) { [System.Windows.Forms.ToolTipIcon]::Error } else { [System.Windows.Forms.ToolTipIcon]::Info }))
}

$menu = New-Object System.Windows.Forms.ContextMenuStrip

$mi1 = $menu.Items.Add('Открыть панель управления')
$mi1.add_Click({ Start-Process 'http://127.0.0.1:8080/' })

$mi2 = $menu.Items.Add('Обновить списки доменов')
$mi2.add_Click({
    try {
        Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:8080/api/refresh' -TimeoutSec 5 | Out-Null
        Balloon 'Обновление списка запущено'
    } catch { Balloon 'Панель не отвечает' $true }
})

$mi3 = $menu.Items.Add('Перезапустить VPN-прокси')
$mi3.add_Click({
    try {
        Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:8080/api/restart-proxy' -TimeoutSec 5 | Out-Null
        Balloon 'Перезапуск VPN-прокси'
    } catch { Balloon 'Панель не отвечает' $true }
})

[void]$menu.Items.Add('-')

$mi4 = $menu.Items.Add('Выход (остановить всё)')
$mi4.add_Click({
    Stop-Process -Name wireproxy -Force -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process |
        Where-Object { $_.CommandLine -match 'manager\.js' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    $script:ni.Visible = $false
    $script:ni.Dispose()
    [System.Windows.Forms.Application]::Exit()
})

$script:ni.ContextMenuStrip = $menu
$script:ni.ShowBalloonTip(3000, 'wireproxy', 'VPN-прокси работает. Панель: http://127.0.0.1:8080', [System.Windows.Forms.ToolTipIcon]::Info)

[System.Windows.Forms.Application]::Run((New-Object System.Windows.Forms.ApplicationContext))
