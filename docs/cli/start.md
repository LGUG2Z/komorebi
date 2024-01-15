# start

```
Start komorebi.exe as a background process

Usage: komorebic.exe start [OPTIONS]

Options:
  -f, --ffm
          Allow the use of komorebi's custom focus-follows-mouse implementation

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

  -h, --help
          Print help

```
