{
  config,
  lib,
  pkgs,
  hostPkgs,
  ...
}:

# A NixOS Plasma 6 guest to develop and test against a real KWin, with komorebi installed by the
# flake's module, `services.komorebi`, from the flake's packages. The flake builds it for an Apple
# silicon Mac, which runs it under QEMU with HVF; docs/dev/utm.md says how to build and run it.
#
# The guest mounts the repository at /repo, and keeps its disk in the directory it is started
# from, which is best .vm (gitignored):
#
#   nix build .#nixosConfigurations.plasma-vm.config.system.build.vm -o .vm/result-plasma-vm
#   cd .vm && KOMOREBI_REPO=$PWD/.. ./result-plasma-vm/bin/run-plasma-vm-vm
#
# The console, in the terminal that runs the guest, is logged in as the user `komorebi`. ssh is
# forwarded to localhost:2222 (2223 for the session), for the keys in ~/.ssh/authorized_keys of
# that user, which the guest's disk keeps.
#
# The session variant is the one to live in: Plasma logs in by itself, the module's unit runs
# komorebi in it, and it can be reached over RDP (krdp.nix).
let
  cfg = config.komorebi.plasmaVm;
in
{
  imports = [ ./krdp.nix ];

  options.komorebi.plasmaVm = {
    session = lib.mkEnableOption ''
      a full autologin Plasma Wayland session (sddm + plasmashell), in addition to the headless
      `kwin_wayland --virtual` workflow that is always available from the console and over ssh
    '';
  };

  config = lib.mkMerge [
    {
      system.stateVersion = "26.05";
      networking.hostName = "plasma-vm";

      services.desktopManager.plasma6.enable = true;
      programs.dconf.enable = true;
      # mesa (llvmpipe over the virtio-gpu render node) so kwin can composite with GL
      hardware.graphics.enable = true;
      fonts.enableDefaultPackages = true;

      # komorebi and komorebic, from the flake's packages; the unit starts with a Plasma session
      services.komorebi.enable = true;

      services.openssh = {
        enable = true;
        settings = {
          PasswordAuthentication = false;
          PermitRootLogin = "no";
        };
      };

      # the way in before any key is in ~/.ssh/authorized_keys: there is no password anywhere
      services.getty.autologinUser = "komorebi";

      users.users.komorebi = {
        isNormalUser = true;
        uid = 1000;
        extraGroups = [
          "wheel"
          "video"
          "render"
        ];
        # keep a user manager (and XDG_RUNTIME_DIR) alive across ssh sessions
        linger = true;
      };
      security.sudo.wheelNeedsPassword = false;

      nix.settings.experimental-features = [
        "nix-command"
        "flakes"
      ];

      environment.systemPackages = with pkgs; [
        # clients to tile: Qt/KDE, GTK3/4 (CSD), Electron-ish via firefox, plain wayland, XWayland
        kdePackages.konsole
        kdePackages.dolphin
        kdePackages.kate
        kdePackages.spectacle
        firefox
        gnome-text-editor
        gnome-calculator
        foot
        xterm

        # poking at KWin
        kdePackages.qttools # qdbus6
        dbus
        jq
        kdotool
        strace

        # installing plasma-utm onto a disk image (docs/dev/utm.md)
        nixos-install-tools
        dosfstools
        parted
        e2fsprogs

        # building komorebi from /repo in the guest
        rustc
        cargo
        gcc
        pkg-config
        git
        rsync
        htop
      ];

      environment.sessionVariables = {
        # kwin refuses to load animating effects (including the stock scripted ones) under software
        # rendering; llvmpipe is all this vm has
        KWIN_EFFECTS_FORCE_ANIMATIONS = "1";
        # always build in guest-local storage; /repo is a 9p mount and is slow for target dirs
        CARGO_TARGET_DIR = "/home/komorebi/target";
      };

      virtualisation.vmVariant = {
        virtualisation = {
          host.pkgs = hostPkgs;
          qemu.forceAccel = true;
          cores = 6;
          memorySize = 8192;
          diskSize = 20 * 1024;
          graphics = false;
          # no display, but a gpu so there is a drm render node
          qemu.options = lib.mkIf (!cfg.session) [ "-device virtio-gpu-pci" ];
          forwardPorts = [
            {
              from = "host";
              host.address = "127.0.0.1";
              host.port = if cfg.session then 2223 else 2222;
              guest.port = 22;
            }
          ];
          sharedDirectories.repo = {
            # the runner says so when it is not set
            source = "\${KOMOREBI_REPO:?set KOMOREBI_REPO to the root of the repository}";
            target = "/repo";
            writable = true;
          };
        };
      };
    }

    (lib.mkIf cfg.session {
      # a separate disk image (named after the host) and ssh port so both vms can run at once
      networking.hostName = lib.mkForce "plasma-vm-session";
      services.displayManager = {
        sddm = {
          enable = true;
          wayland.enable = true;
        };
        autoLogin = {
          enable = true;
          user = "komorebi";
        };
        defaultSession = "plasma";
      };
      # kwin drives virtio-gpu through drm on the primary node; that is the only place mesa's software
      # rasterizer can allocate buffers (see docs/dev/PLATFORM.md: the headless --virtual backend
      # cannot)
      virtualisation.vmVariant.virtualisation.qemu.options = [
        "-device virtio-gpu-pci,xres=1920,yres=1080"
      ];
      # an idle lock screen or dimmed display changes how kwin animates and what screenshots show
      environment.etc."xdg/kscreenlockerrc".text = ''
        [Daemon]
        Autolock=false
        LockOnResume=false
      '';
      # and so does a display that has gone to sleep: with the output off, KWin sends no frame
      # callbacks, so clients stop answering resizes and every window stays where it was while komorebi
      # asks for another place (found on the UTM guest after it sat idle: black screenshots, nothing tiled)
      environment.etc."xdg/powerdevilrc".text = ''
        [AC][Display]
        DimDisplayWhenIdle=false
        TurnOffDisplayWhenIdle=false

        [AC][SuspendAndShutdown]
        AutoSuspendAction=0
      '';

      komorebi.krdp.enable = true;

      # only the host's loopback reaches it
      virtualisation.vmVariant.virtualisation.forwardPorts = [
        {
          from = "host";
          host.address = "127.0.0.1";
          host.port = 3389;
          guest.port = 3389;
        }
      ];
    })
  ];
}
