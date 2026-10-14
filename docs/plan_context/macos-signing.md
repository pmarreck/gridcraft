# Sealing the macOS bundle signature

- Add `buildPkgs.rcodesign` and run `rcodesign sign "$out/Applications/GridCraft.app"` at the end of postFixup, after stripping and the libiconv relink, as the WordCraft sibling project does.
- Accept only Apple's `/usr/bin/codesign --verify --deep --strict` on a Mac. rcodesign 0.29's own verify falsely rejects ad hoc signatures.
- The check needs a Mac reachable from the build host; keep its name out of tracked files.
- Pre-existing: `codesign --verify` on the bundle reports "code has no resources but signature indicates they must be present".
