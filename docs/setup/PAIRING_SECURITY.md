# Otomatik eşleştirme: güvenlik sınırları

Kart dosyası geri gönderilmez; cihaz kimliği ve davet sahipliği kanıtı şifreli peer bağlantısından gelir. **Otomatik yanıt, otomatik yetkilendirme değildir:** gönderen doğru projede beklenen cihazı doğrulayıp onaylar. Yalnızca onay mevcut SQLite trust/project association kayıtlarını oluşturur.

Ana Desktop sırası Link folder → Invite collaborator → v2 bağlantıyı aç/yapıştır → otomatik imzalı istek → Approve → boş, daha önce sürüm almamış klasöre otomatik ilk sürümdür. Dosya daveti ve CLI Advanced yollar, manuel kart alışverişi legacy yoldur. Ayrıntılı adımlar [ortak rehberdedir](README.md).

## İşlem sırası

1. Gönderen Invite collaborator ile 15 dakikalık, tek okunabilen şifreli drop bağlantısı oluşturur. Yalnızca o proje için 15 dakikalık onay penceresi açılır. Aynı proje için pencereyi yeniden açmak süresini uzatır ve bekleyen istekleri korur; başka projeye pencere açmak önceki onaysız istekleri temizler. Pencere ayrıca elle açılabilir/kapatılabilir; restart veya süre sonu onaysız istekleri temizler.
2. Alıcı native çözme/import ile imzalı inviter kimliğini ve proje anahtarını yerelde öğrenir. TCP/Noise veya QUIC/TLS bağlantısı davetteki PeerId'ye yapılır. Çözülmüş payload ve anahtar React/agent/log'a girmez. Bağlantı yalnız kullanıcı eylemiyle native panoya kopyalanır; fragment çözme anahtarını taşır. History/cloud dışlama ve 60 saniyelik temizleme üçüncü taraf capture'ını engellemez.
3. `/trackmesh/pairing/1` kanalındaki Begin mesajı proje UUID'si ve mevcut imzalı DeviceCard'ı taşır. Core kart imzasını, public key'den türetilen PeerId'yi ve gerçek transport PeerId eşleşmesini doğrular.
4. Gönderen 24-byte OS-random challenge üretir. Challenge 60 saniye geçerlidir, tek peer'a ve o anki imzalı karta bağlıdır. Prove mesajı nonce ve empty-message XChaCha20-Poly1305 authentication tag taşır; **proje anahtarını taşımaz**. AAD domain, proje UUID'si, inviter/requester PeerId ve kartın mevcut signing bytes/imzasını bağlar. Crypto mevcut AEAD primitive'ini kullanır; yeni dependency yoktur.
5. Challenge doğrulamadan önce tüketilir; geçersiz veya tekrar kullanılan proof aynı nonce ile yeniden denenemez. Başarılı proof yalnızca bekleyen aday oluşturur, DB yetkisi vermez. İsim kimlik kanıtı değildir; public Device/Identity değerlerini beklediğiniz kişiyle karşılaştırın. Gönderen Approve veya Reject seçer.
6. Desktop alıcı Status mesajlarıyla onay durumunu sorgular ve bekleme/ret/erişilememe durumunu gösterir. CLI `request-pairing` onay için 60 saniyeye kadar bekler. Daha geç onayda **Ask for approval** / aynı CLI komutu sonucu alır. Pencere/bağlantı kapanırsa yeniden import yerine uygun koşullarda retry yapılır. Reddedilen PeerId aynı pencere içinde yeniden aday oluşturamaz; aynı proje penceresini uzatmak retleri temizlemez. Kapanmış veya süresi bitmiş pencereden sonra yeni pencere açmak yeni state oluşturur.

Supabase grant onaydan sonra otomatik kuyruğa alınır; yerel dosya yetkisinin yerine geçmez. Projeyi oluşturan cihazın Revoke'u üyelik revoke'unu da kuyruğa alır ve sunucu hedefin head kaydını siler. Yeni import'lar sahiplenme isteği göndermez. Revoke anahtarı döndürmez: eski dosyalar ve anahtar çıkarılan cihazda kalır. Eksik fonksiyon ve başarısız revoke tanısı Advanced altındadır.

## Ağ ve kaynak sınırları

- Tek mevcut swarm/actor kullanılır. Eşleştirme kapısı normalde kapalıdır; yalnızca aktif pencerede en fazla dört geçici inbound connection, PeerId başına bir connection kabul eder. Geçici bağlantılar yaklaşık 60 saniye sonra veya pencere kapanınca actor'ın 6 saniyelik bakım turunda kapatılır. Mevcut genel transport/connection limitleri korunur.
- Outbound keşif/dial hâlâ paired cihazlar ve pinned altyapı içindir. Eşleştirme için kabul edilen bilinmeyen kimlikler identify/discovery learned-address kayıtlarına eklenmez.
- Eşleştirme mesajı üst sınırı 32 KiB; kanal en fazla sekiz concurrent stream, 20 saniye request timeout kullanır. Actor en fazla sekiz outbound pairing RPC ve 64 peer için dakikada 16 mesajlık bounded rate kayıtları tutar. Mevcut incoming queue/responder worker sınırları korunur.
- Core tek proje penceresinde en fazla sekiz challenge, sekiz bekleyen aday ve sekiz ret kimliği tutar; ret kapasitesi aşılırsa pencere kapatılır. Adaylar sadece bellektedir; onaysız adaylar kalıcı DB/schema/state label eklemez.
- Ordinary transfer Head/Manifest/Chunk/Verified istekleri mevcut project authorization kontrolünden geçer. Eşleştirme bağlantısı/proof tek başına dosya erişimi sağlamaz. Pairing, `/trackmesh/transfer/1`, invitation/card signing serialization, mevcut SQLite schema değerleri ve recovery/journal sırasını değiştirmez.
- Pairing request/response Debug çıktısı alanları gizler; generic unavailable hataları anahtar, proof, challenge veya invitation payload loglamaz. Frontend yalnızca public aday kimliği, request UUID, pencere süresi ve güvenli sonuç mesajlarını görür.

## Sniffing ve kalan riskler

Eski manuel kart akışında transport yalnızca önceden tanınan cihazları/pinned altyapıyı kabul ediyordu. Yeni akış aktif pencerede sınırlı bilinmeyen inbound bağlantı kabul eder; saldırı yüzeyi ve kaynak tüketme/DoS riski artar. Süre/connection/frame/rate sınırları azaltıcı kontrollerdir; saldırı dayanıklılığı kanıtı değildir. Beklenen cihaz onaylandıktan sonra pencereyi kapatabilirsin; onaylı cihazın proje yetkisi sürer. Mevcut dosya yetkilendirmesi ve şifreleme korunur.

Panel yalnızca geçerli proof üreten adayların public kimliğini gösterir. Aday satırı bağlantı kesilse de pencere kapanana kadar kalabilir; canlı erişim veya audit kaydı sayılmaz. Onaylı cihazların seçili bağlantı durumu ayrı Collaborator alanındadır. Daveti açmayan kişiler ve bütün yetkisiz/başarısız network denemeleri için kalıcı güvenlik geçmişi bulunmaz.

Pasif ağ dinleme şifreli bağlantının uygulama içeriğini açık metin olarak vermez; IP/port, trafik boyutu ve zamanlama gizlenmez. [libp2p güvenli kanallar](https://libp2p.io/docs/secure-channels-overview/), [Noise](https://docs.libp2p.io/concepts/secure-comm/noise/) ve [QUIC/TLS RFC 9001](https://www.rfc-editor.org/info/rfc9001/) transport korumasını açıklar.

Çalınmış davet, proje anahtarına sahip olma kanıtını üretebilir; bu yüzden son onayda beklenen cihazı doğrulamak gerekir. 15 dakikalık pencere davet/proje anahtarının ömrünü kısaltmaz ve eski daveti iptal etmez. Zararlı uç cihaz, yanlış kişiyi onaylama, yanlış/yerine konmuş davet paylaşımı ve pairing penceresi açıkken kaynak tüketme/DoS ayrı risklerdir. Bu sınırlar bir güvenlik denetimi veya gerçek internet saldırı doğrulaması değildir. Gerçek gözlem ve kalan doğrulama işleri yalnızca [IMPLEMENTATION.md](../IMPLEMENTATION.md) içinde kaydedilir.

İki taraf aynı güncel build'i kullanmalıdır. Henüz güncel deneme paketi yoktur; gerektiğinde incelenmiş master'dan hazırlanır. Eski ZIP'ler yeni bağlantı akışını içermez. Eski invitation/card dosyaları uyumludur; manuel kart alışverişi **Legacy file pairing** altında kalır. Relay ve mailbox kodu üründe yoktur; `future/` altında post-1.0 için durur. [Ortak akış](README.md), [gönderen](host/README.md), [alıcı](peer/README.md).
