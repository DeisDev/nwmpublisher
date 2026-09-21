# Based on the gmpublisher-bin PKGBUILD by Lythium <max@lythium.dev>

pkgname=nwmpublisher-bin
_realname=nwmpublisher
pkgver=3.1.0
pkgrel=1
pkgdesc="Workshop Publishing Utility for Garry's Mod, written in Rust & Svelte and powered by Tauri"
arch=('x86_64')
url="https://github.com/DeisDev/nwmpublisher"
license=('GPL-3.0')

depends=('webkit2gtk-4.1' 'gtk3' 'openssl' 'xdotool' 'hicolor-icon-theme')
provides=("${_realname}")
conflicts=("${_realname}")
source=("${url}/releases/download/${pkgver}/${_realname}_${pkgver}_amd64.deb")
sha256sums=('SKIP')

package() {
  # Reuse the native package's executable, private Steam library, icons and launcher.
  bsdtar -xf data.tar.gz -C "$pkgdir"
  install -Dm644 "$pkgdir/usr/share/doc/${_realname}/copyright" \
    "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
