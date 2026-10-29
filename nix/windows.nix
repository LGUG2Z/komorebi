# Building for Windows (x86_64-pc-windows-msvc) from any host.
#
# The MSVC CRT and the Windows SDK come from xwin, the compiler and linker from LLVM, so nothing
# here needs a Windows machine. Copied from komorebi for Windows' own flake, with the same pinned
# versions.
let
  # The SDK is made with `--disable-symlinks`, so that it is the same tree, and has the same hash,
  # on every host: xwin switches symlinks off by itself on a case-insensitive filesystem (macOS)
  # and leaves them on a case-sensitive one (Linux), and the two trees hash differently. The
  # linker asks for `kernel32.lib` of a file that is `Kernel32.lib`, which only a case-sensitive
  # filesystem needs an alias for, so on those hosts this adds a lowercase one for each name that
  # has a capital in it, and one for each spelling the headers include each other by that differs
  # in case from the file (`DriverSpecs.h` for `driverspecs.h`).
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

        # headers include each other with spellings that differ from the file name
        # (`DriverSpecs.h` for `driverspecs.h`), which xwin covers with symlinks unless
        # told not to
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
in
{
  windowsSdkVersion = "10.0.26100";
  windowsCrtVersion = "14.44.17.14";

  mkWindowsSdk =
    {
      pkgs,
      windowsSdkVersion,
      windowsCrtVersion,
    }:
    pkgs.stdenvNoCC.mkDerivation {
      name = "windows-sdk-${windowsSdkVersion}-crt-${windowsCrtVersion}";

      nativeBuildInputs = [
        pkgs.xwin
        pkgs.cacert
      ];

      outputHashAlgo = "sha256";
      outputHashMode = "recursive";
      # The hash of `xwin splat --disable-symlinks` of this SDK, with xwin 0.9.0 (nixpkgs' at the
      # time). macOS produced it on 2026-10-27, where xwin switched symlinks off by itself (the filesystem
      # is case-insensitive), and Linux produces it on 2026-10-29 with the flag. Not checked on macOS
      # since the flag, the CA bundle and the cache directory were added.
      # It is NOT what komorebi for Windows' own flake pins (sha256-6cLS5q1BDRpLPScfmmKpTTEHUzsgKTKD1+mKvGX9Deo=),
      # which this does not reproduce; the cause of that is not known. Without `--disable-symlinks` the
      # hash depends on the host: xwin makes case-alias symlinks on a case-sensitive filesystem and
      # none on a case-insensitive one, and the tree with them (2914 links) hashes to
      # sha256-UFQjsFVBwcF/9e9tVFoG0Z1JySxyTnFqoaRwr/tUWzA=. A new xwin may change the tree: if the
      # hash does not reproduce after an update of nixpkgs, look at that first.
      outputHash = "sha256-YjmEfoxtcmQXvUc+WtaElnyP/4osNvtSI7X6MDaNTjg=";

      buildCommand = ''
        export HOME=$(mktemp -d)
        mkdir -p $out
        xwin --accept-license \
          --cache-dir $out/.xwin-cache \
          --sdk-version ${windowsSdkVersion} \
          --crt-version ${windowsCrtVersion} \
          splat --disable-symlinks --output $out
        rm -rf $out/.xwin-cache
      '';
    };

  mkMsvcEnv =
    { pkgs, windowsSdk }:
    let
      sdk = caseFolded { inherit pkgs windowsSdk; };
      clangVersion = pkgs.lib.versions.major pkgs.llvmPackages.clang.version;
      includes = [
        # IMPORTANT: libclang include path MUST come first to avoid header conflicts
        "/imsvc${pkgs.llvmPackages.libclang.lib}/lib/clang/${clangVersion}/include"
        "/imsvc${sdk}/crt/include"
        "/imsvc${sdk}/sdk/include/ucrt"
        "/imsvc${sdk}/sdk/include/um"
        "/imsvc${sdk}/sdk/include/shared"
      ];
      cflags = builtins.concatStringsSep " " (
        [
          "--target=x86_64-pc-windows-msvc"
          "-Wno-unused-command-line-argument"
          "-fuse-ld=lld-link"
        ]
        ++ includes
      );
    in
    {
      # linker for the windows target
      CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "lld-link";

      # c/c++ compiler
      CC_x86_64_pc_windows_msvc = "clang-cl";
      CXX_x86_64_pc_windows_msvc = "clang-cl";
      AR_x86_64_pc_windows_msvc = "llvm-lib";
      CFLAGS_x86_64_pc_windows_msvc = cflags;
      CXXFLAGS_x86_64_pc_windows_msvc = cflags;

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
}
