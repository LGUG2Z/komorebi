# Building for Windows (x86_64-pc-windows-msvc) from any host.
#
# The MSVC CRT and the Windows SDK come from xwin, the compiler and linker from LLVM, so nothing
# here needs a Windows machine. Copied from komorebi for Windows' own flake, with the same pinned
# versions.
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

      nativeBuildInputs = [ pkgs.xwin ];

      outputHashAlgo = "sha256";
      outputHashMode = "recursive";
      # NOT the hash komorebi for Windows' own flake pins (sha256-6cLS5q1BDRpLPScfmmKpTTEHUzsgKTKD1+mKvGX9Deo=):
      # that did not reproduce on aarch64-darwin on 2026-10-27 with xwin 0.7.0, which produced this one.
      # Whether Microsoft re-published that SDK or the splat differs by host is not known.
      outputHash = "sha256-YjmEfoxtcmQXvUc+WtaElnyP/4osNvtSI7X6MDaNTjg=";

      buildCommand = ''
        export HOME=$(mktemp -d)
        xwin --accept-license \
          --sdk-version ${windowsSdkVersion} \
          --crt-version ${windowsCrtVersion} \
          splat --output $out
      '';
    };

  mkMsvcEnv =
    { pkgs, windowsSdk }:
    let
      clangVersion = pkgs.lib.versions.major pkgs.llvmPackages.clang.version;
      includes = [
        # IMPORTANT: libclang include path MUST come first to avoid header conflicts
        "/imsvc${pkgs.llvmPackages.libclang.lib}/lib/clang/${clangVersion}/include"
        "/imsvc${windowsSdk}/crt/include"
        "/imsvc${windowsSdk}/sdk/include/ucrt"
        "/imsvc${windowsSdk}/sdk/include/um"
        "/imsvc${windowsSdk}/sdk/include/shared"
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
        "-Lnative=${windowsSdk}/crt/lib/x86_64"
        "-Lnative=${windowsSdk}/sdk/lib/um/x86_64"
        "-Lnative=${windowsSdk}/sdk/lib/ucrt/x86_64"
      ];

      # cargo target
      CARGO_BUILD_TARGET = "x86_64-pc-windows-msvc";
    };
}
