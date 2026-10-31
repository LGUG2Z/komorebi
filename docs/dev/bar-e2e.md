# Windows e2e for komorebi-bar

The `bar` scenario (`e2e/platforms/qemu_windows/extras/bar.py`, registered as `EXTRAS["bar"]` in
`run.py`) is the runtime gate for changes to `komorebi-bar` and `komorebi-bar-core`: the unit tests
and the schema oracle cannot run the bar. Run it on the golden Windows guest:

```sh
E2E_GOLDEN=golden KOMOREBI_E2E_SCALE=1.5 nix run .#e2e-windows -- OUT bar
# another commit: build-ref.sh REF bins; KOMOREBI_E2E_BINS=bins ... (bins must have komorebi-bar.exe)
```

It checks: the `--schema` output equals `schema.bar.json`; the configuration (storage, time,
applications, komorebi workspaces and layout, Keyboard, Systray, a `mouse` block with a message and
a command) is accepted and the bar starts; its window appears; it keeps running 15 s; the daemon
keeps answering and does not tile the bar; the bar survives the daemon stopping and starting again
(it reconnects, it does not exit when the daemon stops); `komorebic stop --bar` ends both.

Artefacts in `OUT/bar/`: `bar.png`, `bar-after-restart.png` (screenshots, no pixel assertions),
`bar-schema.json`, `bar.out`, `bar.json`, `result.json`. Not covered: clicks, text of the widgets,
the PowerShell session of a `Command` mouse message (the bar starts it; nothing sends the click),
macOS (see `docs/dev/macos-handcheck.md`).

Not run when it was written (the guest was reserved): lint only (`py_compile`, `ruff`, the runner's
157 unit tests pass).
