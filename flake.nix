{
  description = "komorebi, a tiling window manager for Windows, KDE Plasma and macOS";

  inputs = {
    # One nixpkgs for the three platforms. The KDE VM tests run against the Plasma and KWin of this
    # revision (nix/linux/supported-kwin.nix says which KWin that is), so it is the one an update has
    # to keep green for the VM suites; see docs/dev/flake.md.
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # Intel Macs only: nixpkgs 26.11 dropped x86_64-darwin (evaluating it throws), so the x86_64-darwin
    # outputs use the last revision that still supported it, the one the macOS flake was pinned to.
    # Nothing else uses it; drop it with x86_64-darwin when Intel Macs are no longer supported.
    nixpkgs-x86_64-darwin.url = "github:NixOS/nixpkgs/16c7794d0a28b5a37904d55bcca36003b9109aaa";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    git-hooks-nix.url = "github:cachix/git-hooks.nix";
    git-hooks-nix.inputs.nixpkgs.follows = "nixpkgs";
    # the application-specific configuration that komorebic embeds on Windows
    asc = {
      url = "github:LGUG2Z/komorebi-application-specific-configuration";
      flake = false;
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        ./nix/common.nix
        ./nix/checks.nix
        ./nix/windows/flake-module.nix
        ./nix/linux/flake-module.nix
        ./nix/macos/flake-module.nix
        ./nix/recorder/flake-module.nix
        ./nix/nixgen/flake-module.nix
        ./nix/docgen/flake-module.nix
      ];
    };
}
