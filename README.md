# komorebi-recorder

Synchronized screen, log and trace capture for komorebi bug reproduction. `komorebi-recorder` records a bug while it
happens, on macOS and on Windows: the screen, komorebi's log, every event komorebi sends its subscribers and the
operating system's own trace, all on one clock, into a single _session directory_. It then answers, from that
directory alone, what komorebi and the OS were doing at any second of the video and where in the video any log line or
event is: for a person, with a window that plays the recording with everything that happened laid out on a timeline,
and for an agent, with commands that print JSON.

## Install

There are no releases yet: build it from source, below.

### From source

With a stable Rust toolchain, in a checkout:

```sh
cargo install --path komorebi-recorder --locked
```

## How it fits together

Two things vary independently: the _host_ the recorder runs on (macOS or Windows) and the _flavor_ of komorebi it
records (komorebi for Mac or komorebi for Windows). A session records both, so one recorded on Windows can be
investigated on a Mac. Every record in a session is stamped on one monotonic clock, and the files in it are never
rewritten after they are written.

## Crates

- [`komorebi-recorder`](komorebi-recorder): the library, the `komorebi-recorder` command and, with the same code, the
  `komorebi-recorder-gui` executable (no console window on Windows).

## Checks

`just check` runs formatting, clippy, the tests, rustdoc with its warnings as errors, and `cargo deny`.
CI runs the same checks, builds both platforms and, for Windows, the installer.

## Documentation

`cargo doc --workspace --no-deps --open` for the code. [CONTRIBUTING.md](CONTRIBUTING.md) says how to work on it.

## License

[Komorebi License 2.0.0](LICENSE.md).
