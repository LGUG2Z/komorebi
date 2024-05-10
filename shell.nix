{pkgs ? import <nixpkgs> {}}:
with pkgs;
  mkShell {
    name = "komorebi";

    buildInputs = [
      python311Packages.mkdocs-material
      python311Packages.mkdocs-macros
      python311Packages.setuptools
    ];
  }
