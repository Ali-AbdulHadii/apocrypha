# AUR packaging

`apocrypha-bin` for Arch and derivatives, built from the published `.deb`
rather than from source. It exists because a large share of Linux gamers are on
Arch-family distributions, and `pacman -Syu` is a better update story than any
in-app updater — the app never has to replace its own files.

The package is not source-built on purpose. A source PKGBUILD would pull the
whole Rust and Node toolchain onto every user's machine to reproduce a binary
that is already built, signed off and published.

## First time: account and SSH key

The AUR authenticates by SSH key only. There is no password login for git, and
an unregistered key does not get a permission error — the server just closes
the connection:

```
Connection closed by 2604:cac0:a104:d::2 port 22
fatal: Could not read from remote repository.
```

That message means "I do not know this key", not "the repository is missing",
and it looks identical whether the package exists or not.

1. **Make a key for the AUR.** A dedicated one, not a deploy key borrowed from
   somewhere else: deploy keys are scoped to a single repository and sharing
   them across services spreads the blast radius of losing one.

   ```bash
   ssh-keygen -t ed25519 -f ~/.ssh/aur -C "aur@apocrypha"
   ```

2. **Point ssh at it**, so the key does not have to be named on every command.
   In `~/.ssh/config`:

   ```
   Host aur.archlinux.org
       HostName aur.archlinux.org
       User aur
       IdentityFile ~/.ssh/aur
       IdentitiesOnly yes
   ```

3. **Register at <https://aur.archlinux.org>** and paste the *public* half —
   `cat ~/.ssh/aur.pub` — into My Account → SSH Public Key.

   The AUR account is separate from an Arch or GitHub account, and registration
   needs an email it can confirm. The register page returns an intermittent
   `503 Service Temporarily Unavailable` under load; it is the AUR rather than
   anything local, and reloading a minute later works. There is no non-web way
   to register.

4. **Check it took.** This should print a help message rather than closing:

   ```bash
   ssh aur@aur.archlinux.org help
   ```

## Publishing the package the first time

Confirm the name is free first. The RPC answers without a login:

```bash
curl -s "https://aur.archlinux.org/rpc/v5/info?arg[]=apocrypha-bin"
```

A `resultcount` of `0` means nobody has taken it.

```bash
git clone ssh://aur@aur.archlinux.org/apocrypha-bin.git
```

**`warning: You appear to have cloned an empty repository` is expected** for a
package that does not exist yet — the AUR creates it on the first push.

```bash
cp packaging/aur/PKGBUILD packaging/aur/.SRCINFO apocrypha-bin/
cd apocrypha-bin
git add PKGBUILD .SRCINFO
git commit -m "Add apocrypha-bin 0.4.0"
git push origin master
```

Two things that catch people: the AUR's default branch is **`master`**, not
`main`; and the repository holds only `PKGBUILD` and `.SRCINFO` — never built
packages, `src/`, `pkg/`, or the downloaded `.deb`.

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
   curl -sL https://github.com/Apocrypha-Mods/apocrypha/releases/download/vX.Y.Z/Apocrypha_X.Y.Z_amd64.deb | sha256sum
   curl -sL https://raw.githubusercontent.com/Apocrypha-Mods/apocrypha/vX.Y.Z/LICENSE | sha256sum
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

## Pushing the update

Once the package exists, a release is three files copied and a push:

```bash
cd aur-apocrypha-bin          # the AUR clone, not this repository
cp ../apocrypha/packaging/aur/PKGBUILD ../apocrypha/packaging/aur/.SRCINFO .
git commit -am "Update to X.Y.Z"
git push origin master
```

Keep this directory and the AUR clone in step. This repository is where the
PKGBUILD is edited and reviewed; the AUR clone is only a publishing target, and
editing it directly is how the two drift apart.
