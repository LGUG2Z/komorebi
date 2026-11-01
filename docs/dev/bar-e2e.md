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

## The golden guest has no OpenGL 2.0

The Windows gate (candidate 5) found that the bar cannot open a window in the golden guest:
eframe's default renderer is `egui_glow`, the guest has the basic display adapter and QXL and no
OpenGL driver, so Windows offers only the generic software OpenGL 1.1 of `opengl32.dll`, and the bar
exits at start with `Error: egui_glow: OpenGL: egui_glow requires opengl 2.0+`
(`komorebi-bar/src/windows/mod.rs`, the `run_native` error). The scenario therefore reports the
window, stays-alive and daemon-restart checks as `skipped: guest has no OpenGL 2.0` when, and only
when, the bar's own output says that (`extras/bar.py`, `classify`); any other early exit is a
failure. The schema equality, the configuration read, `komorebic state` and `stop --bar` stay real
checks. A skip is not a pass: the bar's runtime behaviour is unverified until the guest can draw.

### Getting rendering into the guest (analysis, nothing added)

How the context is chosen: `komorebi-bar` uses `eframe` 0.33 with its default features, so only
the glow renderer is compiled (glutin + `egui_glow`); `NativeOptions` is left at its defaults on
Windows (the macOS bar sets its own), so the bar asks for a hardware OpenGL 2.0+ config through
glutin, falls back to the Windows generic GDI implementation (1.1) and `egui_glow` refuses it.

1. Mesa llvmpipe `opengl32.dll` next to `komorebi-bar.exe` (the classic recipe: Mesa's
   `opengl32.dll` takes over from the system one for that process). Not possible from this flake:
   nixpkgs marks `pkgsCross.mingwW64.mesa` as an unsupported platform (`nix build --dry-run` refuses
   it), and `nixpkgs` has no Windows `opengl32sw.dll` (that file ships with Qt) or ANGLE. Building
   Mesa for mingw means porting its nix expression (swrast + llvm cross for mingw, a cross LLVM
   build) and carrying a binary of unknown license status in the image: days of work, hours of
   build, and the result is a third-party DLL in the test that real users do not run.
2. A wgpu renderer with the Windows software adapter. Windows 11 ships WARP, the software
   Direct3D 12 rasteriser, in the Basic Render Driver, which the guest has; wgpu's DX12 backend
   finds it as an adapter, no download. In `eframe` this is the `wgpu` feature plus
   `NativeOptions { renderer: Renderer::Wgpu, .. }` (and `wgpu_options` with a fallback adapter if
   it does not pick WARP by itself). The sources of the wgpu stack (wgpu 27, naga, wgpu-hal,
   gpu-allocator, ash) are already in the local cargo registry because they are in `Cargo.lock`
   as optional dependencies of eframe, so it should build offline. Proposal, smallest first:
   in `komorebi-bar`'s `[target.'cfg(windows)'.dependencies]` ask for `eframe` with the `wgpu`
   feature, and at start: try glow as now; if `run_native` fails with the OpenGL message, run again
   with `Renderer::Wgpu` (the e2e then passes the window checks, and a user without OpenGL gets a
   bar instead of an error). Cost: about 60 more crates in `Cargo.lock` that are compiled for
   Windows only (cold build +3 to 5 minutes in CI, +8 to 12 MB in `komorebi-bar.exe`); a retry path
   that needs `NativeOptions` to be built twice; a new, untested code path for real users (low
   risk: it only runs where glow already failed). Without the fallback, a `KOMOREBI_BAR_RENDERER=wgpu`
   switch the scenario sets would keep users on glow and cost the same dependencies.
3. ANGLE (EGL on Direct3D 11, itself on WARP) through `glutin`'s EGL support: needs
   `libEGL.dll`/`libGLESv2.dll`, which are not in nix either. Rejected.

Recommendation: 2, with the glow-then-wgpu fallback, as its own revision with the e2e `bar`
scenario as its gate (the window checks stop being skipped). Whether the guest's WARP accepts
wgpu's DX12 device (feature level, no HLSL compiler problem for egui's shaders) can only be
learned by running it; the first run should keep `RUST_LOG=wgpu_hal=debug` output in `bar.out`.
