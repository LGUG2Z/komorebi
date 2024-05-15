# AutoHotKey

If you would like to use Autohotkey, please make sure you have AutoHotKey v2
installed.

Generally, users who opt for AHK will have specific needs that can only be
addressed by the advanced functionality of AHK, and so they are assumed to be
able to craft their own configuration files.

If you would like to try out AHK, here is a simple sample configuration which
largely matches the `whkdrc` sample configuration.

```
{% include "../komorebi.ahk" %}
```

By default, the `komorebi.ahk` file should be located in the `$Env:USERPROFILE`
directory, however, if `$Env:KOMOREBI_CONFIG_HOME` is set, it should be located
there.

Once the file is in place, you can stop komorebi and whkd by running `komorebic stop --whkd`,
and then start komorebi with Autohotkey by running `komorebic start --ahk`.