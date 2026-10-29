# usage: powershell -File wallpaper.ps1 get DISPLAY
#        powershell -File wallpaper.ps1 set DISPLAY PATH
# Prints or sets the wallpaper of the monitor DISPLAY (0 is the first), through
# the IDesktopWallpaper interface of the shell.
param([string]$Command = "get", [int]$Display = 0, [string]$Path = "")

Add-Type @"
using System;
using System.Runtime.InteropServices;

[ComImport, Guid("B92B56A9-8B55-4E14-9A89-0199BBB6F93B"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IDesktopWallpaper {
  void SetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitorID,
                    [MarshalAs(UnmanagedType.LPWStr)] string wallpaper);
  [return: MarshalAs(UnmanagedType.LPWStr)]
  string GetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitorID);
  [return: MarshalAs(UnmanagedType.LPWStr)]
  string GetMonitorDevicePathAt(uint monitorIndex);
  uint GetMonitorDevicePathCount();
}

[ComImport, Guid("C2CF3110-460E-4fc1-B9D0-8A1C0C9CC4BD")]
class DesktopWallpaperClass {}

public static class Wallpaper {
  static IDesktopWallpaper Open() {
    return (IDesktopWallpaper)new DesktopWallpaperClass();
  }
  public static string Get(int display) {
    var w = Open();
    return w.GetWallpaper(w.GetMonitorDevicePathAt((uint)display));
  }
  public static void Set(int display, string path) {
    var w = Open();
    w.SetWallpaper(w.GetMonitorDevicePathAt((uint)display), path);
  }
}
"@

if ($Command -eq "get") {
  [Wallpaper]::Get($Display)
} elseif ($Command -eq "set") {
  [Wallpaper]::Set($Display, $Path)
  "set"
}
