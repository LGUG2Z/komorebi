# Themes

A theme gives the borders their colors. Instead of choosing the color of each kind of window with
[`komorebic border-colour`](../cli/border-colour.md), a program tells the daemon to take a theme,
and the six kinds of border (`single`, `stack`, `monocle`, `floating`, `unfocused` and
`unfocused-locked`) take the colors the theme gives them: a Catppuccin theme, a Base16 theme, or a
Base16 palette of your own.

There is no `komorebic` command for it: the daemon takes it from the `theme` key of
[`komorebi.json`](configuration.md#borders), or as a message on its socket, the `Theme` message,
which a program sends with [`komorebi-client`](../../komorebi-client/src/lib.rs) or a script writes
as a line of JSON.

```rust
use komorebi_client::SocketMessage;
use komorebi_client::send_message;

let theme: SocketMessage = r#"{
    "type": "Theme",
    "content": { "palette": "Catppuccin", "name": "Mocha" }
}"#
.parse()?;
send_message(&theme)?;
```

The key has the same fields as the message, and wins over `border_colours` of the file, which has
no effect while a theme is set:

```json
{
  "theme": { "palette": "Catppuccin", "name": "Mocha" }
}
```

The file sets the theme when the daemon starts and when the file changes. A file without `theme`
leaves the one the daemon has.

## What a theme gives

`palette` is the kind of theme (`Catppuccin`, `Base16` or `Custom`), and `name` the theme: one of
Catppuccin's four, `Latte`, `Frappe`, `Macchiato` and `Mocha`, or one of the Base16 schemes
(`Custom` has a `colours` of the sixteen Base16 colors instead). A kind of border that the message
does not name takes the theme's default for it:

| Border | Catppuccin | Base16 |
|---|---|---|
| `single_border` | Blue | Base0D |
| `stack_border` | Green | Base0B |
| `monocle_border` | Pink | Base0F |
| `floating_border` | Yellow | Base09 |
| `unfocused_border` | Base | Base01 |
| `unfocused_locked_border` | Red | Base08 |

The message can name another color of the theme for any of them, as `"single_border": "Mauve"`
for Catppuccin or `"single_border": "Base0E"` for Base16.

A theme sets the colors that `border-colour` sets, and replaces them, and the colors set after it
replace the theme's. The theme is kept as the current one. The border has to be on and drawn by
the effect for any of it to show ([Borders](borders.md)).

## Palettes from a picture

The Base16 palette of a picture can be made from the picture itself: the same palette generator as
komorebi for Windows takes the picture's colors and gives a light or a dark variant. A palette
takes seconds to make for a large picture, so each one is kept in `$XDG_CACHE_HOME/komorebi`
(`~/.cache/komorebi`), in a file named for the picture, a hash of its path, size and time of last
change, and the variant. It is made again when the path, the size or the time of change differ. A
picture with fewer than eight colors has no palette.
