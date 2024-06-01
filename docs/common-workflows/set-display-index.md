# Setting a given display to a specific index

If you have issues with `komorebi` forgetting monitor index positions, you will
need to set the display_index_preferences entry in the json schema.

Every display ID can be found using `komorebic state | findstr -I -N device_id`

Then, in komorebi.json, you simply need to add:

```json
    "display_index_preferences": {
        "0": "<display_id>",
        "1": "<display_id>",
    }
```
