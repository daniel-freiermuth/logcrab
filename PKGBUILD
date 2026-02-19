# Maintainer: Daniel Freiermuth <daniel@freiermuth.dev>
pkgname=logcrab
pkgver=0.25.1
pkgrel=1
pkgdesc="A polyscopic anomaly explorer built with Rust and egui"
arch=('x86_64')
url="https://github.com/daniel-freiermuth/logcrab"
license=('GPL-3.0-or-later')
depends=('gcc-libs')
makedepends=('cargo' 'rust')
source=("$pkgname-$pkgver.tar.gz::https://github.com/daniel-freiermuth/logcrab/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')  # Replace with actual checksum after first build

prepare() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release --all-features
}

check() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    cargo test --frozen --all-features
}

package() {
    cd "$pkgname-$pkgver"
    
    # Install binary
    install -Dm755 "target/release/${pkgname}" "$pkgdir/usr/bin/${pkgname}"
    
    # Install desktop file
    install -Dm644 logcrab-system.desktop "$pkgdir/usr/share/applications/${pkgname}.desktop"
    
    # Install MIME type definition
    install -Dm644 logcrab-mime.xml "$pkgdir/usr/share/mime/packages/${pkgname}.xml"
    
    # Install documentation
    install -Dm644 README.md "$pkgdir/usr/share/doc/${pkgname}/README.md"
    
    # Install license
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/${pkgname}/LICENSE"
    
    install -Dm644 logo.png "$pkgdir/usr/share/icons/hicolor/256x256/apps/${pkgname}.png"
}
