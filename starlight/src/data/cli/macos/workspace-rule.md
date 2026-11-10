```
Add a rule to associate an application with a workspace

Usage: workspace-rule <IDENTIFIER> <ID> <MONITOR> <WORKSPACE>

Arguments:
  <IDENTIFIER>
          Possible values:
          - exe:   Executable name
          - class: Role or subrole
          - title: Window title
          - path:  Executable path

  <ID>
          Identifier as a string

  <MONITOR>
          Monitor index (zero-indexed)

  <WORKSPACE>
          Workspace index on the specified monitor (zero-indexed)

Options:
  -h, --help
          Print help (see a summary with '-h')

```