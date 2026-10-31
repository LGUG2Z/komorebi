{
  tier = "slow";
  kind = "session";
  timeoutMinutes = 10;
  # the session draws without komorebi: a tiled terminal would cover the desktop this test looks at
  node.services.komorebi.enable = false;
}
