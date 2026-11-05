# schemars-nixgen

Writes Nix module options from a JSON Schema made by `schemars`. komorebi's NixOS and nix-darwin modules type their
`config` with the options it writes from `schemas/<platform>/schema.json`.

It is a development tool: no release ships it, and it is not a default member of the workspace, so a bare
`cargo build` leaves it out. Run it from the tree:

```sh
just nixgen-linux    # nix/linux/komorebi-options.nix
just nixgen-macos    # nix/macos/komorebi-options.nix and komorebi-bar-options.nix
```

The first line of what it writes is `# schema-sha256: <hash>`, the hash of the schema it was made from, which is how
the `linux-schema-fresh` check tells that the options are stale without running it.

`nix build .#schemars-nixgen` builds it, and `nix build .#checkSets.nixgen-lint .#checkSets.nixgen-test` check it.
