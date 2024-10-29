# Ignore Windows

❗️**NOTE**: A significant number of ignored window rules for the most common
applications are [already generated for
you](https://github.com/LGUG2Z/komorebi-application-specific-configuration)

Sometimes you will want a specific application to never be tiled, and instead
be completely ignored. You can add rules to enforce this behaviour in the
`komorebi.json` configuration file.

```json
{
  "ignore_rules": [
    {
      "kind": "Title",
      "id": "Media Player",
      "matching_strategy": "Equals"
    }
  ]
}
```
