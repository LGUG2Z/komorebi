# Troubleshooting

## Phantom Tiles

Sometimes you may experience an application which leaves "ghost tiles" on a workspace, where there is space reserved for
a window but no window visible.

You can ignore these windows by following these steps:

* Run `komorebic visible-windows` to find details about the invisible window
* Using that information, [create a rule to ignore that window](common-workflows/ignore-windows.md)

## AutoHotKey executable not found

If you try to start komorebi with AHK using `komorebic start --ahk`, and you have
not installed AHK using `scoop`, you'll probably receive an error:

```text
Error: could not find autohotkey, please make sure it is installed before using the --ahk flag
```

Depending on how AHK is installed the executable on your system may have a
different name. In order to account for this, you may set the `KOMOREBI_AHK_EXE`
environment variable in your
[PowerShell profile](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_profiles?view=powershell-7.4)
to match the name of the executable as it is found on your system.

After setting `KOMOREBI_AHK_EXE` make sure to either reload your PowerShell
profile or open a new terminal tab.

## Komorebi is unresponsive when the display wakes from sleep

This can happen in rare cases when your monitor state is not preserved after it
wakes from sleep.

### Problem

Your hotkeys in _whkd_ work, but it feels as if _komorebi_ knows nothing about
the previous state (you can't control previous windows, although newly launched ones
can be manipulated as normal).

### Solution

Some monitors, such as the Samsung G8/G9 (LED, Neo, OLED) have an _adaptive
sync_ or _variable refresh rate_ setting within the actual monitor OSD that can
disrupt how the device is persisted in the _komorebi_ state following suspension.

To fix this, please try to disable _Adaptive Sync_ or any other _VRR_ branded
alias by referring to the manufacturer's documentation.

!!! warning

    Disabling VRR within Windows (e.g. _Nvidia Control Panel_) may work and can indeed
    change the configuration you see within your monitor's OSD, but some monitors
    will re-enable the setting regardless following suspension.

### Reproducing

Ensure _komorebi_ is in an operational state by executing `komorebic start` as
normal.

If _komorebi_ is already unresponsive, then please restart _komorebi_ first by
running `komorebic stop` and `komorebic start`.

1. **`komorebic state`**

   ```json
   {
     "monitors": {
       "elements": [
         {
           "id": 65537,
           "name": "DISPLAY1",
           "device": "SAM71AA",
           "device_id": "SAM71AA-5&a1a3e88&0&UID24834",
           "size": {
             "left": 0,
             "top": 0,
             "right": 5120,
             "bottom": 1440
           }
         }
       ]
     }
   }
   ```

   This appears to be fine -- _komorebi_ is aware of the device and associated
   window handles.

2. **Let your display go to sleep.**

   Simply turning the monitor off is not enough to reproduce the problem; you must
   let Windows turn off the display itself.

   To avoid waiting an eternity:

    - _Control Panel_ -> _Hardware and Sound_ -> _Power Options_ -> _Edit Plan
      Settings_

      _Turn off the display: 1 minute_

   Allow a minute for the display to reset, then once it actually shuts off
   allow for any additional time as prompted by your monitor for the cycle to
   complete.

3. **Wake your display again** by pressing any key.

   _komorebi_ should now be unresponsive.

4. **`komorebic state`**

   Don't stop _komorebi_ just yet.

   Since it's unresponsive, you can open another shell instead to execute the above command.

   ```json
   {
     "monitors": {
       "elements": [
         {
           "id": 65537,
           "name": "DISPLAY1",
           "device": null,
           "device_id": null
         }
       ]
     }
   }
   ```

   We can see the _komorebi_ state is no longer associated with the previous
   device: `null`, suggesting an issue when the display resumes from a suspended
   state.

## Komorebi Bar does not render transparency on Nvidia GPUs

Users with Nvidia GPUs may have issues with transparency on the Komorebi Bar.

To solve this the user can do the following:

- Open the Nvidia Control Panel
- On the left menu tree, under "3D Settings", select "Manage 3D Settings"
- Select the "Program Settings" tab
- Press the "Add" button and select "komorebi-bar"
- Under "3. Specify the settings for this program:", find the feature labelled, "OpenGL GDI compatibility"
- Change the setting to "Prefer compatibility"
- At the bottom of the window select "Apply"
- Restart the Komorebi Bar with "komorebic stop --bar; komorebic start --bar"

This should resolve the issue and your Komorebi Bar should render with the proper transparency.
