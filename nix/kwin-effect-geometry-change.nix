{
  lib,
  stdenvNoCC,
  fetchFromGitHub,
}:

# "Geometry Change" by Peter Fajdiga (GPLv3): animates windows that are moved or resized by programs
# or scripts, which stock KWin does not. It is a scripted (JS) effect package, so installing it is
# only a copy into `share/kwin/effects`; kwin discovers it at startup, not afterwards. Its metadata
# already says `"EnabledByDefault": false`, so KWin does not load it until it is asked for.
stdenvNoCC.mkDerivation {
  pname = "kwin-effect-geometry-change";
  version = "1.5";

  src = fetchFromGitHub {
    owner = "peterfajdiga";
    repo = "kwin4_effect_geometry_change";
    rev = "398491e8a3f5b9bb785850439abd41820f3c5986";
    hash = "sha256-aJx3hDzIk/bQrrRvs/CPigJ0miTgqxd8OI9I6kuiXkk=";
  };

  dontBuild = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/share/kwin/effects/kwin4_effect_geometry_change
    cp -r package/* $out/share/kwin/effects/kwin4_effect_geometry_change/
    runHook postInstall
  '';

  meta = {
    description = "KWin effect that animates windows moved or resized by programs or scripts";
    homepage = "https://github.com/peterfajdiga/kwin4_effect_geometry_change";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
