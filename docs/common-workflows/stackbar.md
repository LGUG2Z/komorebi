# Stackbar

If you would like to add a visual stackbar to show which windows are in a container
stack ensure the following options are defined in the `komorebi.json` configuration
file.

```json
{
  "stackbar": {
    "height": 40,
    "mode": "OnStack",
    "tabs": {
      "width": 300,
      "focused_text": "#00a542",
      "unfocused_text": "#b3b3b3",
      "background": "#141414"
    }
  }
}
```

This feature is not considered stable, and you may encounter visual artifacts
from time to time.