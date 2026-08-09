# AUR packaging

`apocrypha-bin` for Arch and derivatives, built from the published `.deb`
rather than from source. It exists because a large share of Linux gamers are on
Arch-family distributions, and `pacman -Syu` is a better update story than any
in-app updater — the app never has to replace its own files.

The package is not source-built on purpose. A source PKGBUILD would pull the
whole Rust and Node toolchain onto every user's machine to reproduce a binary
that is already built, signed off and published.

## Updating it for a release

1. Set `pkgver` to the new version and reset `pkgrel=1`.
2. Refresh both checksums against the published assets:

   ```bash
   cd packaging/aur
   updpkgsums
   ```

   Or by hand — the first is the release `.deb`, the second is `LICENSE` at
   that tag:

   ```bash
   curl -sL https://github.com/Ali-AbdulHadii/apocrypha/releases/download/vX.Y.Z/Apocrypha_X.Y.Z_amd64.deb | sha256sum
   curl -sL https://raw.githubusercontent.com/Ali-AbdulHadii/apocrypha/vX.Y.Z/LICENSE | sha256sum
   ```

3. Regenerate the metadata and build it once locally:

   ```bash
   makepkg --printsrcinfo > .SRCINFO
   makepkg -f
   ```

   `.SRCINFO` is what the AUR reads. A PKGBUILD change that is not reflected
   there is invisible to users.

4. Check the runtime dependencies still match reality rather than assuming:

   ```bash
   ldd target/release/apocrypha-desktop | grep -E 'webkit|gtk|appindicator'
   ```

   `depends` is currently `webkit2gtk-4.1` and `gtk3`. There is no tray icon,
   so `libayatana-appindicator` is deliberately absent.

## Publishing

This part needs an AUR account and an SSH key registered with it, so it cannot
be automated from here.

```bash
git clone ssh://aur@aur.archlinux.org/apocrypha-bin.git aur-apocrypha-bin
cp packaging/aur/{PKGBUILD,.SRCINFO} aur-apocrypha-bin/
cd aur-apocrypha-bin
git add PKGBUILD .SRCINFO
git commit -m "Update to X.Y.Z"
git push
```

The AUR repository holds only `PKGBUILD` and `.SRCINFO`. Do not commit built
packages, `src/`, `pkg/`, or the downloaded `.deb` to either repository.
