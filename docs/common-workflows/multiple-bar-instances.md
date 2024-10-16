# Multiple Bar Instances

If you would like to run multiple instances of `komorebi-bar` to target different monitors, it is possible to do so
by maintaining multiple `komorebi.bar.json` configuration files and specifying their paths in the `bar_configurations`
array in your `komorebi.json` configuration file.

```json
{
  "bar_configurations": [
    "C:/Users/LGUG2Z/komorebi.bar.monitor1.json",
    "C:/Users/LGUG2Z/komorebi.bar.monitor2.json"
  ]
}
```

You may also use `$Env:USERPROFILE` or `$Env:KOMOREBI_CONFIG_HOME` when specifying the paths.

The main difference between different `komorebi.bar.json` files will be the value of `monitor.index` which is used to
target the monitor for each instance of `komorebi-bar`.