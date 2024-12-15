{pkgs ? import <nixpkgs> {}}:
with pkgs;
  mkShell {
    name = "komorebi";

    buildInputs = [
      python311Packages.mkdocs-material
      python311Packages.mkdocs-macros
      python311Packages.setuptools
      python311Packages.json-schema-for-humans
    ];
  }
