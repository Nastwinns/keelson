# Installing hawser

`hawser` ships as a single binary named `haw`. This page is the full install
matrix: every channel, how to verify the signed release, the air-gap workflow, and
building from source. For the short version, see the
[README Install section](https://github.com/Nastwinns/hawser#install).

The current release is **v0.1.2**, published with signed, reproducible archives for
every supported platform.

## Channel matrix

| Channel | Platform | Command / source | Prerequisites |
|---------|----------|------------------|---------------|
| **crates.io** | any (Rust) | `cargo install hawser` | Rust 1.90+ toolchain |
| **Homebrew** | macOS + Linux | `brew install nastwinns/tap/hawser` | Homebrew |
| **Scoop** | Windows | `scoop bucket add nastwinns https://github.com/Nastwinns/scoop-bucket` then `scoop install hawser` | Scoop |
| **Static musl binary** | Linux x86_64 | download `haw-0.1.2-x86_64-unknown-linux-musl.tar.gz` (see below) | none (zero-dependency) |
| **Prebuilt archive** | Linux gnu (x86_64/aarch64), Linux musl (x86_64), macOS (x86_64/aarch64), Windows (x86_64) | [GitHub Release](https://github.com/Nastwinns/hawser/releases/latest) | none (optional: `cosign`, `sha256sum` to verify) |
| **Private registries** | any | Nexus / Artifactory / GitLab / Bitbucket mirror — see [DISTRIBUTION.md](DISTRIBUTION.md) | registry credentials |
| **Docker** | any (with Docker) | `docker build -t haw .` | Docker + the repo |
| **From source** | any (Rust) | `cargo install --git …` or `cargo build --release` | Rust 1.90+ toolchain |

All channels install the same `haw` binary. `cargo install hawser` is the canonical
Rust install.

## Package managers

### crates.io (Rust)

```bash
cargo install hawser
```

Builds from source against your local toolchain and drops `haw` into
`~/.cargo/bin`. Requires a Rust 1.90+ toolchain.

### Homebrew (macOS + Linux)

```bash
brew install nastwinns/tap/hawser
```

The tap lives at [`Nastwinns/homebrew-tap`](https://github.com/Nastwinns/homebrew-tap).
Homebrew pulls the prebuilt archive for your platform, so no compiler is needed.

### Scoop (Windows)

```powershell
scoop bucket add nastwinns https://github.com/Nastwinns/scoop-bucket
scoop install hawser
```

### AUR (Arch Linux)

A `hawser-bin` package (prebuilt from the GitHub Release) — see
[`packaging/aur/PKGBUILD`](../packaging/aur/PKGBUILD):

```bash
yay -S hawser-bin            # or: paru -S hawser-bin
```

### Nix (flake)

Run without installing, or add to a profile — the flake builds `haw` from source
and wraps it with `git`:

```bash
nix run github:Nastwinns/hawser              # run once
nix profile install github:Nastwinns/hawser  # install
```

### Debian / RPM

Each GitHub Release ships a `.deb` and `.rpm` for x86_64 Linux (gnu):

```bash
# Debian/Ubuntu
curl -sSLO https://github.com/Nastwinns/hawser/releases/latest/download/hawser_0.1.2-1_amd64.deb
sudo dpkg -i hawser_0.1.2-1_amd64.deb
# Fedora/RHEL
sudo rpm -i https://github.com/Nastwinns/hawser/releases/latest/download/hawser-0.1.2-1.x86_64.rpm
```

## Static musl binary (Linux, zero-dependency, air-gap friendly)

The recommended universal Linux install. The musl build is fully static — no glibc,
no runtime — so it runs identically on any Linux host, drops into minimal containers,
and installs cleanly on air-gapped machines as a single file.

```bash
curl -sSL https://github.com/Nastwinns/hawser/releases/download/v0.1.2/haw-0.1.2-x86_64-unknown-linux-musl.tar.gz \
  | tar xz && sudo install haw /usr/local/bin/
```

For air-gapped hosts, download the archive (plus its `.sha256`, `.sig`, and `.pem`)
on a connected machine, verify it (below), copy all four files across, then install.

## Prebuilt archives (signed)

Every platform ships an archive on the
[GitHub Release](https://github.com/Nastwinns/hawser/releases/latest):

- `haw-0.1.2-x86_64-unknown-linux-gnu.tar.gz`
- `haw-0.1.2-aarch64-unknown-linux-gnu.tar.gz`
- `haw-0.1.2-x86_64-unknown-linux-musl.tar.gz` (static)
- `haw-0.1.2-x86_64-apple-darwin.tar.gz`
- `haw-0.1.2-aarch64-apple-darwin.tar.gz`
- `haw-0.1.2-x86_64-pc-windows-msvc.zip`

Each archive is accompanied by:

- `<archive>.sha256` — a SHA-256 checksum
- `<archive>.sig` and `<archive>.pem` — a [cosign](https://github.com/sigstore/cosign)
  keyless signature and its certificate

The release is **reproducible and signed**. Verifying is optional but recommended,
and it is the whole point on locked-down or air-gapped hosts.

### Verify the checksum

```bash
sha256sum -c haw-0.1.2-x86_64-unknown-linux-musl.tar.gz.sha256
```

Expect `… OK`. (On macOS, `shasum -a 256 -c` is the equivalent.)

### Verify the cosign signature

Keyless verification checks the signature against the Sigstore transparency log. You
need [`cosign`](https://github.com/sigstore/cosign) installed:

```bash
cosign verify-blob \
  --certificate haw-0.1.2-x86_64-unknown-linux-musl.tar.gz.pem \
  --signature   haw-0.1.2-x86_64-unknown-linux-musl.tar.gz.sig \
  --certificate-identity-regexp 'https://github.com/Nastwinns/hawser' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  haw-0.1.2-x86_64-unknown-linux-musl.tar.gz
```

Expect `Verified OK`. Once verified, unpack and install:

```bash
tar xzf haw-0.1.2-x86_64-unknown-linux-musl.tar.gz
sudo install haw /usr/local/bin/
```

### Air-gap workflow

1. On a connected machine, download the archive and its `.sha256`, `.sig`, and
   `.pem` companions.
2. Verify the checksum and the cosign signature (above) — this establishes trust
   while you still have network access to the transparency log.
3. Copy all four files to the air-gapped host.
4. Verify the checksum again offline (`sha256sum -c …`), unpack, and install.

The static musl binary has no runtime dependencies, so nothing else needs to cross
the air gap.

## Private registries (Nexus / Artifactory / GitLab / Bitbucket)

Organizations that mirror releases to an internal registry can pull the exact same
signed archives (plus `.sha256`, `.sig`, `.pem`, and the `.deb`/`.rpm`) from Nexus,
Artifactory, GitLab, or Bitbucket. Each tagged release publishes the GitHub Release
first, then mirrors the artifacts to whichever of these registries are configured.

See [DISTRIBUTION.md](DISTRIBUTION.md) for the exact upload paths, the secret matrix to
enable each registry, and the per-registry download/install commands. Example (Nexus):

```bash
curl -u "$NEXUS_USER:$NEXUS_PASS" -O \
  "$NEXUS_URL/repository/raw-hosted/haw/0.1.2/haw-0.1.2-x86_64-unknown-linux-musl.tar.gz"
```

Verify the checksum and cosign signature exactly as for the GitHub Release (above).

## Docker

An image builds directly from the repository `Dockerfile`:

```bash
docker build -t haw .
docker run --rm haw --version
```

Requires Docker and a checkout of the repo.

## From source

Requires a Rust 1.90+ toolchain.

Install the latest `main` straight from Git:

```bash
cargo install --git https://github.com/Nastwinns/hawser hawser
```

Or clone and build a release binary:

```bash
git clone https://github.com/Nastwinns/hawser
cd hawser
cargo build --release
# binary at target/release/haw
```

## Verify the install

Whichever channel you used:

```bash
haw --version
```

---

Back to the [README](https://github.com/Nastwinns/hawser#readme).
