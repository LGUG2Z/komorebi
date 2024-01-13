---
hide:
  - navigation
---
## Installation

Komorebi is available pre-built to install via
[Scoop](https://scoop.sh/#/apps?q=komorebi) and
[WinGet](https://winget.run/pkg/LGUG2Z/komorebi), and you may also built
it from [source](https://github.com/LGUG2Z/komorebi) if you would prefer.

 - [Scoop](install/scoop.md)
 - [WinGet](install/winget.md)
 - [Build from source](install/source.md)

## Getting Example Configurations

Run the following command to download example configuration files for
`komorebi` and `whkd`. Pay attention to the output of the command to see
where the example files have been downloaded.

```powershell
komorebic quickstart
```

## Starting Komorebi

Run the following command to start `komorebi` and `whkd` with the example
configurations.

It is important that you run this command for the first time without
making any modifications to the example configurations to validate that
you have a working installation of both `komorebi` and `whkd`.

```powershell
komorebic start --whkd
```
