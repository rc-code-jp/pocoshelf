class Minishelf < Formula
  # This template is consumed by maintainers; Formula/minishelf.rb is updated
  # through a release automation PR and reviewed before merge.
  desc "Rust TUI file explorer with git-aware coloring"
  homepage "https://github.com/YOUR_GITHUB_USER/minishelf"
  version "__VERSION__"

  on_macos do
    url "https://github.com/YOUR_GITHUB_USER/minishelf/releases/download/v#{version}/minishelf-#{version}-macos-aarch64.tar.gz"
    sha256 "__SHA256_MACOS_ARM64__"
  end

  on_linux do
    url "https://github.com/YOUR_GITHUB_USER/minishelf/releases/download/v#{version}/minishelf-#{version}-linux-x86_64.tar.gz"
    sha256 "__SHA256_LINUX_X86_64__"
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
