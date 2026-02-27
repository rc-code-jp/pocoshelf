class Minishelf < Formula
  desc "Rust TUI file explorer with git-aware coloring"
  homepage "https://github.com/rc-code-jp/minishelf"
  version "0.1.11"

  on_macos do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-aarch64.tar.gz"
    sha256 "1fd1a301db716b52e6b1df30436e4e6423ad8bdfcbac8cf28df2335fd62c8023"
  end

  on_linux do
    url "https://github.com/rc-code-jp/minishelf/releases/download/v#{version}/minishelf-#{version}-linux-x86_64.tar.gz"
    sha256 "85c3e4a65a83389316a5c6229d0da303604904b4d4f9187ba0d79c59a8a88783"
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
