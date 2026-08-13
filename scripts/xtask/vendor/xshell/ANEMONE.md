# xshell in Anemone

Anemone maintains this dependency in-tree so `xtask` does not need to fetch a
personal GitHub fork while building.

The source was imported from `https://github.com/doruche/xshell` at commit
`7de2558ebb22f2fdb21e47bc420bdfb6c32db5b2` (`feature/echo`). It contains
xshell `0.3.0-pre.2` plus the chainable `Cmd::echo()` extension used by
Anemone. Only the manifests, build sources, and license files required by the
dependency were imported; upstream CI, editor settings, examples, and tests
remain in the source repository.

Future changes to xshell or xshell-macros should be made directly in this
directory and reviewed as normal Anemone build-system changes.
