#SingleInstance Force

; Enable focus follows mouse
Run, komorebic.exe focus-follows-mouse enable

; Ensure there are 3 workspaces created on monitor 0
Run, komorebic.exe ensure-workspaces 0 4

; Give the workspaces some optional names
Run, komorebic.exe workspace-name 0 0 bsp
Run, komorebic.exe workspace-name 0 1 columns
Run, komorebic.exe workspace-name 0 2 thicc
Run, komorebic.exe workspace-name 0 3 matrix

; Set the padding of the different workspaces
Run, komorebic.exe workspace-padding 0 1 50
Run, komorebic.exe container-padding 0 1 50
Run, komorebic.exe workspace-padding 0 2 200
Run, komorebic.exe workspace-padding 0 3 0
Run, komorebic.exe container-padding 0 3 0

; Set the layouts of different workspaces
Run, komorebic.exe workspace-layout 0 1 columns

; Always float IntelliJ popups, matching on class
Run, komorebic.exe float-class SunAwtDialog, , Hide
; Always float Control Panel, matching on title
Run, komorebic.exe float-title "Control Panel", , Hide
; Always float Task Manager, matching on class
Run, komorebic.exe float-class TaskManagerWindow, , Hide
; Always float Wally, matching on executable name
Run, komorebic.exe float-exe Wally.exe, , Hide
; Always float Calculator app, matching on window title
Run, komorebic.exe float-title Calculator, , Hide
Run, komorebic.exe float-exe 1Password.exe, , Hide

; Change the focused window, Alt + Vim direction keys
!h::
Run, komorebic.exe focus left, , Hide
return

!j::
Run, komorebic.exe focus down, , Hide
return

!k::
Run, komorebic.exe focus up, , Hide
return

!l::
Run, komorebic.exe focus right, , Hide
return

; Move the focused window in a given direction, Alt + Shift + Vim direction keys
!+h::
Run, komorebic.exe move left, Hide
return

!+j::
Run, komorebic.exe move down, Hide
return

!+k::
Run, komorebic.exe move up, Hide
return

!+l::
Run, komorebic.exe move right, Hide
return

; Stack the focused window in a given direction, Alt + direction keys
!Left::
Run, komorebic.exe stack left, Hide
return

!Down::
Run, komorebic.exe stack down, Hide
return

!Up::
Run, komorebic.exe stack up, Hide
return

!Right::
Run, komorebic.exe stack right, Hide
return

!]::
Run, komorebic.exe cycle-stack next, , Hide
return

![::
Run, komorebic.exe cycle-stack previous, , Hide
return

; Unstack the focused window
!d::
Run, komorebic.exe unstack, Hide
return

; Promote the focused window to the top of the tree, Alt + Shift + Enter
!+Enter::
Run, komorebic.exe promote, Hide
return

; Switch to an equal-width, max-height column layout on the main workspace, Alt + Shift + C
!+c::
Run, komorebic.exe workspace-layout 0 0 columns, Hide
return

; Switch to the default bsp tiling layout on the main workspace, Alt + Shift + T
!+t::
Run, komorebic.exe workspace-layout 0 0 bsp, Hide
return

; Toggle the Monocle layout for the focused window, Alt + Shift + F
!+f::
Run, komorebic.exe toggle-monocle, Hide
return

; Flip horizontally, Alt + X
!x::
Run, komorebic.exe flip-layout horizontal, Hide
return

; Flip vertically, Alt + Y
!y::
Run, komorebic.exe flip-layout vertical, Hide
return

; Force a retile if things get janky, Alt + Shift + R
!+r::
Run, komorebic.exe retile, Hide
return

; Float the focused window, Alt + T
!t::
Run, komorebic.exe toggle-float, Hide
return

; Pause responding to any window events or komorebic commands, Alt + P
!p::
Run, komorebic.exe toggle-pause, Hide
return

; Switch to workspace
!1::
Send !
Run, komorebic.exe focus-workspace 0, Hide
return

!2::
Send !
Run, komorebic.exe focus-workspace 1, Hide
return

!3::
Send !
Run, komorebic.exe focus-workspace 2, Hide
return

!4::
Send !
Run, komorebic.exe focus-workspace 3, Hide
return

; Move window to workspace
!+1::
Run, komorebic.exe move-to-workspace 0, Hide
return

!+2::
Run, komorebic.exe move-to-workspace 1, Hide
return

!+3::
Run, komorebic.exe move-to-workspace 2, Hide
return

!+4::
Run, komorebic.exe move-to-workspace 3, Hide
return
