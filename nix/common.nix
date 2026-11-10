# What the three platforms share: the systems, nixpkgs with the rust overlay, the formatter, the
# pre-commit hook and the dev shell.
{ inputs, self, ... }:
let
  lib' = import ./lib.nix { inherit inputs; };
in
{
  imports = [
    # pkgs of every perSystem module (a module of its own: one that takes `pkgs` cannot define it)
    {
      perSystem =
        { system, ... }:
        {
          _module.args.pkgs = lib'.mkPkgs system;
        };
    }
    inputs.treefmt-nix.flakeModule
    inputs.git-hooks-nix.flakeModule
  ];

  # Linux and Windows (cross) builds on both Linux architectures, macOS on both Apple ones.
  # x86_64-darwin uses an older nixpkgs (flake.nix, input nixpkgs-x86_64-darwin).
  systems = [
    "x86_64-linux"
    "aarch64-linux"
    "aarch64-darwin"
    "x86_64-darwin"
  ];

  flake = {
    # the packages of the platform of the system the overlay is applied to: the Linux ones (overlays.linux,
    # nix/linux/flake-module.nix) or the macOS ones (overlays.macos)
    overlays.default =
      final: prev:
      (if final.stdenv.hostPlatform.isDarwin then self.overlays.macos else self.overlays.linux) final prev
      // self.overlays.recorder final prev;
    overlays.komorebi = self.overlays.default;
  };

  perSystem =
    {
      config,
      pkgs,
      system,
      msvcEnv,
      ovmfWithTpm,
      ...
    }:
    let
      crossToWindows = builtins.elem system lib'.windowsSystems;
      # the toolchain of rust-toolchain.toml, with the Windows target on the systems that cross-build
      rustToolchain =
        let
          stable = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
        in
        if crossToWindows then stable.override { targets = [ "x86_64-pc-windows-msvc" ]; } else stable;
      nightlyRustfmt = pkgs.rust-bin.nightly.latest.rustfmt;
      rustToolchainWithNightlyRustfmt = pkgs.symlinkJoin {
        name = "rust-toolchain-with-nightly-rustfmt";
        paths = [
          nightlyRustfmt
          rustToolchain
        ];
      };
      nightlyToolchain = pkgs.rust-bin.nightly.latest.default.override {
        targets = pkgs.lib.optional crossToWindows "x86_64-pc-windows-msvc";
      };
      # cargo-udeps needs a nightly compiler
      cargo-udeps = pkgs.writeShellScriptBin "cargo-udeps" ''
        export PATH="${nightlyToolchain}/bin:$PATH"
        exec ${pkgs.cargo-udeps}/bin/cargo-udeps "$@"
      '';

      # the tools of every platform's development
      tools = [
        rustToolchainWithNightlyRustfmt
        cargo-udeps

        # cross-compilation tooling for Windows (xwin): clang-cl, lld-link, llvm-lib
        pkgs.llvmPackages.clang-unwrapped
        pkgs.llvmPackages.lld
        pkgs.llvmPackages.llvm

        # komorebic's build script (reqwest) runs on the build host
        pkgs.pkg-config
        pkgs.openssl

        pkgs.cargo-deny
        pkgs.cargo-nextest
        pkgs.cargo-outdated
        # writes the Nix options of the modules from the schemas (`just nixgen-linux`, `just nixgen-macos`)
        config.packages.schemars-nixgen
        # writes the reference pages of the docs site from the schemas (`just docgen`)
        config.packages.schemars-docgen
        # `komorebi-recorder frames` and `selftest` extract frames with ffmpeg; it is never used for
        # capture and is not bundled into the packaged binary
        pkgs.ffmpeg-headless
        # `just icon` renders the watermelon of the recorder
        pkgs.resvg
        pkgs.check-jsonschema
        pkgs.jq
        pkgs.just
        pkgs.prettier
        pkgs.ripgrep

        # the tests of the sleep and lock signals of the KDE daemon start private buses (`kwin::power`)
        pkgs.dbus

        # the KDE bridge's type check and tests (`just check-js`)
        pkgs.nodejs
        pkgs.typescript

        # the VM test library of the KDE suites
        pkgs.ruff
        pkgs.ty

        # the Windows e2e guest (e2e/platforms/qemu_windows) and the tests of e2e/lib
        pkgs.qemu
        pkgs.swtpm
        pkgs.xorriso
        pkgs.socat
        pkgs.git
        (pkgs.python3.withPackages (p: [
          p.pillow
          p.pytest
        ]))
      ]
      # publishes the schema site of the macOS tree
      ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isDarwin pkgs.wrangler;

      mkDevShell =
        {
          name,
          # the cross-compilation environment of the Windows build: CARGO_BUILD_TARGET makes every
          # `cargo` command build for Windows, so a shell that builds the Linux or macOS packages
          # natively leaves it out
          cross,
        }:
        pkgs.mkShell (
          {
            inherit name;

            RUST_BACKTRACE = "full";

            # komorebic's build script runs on the host and wants OpenSSL, as in
            # the nix build
            OPENSSL_NO_VENDOR = "1";
            OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
            OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";

            packages = tools;
          }
          # the recorder's video on Linux is GStreamer's, and the libraries its window draws through are
          # opened when they are needed, not linked
          // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (
            let
              runtime = import ./recorder/runtime.nix { inherit pkgs; };
            in
            {
              buildInputs = runtime.gstreamerPackages;
              GST_PLUGIN_SYSTEM_PATH_1_0 = runtime.gstreamerPluginPath;
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtime.linuxRuntimeLibraries;
            }
          )
          // pkgs.lib.optionalAttrs (crossToWindows) {
            # the Windows e2e guest (e2e/platforms/qemu_windows): the build
            # scripts and the runner find OVMF here, and the vendored core
            # and the platform on PYTHONPATH
            OVMF_FD = "${ovmfWithTpm}";
            shellHook = ''
              export PYTHONPATH="$PWD/e2e/lib:$PWD/e2e''${PYTHONPATH:+:$PYTHONPATH}"
            '';
          }
          // pkgs.lib.optionalAttrs (cross && crossToWindows) {
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
          }
        );
    in
    {
      treefmt = {
        projectRootFile = "flake.nix";
        flakeCheck = false;
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

      # `nix develop`: everything the three platforms need, and the Windows cross-compilation
      # environment (cargo builds for x86_64-pc-windows-msvc by default). `nix develop .#native` is the
      # same without it, for `cargo` commands on the Linux or macOS packages (-p komorebi-linux, ...).
      devShells = {
        default = mkDevShell {
          name = "komorebi";
          cross = true;
        };
        native = mkDevShell {
          name = "komorebi-native";
          cross = false;
        };
        # What the first stage of CI runs (.github/workflows/ci.yaml: hygiene, fmt, deny) and nothing more:
        # the versions the flake pins, without the Windows SDK, the VM tooling and the rest of the full shell,
        # so that the checks of that stage take seconds to set up and a local `nix develop .#hygiene` runs
        # the same tools as CI does.
        hygiene = pkgs.mkShell {
          name = "komorebi-hygiene";
          packages = [
            rustToolchainWithNightlyRustfmt
            pkgs.actionlint
            pkgs.cargo-deny
            pkgs.git
            pkgs.jq
            pkgs.ripgrep
            pkgs.shellcheck
          ];
        };
      };

      # the two checks of these modules are members of the set `fmt` (nix/checks.nix), which adds them
      # to `checks` itself: the modules' own `checks` entries are off
      allChecks = {
        treefmt = config.treefmt.build.check config.treefmt.projectRoot;
        pre-commit = config.pre-commit.settings.run;
      };

      pre-commit = {
        check.enable = false;
        settings.hooks.treefmt = {
          enable = true;
          package = config.treefmt.build.wrapper;
          pass_filenames = false;
        };
      };
    };
}
