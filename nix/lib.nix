# Helpers shared by the platform modules of the flake.
{ inputs }:
rec {
  # the repository, as the source of the flake
  repoRoot = ./..;

  # a file or directory of the repository as a store path of its own: `./tools/format-all` would name
  # a place in the store path of the whole flake source, which changes with every edit of the
  # repository and so would rebuild the derivation that reads it
  repoFile =
    rel:
    builtins.path {
      path = repoRoot + "/${rel}";
      name = builtins.baseNameOf rel;
    };

  # the version of the workspace: every package says `version.workspace = true`
  version =
    (builtins.fromTOML (builtins.readFile (repoRoot + "/Cargo.toml"))).workspace.package.version;

  # the systems that cross-build for Windows, build the Linux packages and build the macOS packages
  windowsSystems = [
    "x86_64-linux"
    "aarch64-linux"
    "aarch64-darwin"
  ];
  linuxSystems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  # Apple silicon also gets the checks of the KDE crates (they run against a fake desktop)
  linuxCheckSystems = linuxSystems ++ [ "aarch64-darwin" ];
  macosSystems = [
    "aarch64-darwin"
    "x86_64-darwin"
  ];

  # The manifests (Cargo.toml, Cargo.lock, build scripts, .cargo/config.toml) of every member of the
  # workspace, of all three platforms, and nothing else: crane makes the dummy source of the dependency
  # derivations from it. A pure path (no derivation), so that crane reads it at evaluation without
  # building anything, which `nix flake check --all-systems` needs on a host that cannot build for the
  # other systems; and the same for every platform, so that the dependencies do not depend on the stubs.
  manifestSource =
    lib:
    lib.fileset.toSource {
      root = repoRoot;
      fileset = lib.fileset.fileFilter (
        file:
        builtins.elem file.name [
          "Cargo.toml"
          "Cargo.lock"
          "build.rs"
          "config.toml"
        ]
      ) repoRoot;
    };

  # nixpkgs with the rust overlay, for a system (x86_64-darwin: see the input nixpkgs-x86_64-darwin)
  mkPkgs = system: pkgsOf.${system};
  pkgsOf = builtins.listToAttrs (
    map (system: {
      name = system;
      value =
        import (if system == "x86_64-darwin" then inputs.nixpkgs-x86_64-darwin else inputs.nixpkgs)
          {
            inherit system;
            overlays = [ (import inputs.rust-overlay) ];
          };
    }) (windowsSystems ++ linuxSystems ++ macosSystems)
  );
}
