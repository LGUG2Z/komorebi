{
  tier = "slow";
  kind = "headless";
  timeoutMinutes = 20;
  node =
    { pkgs, ... }:
    {
      # eight clients, Firefox among them
      virtualisation = {
        memorySize = 6144;
        cores = 4;
      };
      environment.systemPackages = with pkgs; [
        firefox
        gnome-calculator
        gnome-text-editor
        kdePackages.dolphin
        kdePackages.kate
        kdePackages.konsole
        xterm
      ];
    };
}
