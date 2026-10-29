{
  description = "komorebi for Windows";

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
    asc = {
      url = "github:LGUG2Z/komorebi-application-specific-configuration";
      flake = false;
    };
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
      windowsSdkVersion = "10.0.26100";
      windowsCrtVersion = "14.44.17.14";

      mkWindowsSdk =
        pkgs:
        pkgs.stdenvNoCC.mkDerivation {
          name = "windows-sdk-${windowsSdkVersion}-crt-${windowsCrtVersion}";

          nativeBuildInputs = [
            pkgs.xwin
            pkgs.cacert
          ];

          outputHashAlgo = "sha256";
          outputHashMode = "recursive";
          # hash of `xwin splat --disable-symlinks`: without the flag xwin makes
          # case-alias symlinks on a case-sensitive filesystem and none on a
          # case-insensitive one, so the hash depends on the host
          outputHash = "sha256-YjmEfoxtcmQXvUc+WtaElnyP/4osNvtSI7X6MDaNTjg=";

          buildCommand = ''
            export HOME=$(mktemp -d)
            # keep the cache on the same filesystem as $out, as xwin moves
            # files from the cache into the output and rename(2) fails with
            # EXDEV when the build dir and the store are separate mounts
            mkdir -p $out
            xwin --accept-license \
              --cache-dir $out/.xwin-cache \
              --sdk-version ${windowsSdkVersion} \
              --crt-version ${windowsCrtVersion} \
              splat --disable-symlinks --output $out
            rm -rf $out/.xwin-cache
          '';
        };

      # the linker asks for `kernel32.lib` of a file that is `Kernel32.lib`, so
      # on a case-sensitive host add a lowercase alias for every name with a
      # capital in it, and the spellings the headers include each other by (the
      # SDK itself is made without symlinks)
      caseFolded =
        { pkgs, windowsSdk }:
        if pkgs.stdenv.hostPlatform.isDarwin then
          windowsSdk
        else
          pkgs.runCommandLocal "${windowsSdk.name}-case-folded" { } ''
            cp -rs ${windowsSdk} $out
            chmod -R u+w $out
            cd $out
            find . -name '*[A-Z]*' | while read -r path; do
              directory=$(dirname "$path")
              name=$(basename "$path")
              lower=$(echo "$name" | tr 'A-Z' 'a-z')
              if [ ! -e "$directory/$lower" ] && [ ! -L "$directory/$lower" ]; then
                ln -s "$name" "$directory/$lower"
              fi
            done

            # headers include each other with spellings that differ from the
            # file name (`DriverSpecs.h` for `driverspecs.h`), which xwin
            # covers with symlinks unless told not to
            grep -Rhoi -E '^[[:space:]]*#[[:space:]]*include[[:space:]]*[<"][^>"]+[>"]' \
              crt/include sdk/include 2>/dev/null \
              | sed -E 's/.*[<"]([^>"]+)[>"]/\1/; s#\\#/#g; s#.*/##' \
              | sort -u > $TMPDIR/spellings
            find crt/include sdk/include \( -type f -o -type l \) \
              | awk -v spellings=$TMPDIR/spellings '
                  BEGIN {
                    while ((getline line < spellings) > 0)
                      known[tolower(line)] = known[tolower(line)] "\n" line
                  }
                  {
                    n = split($0, parts, "/"); base = parts[n]
                    dir = substr($0, 1, length($0) - length(base))
                    m = split(known[tolower(base)], names, "\n")
                    for (i = 1; i <= m; i++)
                      if (names[i] != "" && names[i] != base)
                        print dir "\t" base "\t" names[i]
                  }' \
              | while IFS=$'\t' read -r directory name spelling; do
                  if [ ! -e "$directory$spelling" ] && [ ! -L "$directory$spelling" ]; then
                    ln -s "$name" "$directory$spelling"
                  fi
                done
          '';

      mkMsvcEnv =
        { pkgs, windowsSdk }:
        let
          sdk = caseFolded { inherit pkgs windowsSdk; };
          clangVersion = pkgs.lib.versions.major pkgs.llvmPackages.clang.version;
        in
        {
          # linker for the windows target
          CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "lld-link";

          # c/c++ compiler
          CC_x86_64_pc_windows_msvc = "clang-cl";
          CXX_x86_64_pc_windows_msvc = "clang-cl";
          AR_x86_64_pc_windows_msvc = "llvm-lib";

          # IMPORTANT: libclang include path MUST come first to avoid header conflicts
          CFLAGS_x86_64_pc_windows_msvc = builtins.concatStringsSep " " [
            "--target=x86_64-pc-windows-msvc"
            "-Wno-unused-command-line-argument"
            "-fuse-ld=lld-link"
            "/imsvc${pkgs.llvmPackages.libclang.lib}/lib/clang/${clangVersion}/include"
            "/imsvc${sdk}/crt/include"
            "/imsvc${sdk}/sdk/include/ucrt"
            "/imsvc${sdk}/sdk/include/um"
            "/imsvc${sdk}/sdk/include/shared"
          ];

          CXXFLAGS_x86_64_pc_windows_msvc = builtins.concatStringsSep " " [
            "--target=x86_64-pc-windows-msvc"
            "-Wno-unused-command-line-argument"
            "-fuse-ld=lld-link"
            "/imsvc${pkgs.llvmPackages.libclang.lib}/lib/clang/${clangVersion}/include"
            "/imsvc${sdk}/crt/include"
            "/imsvc${sdk}/sdk/include/ucrt"
            "/imsvc${sdk}/sdk/include/um"
            "/imsvc${sdk}/sdk/include/shared"
          ];

          # target-specific rust flags with linker flavor and library search paths
          CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = builtins.concatStringsSep " " [
            "-Clinker-flavor=lld-link"
            "-Lnative=${sdk}/crt/lib/x86_64"
            "-Lnative=${sdk}/sdk/lib/um/x86_64"
            "-Lnative=${sdk}/sdk/lib/ucrt/x86_64"
          ];

          # cargo target
          CARGO_BUILD_TARGET = "x86_64-pc-windows-msvc";
        };

      mkKomorebiPackages =
        {
          pkgs,
          windowsSdk,
          # the commit stamped into the binaries; a build for the e2e guest
          # gives a constant one, so that a new commit does not rebuild it
          commit ? self.rev or (pkgs.lib.removeSuffix "-dirty" self.dirtyRev),
        }:
        let
          # toolchain with windows msvc target
          toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          version = "0.1.0";

          msvcEnv = mkMsvcEnv { inherit pkgs windowsSdk; };

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasInfix "/docs/" path)
              || (builtins.match ".*/docs/.*" path != null);
          };

          commonArgs = {
            inherit src version;
            strictDeps = true;
            # komorebic/build.rs downloads applications.json if it is missing, which
            # is not possible in the sandbox, so provide the pinned copy instead
            preBuild = "mkdir -p komorebic && cp ${inputs.asc}/applications.json komorebic/applications.json";
            COMMIT_HASH = commit;

            # build inputs for cross-compilation
            nativeBuildInputs = [
              pkgs.llvmPackages.clang-unwrapped
              pkgs.llvmPackages.lld
              pkgs.llvmPackages.llvm
              # komorebic build script uses reqwest, which runs on the build host
              pkgs.pkg-config
              pkgs.openssl
              pkgs.openssl.dev
            ];

            # point the host-side openssl-sys build at the nix openssl
            OPENSSL_NO_VENDOR = "1";
            OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
            OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";

            # cross-compilation environment
            inherit (msvcEnv)
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER
              CC_x86_64_pc_windows_msvc
              CXX_x86_64_pc_windows_msvc
              AR_x86_64_pc_windows_msvc
              CFLAGS_x86_64_pc_windows_msvc
              CXXFLAGS_x86_64_pc_windows_msvc
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
              CARGO_BUILD_TARGET
              ;
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
              pname = "komorebi-workspace";
            }
          );

          extractBinary =
            binaryName:
            pkgs.runCommand "komorebi-${binaryName}"
              {
                meta = fullBuild.meta // { };
              }
              ''
                mkdir -p $out/bin
                cp ${fullBuild}/bin/${binaryName}.exe $out/bin/
              '';
        in
        {
          inherit
            craneLib
            src
            individualCrateArgs
            fullBuild
            msvcEnv
            ;
          komorebi = extractBinary "komorebi";
          komorebic = extractBinary "komorebic";
          komorebic-no-console = extractBinary "komorebic-no-console";
          komorebi-bar = extractBinary "komorebi-bar";
          komorebi-gui = extractBinary "komorebi-gui";
          komorebi-shortcuts = extractBinary "komorebi-shortcuts";
          komorebi-testwin = extractBinary "komorebi-testwin";
          komorebi-e2e-agent = extractBinary "komorebi-e2e-agent";
        };

      mkPkgs =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem =
        { config, system, ... }:
        let
          pkgs = mkPkgs system;
          windowsSdk = mkWindowsSdk pkgs;
          build = mkKomorebiPackages { inherit pkgs windowsSdk; };
          # the binaries the e2e guest runs: the same build, but with a
          # constant commit stamp, so a new commit (or a dirty tree) that does
          # not change a source file reuses it. The commit under test is
          # KOMOREBI_E2E_COMMIT, not what the binary says.
          e2eBuild = mkKomorebiPackages {
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
              fi
              export OVMF_FD=${ovmfWithTpm}
              export KOMOREBI_E2E_COMMIT=''${KOMOREBI_E2E_COMMIT:-${self.rev or "dirty"}}
              export PYTHONPATH=${./e2e/lib}:${./e2e}
              exec python -m platforms.qemu_windows.run \
                "$E2E_GOLDEN" "$bins" "$out" "$@"
            '';
          };

          # toolchain with windows target and nightly rustfmt
          rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
          rustToolchainWithNightlyRustfmt = pkgs.symlinkJoin {
            name = "rust-toolchain-with-nightly-rustfmt";
            paths = [
              nightlyRustfmt
              rustToolchain
            ];
          };
          nightlyToolchain = pkgs.rust-bin.nightly.latest.default.override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
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
                package = pkgs.rust-bin.nightly.latest.rustfmt;
              };
            };
          };

          checks = {
            komorebi-workspace-clippy = build.craneLib.cargoClippy (
              build.individualCrateArgs
              // {
                cargoClippyExtraArgs = "--all-targets -- -D warnings";
              }
            );

            komorebi-workspace-fmt = build.craneLib.cargoFmt {
              inherit (build) src;
            };

            komorebi-workspace-toml-fmt = build.craneLib.taploFmt {
              src = pkgs.lib.sources.sourceFilesBySuffices build.src [ ".toml" ];
            };

            komorebi-workspace-deny = build.craneLib.cargoDeny {
              inherit (build) src;
            };

            komorebi-workspace-nextest = build.craneLib.cargoNextest build.individualCrateArgs;

            # the Python of the Windows e2e guest that needs no guest: the
            # vendored core's tests and those of the runner
            e2e-tests =
              pkgs.runCommand "e2e-tests"
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
                  cp -r ${./e2e} e2e
                  chmod -R u+w e2e
                  cd e2e
                  export HOME=$TMPDIR PYTHONPATH=$PWD/lib:$PWD
                  python -m pytest -q -p no:cacheprovider lib/tests platforms/qemu_windows/tests
                  touch $out
                '';
          };

          packages = {
            inherit (build)
              komorebi
              komorebic
              komorebic-no-console
              komorebi-bar
              komorebi-gui
              komorebi-shortcuts
              komorebi-testwin
              komorebi-e2e-agent
              ;
            inherit windowsSdk;
            komorebi-full = build.fullBuild;
            default = build.fullBuild;
          };

          apps = {
            komorebi = {
              type = "app";
              program = "${build.komorebi}/bin/komorebi.exe";
            };
            komorebic = {
              type = "app";
              program = "${build.komorebic}/bin/komorebic.exe";
            };
            komorebic-no-console = {
              type = "app";
              program = "${build.komorebic-no-console}/bin/komorebic-no-console.exe";
            };
            komorebi-bar = {
              type = "app";
              program = "${build.komorebi-bar}/bin/komorebi-bar.exe";
            };
            komorebi-gui = {
              type = "app";
              program = "${build.komorebi-gui}/bin/komorebi-gui.exe";
            };
            komorebi-shortcuts = {
              type = "app";
              program = "${build.komorebi-shortcuts}/bin/komorebi-shortcuts.exe";
            };
            default = {
              type = "app";
              program = "${build.fullBuild}/bin/komorebi.exe";
            };
          }
          // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
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

          devShells.default = pkgs.mkShell {
            name = "komorebi";

            RUST_BACKTRACE = "full";

            # the Windows e2e guest (e2e/platforms/qemu_windows): the build
            # scripts and the runner find OVMF here, and the vendored core
            # and the platform on PYTHONPATH
            OVMF_FD = "${ovmfWithTpm}";
            shellHook = ''
              export PYTHONPATH="$PWD/e2e/lib:$PWD/e2e''${PYTHONPATH:+:$PYTHONPATH}"
            '';

            # cross-compilation environment
            inherit (build.msvcEnv)
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER
              CC_x86_64_pc_windows_msvc
              CXX_x86_64_pc_windows_msvc
              AR_x86_64_pc_windows_msvc
              CFLAGS_x86_64_pc_windows_msvc
              CXXFLAGS_x86_64_pc_windows_msvc
              CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
              CARGO_BUILD_TARGET
              ;

            packages = [
              rustToolchainWithNightlyRustfmt
              cargo-udeps

              # cross-compilation tooling
              pkgs.llvmPackages.clang-unwrapped # provides clang-cl
              pkgs.llvmPackages.lld # provides lld-link
              pkgs.llvmPackages.llvm # provides llvm-lib

              pkgs.cargo-deny
              pkgs.cargo-nextest
              pkgs.cargo-outdated
              pkgs.jq
              pkgs.just
              pkgs.prettier

              # the Windows e2e guest
              pkgs.qemu
              pkgs.swtpm
              pkgs.xorriso
              pkgs.socat
              pkgs.git
              (pkgs.python3.withPackages (p: [
                p.pillow
                p.pytest # the tests of e2e/lib
              ]))
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
    };
}
