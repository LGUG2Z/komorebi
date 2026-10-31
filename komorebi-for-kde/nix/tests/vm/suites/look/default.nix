{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 15;
  # The module installs the border effect, and its unit starts komorebi when the session does. The
  # test stops that daemon and starts its own runs of it, so that it sees what komorebi does to
  # KWin from the first moment.
}
