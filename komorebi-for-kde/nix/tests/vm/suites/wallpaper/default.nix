{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 15;
  # The session has the Plasma shell, which keeps the wallpapers, and the module installs the
  # border effect. The module's unit starts komorebi when the session does; the test stops that
  # daemon and starts its own runs of it, with the wallpapers it writes to the user's
  # `komorebi.json`.
}
