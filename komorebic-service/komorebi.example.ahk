#Requires AutoHotkey v2.0
#SingleInstance Force

; =============================================================================
; Komorebi AHK v2 Integration - Using komorebic-service
; =============================================================================
; This script communicates with komorebic-service via Named Pipes for instant
; command execution without process spawning overhead.
;
; Make sure komorebic-service.exe is running before using these hotkeys.
; =============================================================================

; -----------------------------------------------------------------------------
; Core Communication Function
; -----------------------------------------------------------------------------
Komorebic(command) {
    static PIPE_NAME := "\\.\pipe\komorebi-command"
    
    try {
        ; Open the named pipe for writing
        ; Use UTF-8-RAW to avoid BOM (Byte Order Mark)
        ; This is a kernel-level operation with ~1-2ms latency
        pipe := FileOpen(PIPE_NAME, "w", "UTF-8-RAW")
        
        if (pipe) {
            ; Write the JSON command
            pipe.Write(command)
            ; Close to flush and send immediately
            pipe.Close()
        }
    } catch as e {
        ; If the pipe is not available, komorebic-service might not be running
        OutputDebug("Komorebi pipe error: " . e.Message)
        ; Optional: Try to restart the service
        ; Run("komorebic-service.exe", , "Hide")
    }
}

; -----------------------------------------------------------------------------
; Window Focus (Alt + h/j/k/l - Vim-style)
; -----------------------------------------------------------------------------
!h:: Komorebic('Focus Left')
!j:: Komorebic('Focus Down')
!k:: Komorebic('Focus Up')
!l:: Komorebic('Focus Right')

; -----------------------------------------------------------------------------
; Move Windows (Alt + Shift + h/j/k/l)
; -----------------------------------------------------------------------------
!+h:: Komorebic('Move Left')
!+j:: Komorebic('Move Down')
!+k:: Komorebic('Move Up')
!+l:: Komorebic('Move Right')

; -----------------------------------------------------------------------------
; Stack Windows (Alt + Arrow Keys)
; -----------------------------------------------------------------------------
!Left::  Komorebic('Stack Left')
!Right:: Komorebic('Stack Right')
!Up::    Komorebic('Stack Up')
!Down::  Komorebic('Stack Down')

; Unstack
!+BackSpace:: Komorebic('UnstackWindow')

; Stack all windows
!s:: Komorebic('StackAll')

; -----------------------------------------------------------------------------
; Workspace Management (Alt + 1-9)
; -----------------------------------------------------------------------------
!1:: Komorebic('FocusWorkspace 0')
!2:: Komorebic('FocusWorkspace 1')
!3:: Komorebic('FocusWorkspace 2')
!4:: Komorebic('FocusWorkspace 3')
!5:: Komorebic('FocusWorkspace 4')
!6:: Komorebic('FocusWorkspace 5')
!7:: Komorebic('FocusWorkspace 6')
!8:: Komorebic('FocusWorkspace 7')
!9:: Komorebic('FocusWorkspace 8')

; Focus Named Workspace (Win + 0-9)
#0:: Komorebic('FocusNamedWorkspace 0')
#1:: Komorebic('FocusNamedWorkspace 1')
#2:: Komorebic('FocusNamedWorkspace 2')
#3:: Komorebic('FocusNamedWorkspace 3')
#4:: Komorebic('FocusNamedWorkspace 4')
#5:: Komorebic('FocusNamedWorkspace 5')
#6:: Komorebic('FocusNamedWorkspace 6')
#7:: Komorebic('FocusNamedWorkspace 7')
#8:: Komorebic('FocusNamedWorkspace 8')
#9:: Komorebic('FocusNamedWorkspace 9')

; Focus Named Workspace (Win + Numpad 0-9)
#Numpad0:: Komorebic('FocusNamedWorkspace 0')
#Numpad1:: Komorebic('FocusNamedWorkspace 1')
#Numpad2:: Komorebic('FocusNamedWorkspace 2')
#Numpad3:: Komorebic('FocusNamedWorkspace 3')
#Numpad4:: Komorebic('FocusNamedWorkspace 4')
#Numpad5:: Komorebic('FocusNamedWorkspace 5')
#Numpad6:: Komorebic('FocusNamedWorkspace 6')
#Numpad7:: Komorebic('FocusNamedWorkspace 7')
#Numpad8:: Komorebic('FocusNamedWorkspace 8')
#Numpad9:: Komorebic('FocusNamedWorkspace 9')

; Cycle Workspace (Win + Alt + Left/Right)
#!Left:: Komorebic('CycleWorkspace Previous')
#!Right:: Komorebic('CycleWorkspace Next')

; Focus Last Workspace (Win + D)
#d:: Komorebic('FocusLastWorkspace')

; Move to Named Workspace (Win + Shift + 0-9)
#+0:: Komorebic('MoveToNamedWorkspace 0')
#+1:: Komorebic('MoveToNamedWorkspace 1')
#+2:: Komorebic('MoveToNamedWorkspace 2')
#+3:: Komorebic('MoveToNamedWorkspace 3')
#+4:: Komorebic('MoveToNamedWorkspace 4')
#+5:: Komorebic('MoveToNamedWorkspace 5')
#+6:: Komorebic('MoveToNamedWorkspace 6')
#+7:: Komorebic('MoveToNamedWorkspace 7')
#+8:: Komorebic('MoveToNamedWorkspace 8')
#+9:: Komorebic('MoveToNamedWorkspace 9')

; Move Workspace to Monitor (Win + Shift + A/D with cycle)
global monitorOrder := [2, 0, 1]  ; Left, Main, Right - adjust indices for your setup
global currentMonitorPos := 2

#+a:: {
    global currentMonitorPos, monitorOrder
    currentMonitorPos := Mod(currentMonitorPos - 2 + monitorOrder.Length, monitorOrder.Length) + 1
    ; Note: MoveWorkspaceToMonitor would need to be added to parse_simple_command if needed
    ; For now, use the JSON format for this command
    Komorebic('{"type": "MoveWorkspaceToMonitorNumber", "content": ' monitorOrder[currentMonitorPos] '}')
}

#+d:: {
    global currentMonitorPos, monitorOrder
    currentMonitorPos := Mod(currentMonitorPos, monitorOrder.Length) + 1
    Komorebic('{"type": "MoveWorkspaceToMonitorNumber", "content": ' monitorOrder[currentMonitorPos] '}')
}

; -----------------------------------------------------------------------------
; Layout Management
; -----------------------------------------------------------------------------
; Cycle Layout (Win + Space)
#Space:: Komorebic('CycleLayout')

; Cycle Layout Previous (Win + Shift + Space)
#+Space:: Komorebic('CycleLayout Previous')

; -----------------------------------------------------------------------------
; Window Toggles
; -----------------------------------------------------------------------------
; Toggle Float (Alt + f)
!f:: Komorebic('ToggleFloat')

; Toggle Monocle (Alt + m)
!m:: Komorebic('ToggleMonocle')

; Toggle Maximize (Alt + x)
!x:: Komorebic('ToggleMaximize')

; Retile (Alt + r)
!r:: Komorebic('Retile')

; Promote (Alt + p)
!p:: Komorebic('Promote')

; Toggle Tiling (Alt + t)
!t:: Komorebic('ToggleTiling')

; Toggle Pause (Alt + /)
!/:: Komorebic('TogglePause')

; Focus Named Workspace (Win + Numpad 0-9)
#Numpad0:: Komorebic('{"FocusNamedWorkspace":"0"}')
#Numpad1:: Komorebic('{"FocusNamedWorkspace":"1"}')
#Numpad2:: Komorebic('{"FocusNamedWorkspace":"2"}')
#Numpad3:: Komorebic('{"FocusNamedWorkspace":"3"}')
#Numpad4:: Komorebic('{"FocusNamedWorkspace":"4"}')
#Numpad5:: Komorebic('{"FocusNamedWorkspace":"5"}')
#Numpad6:: Komorebic('{"FocusNamedWorkspace":"6"}')
#Numpad7:: Komorebic('{"FocusNamedWorkspace":"7"}')
#Numpad8:: Komorebic('{"FocusNamedWorkspace":"8"}')
#Numpad9:: Komorebic('{"FocusNamedWorkspace":"9"}')

; Cycle Workspace (Win + Alt + Left/Right)
#!Left:: Komorebic('{"CycleWorkspace":"Previous"}')
#!Right:: Komorebic('{"CycleWorkspace":"Next"}')

; Focus Last Workspace (Win + D)
#d:: Komorebic('"FocusLastWorkspace"')

; Move to Named Workspace (Win + Shift + 0-9)
#+0:: Komorebic('{"SendToNamedWorkspace":"0"}')
#+1:: Komorebic('{"SendToNamedWorkspace":"1"}')
#+2:: Komorebic('{"SendToNamedWorkspace":"2"}')
#+3:: Komorebic('{"SendToNamedWorkspace":"3"}')
#+4:: Komorebic('{"SendToNamedWorkspace":"4"}')
#+5:: Komorebic('{"SendToNamedWorkspace":"5"}')
#+6:: Komorebic('{"SendToNamedWorkspace":"6"}')
#+7:: Komorebic('{"SendToNamedWorkspace":"7"}')
#+8:: Komorebic('{"SendToNamedWorkspace":"8"}')
#+9:: Komorebic('{"SendToNamedWorkspace":"9"}')

; Move Workspace to Monitor (Win + Shift + A/D with cycle)
global monitorOrder := [2, 0, 1]  ; Left, Main, Right - adjust indices for your setup
global currentMonitorPos := 2

#+a:: {
    global currentMonitorPos, monitorOrder
    currentMonitorPos := Mod(currentMonitorPos - 2 + monitorOrder.Length, monitorOrder.Length) + 1
    Komorebic('{"MoveWorkspaceToMonitor":' monitorOrder[currentMonitorPos] '}')
}

#+d:: {
    global currentMonitorPos, monitorOrder
    currentMonitorPos := Mod(currentMonitorPos, monitorOrder.Length) + 1
    Komorebic('{"MoveWorkspaceToMonitor":' monitorOrder[currentMonitorPos] '}')
}

; -----------------------------------------------------------------------------
; Layout Management
; -----------------------------------------------------------------------------
; Cycle Layout (Win + Space)
#Space:: Komorebic('"CycleLayout"')

; -----------------------------------------------------------------------------
; Window Toggles
; -----------------------------------------------------------------------------
; Toggle Float (Alt + f)
!f:: Komorebic('"ToggleFloat"')

; Toggle Monocle (Alt + m)
!m:: Komorebic('"ToggleMonocle"')

; Toggle Maximize (Alt + Shift + m)
!+m:: Komorebic('"ToggleMaximize"')

; -----------------------------------------------------------------------------
; Resize Windows
; -----------------------------------------------------------------------------
; Resize with Alt + Ctrl + Arrow Keys
!^Left::  Komorebic('{"ResizeWindowEdge":["Left","Decrease"]}')
!^Right:: Komorebic('{"ResizeWindowEdge":["Right","Increase"]}')
!^Up::    Komorebic('{"ResizeWindowEdge":["Up","Decrease"]}')
!^Down::  Komorebic('{"ResizeWindowEdge":["Down","Increase"]}')

; -----------------------------------------------------------------------------
; Utility
; -----------------------------------------------------------------------------
; Retile (Alt + Shift + r)
!+r:: Komorebic('"Retile"')

; Close Window (Alt + Shift + q)
!+q:: Komorebic('"Close"')

; Promote Window (Alt + Shift + Enter)
!+Enter:: Komorebic('"Promote"')

; -----------------------------------------------------------------------------
; Komorebi Control
; -----------------------------------------------------------------------------
; =============================================================================
; Komorebi Control
; =============================================================================
; Toggle Pause (Alt + p)
!p:: Komorebic('"TogglePause"')

; Reload Configuration (Ctrl + Shift + R)
#^r:: Komorebic('"ReloadConfiguration"')

; Pause/Unpause Komorebi (Ctrl + Shift + P)
#^p:: Komorebic('"TogglePause"')

; Stop Komorebi (Ctrl + Shift + E)
#^e:: {
    ; To stop komorebi with cleanup, we need to use the old komorebic.exe
    ; since the service doesn't handle process management
    Run("komorebic.exe stop --ahk --bar", , "Hide")
}

; =============================================================================
; Notes:
; - All commands must be valid JSON matching the SocketMessage enum format
; - Unit variants (like "Stop") are JSON strings: "Stop"
; - Enum variants with data use object notation: {"Focus":"Left"}
; - For full SocketMessage schema, run: komorebic.exe socket-schema
; =============================================================================
