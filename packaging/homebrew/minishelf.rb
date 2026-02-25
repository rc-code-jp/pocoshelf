class Minishelf < Formula
  desc "Rust TUI file explorer with git-aware coloring"
  homepage "https://github.com/YOUR_GITHUB_USER/minishelf"
  version "__VERSION__"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/YOUR_GITHUB_USER/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-aarch64.tar.gz"
      sha256 "__SHA256_MACOS_ARM64__"
    else
      url "https://github.com/YOUR_GITHUB_USER/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-x86_64.tar.gz"
      sha256 "__SHA256_MACOS_X86_64__"
    end
  end

  on_linux do
    url "https://github.com/YOUR_GITHUB_USER/minishelf/releases/download/v#{version}/minishelf-#{version}-linux-x86_64.tar.gz"
    sha256 "__SHA256_LINUX_X86_64__"
  end

  def install
    bin.install "minishelf"
  end

  test do
    system "#{bin}/minishelf", "--version"
  end
end
