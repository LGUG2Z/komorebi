{ kde-rounded-corners, fetchFromGitHub }:

# 0.11.0 is the release that draws the outline in the right place on KWin 6.8: 6.8 inverted the
# texture coordinates of the offscreen window, and with Breeze's shadow (deeper below a window than
# above it) the whole ring of 0.10.0 lands about 24px too low, across the title bar ("KWin 6.8
# compatibility for inverted texture coordinates"). nixpkgs has 0.11.0 too (2026-10-28; it had
# 0.10.0 at d3b13ad of 2026-10-24), and the version and the source are set here all the same: the
# patch below is written for the 0.11.0 source.
kde-rounded-corners.overrideAttrs (old: {
  version = "0.11.0";

  # per-window outline colors, set over D-Bus (org.komorebi.BorderColours1): komorebi colors a
  # window by what it is to the layout (the focused one on each monitor, a stack, a monocle, a
  # floating window, a locked container), which "the active window" cannot say. Without this the
  # effect still works, with the colors of the settings for the active and the other windows. The
  # patch also redraws a window from a fresh texture when only its opacity changes, which the
  # effect's texture would not show otherwise.
  patches = (old.patches or [ ]) ++ [ ./kde-rounded-corners/komorebi-border-colours.patch ];

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
