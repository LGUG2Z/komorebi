$packageName  = $env:ChocolateyPackageName

$p = Get-Process -Name komorebi -ea 0
if (!$p) {
    Write-Host "$packageName is not running."
    return
}

Write-Host "$packageName is running, trying to gracefully shut it down before upgrade/uninstall..."
komorebic stop
