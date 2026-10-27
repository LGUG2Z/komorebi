{
  description = "komorebi for KDE Plasma";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    git-hooks-nix.url = "github:cachix/git-hooks.nix";
    git-hooks-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-parts,
      crane,
      rust-overlay,
      ...
    }:
    let
      mkPkgs =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

      mkKomorebiPackages =
        { pkgs }:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          # rustfmt.toml uses options only a nightly rustfmt knows
          craneLibFmt = (crane.mkLib pkgs).overrideToolchain (
            pkgs.rust-bin.nightly.latest.minimal.override { extensions = [ "rustfmt" ]; }
          );

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              # the script komorebi loads into KWin, which the daemon includes
              || (pkgs.lib.hasSuffix ".js" path)
              # the messages the tests of the bridge and of the daemon share, the configurations
              # the tests load, and the schemas
              || (pkgs.lib.hasSuffix ".json" path)
              # the page of the docs that shows the example configuration, which a test compares
              # with the example file
              || (pkgs.lib.hasSuffix "docs/example-configurations.md" path)
              # the example a test loads, to see that the docs show one that works
              || (pkgs.lib.hasSuffix "docs/usage/wallpapers.md" path)
              # the sample key bindings that `komorebic quickstart` includes
              || (pkgs.lib.hasSuffix "docs/skhdrc.sample" path)
              # the key binding files the komorebi-hotkeys tests read
              || (pkgs.lib.hasInfix "/komorebi-hotkeys/tests/fixtures/" path);
          };

          commonArgs = {
            inherit src;
            pname = "komorebi-workspace";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
            strictDeps = true;
            # The version crate wants a commit at compile time. It is a constant here, so that the
            # dependencies and every check are the same derivations for every commit; only the final
            # build below knows the real one.
            COMMIT_HASH = "nix-check";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            doDoc = false;
          };

          fullBuild = craneLib.buildPackage (
            individualCrateArgs
            // {
              COMMIT_HASH = self.rev or (pkgs.lib.removeSuffix "-dirty" self.dirtyRev);
            }
          );

          extractBinary =
            binaryName:
            pkgs.runCommand "komorebi-${binaryName}" { meta = fullBuild.meta // { }; } ''
              mkdir -p $out/bin
              cp ${fullBuild}/bin/${binaryName} $out/bin/
            '';
        in
        {
          inherit
            craneLib
            craneLibFmt
            src
            commonArgs
            individualCrateArgs
            fullBuild
            ;
          komorebi = extractBinary "komorebi";
          komorebic = extractBinary "komorebic";
          # komorebic with the constant commit of the checks, whose output the schemas are
          # checked against: it is the same for every commit
          komorebicForChecks = craneLib.buildPackage (
            individualCrateArgs
            // {
              pname = "komorebic-for-checks";
              cargoExtraArgs = "--package komorebic --bin komorebic";
            }
          );
        };
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      # Linux is what komorebi runs on. An Apple silicon Mac gets the dev shell, the formatter and the
      # checks, which run the same tests against the fake desktop. nixpkgs dropped x86_64-darwin in
      # 26.11.
      systems = [
        "aarch64-linux"
        "x86_64-linux"
        "aarch64-darwin"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem =
        {
          config,
          lib,
          system,
          ...
        }:
        let
          pkgs = mkPkgs system;
          build = mkKomorebiPackages { inherit pkgs; };
          isLinux = pkgs.stdenv.hostPlatform.isLinux;
          # the guests install komorebi through the module, as a NixOS user does
          komorebiModule = self.nixosModules.default;
          # a Mac runs the guests under HVF, which is slower than KVM
          vm = import ./nix/tests/vm {
            hostPkgs = pkgs;
            inherit komorebiModule;
            scale = if isLinux then 1 else 1.5;
          };
          nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
          nightlyToolchain = pkgs.rust-bin.nightly.latest.default;
          rustToolchainWithNightlyRustfmt = pkgs.symlinkJoin {
            name = "rust-toolchain-with-nightly-rustfmt";
            paths = [
              nightlyRustfmt
              (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
            ];
          };
          # cargo-udeps needs a nightly compiler
          cargo-udeps = pkgs.writeShellScriptBin "cargo-udeps" ''
            export PATH="${nightlyToolchain}/bin:$PATH"
            exec ${pkgs.cargo-udeps}/bin/cargo-udeps "$@"
          '';
        in
        {
          treefmt = {
            projectRootFile = "flake.nix";
            programs = {
              deadnix.enable = true;
              just.enable = true;
              nixfmt.enable = true;
              taplo.enable = true;
              rustfmt = {
                enable = true;
                package = nightlyRustfmt;
              };
            };
          };

          checks = {
            komorebi-workspace-fmt = build.craneLibFmt.cargoFmt { inherit (build) src; };

            komorebi-workspace-clippy = build.craneLib.cargoClippy (
              build.individualCrateArgs
              // {
                cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
              }
            );

            komorebi-workspace-doc = build.craneLib.cargoDoc (
              build.individualCrateArgs
              // {
                cargoDocExtraArgs = "--workspace --no-deps";
                env.RUSTDOCFLAGS = "-D warnings";
              }
            );

            komorebi-workspace-toml-fmt = build.craneLib.taploFmt {
              src = lib.sources.sourceFilesBySuffices build.src [ ".toml" ];
            };

            komorebi-workspace-deny = build.craneLib.cargoDeny { inherit (build) src; };

            komorebi-workspace-nextest = build.craneLib.cargoNextest (
              build.individualCrateArgs // { doCheck = true; }
            );

            # The bridge is not Rust: tsc checks its types from its JSDoc comments, and node runs
            # its tests against a fake KWin. No package.json and no build step.
            komorebi-bridge-js =
              pkgs.runCommand "komorebi-bridge-js"
                {
                  nativeBuildInputs = [
                    pkgs.nodejs
                    pkgs.typescript
                  ];
                  src = lib.fileset.toSource {
                    root = ./komorebi/src/kwin;
                    fileset = lib.fileset.fileFilter (file: !file.hasExt "rs") ./komorebi/src/kwin;
                  };
                }
                ''
                  cd $src
                  tsc -p tsconfig.json
                  node --test bridge.test.js
                  touch $out
                '';

            vm-lib = vm.lib;

            # The schemas, the options made from one of them and the example are what the Rust types
            # say: `just jsonschema` and `just nixgen` write them
            schema-fresh =
              pkgs.runCommand "komorebi-schema-fresh"
                {
                  nativeBuildInputs = [
                    build.komorebicForChecks
                    pkgs.diffutils
                  ];
                }
                ''
                  komorebic static-config-schema | diff -u ${./schema.json} - \
                    || { echo "schema.json is stale: run just jsonschema"; exit 1; }
                  komorebic application-specific-configuration-schema | diff -u ${./schema.asc.json} - \
                    || { echo "schema.asc.json is stale: run just jsonschema"; exit 1; }
                  expected=$(sha256sum ${./schema.json} | cut -d' ' -f1)
                  head -n 1 ${./nix/komorebi-options.nix} | grep -qx "# schema-sha256: $expected" \
                    || { echo "nix/komorebi-options.nix is stale: run just nixgen"; exit 1; }
                  touch $out
                '';

            # the options the module is typed with, evaluated against the example configuration
            nix-options-validation = import ./nix/tests/default.nix { inherit pkgs; };

            # every file of the key binding corpus through `komorebic hotkeys compile`, as the
            # module runs it
            keybindings-fixtures = import ./nix/tests/keybindings.nix {
              inherit pkgs;
              komorebic = build.fullBuild;
            };

            # The NixOS module in real NixOS systems, evaluated and never built, so a Mac checks it
            # too. A VM suite, `vm-module`, runs it.
            nixos-module-eval = import ./nix/tests/nixos-module.nix {
              inherit pkgs;
              inherit (nixpkgs.lib) nixosSystem;
              module = self.nixosModules.default;
              komorebiFor = guestSystem: self.packages.${guestSystem}.komorebi-full;
              borderEffectFor = guestSystem: self.packages.${guestSystem}.kde-rounded-corners-komorebi;
              animationEffectFor = guestSystem: self.packages.${guestSystem}.kwin-effect-geometry-change-komorebi;
            };

            # The Plasma guests to develop against, evaluated and never built, so that every system
            # sees an option that nixpkgs renamed or an assertion that fails. The QEMU guests are
            # systems in a VM (`vmVariant`), with a runner for the Mac; UTM's is a system of its own.
            plasma-guests-eval =
              let
                guests = self.nixosConfigurations;
                qemuGuest = name: {
                  ${name} = guests.${name}.config.virtualisation.vmVariant.system.build.toplevel;
                  "${name}, its runner" = guests.${name}.config.system.build.vm;
                };
                builds =
                  qemuGuest "plasma-vm"
                  // qemuGuest "plasma-vm-session"
                  // {
                    plasma-utm = guests.plasma-utm.config.system.build.toplevel;
                  };
                lines = lib.mapAttrsToList (
                  name: drv: "${name}: ${builtins.unsafeDiscardStringContext drv.drvPath}"
                ) builds;
              in
              pkgs.runCommand "plasma-guests-eval" { } ''
                ${lib.concatMapStringsSep "\n" (line: "echo ${lib.escapeShellArg line}") lines}
                touch $out
              '';
          }
          # The VM tests need QEMU with KVM, so only Linux checks them. A Mac runs the same tests
          # through legacyPackages.<system>.vm, see docs/dev/vm-tests.md.
          // lib.optionalAttrs isLinux (
            lib.mapAttrs' (name: test: lib.nameValuePair "vm-${name}" test) vm.suites
          )
          # the KWin effects komorebi drives, installed and inspected without a session
          // lib.optionalAttrs isLinux (
            import ./nix/tests/effects.nix {
              inherit pkgs;
              inherit (config) packages;
            }
          );

          legacyPackages = {
            # every suite, as a test with a `driver`, on every system
            vm = vm.suites;
            # the suites of each tier, for the CI matrix
            vmTiers = vm.tiers;
            # the guests of every suite, to build on a Linux builder when the host is a Mac
            vmGuests = vm.guests;
          }
          // lib.optionalAttrs isLinux {
            # the same suites on a machine with no KVM, which is slow but works anywhere
            vmTcg =
              (import ./nix/tests/vm {
                hostPkgs = pkgs;
                inherit komorebiModule;
                scale = 8;
              }).suites;
          };

          packages = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            inherit (build) komorebi komorebic;
            komorebi-full = build.fullBuild;
            default = build.fullBuild;
            kde-rounded-corners-komorebi = pkgs.callPackage ./nix/kde-rounded-corners.nix { };
            kwin-effect-geometry-change-komorebi = pkgs.callPackage ./nix/kwin-effect-geometry-change.nix { };
          };

          apps = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            komorebi = {
              type = "app";
              program = "${build.komorebi}/bin/komorebi";
              meta.description = "The komorebi daemon";
            };
            komorebic = {
              type = "app";
              program = "${build.komorebic}/bin/komorebic";
              meta.description = "The command line interface of komorebi";
            };
            default = config.apps.komorebi;
          };

          devShells.default = pkgs.mkShell {
            name = "komorebi";

            RUST_BACKTRACE = "full";

            inputsFrom = [ build.fullBuild ];

            packages = [
              rustToolchainWithNightlyRustfmt
              cargo-udeps

              pkgs.cargo-deny
              pkgs.cargo-nextest
              pkgs.check-jsonschema
              pkgs.jq
              pkgs.just
              pkgs.ripgrep

              # the tests of the sleep and lock signals start private buses (`kwin::power`)
              pkgs.dbus

              # the bridge's type check and tests (`just check-js`)
              pkgs.nodejs
              pkgs.typescript

              # the VM test library
              pkgs.ruff
              pkgs.ty
              (pkgs.python3.withPackages (ps: [ ps.pytest ]))
            ];
          };

          pre-commit = {
            check.enable = true;
            settings.hooks.treefmt = {
              enable = true;
              package = config.treefmt.build.wrapper;
              pass_filenames = false;
            };
          };
        };

      flake = {
        overlays.default =
          final: _:
          let
            build = mkKomorebiPackages { pkgs = mkPkgs final.stdenv.hostPlatform.system; };
          in
          {
            inherit (build) komorebi komorebic;
            komorebi-full = build.fullBuild;
            # with the consumer's KWin, so that the plugin links the libkwin it is loaded into
            kde-rounded-corners-komorebi = final.callPackage ./nix/kde-rounded-corners.nix { };
            kwin-effect-geometry-change-komorebi = final.callPackage ./nix/kwin-effect-geometry-change.nix { };
          };
        overlays.komorebi = self.overlays.default;

        # `services.komorebi`, with the overlay that provides its package, so that a NixOS
        # configuration needs this flake as an input and one module line (docs/installation.md)
        nixosModules.default = {
          imports = [ ./nix/nixos-module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
        };
        nixosModules.komorebi = self.nixosModules.default;

        # Plasma guests to develop against, for an Apple silicon Mac (docs/dev/utm.md): QEMU runs
        # the first two under HVF, and UTM boots the third from a disk image
        nixosConfigurations =
          let
            mkPlasmaGuest =
              modules:
              nixpkgs.lib.nixosSystem {
                specialArgs.hostPkgs = nixpkgs.legacyPackages.aarch64-darwin;
                modules = [
                  self.nixosModules.default
                  { nixpkgs.hostPlatform = "aarch64-linux"; }
                ]
                ++ modules;
              };
          in
          {
            plasma-vm = mkPlasmaGuest [ ./nix/dev/plasma-vm.nix ];
            plasma-vm-session = mkPlasmaGuest [
              ./nix/dev/plasma-vm.nix
              { komorebi.plasmaVm.session = true; }
            ];
            plasma-utm = mkPlasmaGuest [ ./nix/dev/plasma-utm.nix ];
          };
      };
    };
}
