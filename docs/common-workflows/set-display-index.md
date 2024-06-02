# Setting a Given Display to a Specific Index

If you would like `komorebi` to remember monitor index positions, you will need to set the `display_index_preferences`
configuration option in the static configuration file.

Display IDs can be found using `komorebic monitor-information`.

Then, in `komorebi.json`, you simply need to specify the preferred index position for each display ID:

```json
{
  "display_index_preferences": {
    "0": "DEL4310-5&1a6c0954&0&UID209155",
    "1": "<another-display_id>"
  }
}
```
