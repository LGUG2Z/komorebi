# komorebi for Windows, cross-built with xwin (x86_64-pc-windows-msvc): the `windows-*` packages,
# apps and checks, and the Windows e2e guest. Built on Linux (and on Apple silicon); the binaries run
# on Windows only.
{
  inputs,
  self,
  lib,
  ...
}:
let
  lib' = import ../lib.nix { inherit inputs; };
  windows = import ./build.nix { inherit inputs self; };
in
{
  perSystem =
    { pkgs, system, ... }:
    let
      windowsSdk = windows.mkWindowsSdk pkgs;
      build = windows.mkKomorebiPackages { inherit pkgs windowsSdk; };
      # the binaries the e2e guest runs: the same build, but with a
      # constant commit stamp, so a new commit (or a dirty tree) that does
      # not change a source file reuses it. The commit under test is
      # KOMOREBI_E2E_COMMIT, not what the binary says.
      e2eBuild = windows.mkKomorebiPackages {
        inherit pkgs windowsSdk;
        commit = "e2e";
      };

      # OVMF without SMM and with the TPM support Windows 11 Setup asks for
      ovmfWithTpm = (pkgs.OVMF.override { tpmSupport = true; }).fd;
      e2eWindows = pkgs.writeShellApplication {
        name = "e2e-windows";
        runtimeInputs = [
          pkgs.qemu
          pkgs.swtpm
          (pkgs.python3.withPackages (p: [ p.pillow ]))
        ];
        text = ''
          : "''${E2E_GOLDEN:?set E2E_GOLDEN to the golden image directory}"
          out=''${1:?usage: e2e-windows OUT SCENARIO...}
          shift
          # the binaries under test: this checkout's, or those of another
          # commit, made by e2e/platforms/qemu_windows/build-ref.sh
          if [ -n "''${KOMOREBI_E2E_BINS:-}" ]; then
            bins=$KOMOREBI_E2E_BINS
            if [ -f "$bins/commit" ]; then
              KOMOREBI_E2E_COMMIT=$(cat "$bins/commit")
            fi
          else
            bins=$(mktemp -d)
            trap 'rm -rf "$bins"' EXIT
            cp ${e2eBuild.komorebi}/bin/komorebi.exe "$bins"
            cp ${e2eBuild.komorebic}/bin/komorebic.exe "$bins"
            cp ${e2eBuild.komorebi-testwin}/bin/komorebi-testwin.exe "$bins"
            cp ${e2eBuild.komorebi-bar}/bin/komorebi-bar.exe "$bins"
          fi
          export OVMF_FD=${ovmfWithTpm}
          export KOMOREBI_E2E_BAR_SCHEMA=${lib'.repoFile "schemas/windows/schema.bar.json"}
          export KOMOREBI_E2E_COMMIT=''${KOMOREBI_E2E_COMMIT:-${self.rev or "dirty"}}
          export PYTHONPATH=${lib'.repoFile "e2e/lib"}:${lib'.repoFile "e2e"}
          exec python -m platforms.qemu_windows.run \
            "$E2E_GOLDEN" "$bins" "$out" "$@"
        '';
      };
      # the nightly rustfmt of the checks is the one of the dev shell: see nix/dev-shell.nix
      nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
    in
    {
      # the dev shell (nix/dev-shell.nix) cross-compiles with these
      _module.args = {
        inherit windowsSdk ovmfWithTpm;
        inherit (build) msvcEnv;
      };
    }
    // lib.optionalAttrs (builtins.elem system lib'.windowsSystems) {
      allChecks = {
        windows-clippy = build.craneLib.cargoClippy (
          build.individualCrateArgs
          // {
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );

        # the nightly rustfmt over the Windows files, as tools/format-all does locally. Not `cargo fmt`:
        # it runs the stable rustfmt (which ignores the nightly options of rustfmt.toml) and follows
        # `mod linux;` into files the Windows source does not have.
        windows-fmt =
          pkgs.runCommand "komorebi-windows-fmt"
            {
              nativeBuildInputs = [ nightlyRustfmt ];
            }
            ''
              RUSTFMT=rustfmt bash ${lib'.repoFile "tools/format-all"} --check --root ${build.real}
              touch $out
            '';

        windows-toml-fmt = build.craneLib.taploFmt (
          build.workspaceArgs
          // {
            src = pkgs.lib.sources.sourceFilesBySuffices build.real [ ".toml" ];
          }
        );

        # cargo deny sees every member of the workspace: leave out the Linux and macOS packages, which
        # are stubs here and have their own checks (linux-deny, macos-deny)
        windows-deny = build.craneLib.cargoDeny (
          build.workspaceArgs
          // {
            inherit (build) src;
            cargoDenyExtraArgs =
              "--workspace " + pkgs.lib.concatMapStringsSep " " (p: "--exclude ${p}") build.stubbedPackages;
          }
        );

        windows-nextest = build.craneLib.cargoNextest build.individualCrateArgs;
      }
      // {
        # the Python of the Windows e2e guest that needs no guest: the
        # vendored core's tests and those of the runner
        windows-e2e-tests =
          pkgs.runCommand "windows-e2e-tests"
            {
              nativeBuildInputs = [
                pkgs.git
                (pkgs.python3.withPackages (p: [
                  p.pillow
                  p.pytest
                ]))
              ];
            }
            ''
              cp -r ${lib'.repoFile "e2e"} e2e
              chmod -R u+w e2e
              cd e2e
              export HOME=$TMPDIR PYTHONPATH=$PWD/lib:$PWD
              python -m pytest -q -p no:cacheprovider lib/tests platforms/qemu_windows/tests
              touch $out
            '';
      };

      packages = {
        windows-komorebi = build.komorebi;
        windows-komorebic = build.komorebic;
        windows-komorebic-no-console = build.komorebic-no-console;
        windows-komorebi-bar = build.komorebi-bar;
        windows-komorebi-gui = build.komorebi-gui;
        windows-komorebi-shortcuts = build.komorebi-shortcuts;
        windows-komorebi-testwin = build.komorebi-testwin;
        windows-komorebi-e2e-agent = build.komorebi-e2e-agent;
        windows-sdk = windowsSdk;
        # the test window of the Windows e2e guest, built with the constant
        # stamp: a new commit does not rebuild what it depends on
        windows-komorebi-testwin-e2e = e2eBuild.komorebi-testwin;
        windows-komorebi-full = build.fullBuild;
        windows-default = build.fullBuild;
      };

      apps = {
        windows-komorebi = {
          type = "app";
          program = "${build.komorebi}/bin/komorebi.exe";
        };
        windows-komorebic = {
          type = "app";
          program = "${build.komorebic}/bin/komorebic.exe";
        };
        windows-komorebic-no-console = {
          type = "app";
          program = "${build.komorebic-no-console}/bin/komorebic-no-console.exe";
        };
        windows-komorebi-bar = {
          type = "app";
          program = "${build.komorebi-bar}/bin/komorebi-bar.exe";
        };
        windows-komorebi-gui = {
          type = "app";
          program = "${build.komorebi-gui}/bin/komorebi-gui.exe";
        };
        windows-komorebi-shortcuts = {
          type = "app";
          program = "${build.komorebi-shortcuts}/bin/komorebi-shortcuts.exe";
        };
        windows-default = {
          type = "app";
          program = "${build.fullBuild}/bin/komorebi.exe";
        };
      }
      // lib.optionalAttrs (system == "x86_64-linux") {
        # the neutral scenarios against a throwaway Windows guest:
        #   E2E_GOLDEN=<golden image dir> nix run .#e2e-windows -- \
        #     OUT SCENARIO...
        # KOMOREBI_E2E_BINS=<dir from build-ref.sh> tests another commit
        # the golden image is built once with
        # e2e/platforms/qemu_windows/build-golden.sh
        e2e-windows = {
          type = "app";
          program = pkgs.lib.getExe e2eWindows;
        };
      };
    };
}
