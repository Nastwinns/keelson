# Homebrew formula for the haw binary (tap: keelson/tap).
# Release flow: `cargo xtask dist` on each platform, upload the archives to the
# GitHub release, then fill in VERSION and the per-platform SHA256 values.
class Keelson < Formula
  desc "Reproducible multi-repo stack composition + cross-repo MR orchestration"
  homepage "https://github.com/balin/keelson"
  version "0.1.0"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/balin/keelson/releases/download/v#{version}/haw-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256_MACOS_ARM"
    end
    on_intel do
      url "https://github.com/balin/keelson/releases/download/v#{version}/haw-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256_MACOS_X64"
    end
  end

  on_linux do
    url "https://github.com/balin/keelson/releases/download/v#{version}/haw-#{version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "REPLACE_WITH_SHA256_LINUX_X64"
  end

  def install
    bin.install "haw"
  end

  test do
    assert_match "haw", shell_output("#{bin}/haw --version")
  end
end
