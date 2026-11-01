# Window-rule matching: the three copies and the shared one

> **Paths (cutover stack):** this document was written before the directories were renamed and keeps the paths of its time. Now: `komorebi/` is `komorebi-windows/`, `komorebi-for-kde/komorebi/` is `komorebi-linux/`, `komorebi-for-mac/komorebi/` is `komorebi-macos/`, `komorebi-for-kde/komorebi-hotkeys/` is `komorebi-hotkeys/`, `komorebi-for-kde/nix/` and `komorebi-for-mac/nix/` are `nix/linux/` and `nix/macos/`, `komorebi-for-kde/docs/` and `komorebi-for-mac/docs/` are `docs/linux/` and `docs/macos/`, `schema*.json` are in `schemas/{windows,linux,macos}/`, and every package is at version 0.2.0 (the 0.1.x numbers below are history).

`should_act` (the last rule of a list that matches a window, a composite rule needing all its
conditions) and `should_act_individual` (one condition, by its matching strategy) existed three
times: `komorebi/src/window.rs` (Windows), `komorebi-for-kde/komorebi/src/window.rs` and
`komorebi-for-mac/komorebi/src/window.rs`. They are now `komorebi_config::rules`, one
implementation that all three daemons call; each keeps its own function signatures as a thin
wrapper so no call site changed.

## Classification (diff of the three copies, before the move)

| aspect | Windows | KDE | macOS |
|---|---|---|---|
| inputs | title, exe_name, `class: &str`, path | title, exe_name, `classes: &[&str]`, path | same as KDE |
| classes | exactly one | any of the slice (`for` loop, true if any element satisfies the test) | same as KDE |
| `.exe` in the rule id | kept: `exe_name` carries `.exe` on Windows, ids are compared as written | `identifier.id.replace(".exe", "")` before anything else (every occurrence, also in a regex id and so in the regex-cache key looked up) | same as KDE |
| strategies (Legacy/None, Equals, DoesNotEqual, StartsWith, DoesNotStartWith, EndsWith, DoesNotEndWith, Contains, DoesNotContain, Regex) | same set, same semantics per kind | same | same |
| Legacy/None | title and class: starts or ends with the id; exe and path: equal | same | same |
| Regex | `regex_identifiers.get(&id)` keyed by the id, no entry means no match; applied to title, class, exe, path | same, class: any of the classes | same |
| case folding | none (all comparisons are case sensitive) | none | none |
| path handling | compared as given, no normalisation | same | same |
| `should_act` | last matching rule wins, composite = all conditions | identical to Windows except the `classes` type | identical to KDE |
| wrappers | `should_act_window(_individual)` pass the one class | same, pass `&[class]` | trait methods in `workspace.rs` call `should_act` |
| regex cache | `REGEX_IDENTIFIERS` static in `komorebi/src/lib.rs`, filled by static config | same name, KDE lib.rs | same name, macOS lib.rs |

KDE and macOS are byte-identical in both functions. The Windows copy differs in exactly two
ways, and both are preserved by a flag or a one-element slice:

1. one class instead of a list: `[class]` is equivalent to the single compare for every strategy
   (checked arm by arm: any-of over one element is the one test; the negated strategies too);
2. no `.exe` stripping: `RuleQuirks::strip_exe_from_ids` (false on Windows, true on KDE and macOS).

No other difference exists, so no per-call-site flag is needed beyond the one platform constant.
The regex cache stays in each daemon (`REGEX_IDENTIFIERS`, lock order unchanged); the shared
function receives it as `&HashMap<String, Regex>` exactly as the copies did.
