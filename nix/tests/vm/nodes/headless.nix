# A machine with Plasma installed and no login: `kwin_wayland --virtual` runs as the test user's
# own user units, once for every session a test starts, each with its own private session bus and
# Wayland socket (`session-bus@NAME`, `kwin-virtual@NAME`). A virtual KWin has no real output, so it
# cannot draw: pixel and pointer tests use the session node instead.
{
  config,
  lib,
  pkgs,
  ...
}:
{
  imports = [ ./options.nix ];

  # KWin, its plugins and the environment a session expects
  services.desktopManager.plasma6.enable = true;
  hardware.graphics.enable = true;

  users.users.komorebi = {
    isNormalUser = true;
    uid = 1000;
    # keeps the user manager, and so /run/user/1000, alive without a login
    linger = true;
  };

  virtualisation = {
    memorySize = lib.mkDefault 3072;
    cores = lib.mkDefault 2;
    # KWin wants a DRM render node, which needs a GPU. The test driver adds one on aarch64 only.
    qemu.options = lib.optionals (config.komorebi.vm.renderNode && pkgs.stdenv.hostPlatform.isx86_64) [
      "-device virtio-gpu-pci"
    ];
  };

  environment.systemPackages = with pkgs; [
    foot
    jq
    kdotool
    kdePackages.libkscreen
    kdePackages.qttools
  ];

  systemd.user.services = {
    "session-bus@" = {
      description = "The private session bus of the test session %i";
      serviceConfig.ExecStart = "${pkgs.dbus}/bin/dbus-daemon --session --address=unix:path=%t/session-%i.bus --nofork --nopidfile";
    };

    "kwin-virtual@" = {
      description = "A headless KWin for the test session %i";
      requires = [ "session-bus@%i.service" ];
      after = [ "session-bus@%i.service" ];
      environment = {
        DBUS_SESSION_BUS_ADDRESS = "unix:path=%t/session-%i.bus";
        XDG_SESSION_TYPE = "wayland";
        # software rendering makes KWin skip every animating effect unless told otherwise
        KWIN_EFFECTS_FORCE_ANIMATIONS = "1";
        # script print() goes through qCDebug, which is off by default
        QT_LOGGING_RULES = "kwin_scripting.debug=true;js.debug=true;qml.debug=true";
      };
      serviceConfig = {
        # the library writes KWIN_ARGS (size, number of outputs) before it starts the unit
        EnvironmentFile = "-%t/kwin-virtual-%i.env";
        ExecStart = "${pkgs.kdePackages.kwin}/bin/kwin_wayland --virtual --socket wayland-%i --no-lockscreen --no-global-shortcuts $KWIN_ARGS";
      };
    };
  };
}
