# Dynamically Layout Switching

With `komorebi` it is possible to define rules to automatically change the
layout on a specified workspace when a threshold of window containers is met.

```json
{
  "monitors": [
    {
      "workspaces": [
        {
          "name": "personal",
          "layout_rules": {
            "1": "BSP"
          }
          "custom_layout_rules": {
            "5": "C:/Users/LGUG2Z/my-custom-layout.json"
          }
        },
      ]
    }
  ]
}
```

In this example, when there are one or more window containers visible on the
screen, the BSP layout is used, and when there are five or more window
containers visible, a custom layout is used.

However, if you add workspace layout rules, you will not be able to manually
change the layout of a workspace until all layout rules for that workspace have
been cleared.

```powershell
# for example, to clear rules from monitor 0, workspace 0
komorebic clear-workspace-layout-rules 0 0
```
