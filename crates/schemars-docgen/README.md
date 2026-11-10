# schemars-docgen

Writes the reference pages of the docs site (`starlight/`) from a JSON Schema made by `schemars`: one MDX page with
every type, property, default and enum variant of `schemas/<platform>/schema.json` or `schema.bar.json`, with links
between the types.

It is a development tool: no release ships it, and it is not a default member of the workspace, so a bare
`cargo build` leaves it out. Run it from the tree:

```sh
just docgen    # on Windows or macOS: the schema pages, the command pages and the komorebic page, into starlight/
```

or by hand, for one schema:

```sh
cargo run -p schemars-docgen -- schemas/macos/schema.json --output page.mdx --title "komorebi.json" --description "..."
```

`ENUMS.md` is the analysis of how `schemars` writes each shape of Rust enum into a schema, which is what the tool
turns back into text.

`nix build .#schemars-docgen` builds it, and `nix build .#checkSets.docgen-lint .#checkSets.docgen-test` check it.
