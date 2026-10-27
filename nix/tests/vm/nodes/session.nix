# A real Plasma Wayland session that logs in by itself, on a machine that can draw. Everything the
# headless node has is here too, so a test can still start a virtual KWin next to it. The module's
# unit, `komorebi.service`, starts komorebi in the session when the user logs in, as on a user's
# machine; a suite that wants the session without it sets `services.komorebi.enable = false` in
# its `node`.
#
# The choice of login manager is made here and nowhere else. nixpkgs' own `plasma6` test uses
# plasma-login-manager, so that is what this node follows; sddm with a Wayland greeter is the other
# one that has been seen to work, and replacing the first block below with
# `services.displayManager.sddm = { enable = true; wayland.enable = true; };` and
# `services.displayManager.defaultSession = "plasma";` switches to it.
{ pkgs, ... }:
{
  imports = [ ./headless.nix ];

  # the one display this node has is the one screenshots are taken of
  komorebi.vm.renderNode = false;

  services.xserver.enable = true;
  services.displayManager = {
    plasma-login-manager.enable = true;
    autoLogin = {
      enable = true;
      user = "komorebi";
    };
  };

  virtualisation = {
    memorySize = 4096;
    cores = 4;
  };

  # An idle lock screen or a dimmed display changes what a screenshot shows, and a display that has
  # gone to sleep gets no frame callbacks from KWin, so clients stop answering resizes.
  environment.etc."xdg/kscreenlockerrc".text = ''
    [Daemon]
    Autolock=false
    LockOnResume=false
  '';
  environment.etc."xdg/powerdevilrc".text = ''
    [AC][Display]
    DimDisplayWhenIdle=false
    TurnOffDisplayWhenIdle=false

    [AC][SuspendAndShutdown]
    AutoSuspendAction=0
  '';

  environment.systemPackages = [ pkgs.kdePackages.konsole ];

  # KWin loads no effect that animates under software rendering, which is all a guest has, unless
  # it is told to: with this it loads Geometry Change, which komorebi asks it to load
  environment.sessionVariables.KWIN_EFFECTS_FORCE_ANIMATIONS = "1";

  # Nothing adds a display device here (see komorebi.vm.renderNode above). The test driver gives
  # every aarch64 guest a virtio-gpu, and an x86_64 guest has the default VGA; screenshots are taken
  # of that one, and a second device would be a second head that KWin draws on and the screenshot
  # never shows.
}
