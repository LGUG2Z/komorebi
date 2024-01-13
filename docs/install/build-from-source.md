## Installing From Source

First, [install `rustup`](https://rustup.rs/) to get the `rust` compiler installed on your system, and make sure you have installed the [Visual Studio prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

Then clone the git repository.

```powershell
git clone https://github.com/LGUG2Z/komorebi.git
```

Once inside the repository, you will need to build and install three separate binaries.

```powershell
cargo +stable install --path komorebi --locked
cargo +stable install --path komorebic --locked
cargo +stable install --path komorebic-no-console --locked
```

If the binaries have been built and added to your `$PATH` correctly, you should see some output when running `komorebi --help` and `komorebic --help`
