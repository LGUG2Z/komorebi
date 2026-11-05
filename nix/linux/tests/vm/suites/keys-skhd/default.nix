{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 10;
  # the same test for the three formats
  script = "../../keys-corpus.py";
  node.services.komorebi.keybindings = {
    format = "skhd";
    file = ../../../../../../crates/komorebi-hotkeys/tests/fixtures/vm.skhdrc;
    mode = "auto";
  };
}
