# window-hiding-behaviour

```
Set the window behaviour when switching workspaces / cycling stacks

Usage: komorebic.exe window-hiding-behaviour <HIDING_BEHAVIOUR>

Arguments:
  <HIDING_BEHAVIOUR>
          Possible values:
          - hide:     Use the SW_HIDE flag to hide windows when switching workspaces (has issues with Electron apps)
          - minimize: Use the SW_MINIMIZE flag to hide windows when switching workspaces (has issues with frequent workspace switching)
          - cloak:    Use the undocumented SetCloak Win32 function to hide windows when switching workspaces (has foregrounding issues)

Options:
  -h, --help
          Print help (see a summary with '-h')

```
