# usage: powershell -File display.ps1 list
#        powershell -File display.ps1 set DEVICE WIDTH HEIGHT
# `list` prints each display device, its current mode and the modes it lists;
# `set` changes the mode of DEVICE (for example \\.\DISPLAY2) for this session.
param([string]$Command = "list", [string]$Device = "", [int]$Width = 0, [int]$Height = 0)

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class Display {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public short dmSpecVersion; public short dmDriverVersion;
    public short dmSize; public short dmDriverExtra;
    public int dmFields;
    public int dmPositionX; public int dmPositionY;
    public int dmDisplayOrientation; public int dmDisplayFixedOutput;
    public short dmColor; public short dmDuplex; public short dmYResolution;
    public short dmTTOption; public short dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public short dmLogPixels;
    public int dmBitsPerPel; public int dmPelsWidth; public int dmPelsHeight;
    public int dmDisplayFlags; public int dmDisplayFrequency;
    public int dmICMMethod; public int dmICMIntent; public int dmMediaType;
    public int dmDitherType; public int dmReserved1; public int dmReserved2;
    public int dmPanningWidth; public int dmPanningHeight;
  }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct DISPLAY_DEVICE {
    public int cb;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceString;
    public int StateFlags;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceID;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceKey;
  }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  static extern bool EnumDisplayDevices(string device, int n, ref DISPLAY_DEVICE dd, int flags);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  static extern bool EnumDisplaySettings(string device, int mode, ref DEVMODE dm);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  static extern int ChangeDisplaySettingsEx(string device, ref DEVMODE dm, IntPtr hwnd, int flags, IntPtr lparam);

  static DEVMODE NewMode() {
    var dm = new DEVMODE();
    dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
    return dm;
  }

  public static string List() {
    var sb = new StringBuilder();
    for (int n = 0; ; n++) {
      var dd = new DISPLAY_DEVICE();
      dd.cb = Marshal.SizeOf(typeof(DISPLAY_DEVICE));
      if (!EnumDisplayDevices(null, n, ref dd, 0)) break;
      sb.AppendLine("device " + dd.DeviceName + " [" + dd.DeviceString + "] flags 0x" + dd.StateFlags.ToString("x"));
      var cur = NewMode();
      bool ok = EnumDisplaySettings(dd.DeviceName, -1, ref cur);
      sb.AppendLine("  current ok=" + ok + " " + cur.dmPelsWidth + "x" + cur.dmPelsHeight + " at " + cur.dmPositionX + "," + cur.dmPositionY);
      var seen = new SortedSet<string>();
      for (int i = 0; ; i++) {
        var m = NewMode();
        if (!EnumDisplaySettings(dd.DeviceName, i, ref m)) break;
        seen.Add(m.dmPelsWidth + "x" + m.dmPelsHeight);
      }
      sb.AppendLine("  modes: " + string.Join(" ", seen));
    }
    return sb.ToString();
  }

  // 0 is DISP_CHANGE_SUCCESSFUL
  public static int Set(string device, int width, int height) {
    var dm = NewMode();
    if (!EnumDisplaySettings(device, -1, ref dm)) return -100;
    dm.dmPelsWidth = width;
    dm.dmPelsHeight = height;
    dm.dmFields = 0x80000 | 0x100000;
    return ChangeDisplaySettingsEx(device, ref dm, IntPtr.Zero, 0, IntPtr.Zero);
  }
}
"@

if ($Command -eq "list") {
  [Display]::List()
} elseif ($Command -eq "set") {
  "set $Device ${Width}x${Height}: " + [Display]::Set($Device, $Width, $Height)
}
