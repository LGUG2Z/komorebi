# KRDP, Plasma's RDP server, in the Plasma session of every user who logs in. The session guest
# of plasma-vm.nix enables it, and so does the VM suite `rdp`, which checks that a client can log
# in and get a picture of the desktop (docs/dev/PLATFORM.md says what it does not check).
#
# The password is not in the repository: the unit starts once the user has written one to
# ~/.config/komorebi-vm/rdp-password, and a client logs in with the user's name and that password.
# A guest whose password comes from outside, as the demo guest's does (run-kde.nix), names a file
# of the guest in `passwordFile`, which the unit reads in place of that one when it is there.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.komorebi.krdp;

  startKrdp = pkgs.writeShellScript "krdp-start" ''
    set -eu
    dir=$HOME/.config/komorebi-vm
    password_file=$dir/rdp-password
    ${lib.optionalString (cfg.passwordFile != null) ''
      if [ -r ${lib.escapeShellArg cfg.passwordFile} ]; then
        password_file=${lib.escapeShellArg cfg.passwordFile}
      fi
    ''}
    mkdir -p "$dir"
    # krdpserver refuses to run without a TLS certificate and key (its --help promises a temporary
    # one, but it only checks that the files exist), so make a throwaway self-signed pair
    if [ ! -f "$dir/rdp.crt" ] || [ ! -f "$dir/rdp.key" ]; then
      ${pkgs.openssl}/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj /CN=komorebi-vm \
        -addext subjectAltName=DNS:localhost,IP:127.0.0.1 -keyout "$dir/rdp.key" -out "$dir/rdp.crt"
      chmod 600 "$dir/rdp.key"
    fi
    # --plasma asks KWin for the screen and for input directly, with no portal prompt to click
    exec ${pkgs.kdePackages.krdp}/bin/krdpserver --plasma --address 0.0.0.0 --port 3389 \
      --certificate "$dir/rdp.crt" --certificate-key "$dir/rdp.key" \
      --username "$USER" --password "$(cat "$password_file")"
  '';
in
{
  options.komorebi.krdp = {
    enable = lib.mkEnableOption "KRDP, an RDP server, in every Plasma session";

    passwordFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/run/run-kde-guest/rdp-password";
      description = ''
        The path, in the guest, of a file with the password a client logs in with. It is a path in
        the running guest and not a file of the Nix store, so that no password is part of the
        system. The unit reads it in place of `~/.config/komorebi-vm/rdp-password` when it exists,
        and starts when either exists.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      pkgs.kdePackages.krdp
      # A client, to try the server from inside the guest. FreeRDP decodes H.264 with OpenH264
      # when it is built with it, and OpenH264 shows nothing of the video KRDP sends; without it,
      # FreeRDP decodes with FFmpeg (the VM suite `rdp`, docs/dev/PLATFORM.md).
      (pkgs.freerdp.override { openh264 = null; })
    ];

    # krdp streams the screen through pipewire
    services.pipewire = {
      enable = true;
      wireplumber.enable = true;
    };
    security.rtkit.enable = true;

    networking.firewall.allowedTCPPorts = [ 3389 ];

    systemd.user.services.krdp = {
      description = "RDP into this Plasma session";
      wantedBy = [ "plasma-workspace.target" ];
      after = [ "plasma-core.target" ];
      # with no password the unit is skipped, not failed: `systemctl --user start krdp` once the
      # file is there
      unitConfig.ConditionPathExists =
        if cfg.passwordFile == null then
          "%h/.config/komorebi-vm/rdp-password"
        else
          [
            "|%h/.config/komorebi-vm/rdp-password"
            "|${cfg.passwordFile}"
          ];
      serviceConfig = {
        ExecStart = startKrdp;
        Restart = "on-failure";
        RestartSec = 3;
      };
    };
  };
}
