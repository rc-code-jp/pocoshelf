class Minishelf < Formula
  desc "Rust TUI file explorer with git-aware coloring"
  homepage "https://github.com/rc-code-jp/minishelf"
  version "0.2.0"

  on_macos do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-aarch64.tar.gz"
    sha256 "582e288487acd992cf26f05659b19d183a9989159415167e3b0c220936c28371"
  end

  on_linux do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-linux-x86_64.tar.gz"
    sha256 "47e18299ac763195788d4086fbeb711a981a874ba0746cfcd208afe9b224c84f"
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
