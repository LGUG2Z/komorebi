#!/bin/sh
# Runs in the guest, as the logged-in user, to make it what a scenario clones: the agent starts at
# login from a Terminal window (which holds the grants, so the agent and the daemon it starts are
# attributed to it), nothing from earlier runs is left, and Terminal does not reopen old windows.
set -eu

pkill -x komorebi || true
pkill -f komorebi-e2e-agent || true
pkill -f komorebi-testwin || true
killall Terminal 2>/dev/null || true
sleep 1

rm -rf "$HOME/Library/Saved Application State/com.apple.Terminal.savedState" \
  "$HOME/Library/Application Support/komorebi"
defaults write com.apple.Terminal NSQuitAlwaysKeepsWindows -bool false
defaults write -g ApplePersistenceIgnoreState -bool true

mkdir -p "$HOME/e2e/bin" "$HOME/e2e/tmp" "$HOME/Library/LaunchAgents"

# what the launch agent opens: a Terminal window that runs the agent
cat > "$HOME/e2e/agent.command" <<COMMAND
#!/bin/sh
exec \$HOME/e2e/bin/komorebi-e2e-agent
COMMAND
chmod +x "$HOME/e2e/agent.command"

cat > "$HOME/Library/LaunchAgents/dev.komorebi.e2e-agent.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.komorebi.e2e-agent</string>
  <key>ProgramArguments</key><array>
    <string>/usr/bin/open</string><string>-a</string><string>Terminal</string>
    <string>$HOME/e2e/agent.command</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict></plist>
PLIST
plutil -lint "$HOME/Library/LaunchAgents/dev.komorebi.e2e-agent.plist"
