```
Set the layout for the specified workspace

Usage: workspace-layout <MONITOR> <WORKSPACE> <VALUE>

Arguments:
  <MONITOR>
          Monitor index (zero-indexed)

  <WORKSPACE>
          Workspace index on the specified monitor (zero-indexed)

  <VALUE>
          Possible values:
          - bsp:                       BSP Layout
          - columns:                   Columns Layout
          - rows:                      Rows Layout
          - vertical-stack:            Vertical Stack Layout
          - horizontal-stack:          Horizontal Stack Layout
          - ultrawide-vertical-stack:  Ultrawide Vertical Stack Layout
          - grid:                      Grid Layout
          - right-main-vertical-stack: Right Main Vertical Stack Layout
          - scrolling:                 Scrolling Layout

Options:
  -h, --help
          Print help (see a summary with '-h')

```