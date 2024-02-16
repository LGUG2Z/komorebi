# Tray and Multi-Window Applications

❗️**NOTE**: A significant number of tray and multi-window application rules for
the most common applications are [already generated for
you](https://github.com/LGUG2Z/komorebi/#generating-common-application-specific-configurations)

If you are experiencing behaviour where closing a window leaves a blank tile,
but minimizing the same window does not, you have probably enabled a
'close/minimize to tray' option for that application. You can tell `komorebi`
to handle this application appropriately by identifying it via the executable
name or the window class.

```json
{
  "tray_and_multi_window_applications": [
    {
      "kind": "Class",
      "id": "SDL_app",
      "matching_strategy": "Equals"
    }
  ]
}
```
