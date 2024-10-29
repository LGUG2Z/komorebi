# start

```
Start komorebi.exe as a background process

Usage: komorebic.exe start [OPTIONS]

Options:
  -c, --config <CONFIG>
          Path to a static configuration JSON file

  -a, --await-configuration
          Wait for 'komorebic complete-configuration' to be sent before processing events

  -t, --tcp-port <TCP_PORT>
          Start a TCP server on the given port to allow the direct sending of SocketMessages

      --whkd
          Start whkd in a background process

      --ahk
          Start autohotkey configuration file

      --bar
          Start komorebi-bar in a background process

  -h, --help
          Print help

```
