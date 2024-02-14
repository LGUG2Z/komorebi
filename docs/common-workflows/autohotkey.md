# AutoHotKey

<!-- TODO: Update this completely -->

If you would like to use Autohotkey, please make sure you have AutoHotKey v2
installed.

Generally, users who opt for AHK will have specific needs that can only be
addressed by the advanced functionality of AHK, and so they are assumed to be
able to craft their own configuration files.

If you would like to try out AHK, a simple sample configuration powered by
`komorebic.lib.ahk` is provided as a starting point. This sample configuration
does not take into account the use of a static configuration file; if you
choose to use a static configuration file alongside AHK, you can remove all the
configuration options from your `komorebi.ahk` and use it solely to handle
hotkey bindings.


```powershell
# save the latest generated komorebic library to ~/komorebic.lib.ahk
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/v0.1.20/komorebic.lib.ahk -OutFile $Env:USERPROFILE\komorebic.lib.ahk

# save the latest generated app-specific config tweaks and fixes to ~/komorebi.generated.ahk
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/v0.1.20/komorebi.generated.ahk -OutFile $Env:USERPROFILE\komorebi.generated.ahk

# save the sample komorebi configuration file to ~/komorebi.ahk
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/v0.1.20/komorebi.sample.ahk -OutFile $Env:USERPROFILE\komorebi.ahk
```

