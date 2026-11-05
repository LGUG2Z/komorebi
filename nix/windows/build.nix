# The Windows cross build: xwin's SDK and CRT, the clang-cl/lld-link environment and the crane
# derivations of the Windows workspace (target x86_64-pc-windows-msvc), for the systems that build it.
{ inputs, self }:
let
  lib' = import ../lib.nix { inherit inputs; };
  inherit (lib') version;
  inherit (inputs) crane;
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
      toolchain = (pkgs.rust-bin.fromRustupToolchainFile ../../rust-toolchain.toml).override {
        targets = [ "x86_64-pc-windows-msvc" ];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      msvcEnv = mkMsvcEnv { inherit pkgs windowsSdk; };

      # the Windows members of the one Cargo workspace; the Linux and macOS members are stubs
      # (nix/workspace-src.nix), so that an edit of one changes nothing here
      inherit
        (import ../workspace-src.nix {
          inherit pkgs craneLib;
          repoRoot = ../..;
          platform = "windows";
        })
        real
        src
        stubbedPackages
        ;

      lockFile = lib'.repoFile "Cargo.lock";
      tomlFile = lib'.repoFile "Cargo.toml";

      workspaceArgs = {
        pname = "komorebi-workspace-windows";
        inherit version;
        # the lock and the root manifest as files of their own: a path inside the filtered source would put the
        # whole filtered source into the context of every derivation, and then an edit of any file
        # would change the dependencies
        cargoLock = lockFile;
        cargoToml = tomlFile;
        cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = lockFile; };
      };

      commonArgs = workspaceArgs // {
        inherit src;
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

      # the dummy source is of the manifests alone (nix/lib.nix): the dependencies are the same
      # derivations for every source file and every stub, and nothing is built at evaluation
      cargoArtifacts = craneLib.buildDepsOnly (
        builtins.removeAttrs commonArgs [ "src" ]
        // {
          dummySrc = craneLib.mkDummySrc {
            src = lib'.manifestSource pkgs.lib;
            cargoLock = lockFile;
          };
        }
      );

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
        workspaceArgs
        stubbedPackages
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
      komorebi-recorder = extractBinary "komorebi-recorder";
      komorebi-recorder-gui = extractBinary "komorebi-recorder-gui";
      komorebi-testwin = extractBinary "komorebi-testwin";
      komorebi-e2e-agent = extractBinary "komorebi-e2e-agent";
    };
in
{
  inherit mkWindowsSdk mkMsvcEnv mkKomorebiPackages;
}
