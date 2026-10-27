# A machine with Plasma installed and no login: `kwin_wayland --virtual` runs as the test user's
# own user units, once for every session a test starts, each with its own private session bus and
# Wayland socket (`session-bus@NAME`, `kwin-virtual@NAME`). A virtual KWin has no real output, so it
# cannot draw: pixel and pointer tests use the session node instead. `komorebi@NAME` runs the daemon
# in a session; `komorebi` is the package of the daemon and `komorebic`, which the test framework
# passes in.
{
  config,
  lib,
  pkgs,
  komorebi,
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
    komorebi
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
      # KWin looks for Xwayland on its PATH, when it is started with --xwayland
      path = [ pkgs.xwayland ];
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

    # Not bound to KWin's unit: a test restarts KWin under a running daemon
    "komorebi@" = {
      description = "komorebi in the test session %i";
      requires = [ "session-bus@%i.service" ];
      after = [
        "session-bus@%i.service"
        "kwin-virtual@%i.service"
      ];
      environment = {
        DBUS_SESSION_BUS_ADDRESS = "unix:path=%t/session-%i.bus";
        WAYLAND_DISPLAY = "wayland-%i";
        XDG_SESSION_TYPE = "wayland";
        # the log is read by the test, not a terminal
        NO_COLOR = "1";
      };
      serviceConfig = {
        # the library writes KOMOREBI_ARGS (the log level) before it starts the unit
        EnvironmentFile = "-%t/komorebi-%i.env";
        # a log file per run, emptied on every start, which the library reads
        ExecStart = "${pkgs.runtimeShell} -c 'exec ${komorebi}/bin/komorebi $${KOMOREBI_ARGS} > %t/komorebi-%i.log 2>&1'";
        # what komorebi exits with when it is told to stop
        SuccessExitStatus = 130;
      };
    };
  };
}
