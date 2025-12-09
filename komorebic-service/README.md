# komorebic-service

A persistent IPC service that bridges AutoHotkey v2 to komorebi via Named Pipes.

## Overview

`komorebic-service` eliminates the process spawning overhead when sending commands from AutoHotkey to komorebi. Instead of launching `komorebic.exe` for every hotkey press, this service runs persistently and accepts commands via a Windows Named Pipe.

## Benefits

- **Ultra-low latency**: ~1-2ms command execution (vs 50-200ms+ with process spawning)
- **No anti-cheat interference**: Eliminates process spawn delays caused by kernel-level anti-cheat software and Windows Defender scanning
- **Simpler integration**: Direct pipe communication from AHK without subprocess management

## Architecture

```
AHK Hotkey
    ↓
Named Pipe: \\.\pipe\komorebi-command
    ↓
komorebic-service.exe
    ↓
Unix Domain Socket: %LOCALAPPDATA%\komorebi\komorebi.sock
    ↓
komorebi.exe
```

## Usage

### 1. Start the service

```bash
komorebic-service.exe
```

The service will:
- Listen on `\\.\pipe\komorebi-command`
- Forward all commands to komorebi's Unix domain socket
- Automatically exit when komorebi stops

### 2. Configure AutoHotkey

See `komorebi.example.ahk` for a complete example. Basic usage:

```autohotkey
#Requires AutoHotkey v2.0

Komorebic(command) {
    static PIPE_NAME := "\\.\pipe\komorebi-command"
    
    try {
        pipe := FileOpen(PIPE_NAME, "w", "UTF-8")
        if (pipe) {
            pipe.Write(command)
            pipe.Close()
        }
    } catch as e {
        OutputDebug("Komorebi pipe error: " . e.Message)
    }
}

; Example hotkeys
!h:: Komorebic('{"Focus":"Left"}')
!f:: Komorebic('"ToggleFloat"')
!1:: Komorebic('{"FocusWorkspace":0}')
```

## Command Format

Commands must be valid JSON matching the `SocketMessage` enum:

- **Unit variants** (no data): `"Stop"`, `"ToggleFloat"`, `"Retile"`
- **Enum variants** (with data): `{"Focus":"Left"}`, `{"FocusWorkspace":0}`

To see the full schema:
```bash
komorebic.exe socket-schema
```

## Lifecycle

The service automatically exits when:
- komorebi stops (detected via socket connection failure)
- The service receives a termination signal

## Troubleshooting

**Service won't start:**
- Ensure komorebi is running first
- Check that port isn't already in use: `Get-Process komorebic-service`

**Commands not working:**
- Verify service is running
- Check JSON syntax in your AHK script
- Enable debug output: set `RUST_LOG=debug`

**High latency:**
- Ensure you're not launching `komorebic.exe` alongside the service
- Each command should complete in ~1-2ms (check with timestamps in AHK)

## Building

```bash
cargo build --release -p komorebic-service
```

The binary will be in `target/release/komorebic-service.exe`.
