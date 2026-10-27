# Install checks of the KWin effects komorebi drives: each package is built and its files are
# inspected the way KWin finds them, without a session. Whether an effect loads in a real KWin is a
# question for the VM tests.
{ pkgs, packages }:
let
  inherit (pkgs.kdePackages) kwin qttools;
in
{
  kde-rounded-corners-komorebi-install =
    pkgs.runCommand "kde-rounded-corners-komorebi-install"
      {
        nativeBuildInputs = [
          pkgs.jq
          pkgs.patchelf
          # qtplugininfo reads a plugin's metadata as Qt and KWin read it
          qttools
        ];
      }
      ''
        fail() {
          echo "FAIL: $*" >&2
          exit 1
        }
        plugin=${packages.kde-rounded-corners-komorebi}/lib/qt-6/plugins/kwin/effects/plugins/kwin4_effect_shapecorners.so

        [ -f "$plugin" ] || fail "no plugin at $plugin"
        echo "ok: the plugin is where KWin looks for effect plugins"

        # The metadata is compiled into the plugin; KWin does not read metadata.json at run time.
        qtplugininfo --full-json "$plugin" > metadata.json || fail "qtplugininfo cannot read $plugin"
        if ! jq -e '.MetaData.KPlugin.EnabledByDefault == false' metadata.json > /dev/null; then
          cat metadata.json
          fail "the plugin's metadata does not say EnabledByDefault false"
        fi
        echo "ok: the plugin's metadata says EnabledByDefault false"

        # The plugin is loaded into KWin, so it has to link the libkwin of that KWin: the dynamic
        # loader takes the first directory of the RUNPATH that has the library.
        needed=$(patchelf --print-needed "$plugin" | grep -m 1 -x 'libkwin\.so\.[0-9]*') ||
          fail "the plugin does not link libkwin"
        found=
        IFS=: read -r -a dirs <<< "$(patchelf --print-rpath "$plugin")"
        for dir in "''${dirs[@]}"; do
          if [ -e "$dir/$needed" ]; then
            found=$dir/$needed
            break
          fi
        done
        [ "$found" = "${kwin}/lib/$needed" ] ||
          fail "$needed resolves to '$found', not to the KWin of this nixpkgs (${kwin})"
        echo "ok: $needed is ${kwin}/lib/$needed"

        touch $out
      '';
}
