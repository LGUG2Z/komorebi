# The versions of KWin komorebi supports. The NixOS module checks the KWin of a system against
# them (nix/nixos-module.nix). The file is a plain attribute set, so a script or a CI job can
# read it without the flake:
#
#   nix eval --raw --file nix/supported-kwin.nix testedMax
{
  # The oldest KWin komorebi runs on. The bridge reads the work area of each output for the
  # virtual desktop that output shows, which needs the per-output virtual desktops of KWin 6.8.
  min = "6.8";

  # The newest KWin that every VM suite has passed against. A commit that raises it says which
  # suites ran, and against which nixpkgs (CONTRIBUTING.md).
  # 2026-10-27: every VM suite, nixpkgs-unstable d3b13ad (2026-10-24), KWin 6.8.1.
  testedMax = "6.8.1";
}
