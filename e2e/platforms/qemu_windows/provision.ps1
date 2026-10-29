# GOLDEN layer provisioning, run in the guest through the agent. The steps of
# the former single-step setup (policies, Defender, VC++ runtime, widgets, QXL driver, firewall);
# the agent copy / Run key / shutdown are not here: the base has the agent and
# the host shuts the guest down. Expects the qxldod files in C:\e2e\drv.
$ErrorActionPreference = 'Continue'
$log = 'C:\e2e-provision.log'
function Log($m) { "$(Get-Date -Format o) $m" | Add-Content $log }
Log "provision start; elevated=$(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) user=$env:USERNAME"

$policies = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies'
Set-ItemProperty "$policies\System" EnableLUA 0
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization' |
  New-ItemProperty -Name NoLockScreen -Value 1 -Force | Out-Null
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\System' |
  New-ItemProperty -Name DisableLockWorkstation -Value 1 -Force | Out-Null
Set-ItemProperty 'HKCU:\Control Panel\Desktop' ScreenSaveActive 0
powercfg /x -monitor-timeout-ac 0
powercfg /x -standby-timeout-ac 0
powercfg /h off
Log 'policies done'

Set-MpPreference -DisableRealtimeMonitoring $true
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender' |
  New-ItemProperty -Name DisableAntiSpyware -Value 1 -Force | Out-Null
foreach ($s in 'wuauserv', 'UsoSvc', 'WaaSMedicSvc') {
  Stop-Service $s -Force -ErrorAction SilentlyContinue
  Set-Service $s -StartupType Disabled -ErrorAction SilentlyContinue
}
Log 'defender/update done'

$ProgressPreference = 'SilentlyContinue'
Invoke-WebRequest https://aka.ms/vs/17/release/vc_redist.x64.exe `
  -OutFile C:\vc_redist.x64.exe
Log 'vc redist downloaded'
Start-Process C:\vc_redist.x64.exe -ArgumentList /quiet,/norestart -Wait
Remove-Item C:\vc_redist.x64.exe -ErrorAction SilentlyContinue
New-Item -Force 'HKLM:\SOFTWARE\Policies\Microsoft\Dsh' |
  New-ItemProperty -Name AllowNewsAndInterests -Value 0 -Force | Out-Null
Log "vc redist: $(Test-Path C:\Windows\System32\vcruntime140.dll)"

pnputil /add-driver C:\e2e\drv\qxldod.inf /install | Out-Null
Log "qxl driver stored: $(pnputil /enum-drivers | Select-String -Pattern 'qxldod' -Quiet)"

Set-NetFirewallProfile -Profile Domain,Public,Private -Enabled False
Log 'provision done'
