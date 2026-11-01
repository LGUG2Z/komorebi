# Windows e2e for komorebi-bar

> **Status (wip-cand4):** current; the `bar` scenario exists and is the runtime gate for `komorebi-bar` and `komorebi-bar-core` on Windows.

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


## Renderers: glow first, wgpu when there is no OpenGL

The golden guest has no OpenGL 2.0 (the basic display adapter and QXL; Windows offers only the generic
OpenGL 1.1), and eframe's `egui_glow` refuses that: the first gate of the `bar` scenario saw
`Error: egui_glow: OpenGL: egui_glow requires opengl 2.0+` and the bar never opened a window.
Decision D-coord-14: the Windows bar tries glow as before and, only when glow cannot make its context
(`eframe::Error::OpenGL`, `Glutin` or `NoGlutinConfigs`), runs again with eframe's wgpu renderer, which
draws through Direct3D 12 and finds the software adapter WARP (Windows 11 ships it in the Basic Render
Driver) with no driver and no download (`run_with_fallback` in `komorebi-bar/src/windows/mod.rs`; the
`wgpu` feature of `eframe` is in the Windows-only table of `komorebi-bar/Cargo.toml`, the bar core and
the macOS bar do not have it). The bar logs `renderer: glow`, and on a fallback `renderer: glow failed
(...); trying wgpu`. With both features compiled eframe's default renderer is wgpu, so the first attempt
names `Renderer::Glow`: machines where glow works run exactly as before.

In the scenario:

- the bar's window, stays-alive, daemon-restart and notify checks are real when either renderer opens the
  window; the scenario prints which one drew it;
- only when both fail (the bar ends with `Error: glow: ...; wgpu: ...`) those checks are skipped, and the
  skip line names both errors (`skip ...: guest cannot draw: glow: ...; wgpu: ...`); the schema, the
  configuration read, `komorebic state` and `stop --bar` stay checks; any other early exit of the bar is a
  failure;
- the bar is started with `RUST_LOG=info,wgpu_hal=debug,wgpu_core=warn` so `bar.out` keeps what wgpu says
  about its adapters (read it first when the window does not appear).

Cost, measured: 622 -> 664 distinct crate versions in the Windows bar's dependency tree (+42), one new
lock entry (`pollster` 0.4.0; the rest of the wgpu stack was already locked as optional dependencies of
eframe). Not measured here (no nix build was run): compile time and the size of `komorebi-bar.exe`
(estimate: a few minutes cold, 8 to 12 MB).

Risks: WARP may not accept egui's Direct3D 12 device or the transparent surface the bar asks for
(`with_transparent(true)`; only the e2e gate can say); wgpu picks a hardware Vulkan/D3D12 adapter before
WARP when there is one, so on a machine with a GPU but broken OpenGL the bar now uses that adapter; a
second `run_native` on the same thread relies on eframe reusing its thread-local event loop (it does, by
design, for reopening windows).
