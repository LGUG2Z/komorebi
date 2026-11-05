{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 10;
  # the same test for the three formats
  script = "../../keys-corpus.py";
  node.services.komorebi.keybindings = {
    format = "sxhkd";
    file = ../../../../../../crates/komorebi-hotkeys/tests/fixtures/vm.sxhkdrc;
    mode = "auto";
  };
}
