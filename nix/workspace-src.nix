# The source of a build from the one Cargo workspace of this repository.
#
# Cargo has to load every member of the workspace, but a build of one platform needs the sources of
# that platform's crates only. `real` is the filtered source (the platform's crates, the crates of the
# root workspace they depend on, the root manifest and lock); every other member (`isReal` is false)
# is added as its own manifest with an empty library and a `main` that does nothing. An edit of a
# crate of another platform then changes nothing here, and an edit of another platform's manifest
# changes only the stub copy of it.
#
#   workspaceSrc { pkgs; repoRoot; real; isReal = member: bool; }
{
  pkgs,
  repoRoot,
  real,
  isReal,
}:
let
  inherit (pkgs) lib;
  members = (builtins.fromTOML (builtins.readFile "${real}/Cargo.toml")).workspace.members;
  stub = member: ''
    mkdir -p $out/${member}/src
    cp ${
      builtins.path {
        path = repoRoot + "/${member}/Cargo.toml";
        name = "Cargo.toml";
      }
    } $out/${member}/Cargo.toml
    : > $out/${member}/src/lib.rs
    echo 'fn main() {}' > $out/${member}/src/main.rs
  '';
in
pkgs.runCommand "komorebi-workspace-src" { } ''
  cp -r ${real} $out
  chmod -R u+w $out
  ${lib.concatMapStrings stub (builtins.filter (m: !(isReal m)) members)}
''
