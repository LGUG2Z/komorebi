```text
Turn an skhdrc into KDE global shortcuts (they work over RDP, unlike evdev hotkey daemons)

Usage: export-kde-shortcuts [OPTIONS]

Options:
  -s, --skhdrc <SKHDRC>
          The skhdrc to translate [default: skhdrc in the komorebi config directory]

  -o, --output <OUTPUT>
          Where to write the shortcut files [default: $XDG_DATA_HOME/kglobalaccel]

      --reload
          Activate shortcuts that are new straight away, instead of at the next login

      --dry-run
          Show the shortcuts that would be written without writing anything

  -h, --help
          Print help

```
