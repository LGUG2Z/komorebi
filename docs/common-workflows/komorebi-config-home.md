# `KOMOREBI_CONFIG_HOME`

If you do not want to keep _komorebi_-related files in your `$Env:USERPROFILE`
directory, you can specify a custom directory by setting the
`$Env:KOMOREBI_CONFIG_HOME` environment variable.

For example, to use the `~/.config/komorebi` directory:

```powershell
# Run this command to make sure that the directory has been created
mkdir -p ~/.config/komorebi

# Run this command to open up your PowerShell profile configuration in Notepad
notepad $PROFILE

# Add this line (with your login user!) to the bottom of your PowerShell profile configuration
$Env:KOMOREBI_CONFIG_HOME = 'C:\Users\LGUG2Z\.config\komorebi'

# Save the changes and then reload the PowerShell profile
. $PROFILE
```

If you already have configuration files that you wish to keep, move them to the
`~/.config/komorebi` directory.

The next time you run `komorebic start`, any files created by or loaded by
_komorebi_ will be placed or expected to exist in this folder.

[![Watch the tutorial
video](https://img.youtube.com/vi/C_KWUqQ6kko/hqdefault.jpg)](https://www.youtube.com/watch?v=C_KWUqQ6kko)
