# System Tray

The System Tray widget brings native Windows system tray icons into
`komorebi-bar`. It intercepts tray icon data by creating a hidden window that
mimics the Windows taskbar, receiving the same broadcast messages that
applications send via `Shell_NotifyIcon`.

## Basic configuration

```json
{
  "right_widgets": [
    {
      "Systray": {
        "enable": true
      }
    }
  ]
}
```

## Hiding icons

The `hidden_icons` config field accepts a list of rules. Each rule can be either
a plain string or a structured object.

A **plain string** matches the exe name (case-insensitive). This is the original
format, so existing configs continue to work without changes:

```json
"hidden_icons": [
  "SecurityHealthSystray.exe",
  "PhoneExperienceHost.exe"
]
```

A **structured object** matches one or more icon properties. All specified fields
must match (AND logic). By default matching is exact and case-insensitive.

```json
"hidden_icons": [
  { "exe": "svchost.exe", "tooltip": "Some Specific App" },
  { "guid": "{7820AE73-23E3-4229-82C1-E41CB67D5B9C}" },
  { "tooltip": "App I want hidden" }
]
```

The two forms can be mixed freely:

```json
"hidden_icons": [
  "PhoneExperienceHost.exe",
  { "exe": "svchost.exe", "tooltip": "Specific Notification" },
  { "guid": "{7820AE73-23E3-4229-82C1-E41CB67D5B9C}" }
]
```

Available fields for structured rules:

| Field     | Description                                              |
|-----------|----------------------------------------------------------|
| `exe`     | Executable name (e.g. `"SecurityHealthSystray.exe"`)     |
| `tooltip` | Tooltip text shown on hover                              |
| `guid`    | Icon GUID — most stable identifier across app restarts   |

### Matching strategies

Each field can be a plain string (exact case-insensitive match) or an object
with `value` and `matching_strategy` for advanced matching. This uses the same
`MatchingStrategy` as komorebi's window rules.

```json
"hidden_icons": [
  {
    "exe": "explorer.exe",
    "tooltip": { "value": "Network", "matching_strategy": "StartsWith" }
  }
]
```

The above hides explorer.exe icons whose tooltip starts with "Network", while
leaving other explorer.exe icons visible.

Available strategies:

| Strategy            | Description                                       |
|---------------------|---------------------------------------------------|
| `Equals`            | Exact match (default when using a plain string)   |
| `StartsWith`        | Value starts with the given text                  |
| `EndsWith`          | Value ends with the given text                    |
| `Contains`          | Value contains the given text                     |
| `Regex`             | Value matches a regular expression                |
| `DoesNotEqual`      | Value does not exactly equal the given text        |
| `DoesNotStartWith`  | Value does not start with the given text           |
| `DoesNotEndWith`    | Value does not end with the given text             |
| `DoesNotContain`    | Value does not contain the given text              |

All strategies except `Regex` are case-insensitive. For case-insensitive regex,
include `(?i)` in the pattern.

Plain strings and strategy objects can be mixed across fields:

```json
{
  "exe": "explorer.exe",
  "tooltip": { "value": "notification", "matching_strategy": "Contains" }
}
```

Run komorebi-bar with `RUST_LOG=info` to see the exe, tooltip, and GUID of every
systray icon in the log output.

## Stale icon cleanup

Some applications (e.g. Docker Desktop) may exit without properly removing their
tray icon. The widget detects these stale icons by checking whether the owning
window still exists via the Win32 `IsWindow` API.

### Automatic cleanup

By default, the widget checks for stale icons every 60 seconds. The interval
can be configured with `stale_icons_check_interval` (in seconds). The value is
clamped between 30 and 600. Set to 0 to disable automatic cleanup.

```json
"stale_icons_check_interval": 120
```

### Refresh button

A manual refresh button can be shown by setting `refresh_button`. Clicking it
immediately removes any stale icons.

- `"Visible"` — shows the button in the main icon area
- `"Overflow"` — shows the button in the hidden/overflow section (appears when
  the overflow toggle is expanded)

```json
"refresh_button": "Overflow"
```

When set to `"Overflow"`, the overflow toggle arrow will appear even if there are
no hidden icons, so the refresh button remains accessible.

## Info button

An info button can be shown to open a floating panel that lists all systray icons
with their exe name, tooltip, GUID, and visibility status. This is useful for
identifying which icons to filter with `hidden_icons` rules.

- `"Visible"` — shows the button in the main icon area
- `"Overflow"` — shows the button in the hidden/overflow section

```json
"info_button": "Visible"
```

The info panel shows **all** icons, including those hidden by rules or the OS.
Each row shows the icon image, exe name, tooltip, GUID, and whether it is visible.
Copy buttons are provided on the exe, tooltip, and GUID cells for easy copying
(e.g. to paste a GUID into a filter rule).

Like the refresh button, setting `info_button` to `"Overflow"` will make the
overflow toggle arrow appear even if there are no hidden icons.

## Shortcuts button

A button that toggles komorebi-shortcuts. If the shortcuts process is running
it will be killed; otherwise it will be started.

- `"Visible"` — shows the button in the main icon area
- `"Overflow"` — shows the button in the hidden/overflow section

```json
"shortcuts_button": "Visible"
```

Like the other buttons, setting `shortcuts_button` to `"Overflow"` will make the
overflow toggle arrow appear even if there are no hidden icons.

## Mouse interactions

The widget supports left-click, right-click, middle-click, and double-click on
tray icons. Double-click sends the `LeftDoubleClick` action (via systray-util
0.2.0), which delivers `WM_LBUTTONDBLCLK` and `NIN_SELECT` messages to the icon.

## Click fallbacks

Some systray icons register a click callback but never actually respond to click
messages, effectively becoming "zombie" icons from an interaction standpoint. For
known problematic icons, the widget overrides the native click action with a
direct shell command. Fallback commands take priority — if a fallback is defined
for an icon, it always runs regardless of whether the icon reports itself as
clickable.

| Exe                            | Tooltip condition | Fallback command                |
|--------------------------------|-------------------|---------------------------------|
| `SecurityHealthSystray.exe`    | any               | `start windowsdefender://`      |
| `explorer.exe`                 | ends with `%`     | `start ms-settings:apps-volume` |
| `explorer.exe`                 | empty             | `start ms-settings:batterysaver`|

## Full example

```json
{
  "Systray": {
    "enable": true,
    "hidden_icons": [
      "SecurityHealthSystray.exe",
      { "exe": "explorer.exe", "tooltip": { "value": "Network", "matching_strategy": "StartsWith" } }
    ],
    "stale_icons_check_interval": 60,
    "refresh_button": "Overflow",
    "info_button": "Visible",
    "shortcuts_button": "Overflow"
  }
}
```
