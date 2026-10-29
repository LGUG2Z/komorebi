# First-logon provisioning for the e2e golden image. Runs from the answer
# file ISO; finishes by shutting the guest down so the host sees it exit.
$ErrorActionPreference = 'Continue'
$log = 'C:\e2e-setup.log'
function Log($m) { "$(Get-Date -Format o) $m" | Add-Content $log }
Log 'setup start'

# no UAC prompts, no lock screen, no screensaver, no display or sleep timeout
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' EnableLUA 0
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization' |
  New-ItemProperty -Name NoLockScreen -Value 1 -Force | Out-Null
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' |
  New-ItemProperty -Name DisableLockWorkstation -Value 1 -Force | Out-Null
Set-ItemProperty 'HKCU:\Control Panel\Desktop' ScreenSaveActive 0
powercfg /x -monitor-timeout-ac 0
powercfg /x -standby-timeout-ac 0
powercfg /h off

# no Defender real-time scanning, no Windows Update
Set-MpPreference -DisableRealtimeMonitoring $true
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender' |
  New-ItemProperty -Name DisableAntiSpyware -Value 1 -Force | Out-Null
foreach ($s in 'wuauserv', 'UsoSvc', 'WaaSMedicSvc') {
  Stop-Service $s -Force -ErrorAction SilentlyContinue
  Set-Service $s -StartupType Disabled -ErrorAction SilentlyContinue
}

Log 'setup done'
shutdown /s /t 5
