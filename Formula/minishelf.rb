class Minishelf < Formula
  desc "Rust TUI file explorer with git-aware coloring"
  homepage "https://github.com/rc-code-jp/minishelf"
  version "0.1.12"

  on_macos do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-aarch64.tar.gz"
    sha256 "a7635aca4274291d3646fd7659ca3b9d3b95437f7d17108fca10ede106c8e108"
  end

  on_linux do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-linux-x86_64.tar.gz"
    sha256 "1b7a26c64b9f1fd84736b54e1478ef80b1aefa217c44fe11319f1c645120de49"
  end

  def install
    if OS.mac? && Hardware::CPU.intel?
      odie "Intel macOS binary is not published yet. Please use Apple Silicon or build from source."
    end

    bin.install "minishelf"
  end

  test do
    system "#{bin}/minishelf", "--version"
  end
end
