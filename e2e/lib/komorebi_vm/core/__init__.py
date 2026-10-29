"""The core the ports share: the vocabulary and the helpers a scenario is made of.

kfde (KDE) and kfw (Windows) run the same scenarios, so this package is the same in both
repositories (kfw vendors it with `scripts/sync-e2e-core`). Nothing in it may import from a port's
own code (`komorebi_vm.kde`, `komorebi_vm.platforms`, a runner) or name a compositor, a window
class or a tool of one port; what a port does differently is in its `Platform`, and what only a
port can check is in its extras.
"""
