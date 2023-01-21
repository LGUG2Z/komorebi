if (!(Get-Process whkd -ErrorAction SilentlyContinue))
{
    Start-Process whkd -WindowStyle hidden
}

. $PSScriptRoot\komorebi.generated.ps1

# Default to minimizing windows when switching workspaces
komorebic window-hiding-behaviour minimize
# Set cross-monitor move behaviour to insert instead of swap
komorebic cross-monitor-move-behaviour insert
# Enable hot reloading of changes to this file
komorebic watch-configuration enable

# Configure the invisible border dimensions
komorebic invisible-borders 7 0 14 7

# Ensure there is 1 workspace created on monitor 0
komorebic ensure-workspaces 0 1

# Configure the 1st workspace on the 1st monitor
komorebic workspace-name 0 0 "I"
komorebic workspace-layout 0 0 bsp
komorebic container-padding 0 0 bsp

# Uncomment the next lines if you want a visual border around the active window
# komorebic active-window-border-colour 66 165 245 --window-kind single
# komorebic active-window-border-colour 256 165 66 --window-kind stack
# komorebic active-window-border enable

komorebic complete-configuration