# komorebi for Linux (KDE Plasma): the native packages, the `linux-*` checks, the VM tests (`vm-*`),
# the NixOS module and the Plasma guests. The Rust packages are built on Linux; Apple silicon gets
# the checks and the VM guests' definitions, which run the same tests against the fake desktop.
{
  inputs,
  self,
  lib,
  ...
}:
let
  lib' = import ../lib.nix { inherit inputs; };
  inherit (inputs) nixpkgs;
  linux = import ./build.nix { inherit inputs self; };
  inherit (lib') mkPkgs;
in
{
  perSystem =
    {
      config,
      pkgs,
      system,
      ...
    }:
    let
      build = linux.mkKomorebiPackages { inherit pkgs; };
      isLinux = pkgs.stdenv.hostPlatform.isLinux;
      # the guests install komorebi through the module, as a NixOS user does
      komorebiModule = self.nixosModules.forVmTests;
      # a Mac runs the guests under HVF, which is slower than KVM. A Linux host is no faster
      # than a Mac to the suites that time the daemon: on a 24-core WSL2 machine, under nested
      # KVM and with nothing else running, `latency` measured a p95 of 129 ms against its
      # 100 ms times `scale`, where x86-1 had passed it at 1
      vm = import ./tests/vm {
        hostPkgs = pkgs;
        inherit komorebiModule;
        scale = 1.5;
      };
      # the probe of a NixOS-WSL2 machine (docs/linux/dev/wsl2-probe.md); the checks use a constant
      # revision, so that they are the same derivations for every commit
      wsl2Probe = import ./dev/wsl2-probe.nix {
        inherit pkgs;
        rev = self.rev or self.dirtyRev or "unknown";
      };
      wsl2ProbeForChecks = import ./dev/wsl2-probe.nix {
        inherit pkgs;
        rev = "nix-check";
      };
      # `run-kde` starts the demo guest of the host's architecture (docs/linux/trying-it.md). The guest's
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
          + builtins.readFile ../../scripts/linux/run-kde;
        };
      # rustfmt.toml uses options only a nightly rustfmt knows
      nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
      checkSystem = builtins.elem system lib'.linuxCheckSystems;
    in
    lib.optionalAttrs checkSystem {
      allChecks = {
        # tools/format-all: the nightly rustfmt over the files of the Linux source, file by file. `cargo fmt`
        # followed `mod windows;` of the shared entry files into files this source does not have.
        linux-fmt =
          pkgs.runCommand "komorebi-linux-fmt"
            {
              nativeBuildInputs = [ nightlyRustfmt ];
            }
            ''
              RUSTFMT=rustfmt bash ${lib'.repoFile "tools/format-all"} --check --root ${build.real}
              touch $out
            '';

        linux-clippy = build.craneLib.cargoClippy (
          build.individualCrateArgs
          // {
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );

        linux-doc = build.craneLib.cargoDoc (
          build.individualCrateArgs
          // {
            cargoDocExtraArgs = "--no-deps";
            env.RUSTDOCFLAGS = "-D warnings";
          }
        );

        linux-toml-fmt = build.craneLib.taploFmt (
          build.workspaceArgs
          // {
            src = lib.sources.sourceFilesBySuffices build.real [ ".toml" ];
          }
        );

        # cargo deny sees every member of the workspace: leave out the other platforms' packages (stubs
        # here); the policy is the one deny.toml of the repository
        linux-deny = build.craneLib.cargoDeny (
          build.workspaceArgs
          // {
            inherit (build) src;
            cargoDenyExtraArgs =
              "--workspace " + lib.concatMapStringsSep " " (p: "--exclude ${p}") build.stubbedPackages;
            cargoDenyChecks = "bans licenses sources";
          }
        );

        linux-nextest = build.craneLib.cargoNextest (build.individualCrateArgs // { doCheck = true; });

        # The bridge is not Rust: tsc checks its types from its JSDoc comments, and node runs
        # its tests against a fake KWin. No package.json and no build step.
        linux-bridge-js =
          pkgs.runCommand "komorebi-bridge-js"
            {
              nativeBuildInputs = [
                pkgs.nodejs
                pkgs.typescript
              ];
              src = lib.fileset.toSource {
                root = ../../komorebi-linux/src/kwin;
                fileset = lib.fileset.fileFilter (file: !file.hasExt "rs") ../../komorebi-linux/src/kwin;
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
        # say: `just jsonschema` and `just nixgen-linux` write them
        linux-schema-fresh =
          pkgs.runCommand "komorebi-schema-fresh"
            {
              nativeBuildInputs = [
                build.komorebicForChecks
                pkgs.diffutils
              ];
            }
            ''
              komorebic static-config-schema | diff -u ${lib'.repoFile "schemas/linux/schema.json"} - \
                || { echo "schemas/linux/schema.json is stale: run just jsonschema"; exit 1; }
              komorebic application-specific-configuration-schema | diff -u ${lib'.repoFile "schemas/linux/schema.asc.json"} - \
                || { echo "schemas/linux/schema.asc.json is stale: run just jsonschema"; exit 1; }
              expected=$(sha256sum ${lib'.repoFile "schemas/linux/schema.json"} | cut -d' ' -f1)
              head -n 1 ${./komorebi-options.nix} | grep -qx "# schema-sha256: $expected" \
                || { echo "nix/linux/komorebi-options.nix is stale: run just nixgen-linux"; exit 1; }
              touch $out
            '';

        # the options the module is typed with, evaluated against the example configuration
        linux-nix-options-validation = import ./tests/default.nix { inherit pkgs; };

        # every file of the key binding corpus through `komorebic hotkeys compile`, as the
        # module runs it
        linux-keybindings-fixtures = import ./tests/keybindings.nix {
          inherit pkgs;
          komorebic = build.fullBuild;
        };

        # The NixOS module in real NixOS systems, evaluated and never built, so a Mac checks it
        # too. A VM suite, `vm-module`, runs it.
        linux-nixos-module-eval = import ./tests/nixos-module.nix {
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
        linux-plasma-guests-eval =
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
        linux-wsl2-probe = wsl2ProbeForChecks.probe;
        linux-wsl2-probe-dry-run =
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
        lib.mapAttrs' (name: lib.nameValuePair "linux-${name}") (
          {
            run-kde = config.packages.run-kde;
          }
          // import ./tests/run-kde.nix {
            inherit pkgs;
            runKde = config.packages.run-kde;
          }
        )
      )
      # The VM tests need QEMU with KVM, so only Linux checks them. A Mac runs the same tests
      # through legacyPackages.<system>.vm, see docs/linux/dev/vm-tests.md.
      // lib.optionalAttrs isLinux (
        lib.mapAttrs' (name: test: lib.nameValuePair "vm-${name}" test) vm.suites
      )
      # the KWin effects komorebi drives, installed and inspected without a session
      // lib.optionalAttrs isLinux (
        lib.mapAttrs' (name: lib.nameValuePair "linux-${name}") (
          import ./tests/effects.nix {
            inherit pkgs;
            inherit (config) packages;
          }
        )
      );

      legacyPackages = lib.optionalAttrs checkSystem (
        {
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
            (import ./tests/vm {
              hostPkgs = pkgs;
              inherit komorebiModule;
              scale = 8;
            }).suites;
        }
      );
    }
    // lib.optionalAttrs (builtins.elem system lib'.linuxSystems) {
      packages = {
        inherit (build) komorebi komorebic;
        komorebi-full = build.fullBuild;
        default = build.fullBuild;
        kde-rounded-corners-komorebi = pkgs.callPackage ../linux/kde-rounded-corners.nix { };
        kwin-effect-geometry-change-komorebi =
          pkgs.callPackage ../linux/kwin-effect-geometry-change.nix
            { };
        wsl2-probe = wsl2Probe.probe;
        wsl2-probe-guest = wsl2Probe.guest;
        run-kde = runKde;
      };

      apps = {
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
    };

  flake = {
    overlays.linux =
      final: _:
      let
        build = linux.mkKomorebiPackages { pkgs = mkPkgs final.stdenv.hostPlatform.system; };
      in
      {
        inherit (build) komorebi komorebic;
        komorebi-full = build.fullBuild;
        # with the consumer's KWin, so that the plugin links the libkwin it is loaded into
        kde-rounded-corners-komorebi = final.callPackage ./kde-rounded-corners.nix { };
        kwin-effect-geometry-change-komorebi = final.callPackage ./kwin-effect-geometry-change.nix { };
      };
    # the same packages built with the constant commit of the checks, for the VM tests
    overlays.forVmTests =
      final: prev:
      let
        build = linux.mkKomorebiPackages { pkgs = mkPkgs final.stdenv.hostPlatform.system; };
      in
      (self.overlays.linux final prev)
      // {
        komorebi = build.komorebiForVm;
        komorebic = build.komorebicForVm;
        komorebi-full = build.vmBuild;
      };

    # `services.komorebi`, with the overlay that provides its package, so that a NixOS
    # configuration needs this flake as an input and one module line (docs/linux/installation.md)
    nixosModules.default = {
      imports = [ ./nixos-module.nix ];
      nixpkgs.overlays = [ self.overlays.linux ];
    };
    nixosModules.komorebi = self.nixosModules.default;
    # the names of the root flake before the nested flakes were merged into it
    nixosModules.komorebi-for-kde = self.nixosModules.default;
    overlays.komorebi-for-kde = self.overlays.linux;
    # `services.komorebi` with the packages the VM tests install
    nixosModules.forVmTests = {
      imports = [ ./nixos-module.nix ];
      nixpkgs.overlays = [ self.overlays.forVmTests ];
    };

    # Plasma guests to develop against (docs/linux/dev/utm.md). For an Apple silicon Mac, QEMU runs
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
              # `nix flake check` evaluates the toplevel of every configuration, which needs a root file
              # system and a boot loader; the guests run as VMs (vmVariant), which replace both
              (
                { lib, ... }:
                {
                  fileSystems."/" = lib.mkDefault {
                    device = "/dev/disk/by-label/nixos";
                    fsType = "ext4";
                  };
                  boot.loader.grub.devices = lib.mkDefault [ "nodev" ];
                }
              )
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
          ./dev/plasma-vm.nix
          { komorebi.plasmaVm.session = true; }
        ];
        # the session guest with the demo of `run-kde` (nix/linux/dev/run-kde.nix) and no display
        # device, which the command gives it when it starts the guest
        demo = [
          ./dev/plasma-vm.nix
          {
            komorebi.plasmaVm = {
              session = true;
              display = false;
            };
          }
          ./dev/run-kde.nix
          ({ lib, ... }: { networking.hostName = lib.mkOverride 40 "komorebi-demo"; })
        ];
      in
      {
        plasma-vm = onMac [ ./dev/plasma-vm.nix ];
        plasma-vm-session = onMac session;
        plasma-utm = onMac [ ./dev/plasma-utm.nix ];

        plasma-vm-x86_64 = onLinux "x86_64-linux" [ ./dev/plasma-vm.nix ];
        plasma-vm-session-x86_64 = onLinux "x86_64-linux" session;
        plasma-vm-aarch64 = onLinux "aarch64-linux" [ ./dev/plasma-vm.nix ];
        plasma-vm-session-aarch64 = onLinux "aarch64-linux" session;

        run-kde-x86_64 = onLinux "x86_64-linux" demo;
        run-kde-aarch64 = onLinux "aarch64-linux" demo;
      };
  };
}
