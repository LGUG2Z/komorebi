$ErrorActionPreference = "Stop";

$toolsDir = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  fileFullPath64 = Get-Item $toolsDir\komorebi-*-x86_64-pc-windows-msvc.zip
  destination    = $toolsDir
}
Get-ChocolateyUnzip @packageArgs

# Don't need zip anymore
Remove-Item $toolsDir\*.zip, $toolsDir\*.zip -ea 0 -force

Write-Host "Run 'Copy-Item $toolsDir\komorebi.sample.ahk $env:USERPROFILE\komorebi.ahk' to get started with the sample configuration."
Write-Host "Run 'komorebic ahk-library' if you would like to generate an AHK helper library to use in your configuration."
Write-Host "Once you have a configuration file in place, you can run 'komorebic start' to start the window manager."
