{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 15;
  # The module's unit starts komorebi when the session does, and the test drags against that
  # daemon, reading the drags it has handled from the unit's journal. The pointer is a uinput
  # device the session node has (komorebi_vm.Pointer).
}
