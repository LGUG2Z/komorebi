# The demo guest of `run-kde`: a Plasma session in which komorebi tiles a few windows that open by
# themselves, with a cheat sheet to start from. It is a module to put on the session guest
# (plasma-vm.nix with `session` and not `display`: the flake's `run-kde-x86_64` and
# `run-kde-aarch64`), and the VM suite `run-kde` puts it on the session node, so that what the
# command starts is what CI boots.
#
# The closure is the same for everyone: no password, no key and no configuration of a person is in
# it. The host shares a directory, which the guest mounts at /run/run-kde, with
#
#   rdp-password    the password a client logs in to RDP with
#   ssh_key.pub     the public key that may log in over ssh
#
# and the guest takes them at boot (`run-kde-credentials`). What a person's own run needs at start
# up, the memory, the ports, the display device and the repository, comes from the environment of
# the runner, which `run-kde` sets (docs/dev/utm.md, docs/trying-it.md). The guest shows a window
# through QEMU or is reached over RDP, and the terminal of whoever starts it is its console.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  # where the host's directory is mounted, and where the guest keeps what it takes from it
  shared = "/run/run-kde";
  taken = "/run/run-kde-guest";

  user = "komorebi";

  cheatSheet = pkgs.writeText "run-kde-cheat-sheet.txt" ''
    komorebi for KDE tiles the windows of this Plasma session.

    The windows that opened by themselves (two terminals, Kate and Dolphin) share
    the screen. Open another one and it takes a tile of its own; close one and the
    others grow. This window is not tiled: komorebi leaves it alone. Press q to
    close it, and run `run-kde-cheat-sheet` in a terminal to open it again.

    Start with these. Alt is either Alt key.

      alt + h, j, k, l          focus the window to the left, below, above, right
      alt + shift + h, j, k, l  move the window that way
      alt + shift + return      promote the window to the biggest tile
      alt + t                   take the window out of the tiles and back in
      alt + shift + f           toggle monocle: the window fills the screen
      alt + 1 ... 9             show workspace 1 ... 9, each with its own layout
      alt + shift + 1 ... 9     move the window to that workspace
      alt + x, alt + y          flip the layout horizontally, vertically

    Or type the same in a terminal: `komorebic` is the program that every key
    runs.

      komorebic state                    everything komorebi knows, as JSON
      komorebic change-layout columns    bsp, rows, grid, scrolling, ...
      komorebic toggle-monocle
      komorebic retile                   put every window back in its tile
      komorebic border off               borders are drawn by a KWin effect
      komorebic --help

    komorebic reads its configuration from ~/.config/komorebi unless it is told
    where, and this machine starts komorebi with a file of its own, so tell it:

      KOMOREBI_CONFIG_HOME=/etc/run-kde/config-home komorebic check

    The key bindings komorebi reads are the whole of the file below, written in
    skhd's format. The repository is mounted at /repo: edit komorebi there and
    build it in this machine with `cargo`.

    ${builtins.readFile ../../../docs/linux/skhdrc.sample}
  '';

  # the file the module starts the daemon with, which is the `--config` of its command line, and
  # the directory `komorebic check` reads to see what the daemon runs: it looks in the config
  # home, not at the daemon's command line
  configHome =
    let
      words = lib.splitString " " config.services.komorebi.execStart;
      at = lib.lists.findFirstIndex (word: word == "--config") null words;
    in
    pkgs.linkFarm "run-kde-config-home" [
      {
        name = "komorebi.json";
        path = builtins.elemAt words (at + 1);
      }
      {
        name = "skhdrc";
        path = ../../../docs/linux/skhdrc.sample;
      }
    ];

  # What the window shows: the graphics line, which the guest looks up when it has started, the
  # text above, and what `komorebic check` finds now.
  cheatSheetCommand = pkgs.writeShellApplication {
    name = "run-kde-cheat-sheet";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.less
      config.services.komorebi.package
    ];
    text = ''
      text=$(mktemp)
      {
        printf 'Graphics: %s\n\n' "$(cat "''${XDG_RUNTIME_DIR:-/tmp}/run-kde-renderer" 2>/dev/null || echo "not known yet")"
        cat ${cheatSheet}
        printf '\nkomorebic check\n\n'
        NO_COLOR=1 KOMOREBI_CONFIG_HOME=${configHome} komorebic check 2>&1 || true
      } > "$text"
      exec less -R "$text"
    '';
  };

  # The windows of the demo, opened once the shell has its panel: a window that is tiled before
  # komorebi knows what room the panel takes is tiled under it, and nothing tiles it again.
  demo = pkgs.writeShellApplication {
    name = "run-kde-demo";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.jq
      config.services.komorebi.package
    ];
    text = ''
      panel=0
      for _ in $(seq 1 120); do
        if komorebic state 2>/dev/null |
          jq -e '.monitors.elements[0] | .work_area_size != .size' > /dev/null 2>&1; then
          panel=1
          break
        fi
        sleep 1
      done
      [ "$panel" = 1 ] || echo "komorebi does not know the panel's room: the windows may be tiled under it"

      tiled() {
        local n
        n=$(komorebic state 2>/dev/null |
          jq '[.monitors.elements[].workspaces.elements[] | .containers.elements[].windows.elements[]] | length' 2> /dev/null || true)
        echo "''${n:-0}"
      }

      # starts a program in a scope of its own, which outlives this script, and waits until komorebi
      # has a tile for it
      open() {
        local before
        before=$(tiled)
        systemd-run --user --scope --quiet -- "$@" &
        for _ in $(seq 1 60); do
          [ "$(tiled)" -gt "$before" ] && return 0
          sleep 1
        done
        echo "no tile for $1 after a minute"
      }

      dir=$HOME
      [ -d /repo ] && dir=/repo

      open konsole --workdir "$dir"
      open konsole
      open kate
      open dolphin "$dir"

      # the cheat sheet says what KWin draws with, which the guest looks up once KWin has started
      for _ in $(seq 1 60); do
        [ -s "$XDG_RUNTIME_DIR/run-kde-renderer" ] && break
        sleep 1
      done

      systemd-run --user --scope --quiet -- foot --app-id komorebi-cheat-sheet \
        --title "komorebi cheat sheet" --window-size-chars 100x34 -o main.font=monospace:size=10 \
        run-kde-cheat-sheet &
      sleep 1
    '';
  };

  # What KWin draws with, which is what the cheat sheet and the console say: "GPU" when the host's
  # GPU renders for it (virgl) and "software" when the guest's CPUs do (llvmpipe).
  report = pkgs.writeShellApplication {
    name = "run-kde-report";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.gnused
      pkgs.kdePackages.qttools
    ];
    text = ''
      export XDG_RUNTIME_DIR=/run/user/${toString config.users.users.${user}.uid}
      export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
      renderer=
      for _ in $(seq 1 300); do
        renderer=$(qdbus org.kde.KWin /KWin supportInformation 2> /dev/null |
          sed -n 's/^OpenGL renderer string: //p' | head -n 1 || true)
        [ -n "$renderer" ] && break
        sleep 1
      done
      [ -n "$renderer" ] || exit 0
      case $renderer in
        *llvmpipe* | *softpipe* | *swrast*) kind=software ;;
        *) kind=GPU ;;
      esac
      line="$kind (''${renderer%% *}): $renderer"
      printf '%s\n' "$line" > "$XDG_RUNTIME_DIR/run-kde-renderer"
      echo "run-kde: KWin draws with $line"
    '';
  };
in
{
  imports = [ ./krdp.nix ];

  komorebi.krdp = {
    enable = true;
    passwordFile = "${taken}/rdp-password";
  };

  services.komorebi = {
    config = {
      border = true;
      border_width = 6;
      border_offset = 5;
      default_workspace_padding = 15;
      default_container_padding = 15;
      animation = {
        enabled = true;
        style = "EaseOutExpo";
        duration = 250;
      };
      # this window is the guide to the others, and is not one of them
      ignore_rules = [
        {
          kind = "Class";
          id = "komorebi-cheat-sheet";
          matching_strategy = "Equals";
        }
      ];
      monitors = [
        {
          workspaces = [
            {
              name = "I";
              layout = "BSP";
            }
            {
              name = "II";
              layout = "VerticalStack";
            }
            {
              name = "III";
              layout = "HorizontalStack";
            }
            {
              name = "IV";
              layout = "Columns";
            }
            {
              name = "V";
              layout = "Rows";
            }
            {
              name = "VI";
              layout = "Grid";
            }
            {
              name = "VII";
              layout = "RightMainVerticalStack";
            }
            {
              name = "VIII";
              layout = "Scrolling";
            }
          ];
        }
      ];
    };

    # the module's default mode, with the sample bindings of docs/skhdrc.sample
    keybindings = {
      format = "skhd";
      file = ../../../docs/linux/skhdrc.sample;
    };
  };

  environment.systemPackages = [
    cheatSheetCommand
    pkgs.foot
    pkgs.jq
    pkgs.kdePackages.dolphin
    pkgs.kdePackages.kate
    pkgs.kdePackages.konsole
    pkgs.kdePackages.qttools
    pkgs.less
  ];
  environment.etc."run-kde/cheat-sheet.txt".source = cheatSheet;
  environment.etc."run-kde/config-home".source = configHome;

  # The host's password and key, taken from the directory it shares. The files may belong to a user
  # of the host who is not the guest's user, so the guest copies them, as root, and gives them the
  # owner and the mode that RDP's unit and ssh want. Before the login, so that the unit that reads
  # the password finds it, and before ssh. A guest that is started with no directory has neither,
  # and RDP does not start.
  systemd.services.run-kde-credentials = {
    description = "Take the RDP password and the ssh key that the host shares";
    wantedBy = [ "multi-user.target" ];
    before = [
      "display-manager.service"
      "sshd.service"
    ];
    unitConfig = {
      ConditionPathExists = shared;
      RequiresMountsFor = shared;
    };
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.coreutils ];
    script = ''
      install -d -m 0755 ${taken} ${taken}/authorized_keys.d
      if [ -r ${shared}/rdp-password ]; then
        install -m 0400 -o ${user} -g ${config.users.users.${user}.group} \
          ${shared}/rdp-password ${taken}/rdp-password
      fi
      if [ -r ${shared}/ssh_key.pub ]; then
        install -m 0644 ${shared}/ssh_key.pub ${taken}/authorized_keys.d/${user}
      fi
    '';
  };
  services.openssh.authorizedKeysFiles = [ "${taken}/authorized_keys.d/%u" ];

  systemd.services.run-kde-report = {
    description = "Say what KWin draws with";
    wantedBy = [ "multi-user.target" ];
    after = [ "user@${toString config.users.users.${user}.uid}.service" ];
    serviceConfig = {
      # simple, so that waiting for KWin does not hold up the machine's start
      Type = "simple";
      User = user;
      ExecStart = lib.getExe report;
      # the line goes to the console, which is the terminal of whoever started the guest
      StandardOutput = "journal+console";
    };
  };

  # The windows of the demo, once the session is up. The unit does not wait for the panel, the
  # script does, since only komorebi can say that it knows the panel's room.
  systemd.user.services.run-kde-demo = {
    description = "Open the windows of the demo";
    wantedBy = [ "plasma-workspace.target" ];
    after = [
      "plasma-core.target"
      "komorebi.service"
    ];
    # the PATH a terminal in the session has, as the module's unit has it
    enableDefaultPath = false;
    environment.PATH = lib.concatStringsSep ":" [
      "/run/wrappers/bin"
      "/etc/profiles/per-user/%u/bin"
      "/run/current-system/sw/bin"
    ];
    serviceConfig = {
      Type = "simple";
      ExecStart = lib.getExe demo;
      # the windows are in scopes of their own, and stay when the script is done
      KillMode = "process";
    };
  };

  # Mesa's video compositor (vl_compositor) never loads an RGB to YUV matrix on a driver that takes
  # its graphics path, which virgl does: the draw multiplies by the YUV to RGB one of its initial
  # state, and the NV12 that scale_vaapi makes of the screen, which KRDP encodes, is the wrong
  # picture (nix/dev/mesa-virgl-rgb-yuv-csc.patch). `builtins.path` makes the patch a store path of
  # its own, so that Mesa is built once for any way of reading this file. There is no binary cache:
  # this is a build of Mesa from source.
  #
  # The same driver leaks a sync_file, an out-fence of the kernel, for every picture it converts or
  # uploads: the surface keeps the fence of its last flush in `pipe_fence`, and the next flush of
  # it stores a new one over it, so the first is never released, and its file descriptor is never
  # closed. A KRDP session ran out of descriptors (1024) after about 950 frames, and the stream
  # froze (nix/dev/mesa-virgl-fence-leak.patch).
  hardware.graphics.package = pkgs.mesa.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [
      (builtins.path {
        path = ./mesa-virgl-rgb-yuv-csc.patch;
        name = "mesa-virgl-rgb-yuv-csc.patch";
      })
      (builtins.path {
        path = ./mesa-virgl-fence-leak.patch;
        name = "mesa-virgl-fence-leak.patch";
      })
    ];
  });

  # What only the runner, which is the machine of the person who starts the guest, can say. The
  # ports are forwarded and the display device is added on its command line, and the memory is
  # sized to what that machine has: the shared memory that virtiofs needs is a backend of the
  # size of the guest's memory, which nixpkgs would fix at the size of `memorySize`.
  virtualisation.vmVariant.virtualisation = {
    forwardPorts = lib.mkForce [ ];
    # `run-kde` says whether KVM is there, and `--tcg` lets it start without
    qemu.forceAccel = lib.mkForce false;
    qemu.enableSharedMemory = false;
    # QEMU never asks virglrenderer for the host's video encoder, so the guest's VA-API lists none
    # and KRDP encodes H.264 on the guest's CPUs. The patch does (nix/dev/qemu-virgl-video.patch);
    # VIRGL_RENDERER_USE_VIDEO is only defined with VIRGL_RENDERER_UNSTABLE_APIS. Only the runner
    # has it: there is no binary cache, so it is a QEMU built from source. The virglrenderer under
    # it has four patches, in this order, for a host whose encoder is the d3d12 one of WSL 2:
    #   chroma-format    the guest's Mesa and virglrenderer number the chroma format of a codec
    #                    differently: 4:2:0 was read as 4:2:2, and the encoder refused every codec
    #   video-memory-upload
    #                    the d3d12 driver has no dma-bufs, so the picture to encode goes to the
    #                    surface through memory
    #   h264-packed-sps  Mesa's VA-API takes the level of the stream from a packed SPS, which
    #                    virglrenderer never sent: level 0 crashed the d3d12 encoder
    #   ubo0-constants   a constant buffer 0 bound as a buffer was never read by the shader, so the
    #                    colour matrix of an RGB to YUV conversion was zero
    qemu.package =
      (pkgs.qemu_kvm.override {
        virglrenderer = pkgs.virglrenderer.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [
            ./virglrenderer-chroma-format.patch
            ./virglrenderer-video-memory-upload.patch
            ./virglrenderer-h264-packed-sps.patch
            ./virglrenderer-ubo0-constants.patch
          ];
        });
      }).overrideAttrs
        (old: {
          patches = (old.patches or [ ]) ++ [ ./qemu-virgl-video.patch ];
          configureFlags = (old.configureFlags or [ ]) ++ [
            "--extra-cflags=-DVIRGL_RENDERER_UNSTABLE_APIS"
          ];
          doCheck = false;
        });
    qemu.options = [
      "-object memory-backend-memfd,id=mem0,size=\${RUN_KDE_MEMORY:-8192}M,share=on"
      "-machine memory-backend=mem0"
    ];
    sharedDirectories.run-kde = {
      source = "\${RUN_KDE_STATE:?set RUN_KDE_STATE to the directory with the password and the key}";
      target = shared;
    };
    # a mount under /run is hidden when the root file system's /run is replaced by a tmpfs, which
    # happens before the units run
    fileSystems.${shared}.neededForBoot = lib.mkForce false;
  };
}
