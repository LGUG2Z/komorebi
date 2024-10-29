# Floating Windows

Sometimes you will want a specific application to be managed as a floating window.
You can add rules to enforce this behaviour in the `komorebi.json` configuration file.

```json
{
  "floating_applications": [
    {
      "kind": "Title",
      "id": "Media Player",
      "matching_strategy": "Equals"
    }
  ]
}
```
