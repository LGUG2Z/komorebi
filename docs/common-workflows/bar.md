# Komorebi Bar

`komorebi-bar` is a status bar for komorebi that renders on top of the tiling
window manager. It is configured through a `komorebi.bar.json` file, either
alongside your `komorebi.json` or at the path specified in the
`bar_configurations` array.

## Widgets

Widgets are placed in the `left_widgets`, `center_widgets`, or `right_widgets`
arrays. Each widget is an object with the widget type as key and its
configuration as value.

| Widget       | Description                                            |
|--------------|--------------------------------------------------------|
| `Komorebi`   | Workspaces, layout, focused window, and more           |
| `Battery`    | Battery level and charging status                      |
| `Date`       | Current date in configurable format                    |
| `DateLocalized` | Windows regional date using Windows date-picture format |
| `Time`       | Current time in configurable format                    |
| `Media`      | Currently playing media information                    |
| `Memory`     | System memory usage                                    |
| `Network`    | Network activity and connection status                 |
| `Storage`    | Disk usage information                                 |
| `Update`     | Komorebi update notification                           |
| `Systray`    | Windows system tray icons                              |

Widgets with dedicated documentation pages:

- [System Tray](bar-widgets/systray.md)

> Dedicated pages for the remaining widgets will be added in the future.

`Date` uses Chrono/strftime formats, such as `%Y-%m-%d`. `DateLocalized` is
Windows-only and uses Windows date-picture formats, such as `yyyy-MM-dd` or
`dddd d MMMM yyyy`. It formats the local Windows date and does not support an
IANA timezone option. Both widgets can be enabled at the same time.

## Schema

The full configuration schema is available at
[komorebi-bar.lgug2z.com/schema](https://komorebi-bar.lgug2z.com/schema).

For running a bar on each monitor, see
[Multiple Bar Instances](multiple-bar-instances.md) and
[Multi-Monitor Setup](multi-monitor-setup.md).
