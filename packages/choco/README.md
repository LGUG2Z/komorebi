# [![chocolatey](https://img.shields.io/chocolatey/v/komorebi.svg?color=red&label=komorebi)](https://chocolatey.org/packages/komorebi)

This is the Chocolatey package of komorebi.

## Build and Test From Source

Place `komorebi-$version-x86_64-pc-windows-msvc.zip` to `tools`, then run:

```shell
choco pack
choco install komorebi -s "'.;https://community.chocolatey.org/api/v2/'"
```
