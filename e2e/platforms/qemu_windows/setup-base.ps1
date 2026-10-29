# BASE layer first-logon: only what is needed to reach a desktop with an
# elevated agent running at every logon. Provisioning is the golden layer's job.
$log = 'C:\e2e-setup.log'
function Log($m) { "$(Get-Date -Format o) $m" | Add-Content $log }
Log 'base setup start'
# the agent is the transport of the golden layer and must run elevated
# (UAC off takes effect at the next boot, which is the layer build's)
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' EnableLUA 0
$drive = Get-PSDrive -PSProvider FileSystem |
  Where-Object { Test-Path (Join-Path $_.Root 'komorebi-e2e-agent.exe') } |
  Select-Object -First 1
New-Item -Force -ItemType Directory C:\e2e\bin | Out-Null
Copy-Item (Join-Path $drive.Root 'komorebi-e2e-agent.exe') C:\e2e\bin
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' `
  KomorebiE2eAgent 'C:\e2e\bin\komorebi-e2e-agent.exe'
# the agent links the VC++ runtime dynamically, so it cannot start without it
$ProgressPreference = 'SilentlyContinue'
Invoke-WebRequest https://aka.ms/vs/17/release/vc_redist.x64.exe -OutFile C:\vc_redist.x64.exe
Start-Process C:\vc_redist.x64.exe -ArgumentList /quiet,/norestart -Wait
Remove-Item C:\vc_redist.x64.exe -ErrorAction SilentlyContinue
Log "vc redist: $(Test-Path C:\Windows\System32\vcruntime140.dll)"
# the host reaches the agent through a QEMU port forward
Set-NetFirewallProfile -Profile Domain,Public,Private -Enabled False
Log 'base setup done'
shutdown /s /t 5
