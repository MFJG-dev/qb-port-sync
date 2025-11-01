# Maintainer: qb-port-sync maintainers
pkgname=qb-port-sync
pkgver=0.1.0
pkgrel=1
pkgdesc="Keeps qBittorrent's listening port synchronized with ProtonVPN forwarded ports"
arch=('x86_64' 'aarch64')
url="https://github.com/MFJG-dev/qb-port-sync"
license=('MIT')
depends=('gcc-libs')
makedepends=('rust' 'cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --release --locked --all-features
}

check() {
    cd "$pkgname-$pkgver"
    cargo test --release --locked --all-features
}

package() {
    cd "$pkgname-$pkgver"
    
    # Install binary
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    
    # Install example config
    install -Dm644 config/config.example.toml "$pkgdir/etc/$pkgname/config.example.toml"
    
    # Install systemd units
    install -Dm644 systemd/qb-port-sync.service "$pkgdir/usr/lib/systemd/system/qb-port-sync.service"
    install -Dm644 systemd/qb-port-sync.path "$pkgdir/usr/lib/systemd/user/qb-port-sync.path"
    install -Dm644 systemd/qb-port-sync-oneshot.service "$pkgdir/usr/lib/systemd/user/qb-port-sync-oneshot.service"
    
    # Install license
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
    
    # Install documentation
    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
