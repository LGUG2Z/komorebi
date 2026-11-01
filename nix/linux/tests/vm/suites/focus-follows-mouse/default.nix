{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 15;
  # The module's unit starts komorebi when the session does; the test stops that daemon and runs
  # the unit itself, so that it sees what komorebi does to KWin's configuration from the first
  # moment. The pointer is a uinput device the session node has (komorebi_vm.Pointer).
}
