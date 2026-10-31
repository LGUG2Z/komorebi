# Install checks of the KWin effects komorebi drives: each package is built and its files are
# inspected the way KWin finds them, without a session. Whether an effect loads in a real KWin is a
# question for the VM tests.
{ pkgs, packages }:
let
  inherit (pkgs.kdePackages) kwin qttools;
  fail = ''
    fail() {
      echo "FAIL: $*" >&2
      exit 1
    }
  '';
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
        ${fail}
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

        # komorebi's patch: the interface that takes a color for each window
        grep -q -a -F org.komorebi.BorderColours1 "$plugin" ||
          fail "the plugin has no org.komorebi.BorderColours1: komorebi-border-colours.patch is not in it"
        echo "ok: the plugin has org.komorebi.BorderColours1"

        touch $out
      '';

  kwin-effect-geometry-change-komorebi-install =
    pkgs.runCommand "kwin-effect-geometry-change-komorebi-install" { nativeBuildInputs = [ pkgs.jq ]; }
      ''
        ${fail}
        # KWin finds scripted effects by their metadata.json, in a directory of share/kwin/effects
        effect=${packages.kwin-effect-geometry-change-komorebi}/share/kwin/effects/kwin4_effect_geometry_change

        [ -f "$effect/metadata.json" ] || fail "no metadata.json in $effect"
        jq -e '.KPlugin.Id == "kwin4_effect_geometry_change"' "$effect/metadata.json" > /dev/null ||
          fail "the metadata does not name the effect kwin4_effect_geometry_change"
        main=$effect/contents/$(jq -r '."X-Plasma-MainScript"' "$effect/metadata.json")
        [ -f "$main" ] || fail "no main script at $main"
        echo "ok: the effect is where KWin looks for scripted effects, with its script"

        # upstream's choice, which komorebi relies on: asserted so that an update cannot change it
        if ! jq -e '.KPlugin.EnabledByDefault == false' "$effect/metadata.json" > /dev/null; then
          cat "$effect/metadata.json"
          fail "the metadata does not say EnabledByDefault false"
        fi
        echo "ok: the metadata says EnabledByDefault false"

        # komorebi's patch: the curve is a setting (the stock effect names QEasingCurve.OutExpo too)
        grep -q -F 'readConfig("Curve"' "$main" ||
          fail "the script does not read a Curve setting: curve.patch is not in it"
        grep -q -F '<entry name="Curve"' "$effect/contents/config/main.xml" ||
          fail "the effect's settings have no Curve: curve.patch is not in it"
        echo "ok: the effect reads a Curve setting"

        touch $out
      '';
}
