{
  tier = "fast";
  kind = "session";
  timeoutMinutes = 12;
  node =
    { pkgs, ... }:
    let
      # a key pair for the ssh check: the suite makes one, and the host's directory has its public half
      key = pkgs.runCommand "run-kde-test-key" { nativeBuildInputs = [ pkgs.openssh ]; } ''
        mkdir $out
        ssh-keygen -q -t ed25519 -N "" -f $out/key
      '';
      # the directory the host shares, as the guest finds it once it has mounted it: a user of the
      # host owns it, and the guest's user cannot read it
      host = {
        user = "4242";
        group = "4242";
      };
    in
    {
      # the guest of `run-kde`, as the command starts it, on the session node
      imports = [ ../../../../dev/run-kde.nix ];

      systemd.tmpfiles.settings."run-kde-share" = {
        "/run/run-kde".d = host // {
          mode = "0700";
        };
        "/run/run-kde/rdp-password".f = host // {
          mode = "0600";
          argument = "a-password-of-the-host";
        };
        "/run/run-kde/ssh_key.pub".C = host // {
          mode = "0600";
          argument = "${key}/key.pub";
        };
      };
      environment.etc."run-kde-test/ssh_key".source = "${key}/key";

      # the session guests have sshd, and the key is for it
      services.openssh = {
        enable = true;
        settings.PasswordAuthentication = false;
      };

      # the client of the RDP checks: an X server of its own, and what reads its picture back
      environment.systemPackages = [
        pkgs.iproute2
        pkgs.netpbm
        pkgs.xvfb
        pkgs.xwd
      ];
    };
}
