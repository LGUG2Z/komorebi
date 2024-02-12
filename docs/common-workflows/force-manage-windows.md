# Force Manage Windows

❗️**NOTE**: A significant number of force-manage window rules for the most
common applications are [already generated for
you](https://github.com/LGUG2Z/komorebi/#generating-common-application-specific-configurations)

In some rare cases, a window may not automatically be registered to be managed
by `komorebi`. You can add rules to enforce this behaviour in the
`komorebi.json` configuration file.

```json
{
  "manage_rules": [
    {
      "kind": "Title",
      "id": "Media Player",
      "matching_strategy": "Equals"
    }
  ]
}
```
