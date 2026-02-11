param(
    [string]$VelocityExe = "velocity",
    [string]$Config = "./velocity.toml",
    [string]$DataDir = "./velocitydb"
)

Add-Type -AssemblyName System.Windows.Forms, System.Drawing

$notifyIcon = New-Object System.Windows.Forms.NotifyIcon
$notifyIcon.Icon = [System.Drawing.SystemIcons]::Application
$notifyIcon.Text = "Velocity Service Controller"
$notifyIcon.Visible = $true

$contextMenu = New-Object System.Windows.Forms.ContextMenuStrip
$startItem = $contextMenu.Items.Add("Start Velocity")
$stopItem = $contextMenu.Items.Add("Stop Velocity")
$studioItem = $contextMenu.Items.Add("Open Studio")
$exitItem = $contextMenu.Items.Add("Exit")
$notifyIcon.ContextMenuStrip = $contextMenu

$serviceProcess = $null

function Start-VelocityService {
    if ($serviceProcess -and -not $serviceProcess.HasExited) {
        return
    }
    $args = @("service", "run", "--config", $Config, "--data-dir", $DataDir, "--verbose")
    $serviceProcess = Start-Process -FilePath $VelocityExe -ArgumentList $args -PassThru -WindowStyle Hidden
    $notifyIcon.ShowBalloonTip(1500, "Velocity", "Service started", [System.Windows.Forms.ToolTipIcon]::Info)
}

function Stop-VelocityService {
    if ($serviceProcess -and -not $serviceProcess.HasExited) {
        $serviceProcess.Kill()
        $serviceProcess.WaitForExit()
        $notifyIcon.ShowBalloonTip(1500, "Velocity", "Service stopped", [System.Windows.Forms.ToolTipIcon]::Info)
    }
}

function Open-Studio {
    Start-Process "http://127.0.0.1:3000"
}

$startItem.Add_Click({ Start-VelocityService })
$stopItem.Add_Click({ Stop-VelocityService })
$studioItem.Add_Click({ Open-Studio })
$exitItem.Add_Click({
    Stop-VelocityService
    $notifyIcon.Visible = $false
    [System.Windows.Forms.Application]::Exit()
})

Register-EngineEvent PowerShell.Exiting -Action { Stop-VelocityService }
Start-VelocityService
[System.Windows.Forms.Application]::Run()
