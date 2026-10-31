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

    # the other platforms' flakes (komorebi-for-kde/flake.nix, komorebi-for-mac/flake.nix) are not inputs: their
    # flake.nix files are imported below and called with these inputs, which are pinned to exactly the
    # revisions of their flake.lock (so nothing here pins the content of those directories)
    # inputs of komorebi-for-kde, pinned to the revisions in komorebi-for-kde/flake.lock (keep them equal to it)
    kfde-nixpkgs.url = "github:NixOS/nixpkgs/f05535a0efcb2134c478d1e88434fadf486d42b5";
    kfde-nixpkgs-lib.url = "github:nix-community/nixpkgs.lib/f7cd230690d9fc982d06129f9189b101fddcfa3b";
    kfde-flake-compat = { url = "github:NixOS/flake-compat/5edf11c44bc78a0d334f6334cdaf7d60d732daab"; flake = false; };
    kfde-flake-parts.url = "github:hercules-ci/flake-parts/024633cd702b10285db5cb19b40ad48d2399ba60";
    kfde-flake-parts.inputs.nixpkgs-lib.follows = "kfde-nixpkgs-lib";
    kfde-crane.url = "github:ipetkov/crane/e675a36b7377bebe41d548388a2501cbb16d380d";
    kfde-rust-overlay.url = "github:oxalica/rust-overlay/19ecdd08213836b36c4296a3a4f4af5a3d5308db";
    kfde-rust-overlay.inputs.nixpkgs.follows = "kfde-nixpkgs";
    kfde-treefmt-nix.url = "github:numtide/treefmt-nix/0d6d739a833ba9a9c93034862e4a7b061a062a1c";
    kfde-treefmt-nix.inputs.nixpkgs.follows = "kfde-nixpkgs";
    kfde-git-hooks-nix.url = "github:cachix/git-hooks.nix/1ba1dbfe28738c9a71c266be6efa5b89e75491a9";
    kfde-git-hooks-nix.inputs.nixpkgs.follows = "kfde-nixpkgs";
    kfde-git-hooks-nix.inputs.flake-compat.follows = "kfde-flake-compat";

    # inputs of komorebi-for-mac, pinned to the revisions in komorebi-for-mac/flake.lock (keep them equal to it)
    kfm-nixpkgs.url = "github:NixOS/nixpkgs/16c7794d0a28b5a37904d55bcca36003b9109aaa";
    kfm-nixpkgs-lib.url = "github:nix-community/nixpkgs.lib/2075416fcb47225d9b68ac469a5c4801a9c4dd85";
    kfm-flake-compat = { url = "github:NixOS/flake-compat/5edf11c44bc78a0d334f6334cdaf7d60d732daab"; flake = false; };
    kfm-flake-parts.url = "github:hercules-ci/flake-parts/a34fae9c08a15ad73f295041fec82323541400a9";
    kfm-flake-parts.inputs.nixpkgs-lib.follows = "kfm-nixpkgs-lib";
    kfm-crane.url = "github:ipetkov/crane/7d59256814085fd9666a2ae3e774dc5ee216b630";
    kfm-rust-overlay.url = "github:oxalica/rust-overlay/1d3f83babdd21e16bf5cfe0f1efcb4f49ee1bc2c";
    kfm-rust-overlay.inputs.nixpkgs.follows = "kfm-nixpkgs";
    kfm-treefmt-nix.url = "github:numtide/treefmt-nix/d56486eb9493ad9c4777c65932618e9c2d0468fc";
    kfm-treefmt-nix.inputs.nixpkgs.follows = "kfm-nixpkgs";
    kfm-git-hooks-nix.url = "github:cachix/git-hooks.nix/f0927703b7b1c8d97511c4116eb9b4ec6645a0fa";
    kfm-git-hooks-nix.inputs.nixpkgs.follows = "kfm-nixpkgs";
    kfm-git-hooks-nix.inputs.flake-compat.follows = "kfm-flake-compat";
    kfm-gitignore = { url = "github:hercules-ci/gitignore.nix/637db329424fd7e46cf4185293b9cc8c88c95394"; inputs.nixpkgs.follows = "kfm-nixpkgs"; };
    kfm-git-hooks-nix.inputs.gitignore.follows = "kfm-gitignore";
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
          commit ? self.rev or (pkgs.lib.removeSuffix "-dirty" (self.dirtyRev or "unknown")),
        }:
        let
          # toolchain with windows msvc target
          toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            targets = [ "x86_64-pc-windows-msvc" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          version = "0.1.0";

          msvcEnv = mkMsvcEnv { inherit pkgs windowsSdk; };

          # the files of the Windows crates; the KDE and macOS crates are in the same Cargo workspace and
          # are added as stubs (nix/workspace-src.nix), so that an edit of one changes nothing here
          real = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              # the other platforms' crates are added as stubs below
              !(pkgs.lib.hasInfix "/komorebi-for-kde" path || pkgs.lib.hasInfix "/komorebi-for-mac" path)
              # the Linux and macOS files of komorebic are not read when building for Windows
              && !(builtins.match ".*/komorebi(c/(src/(linux|macos)|tests/(linux|macos)_)|-client/(src/(linux|macos)\\.rs|tests/(linux|macos)_)|-bar/src/macos)[^/]*.*" path != null)
              && (
                (craneLib.filterCargoSources path type)
                || (pkgs.lib.hasInfix "/docs/" path)
                || (builtins.match ".*/docs/.*" path != null)
                || (builtins.match ".*/komorebic/fixtures/.*" path != null)
              );
          };

          src = import ./nix/workspace-src.nix {
            inherit pkgs real;
            repoRoot = ./.;
            isReal = member: !(pkgs.lib.hasPrefix "komorebi-for-" member);
          };

          commonArgs = {
            inherit src version;
            cargoLock = "${real}/Cargo.lock";
            cargoToml = "${real}/Cargo.toml";
            # vendored from the filtered source, not the stubbed one: no build is needed to evaluate
            cargoVendorDir = craneLib.vendorCargoDeps { src = real; };
            strictDeps = true;
            # applications.json is committed now; this copies the pinned asc input over it, so that the
            # embedded file is the pinned one even when the committed copy is older
            preBuild = "mkdir -p komorebic && cp ${inputs.asc}/applications.json komorebic/applications.json";
            # a constant, so that the dependencies and every check are the same
            # derivations for every commit; only fullBuild stamps the real one
            COMMIT_HASH = "nix-check";

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
              COMMIT_HASH = commit;
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
            real
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
        # komorebi for KDE Plasma (kfde) and komorebi for macOS (kfm), re-exported with a prefix.
        # Their flakes keep their own inputs and lock files (inputs.<name>.inputs), but their
        # outputs are made here, from the whole repository as the source: the Rust workspaces of
        # both depend on the crates of this tree by path, which a flake of their own directory
        # cannot see (their `lib.mkOutputs`). The VM tests of kfde are not re-exported as checks,
        # so that `nix flake check` of this flake does not boot them; they are reachable as
        # legacyPackages.<system>.kfde-vm.
        (
          { lib, ... }:
          let
            # the nested flake.nix files are plain attribute sets: import them and call their
            # `lib.mkOutputs` with the inputs declared above (same revisions as their own locks).
            # `self` of a nested flake: its own outputs, with the revision and the path of this one
            nestedOutputs =
              name: dir:
              let
                nestedInputs = lib.genAttrs [
                  "nixpkgs"
                  "flake-parts"
                  "crane"
                  "rust-overlay"
                  "treefmt-nix"
                  "git-hooks-nix"
                ] (n: inputs."${name}-${n}");
                mkOutputs =
                  ((import (self.outPath + "/${dir}/flake.nix")).outputs (nestedInputs // { inherit self; })).lib.mkOutputs;
                nestedSelf =
                  outputs
                  // lib.filterAttrs (n: _: n == "rev" || n == "dirtyRev") self
                  // {
                    outPath = self.outPath + "/${dir}";
                    inputs = nestedInputs;
                  };
                outputs = mkOutputs {
                  self = nestedSelf;
                  inputs = nestedInputs // {
                    self = nestedSelf;
                  };
                  repoRoot = self;
                  platformDir = dir;
                };
              in
              outputs;
            kfde = nestedOutputs "kfde" "komorebi-for-kde";
            kfm = nestedOutputs "kfm" "komorebi-for-mac";
          in
          {
            flake = {
              nixosModules.komorebi-for-kde = kfde.nixosModules.default;
              overlays.komorebi-for-kde = kfde.overlays.default;
              darwinModules.komorebi-for-mac = kfm.darwinModules.default;
            };

            perSystem =
              { system, ... }:
              let
                prefixed = prefix: lib.mapAttrs' (name: lib.nameValuePair "${prefix}-${name}");
                withoutVmTests = lib.filterAttrs (name: _: !(lib.hasPrefix "vm-" name));
              in
              {
                packages =
                  prefixed "kfde" (kfde.packages.${system} or { }) // prefixed "kfm" (kfm.packages.${system} or { });
                checks =
                  prefixed "kfde" (withoutVmTests (kfde.checks.${system} or { }))
                  // prefixed "kfm" (kfm.checks.${system} or { });
                legacyPackages.kfde-vm = (kfde.legacyPackages.${system} or { }).vm or { };
              };
          }
        )
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
              pname = "komorebi-workspace";
              version = "0.1.0";
              cargoToml = "${build.real}/Cargo.toml";
            };

            komorebi-workspace-toml-fmt = build.craneLib.taploFmt {
              src = pkgs.lib.sources.sourceFilesBySuffices build.real [ ".toml" ];
            };

            # cargo deny sees every member of the workspace: leave out the KDE and macOS packages, which
            # have their own policy (komorebi-for-*/deny.toml) in their own checks
            komorebi-workspace-deny = build.craneLib.cargoDeny {
              inherit (build) src;
              pname = "komorebi-workspace";
              version = "0.1.0";
              cargoToml = "${build.real}/Cargo.toml";
              cargoLock = "${build.real}/Cargo.lock";
              cargoDenyExtraArgs =
                "--workspace "
                + pkgs.lib.concatMapStringsSep " " (p: "--exclude ${p}") [
                  "komorebi-linux"
                  "komorebic-linux"
                  "komorebi-client-linux"
                  "komorebi-hotkeys"
                  "version-linux"
                  "komorebi-macos"
                  "komorebic-macos"
                  "komorebi-client-macos"
                  "komorebi-bar-macos"
                  "version-macos"
                ];
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
            # the test window of the Windows e2e guest, built with the constant
            # stamp: a new commit does not rebuild what it depends on
            komorebi-testwin-e2e = e2eBuild.komorebi-testwin;
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

            # komorebic's build script runs on the host and wants OpenSSL, as in
            # the nix build
            OPENSSL_NO_VENDOR = "1";
            OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
            OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";

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

              # komorebic's build script (reqwest) runs on the build host
              pkgs.pkg-config
              pkgs.openssl

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
