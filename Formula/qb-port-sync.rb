class QbPortSync < Formula
  desc "Keeps qBittorrent's listening port synchronized with ProtonVPN forwarded ports"
  homepage "https://github.com/MFJG-dev/qb-port-sync"
  url "https://github.com/MFJG-dev/qb-port-sync/archive/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_ACTUAL_SHA256"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--path", ".", "--locked", "--bin", "qb-port-sync", "--root", prefix, "--all-features"
    
    # Install example config
    (etc/"qb-port-sync").install "config/config.example.toml"
    
    # Install launchd plist
    (prefix/"launchd").install "launchd/com.example.qb-port-sync.plist"
  end

  def caveats
    <<~EOS
      Example configuration installed to:
        #{etc}/qb-port-sync/config.example.toml

      Copy it to create your config:
        cp #{etc}/qb-port-sync/config.example.toml /Library/Application\\ Support/qb-port-sync/config.toml

      To run qb-port-sync as a service via launchd:
        sudo cp #{prefix}/launchd/com.example.qb-port-sync.plist /Library/LaunchDaemons/
        sudo launchctl load -w /Library/LaunchDaemons/com.example.qb-port-sync.plist
    EOS
  end

  test do
    assert_match "Synchronize qBittorrent listening port with ProtonVPN", shell_output("#{bin}/qb-port-sync --help")
  end
end
