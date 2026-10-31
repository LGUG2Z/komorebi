"""What only KDE can check, each file after the shared scenario of the same name.

An extra has the shape of a scenario, `run(platform: KdePlatform, checks, ...)`, and takes the
concrete platform so that it can use `platform.kde`: `kscreen-doctor`, `kdotool`, the daemon's log.
A suite runs it with `platform_only("kde", ...)`. It does not repeat a check the shared scenario
makes.
"""
