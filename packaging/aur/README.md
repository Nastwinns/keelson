# AUR — `hawser-bin`

`PKGBUILD` for the Arch User Repository. It installs the prebuilt `haw` binary
from the GitHub Release (x86_64 + aarch64), so there is no build step and no
sandbox — the right fit for a dev tool that shells out to `git`.

## Publishing (maintainer, needs an AUR account)

```bash
# one-time: clone the AUR package repo
git clone ssh://aur@aur.archlinux.org/hawser-bin.git
cd hawser-bin
cp /path/to/hawser/packaging/aur/PKGBUILD .

# regenerate .SRCINFO and push
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO
git commit -m "hawser-bin 0.1.1"
git push
```

## On a new release

Bump `pkgver`, refresh `sha256sums_x86_64` / `sha256sums_aarch64` from the
release's `.sha256` sidecars (or `updpkgsums`), regenerate `.SRCINFO`, push.
