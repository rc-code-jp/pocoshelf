class Minishelf < Formula
  desc "Rust TUI file explorer with git-aware coloring"
  homepage "https://github.com/rc-code-jp/minishelf"
  version "0.1.13"

  on_macos do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-aarch64.tar.gz"
    sha256 "4d44130ab74b51efa620f74299c54c51ece8bdcbf2a164265931b3c1e5bf0106"
  end

  on_linux do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-linux-x86_64.tar.gz"
    sha256 "d2b7842003cb7e9f06c8dd048ce8f83f0199b9d4af4fee90593be8eade7d83e9"
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
