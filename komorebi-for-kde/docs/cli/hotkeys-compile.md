```text
Plan the bindings of a key binding file, and write those that are static shortcuts as KDE shortcut
files

Usage: compile [OPTIONS] [FILE]

Arguments:
  [FILE]
          The key binding file [default: the first of skhdrc, whkdrc and sxhkdrc in the komorebi
          config directory]

Options:
  -f, --format <FORMAT>
          The format of the file [default: from the end of its name: skhdrc, whkdrc or sxhkdrc]
          
          [possible values: skhd, whkd, sxhkd]

  -m, --mode <MODE>
          Which bindings are static shortcuts: none (engine), every binding a shortcut file can
          express (auto), or only those, leaving the others out (static)
          
          [default: engine]
          [possible values: auto, engine, static]

  -o, --output <OUTPUT>
          Where to write the shortcut files [default: $XDG_DATA_HOME/kglobalaccel]

      --strict
          Write nothing, and fail, when a diagnostic is a warning or an error: something in the file
          is not realized the way its own hotkey daemon realizes it

      --reload
          Activate shortcuts that are new straight away, instead of at the next login

      --dry-run
          Show the shortcuts that would be written without writing anything

  -h, --help
          Print help

```
