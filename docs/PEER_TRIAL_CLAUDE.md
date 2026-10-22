# Peer terminal yönergesi

Dağıtım paketinin ajan yönergesi [setup/peer/CLAUDE.md](setup/peer/CLAUDE.md) şablonudur; pakette kök CLAUDE.md olarak kullanılır ve repo checkout'u gerektirmez. Bu sayfa eski bağlantıları korur.

Ana Desktop akışı özel davet bağlantısı, otomatik cihaz isteği, açık **Approve** ve boş hedefe otomatik ilk sürümdür. CLI'nin bağlantı akışı yoktur; Advanced dosya davetiyle native `join`, güvenli `status`, `request-pairing`, `check`/`receive` kullanır. Davet payload'ı, anahtarlar ve proje dosyaları ajan bağlamına girmez.

- [Ortak sıra ve tek sorun giderme bölümü](setup/README.md)
- [Host / ilk gönderen](setup/host/README.md)
- [Peer / ilk alıcı](setup/peer/README.md)
- [Advanced CLI paket şablonu](setup/peer/CLAUDE.md)

Henüz güncel paket yoktur. Gerektiğinde incelenmiş master'dan yeni build, yönerge ve checksum hazırlanır; eski ZIP sessizce değiştirilmez. Gerçek doğrulama sonuçları [IMPLEMENTATION.md](IMPLEMENTATION.md) içinde tutulur.
