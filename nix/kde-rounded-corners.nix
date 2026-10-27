{ kde-rounded-corners, fetchFromGitHub }:

# nixpkgs ships 0.10.0, which draws its outline in the wrong place on KWin 6.8: 6.8 inverted the
# texture coordinates of the offscreen window, and with Breeze's shadow (deeper below a window than
# above it) the whole ring lands about 24px too low, across the title bar. 0.11.0 is the release
# that handles it ("KWin 6.8 compatibility for inverted texture coordinates").
kde-rounded-corners.overrideAttrs (old: {
  version = "0.11.0";

  src = fetchFromGitHub {
    owner = "matinlotfali";
    repo = "KDE-Rounded-Corners";
    rev = "v0.11.0";
    hash = "sha256-rdVXukBh7doyuZaFGVQghwKqkWGtuLOXxJgvZbZG0hQ=";
  };

  # KWin loads every effect that is enabled by default when it starts, with the settings the effect
  # has then, and at its defaults this one draws a one pixel black outline inside every window,
  # over its title bar. Disabled by default, it stays unloaded until it is asked for (`loadEffect`
  # on KWin's `/Effects`), which can happen after its settings are written. KWin reads the flag
  # from the metadata compiled into the plugin.
  postPatch = (old.postPatch or "") + ''
    substituteInPlace src/metadata.json \
      --replace-fail '"EnabledByDefault": true' '"EnabledByDefault": false'
  '';
})
