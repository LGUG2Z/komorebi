## Installing with Scoop

It highly recommended that you enable support for long paths in Windows by running the following command in an Administrator Terminal before installing komorebi.

```powershell
Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -Value 1
```
### Install komorebi and whkd

This command installs `komorebi` and `whkd`.
```
winget install LGUG2Z.komorebi
winget install LGUG2Z.whkd
```

Once komorebi is installed, proceed to get the [example configurations](../getting-started.html#getting-example-configurations).
