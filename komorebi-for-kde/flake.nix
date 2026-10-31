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

  # The outputs are a function of the repository they are built from, so that the root flake can
  # build them with the whole repository as the source (the Rust workspace here depends on the
  # crates of the root workspace by path) and with its own `self`. `lib.mkOutputs` is that function.
  # The outputs of this flake on its own use the directory it is in: that evaluates everything and
  # builds what is not Rust, but cannot build the Rust workspace.
  outputs =
    inputs@{ self, ... }:
    let
      mkOutputs =
        {
          inputs,
          self,
          # the source of the repository, and the path of this tree in it
          repoRoot,
          platformDir,
        }:
        let
          inherit (inputs)
            nixpkgs
            flake-parts
            crane
            rust-overlay
            ;
          # the crates of the root workspace that this workspace depends on, by path, directly or not.
          # Only these are in the source, so that an edit of another crate (the bar's) changes nothing
          # here. `tools/kde-closure-check` fails if this list and `cargo metadata` differ.
          rootCrates = builtins.fromJSON (builtins.readFile ./nix/root-crates.json);
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

              # the repository, cut down to this workspace and the crates of the root workspace it depends
              # on by path (komorebi-layouts and what they depend on, komorebi-themes), with the root
              # manifest those crates inherit their dependencies from
              real = pkgs.lib.cleanSourceWith {
                src = repoRoot;
                filter =
                  path: type:
                  let
                    relative = pkgs.lib.removePrefix "${toString repoRoot}/" path;
                    inTree =
                      platformDir == "" || relative == platformDir || pkgs.lib.hasPrefix "${platformDir}/" relative;
                    rootCrate = builtins.any (
                      crate: relative == crate || pkgs.lib.hasPrefix "${crate}/" relative
                    ) rootCrates;
                    # the directory that holds the crates of crates/, to reach the listed ones
                    rootCrateParent = relative == "crates";
                    # the unified komorebic package: its manifest, build script, main and the Linux module
                    # and tests; not the Windows and macOS files
                    komorebic =
                      builtins.elem relative [
                        "komorebic"
                        "komorebic/Cargo.toml"
                        "komorebic/build.rs"
                        "komorebic/src"
                        "komorebic/src/main.rs"
                        "komorebic/tests"
                      ]
                      || pkgs.lib.hasPrefix "komorebic/src/linux" relative
                      || pkgs.lib.hasPrefix "komorebic/tests/linux_" relative;
                    # the unified komorebi-client package: its manifest, lib.rs, the Linux module and tests
                    komorebiClient =
                      builtins.elem relative [
                        "komorebi-client"
                        "komorebi-client/Cargo.toml"
                        "komorebi-client/src"
                        "komorebi-client/src/lib.rs"
                        "komorebi-client/src/linux.rs"
                        "komorebi-client/tests"
                      ]
                      || pkgs.lib.hasPrefix "komorebi-client/tests/linux_" relative;
                  in
                  ((rootCrate || rootCrateParent || komorebic || komorebiClient) && !(pkgs.lib.hasSuffix "/Cargo.lock" path) || relative == "Cargo.toml" || relative == "Cargo.lock" || inTree)
                  &&
                    # the VM tests (their library, scripts and the states they keep) are not built, so
                    # editing one does not change the source the daemon is built from
                    !(pkgs.lib.hasInfix "/nix/tests/vm/" path)
                  && (
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
                    || (pkgs.lib.hasInfix "/komorebi-hotkeys/tests/fixtures/" path)
                  );
              };

              # the platform is a part of the one workspace of the repository: cargo loads every member (the
              # others are stubs, see nix/workspace-src.nix) and is told which packages are this platform's
              src = import (repoRoot + "/nix/workspace-src.nix") {
                inherit pkgs repoRoot real;
                isReal =
                  member:
                  pkgs.lib.hasPrefix "${platformDir}/" member
                  || member == "komorebic"
                  || member == "komorebi-client"
                  || builtins.any (crate: member == crate || pkgs.lib.hasPrefix "${crate}/" member) rootCrates;
              };
              packages = "-p komorebi-linux -p komorebic -p komorebi-client -p komorebi-hotkeys";
              workspaceArgs = {
                pname = "komorebi-workspace";
                version = (builtins.fromTOML (builtins.readFile ./komorebi/Cargo.toml)).package.version;
                cargoLock = "${real}/Cargo.lock";
                cargoToml = "${real}/Cargo.toml";
                # vendored from the filtered source, not the stubbed one: no build is needed to evaluate
                cargoVendorDir = craneLib.vendorCargoDeps { src = real; };
              };

              commonArgs = workspaceArgs // {
                cargoExtraArgs = "--locked ${packages}";
                inherit src;
                pname = "komorebi-workspace";
                version = (builtins.fromTOML (builtins.readFile ./komorebi/Cargo.toml)).package.version;
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
                  COMMIT_HASH = self.rev or (pkgs.lib.removeSuffix "-dirty" (self.dirtyRev or "unknown"));
                }
              );

              # the same build with the constant commit of the checks: the VM tests install this one, so
              # that a new commit does not build the daemon again
              vmBuild = craneLib.buildPackage individualCrateArgs;

              extractBinaryOf =
                package: binaryName:
                pkgs.runCommand "komorebi-${binaryName}" { meta = package.meta // { }; } ''
                  mkdir -p $out/bin
                  cp ${package}/bin/${binaryName} $out/bin/
                '';
              extractBinary = extractBinaryOf fullBuild;
            in
            {
              inherit
                craneLib
                craneLibFmt
                src
                real
                workspaceArgs
                packages
                commonArgs
                individualCrateArgs
                fullBuild
                vmBuild
                ;
              komorebi = extractBinary "komorebi";
              komorebic = extractBinary "komorebic";
              komorebiForVm = extractBinaryOf vmBuild "komorebi";
              komorebicForVm = extractBinaryOf vmBuild "komorebic";
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
              komorebiModule = self.nixosModules.forVmTests;
              # a Mac runs the guests under HVF, which is slower than KVM. A Linux host is no faster
              # than a Mac to the suites that time the daemon: on a 24-core WSL2 machine, under nested
              # KVM and with nothing else running, `latency` measured a p95 of 129 ms against its
              # 100 ms times `scale`, where x86-1 had passed it at 1
              vm = import ./nix/tests/vm {
                hostPkgs = pkgs;
                inherit komorebiModule;
                scale = 1.5;
              };
              # the probe of a NixOS-WSL2 machine (docs/dev/wsl2-probe.md); the checks use a constant
              # revision, so that they are the same derivations for every commit
              wsl2Probe = import ./nix/dev/wsl2-probe.nix {
                inherit pkgs;
                rev = self.rev or self.dirtyRev or "unknown";
              };
              wsl2ProbeForChecks = import ./nix/dev/wsl2-probe.nix {
                inherit pkgs;
                rev = "nix-check";
              };
              # `run-kde` starts the demo guest of the host's architecture (docs/trying-it.md). The guest's
              # runner is named in the program, not depended on: the command works out what is missing
              # and builds it, with its progress shown, instead of `nix run` fetching 4 GB first.
              runKde =
                let
                  arch = if system == "x86_64-linux" then "x86_64" else "aarch64";
                  guest = self.nixosConfigurations."run-kde-${arch}";
                  runner = builtins.unsafeDiscardStringContext "${guest.config.system.build.vm}";
                in
                pkgs.writeShellApplication {
                  name = "run-kde";
                  runtimeInputs = with pkgs; [
                    coreutils
                    e2fsprogs
                    gawk
                    git
                    gnugrep
                    gnused
                    mesa-demos
                    openssh
                    qemu-utils
                    virtiofsd
                  ];
                  text = ''
                    : "''${RUN_KDE_GUEST:=run-kde-${arch}}"
                    : "''${RUN_KDE_RUNNER_BIN:=${runner}/bin/run-${guest.config.networking.hostName}-vm}"
                  ''
                  + builtins.readFile ./scripts/run-kde;
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
                komorebi-workspace-fmt = build.craneLibFmt.cargoFmt (
                  build.workspaceArgs // { inherit (build) src; cargoExtraArgs = build.packages; }
                );

                komorebi-workspace-clippy = build.craneLib.cargoClippy (
                  build.individualCrateArgs
                  // {
                    cargoClippyExtraArgs = "--all-targets -- -D warnings";
                  }
                );

                komorebi-workspace-doc = build.craneLib.cargoDoc (
                  build.individualCrateArgs
                  // {
                    cargoDocExtraArgs = "--no-deps";
                    env.RUSTDOCFLAGS = "-D warnings";
                  }
                );

                komorebi-workspace-toml-fmt = build.craneLib.taploFmt (
                  build.workspaceArgs
                  // {
                    src = lib.sources.sourceFilesBySuffices build.real [ ".toml" ];
                  }
                );

                # cargo deny sees every member of the workspace: leave out the other platforms' packages
                # and use this tree's policy
                komorebi-workspace-deny = build.craneLib.cargoDeny (
                  build.workspaceArgs
                  // {
                    inherit (build) src;
                    cargoDenyExtraArgs =
                      "--workspace "
                      + lib.concatMapStringsSep " " (p: "--exclude ${p}") [
                        "komorebi-windows"
                        "komorebi-gui"
                        "komorebic-no-console"
                        "komorebi-bar"
                        "komorebi-shortcuts"
                        "komorebi-e2e-agent"
                        "komorebi-testwin"
                        "komorebi-macos"
                        "komorebic-macos"
                        "komorebi-client-macos"
                        "komorebi-bar-macos"
                        "version-macos"
                      ];
                    cargoDenyChecks = "--config ${platformDir}/deny.toml bans licenses sources";
                  }
                );

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
                # systems in a VM (`vmVariant`), with a runner for the machine that runs QEMU; UTM's is
                # a system of its own. Each guest is for one kind of machine, which is also checked: the
                # architecture of the guest, the machine that runs its QEMU, and that a guest of a Linux
                # machine has KVM and one display device only.
                plasma-guests-eval =
                  let
                    guests = self.nixosConfigurations;
                    machines =
                      let
                        mac = {
                          guest = "aarch64-linux";
                          host = "aarch64-darwin";
                        };
                        linux = system: {
                          guest = system;
                          host = system;
                        };
                        # the runner of the demo is given its display device, its ports and whether to
                        # insist on KVM when it starts, by `run-kde`
                        demo = machine: machine // { demo = true; };
                      in
                      {
                        plasma-vm = mac;
                        plasma-vm-session = mac;
                        plasma-vm-x86_64 = linux "x86_64-linux";
                        plasma-vm-session-x86_64 = linux "x86_64-linux";
                        plasma-vm-aarch64 = linux "aarch64-linux";
                        plasma-vm-session-aarch64 = linux "aarch64-linux";
                        run-kde-x86_64 = demo (linux "x86_64-linux");
                        run-kde-aarch64 = demo (linux "aarch64-linux");
                      };
                    isDisplay =
                      option: lib.hasPrefix "-device virtio-vga" option || lib.hasPrefix "-device virtio-gpu" option;
                    qemuGuest =
                      name:
                      let
                        machine = machines.${name};
                        demo = machine.demo or false;
                        vm = guests.${name}.config.virtualisation.vmVariant.virtualisation;
                      in
                      assert lib.assertMsg (
                        guests.${name}.pkgs.stdenv.hostPlatform.system == machine.guest
                      ) "${name} is not a ${machine.guest} guest";
                      assert lib.assertMsg (
                        vm.host.pkgs.stdenv.hostPlatform.system == machine.host
                      ) "${name} does not run on a ${machine.host} host";
                      assert lib.assertMsg (
                        vm.qemu.forceAccel != demo
                      ) "${name} would run without KVM or HVF, which is too slow to use";
                      assert lib.assertMsg (
                        builtins.length (builtins.filter isDisplay vm.qemu.options) == (if demo then 0 else 1)
                      ) "${name} does not have the display devices it should have";
                      assert lib.assertMsg
                        # an x86_64 machine has a VGA of its own, which would be a second display
                        ((machine.guest == "x86_64-linux" && !demo) == builtins.elem "-vga none" vm.qemu.options)
                        "${name} has `-vga none` if and only if it is an x86_64 guest with a display device";
                      assert lib.assertMsg (
                        demo -> (vm.forwardPorts == [ ] && vm.sharedDirectories.run-kde.target == "/run/run-kde")
                      ) "${name} does not leave its ports and its shared directory to the runner's environment";
                      {
                        ${name} = guests.${name}.config.virtualisation.vmVariant.system.build.toplevel;
                        "${name}, its runner" = guests.${name}.config.system.build.vm;
                      };
                    builds = lib.foldl' (builds: name: builds // qemuGuest name) { } (lib.attrNames machines) // {
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
              # The probe builds (shellcheck runs over the script and over the guest's init as it
              # does) and its dry run works without a machine to probe, so that it cannot rot between
              # the times somebody runs it.
              // lib.optionalAttrs isLinux {
                wsl2-probe = wsl2ProbeForChecks.probe;
                wsl2-probe-dry-run =
                  pkgs.runCommand "wsl2-probe-dry-run" { nativeBuildInputs = [ wsl2ProbeForChecks.probe ]; }
                    ''
                      wsl2-probe --dry-run --no-prompt --report report.txt > out.txt
                      cat out.txt
                      for probe in P0 P1 P2 P3 P4 P5 P6; do
                        grep -q "^$probe " out.txt || { echo "no verdict line for $probe"; exit 1; }
                      done
                      grep -q "PASTE THIS" out.txt
                      grep -q "would run" out.txt
                      # the report file has what the screen has
                      grep -q "PASTE THIS" report.txt
                      # a mistake in the options is refused
                      if wsl2-probe --nonsense 2> err.txt; then exit 1; fi
                      grep -q "unknown option" err.txt
                      touch $out
                    '';
              }
              # `run-kde` is shellchecked when it is built, and its output is what a test says it is
              // lib.optionalAttrs isLinux (
                {
                  run-kde = config.packages.run-kde;
                }
                // import ./nix/tests/run-kde.nix {
                  inherit pkgs;
                  runKde = config.packages.run-kde;
                }
              )
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
                wsl2-probe = wsl2Probe.probe;
                wsl2-probe-guest = wsl2Probe.guest;
                run-kde = runKde;
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
                wsl2-probe = {
                  type = "app";
                  program = lib.getExe wsl2Probe.probe;
                  meta.description = "Probes a NixOS-WSL2 machine for KVM, WSLg, a GPU and RDP";
                };
                run-kde = {
                  type = "app";
                  program = lib.getExe config.packages.run-kde;
                  meta.description = "Start the komorebi demo in a virtual machine";
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
            # the same packages built with the constant commit of the checks, for the VM tests
            overlays.forVmTests =
              final: prev:
              let
                build = mkKomorebiPackages { pkgs = mkPkgs final.stdenv.hostPlatform.system; };
              in
              (self.overlays.default final prev)
              // {
                komorebi = build.komorebiForVm;
                komorebic = build.komorebicForVm;
                komorebi-full = build.vmBuild;
              };

            # `services.komorebi`, with the overlay that provides its package, so that a NixOS
            # configuration needs this flake as an input and one module line (docs/installation.md)
            nixosModules.default = {
              imports = [ ./nix/nixos-module.nix ];
              nixpkgs.overlays = [ self.overlays.default ];
            };
            nixosModules.komorebi = self.nixosModules.default;
            # `services.komorebi` with the packages the VM tests install
            nixosModules.forVmTests = {
              imports = [ ./nix/nixos-module.nix ];
              nixpkgs.overlays = [ self.overlays.forVmTests ];
            };

            # Plasma guests to develop against (docs/dev/utm.md). For an Apple silicon Mac, QEMU runs
            # the first two under HVF, and UTM boots the third from a disk image. For a Linux machine
            # with KVM, QEMU runs the other four: the same two guests for an x86_64 host and for an
            # aarch64 one, each of which runs the guest of its own architecture. The last two are the
            # demo that `run-kde` starts.
            nixosConfigurations =
              let
                # `guest` is the system of the guest, and `host` the pkgs of the machine that runs QEMU
                mkPlasmaGuest =
                  { guest, host }:
                  modules:
                  nixpkgs.lib.nixosSystem {
                    specialArgs.hostPkgs = host;
                    modules = [
                      self.nixosModules.default
                      { nixpkgs.hostPlatform = guest; }
                    ]
                    ++ modules;
                  };
                onMac = mkPlasmaGuest {
                  guest = "aarch64-linux";
                  host = nixpkgs.legacyPackages.aarch64-darwin;
                };
                onLinux =
                  system:
                  mkPlasmaGuest {
                    guest = system;
                    host = nixpkgs.legacyPackages.${system};
                  };
                session = [
                  ./nix/dev/plasma-vm.nix
                  { komorebi.plasmaVm.session = true; }
                ];
                # the session guest with the demo of `run-kde` (nix/dev/run-kde.nix) and no display
                # device, which the command gives it when it starts the guest
                demo = [
                  ./nix/dev/plasma-vm.nix
                  {
                    komorebi.plasmaVm = {
                      session = true;
                      display = false;
                    };
                  }
                  ./nix/dev/run-kde.nix
                  ({ lib, ... }: { networking.hostName = lib.mkOverride 40 "komorebi-demo"; })
                ];
              in
              {
                plasma-vm = onMac [ ./nix/dev/plasma-vm.nix ];
                plasma-vm-session = onMac session;
                plasma-utm = onMac [ ./nix/dev/plasma-utm.nix ];

                plasma-vm-x86_64 = onLinux "x86_64-linux" [ ./nix/dev/plasma-vm.nix ];
                plasma-vm-session-x86_64 = onLinux "x86_64-linux" session;
                plasma-vm-aarch64 = onLinux "aarch64-linux" [ ./nix/dev/plasma-vm.nix ];
                plasma-vm-session-aarch64 = onLinux "aarch64-linux" session;

                run-kde-x86_64 = onLinux "x86_64-linux" demo;
                run-kde-aarch64 = onLinux "aarch64-linux" demo;
              };
          };
        };
    in
    mkOutputs {
      inherit inputs self;
      repoRoot = ./.;
      platformDir = "";
    }
    // {
      lib.mkOutputs = mkOutputs;
    };
}
