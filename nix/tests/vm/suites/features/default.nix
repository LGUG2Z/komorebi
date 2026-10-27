{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 20;
  # The module installs the effects and its unit starts komorebi when the session does. The first
  # section uses that daemon; the others stop it and run the unit themselves, once for each
  # feature.
}
