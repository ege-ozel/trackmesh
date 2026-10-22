# Phase 1 M5 — İmzalı endpoint güncellemesi ve M6 davet tasarımı

**Durum (2026-10-12):** M5a protocol/crypto primitives, M5b additive storage schema v3, M5c direct endpoint RPC, M5d Supabase mailbox, and M6 one-read encrypted invite links are implemented locally. Migration/function deployment, cryptographic runtime interoperability, clipboard/URI OS behavior, and Windows two-peer validation remain unverified. See [IMPLEMENTATION.md](../IMPLEMENTATION.md) for checks actually run.

## 1. Özet karar önerisi

- Endpoint duyurusu `DeviceCard` ve mevcut mailbox `Envelope` tiplerinden ayrı, cihaz anahtarıyla imzalanmış yeni bir protokol kaydı olsun. İmza alan adı `trackmesh-endpoint-v1`; mevcut `DeviceCard`/`Envelope` imzalama baytları ve `Envelope::aad()` aynen kalsın.
- Güncel adresler önce halen açık, kimliği doğrulanmış mevcut doğrudan bağlantılardan taşınsın. Bağlantı yoksa yalnızca proje için yapılandırılmış mailbox'ın ayrı, proje anahtarıyla AEAD şifrelenmiş endpoint kanalı kullanılsın. Sunucu payload'ı açmasın. Yeni mailbox rotası/türü ve istemci API'si ayrıca onaylanmalı.
- Alıcı her duyuruda mevcut proje güvenini, kayıt içindeki `device_id` ve `peer_id` bağını, imzayı, monotonik sırayı, zaman aralığını ve adresleri doğrulasın. Adres öğrenmek hiçbir zaman `Store::trust`, pairing onayı veya proje yetkisi oluşturmasın; mevcut kart üzerine yazılmasın.
- Önerilen kayıt ömrü 10 dakika (üst sınır 15 dakika), en fazla 16 kanonik IP Multiaddr; süre ve adres filtreleri uygulama işinde ayrıca doğrulanmalı. Süresi geçmiş son kayıt, daha eski bir adres listesine geri dönüş için kullanılmasın.
- M6'nın tek kullanımlık şifreli davet bırakma seçeneği yalnız gelecekteki alternatif tasarımdır. Aşama 1'in cloud/public-sharing dışı sınırıyla çeliştiğinden, varsayılan öneri bunu bu MVP'ye almamaktır. Önce yeni ürün kapsamı ve hizmet sahibi açıkça onaylamalı.

## 2. Kod tabanında görülen sınırlar

- `DeviceCard` kendi `trackmesh-device-v1` imza alan adını ve sabit sıralı JSON tuple'ını kullanır (`shared/protocol/src/lib.rs:220-240`). Kart güncellemesi `Store::trust` içinde aynı cihazın kaydını günceller (`crates/storage/src/trust.rs:13-52`); adres güncellemesini bu yola vermek eski imzalı kartın adresleri geriletmesi riskini geri getirir.
- `network::validate_card` public key'den libp2p `PeerId` türetip karttaki değerle karşılaştırır (`crates/network/src/identity.rs:14-34`). `Store::trusted_author` ve `authorized` proje-cihaz bağını denetler (`crates/storage/src/trust.rs:54-72`). Endpoint alımı bu güven kontrollerini mevcut proje için çağırmalı; cihaz etiketi/adı kimlik kanıtı değildir.
- Mevcut `Envelope` AAD'si `trackmesh-mailbox-v1` domain'ine bağlıdır ve imza bu AAD, nonce ve ciphertext'i kapsar (`shared/protocol/src/lib.rs:268-299`; `crates/crypto/src/lib.rs:90-128`). Endpoint kaydını bu `Envelope` içine sıkıştırmak, snapshot ID anlamını değiştirmek veya mevcut AAD'yi `kind` alanıyla değiştirmek eski protokolü bozar; yapılmamalı.
- Mailbox istemcisi yalnızca mevcut announcement POST/page çağrılarını yapar (`crates/mailbox/src/http.rs:68-145`). Servis yalnız doğrulanmış `Envelope` alır; ACL imzalı DeviceCard'lardan kurulur ve proje başına üç cihazla sınırlıdır (`services/mailbox/src/api.rs:20-127`). Ayrı endpoint rotası/tipi gerekirse servis ACL'si aynı kalmalı; ACL provisioning otomatik varsayılmamalı.
- Yerel depolama schema v2'dir; daha yeni DB sürümü açılmaz ve eski sürümdeki migration transaction içinde tamamlanır (`crates/storage/src/lib.rs:27-40`). `trusted_peers`, mevcut `devices` ve transfer/snapshot tabloları korunmalıdır (`crates/storage/src/schema.sql:10-54`).
- Bu belge yazılırken yeni bir Windows/runtime davranışı doğrulanmadı. Aşağıdaki filtre, TTL, retention ve clipboard ayrıntıları tasarım önerisidir; uygulama öncesi platform ve router davranışı ayrıca doğrulanmalıdır.

## 3. M5 endpoint kaydı

### 3.1 Önerilen tip ve imza

Wire kaydı:

```text
EndpointRecord {
  device_id: Uuid,
  peer_id: String,
  addresses: [String; 0..=16],
  issued_at: UTC timestamp,
  expires_at: UTC timestamp,
  seq: u64,
  signature: hex Ed25519 signature
}
```

JSON decode sırasında bilinmeyen alanlar reddedilsin. `public_key` kayda kopyalanmasın: doğrulama anahtarı mevcut trusted device binding'den alınsın. Alanların imzalı baytları tek, açık sıralı serialization olsun; mevcut kod stilini izleyen somut öneri:

```text
serde_json::to_vec((
  "trackmesh-endpoint-v1",
  device_id,
  peer_id,
  canonical_addresses,
  issued_at,
  expires_at,
  seq
))
```

`signature` tuple'a dahil edilmez. M5a bu tuple sırasını ve JSON serialization'ı sabitledi; hex Ed25519 imza biçimi mevcut crypto helper'ını kullanır. M5a `EndpointRecord` tipi `shared/protocol/src/endpoint.rs` içindedir; `Identity::sign_endpoint` imzalar ve `crypto::verify_endpoint` doğrular. Golden byte fixture bu işte eklenmedi.

Sıra önerisi: tek cihaz kimliği başına global, kalıcı, monotonik `u64`. Alıcı `seq > highest_seq` şartını uygular. Gönderenin durable allocator'ı `max(last_seq + 1, current_unix_epoch_millis)` kullanır; saat geriye giderse `last_seq + 1` ilerlemeyi sürdürür, yedekten dönüş çoğu durumda güncel epoch millis'e sıçrayarak peer watermark kilitlenmesini önler. `last_seq + 1` taşarsa hata ile fail-closed olur. U64 değerleri signed SQLite INTEGER aralığına sığmayabileceğinden kanonik decimal TEXT saklanıp parse hatası fail-closed ele alınır. Kimlik anahtarının sıfırlanması farklı device_id üretir; monotoniklik her device_id kapsamındadır.

### 3.2 Alımda fail-closed doğrulama sırası

1. Kaydın toplam boyutu, `addresses.len() <= 16`, alan uzunlukları ve decode biçimi sınır içinde mi?
2. Gelen taşıma/kanalın proje kimliğinde author `device_id` için `trusted_author(project, device_id, known_public_key)` doğru mu? Mailbox ACL üyeliği tek başına yerel proje güveni sayılmaz.
3. İmzayı trusted device public key ile doğrula. Kaydın `device_id` değerinin beklenen cihaz kaydıyla ve `peer_id` değerinin aynı public key'den türetilen sabit libp2p PeerId ile eşleşmesini doğrula. Yeni `peer_id` için pairing/trust akışına dön; endpoint kaydı kimlik anahtarını değiştiremez.
4. `seq` parse edilebilir ve cihaz için durable `highest_seq` değerinden büyük mü? Eşit sıra duplicate, düşük sıra replay'dir; ikisi de adres tablosunu değiştirmez. Aynı imzalı kayıt tekrar gelirse harmless duplicate sonucu verilebilir.
5. Zamanı UTC olarak doğrula: `issued_at` gelecekteki küçük clock skew penceresinden (öneri 2 dakika) ileri olmasın; `expires_at > issued_at`, `now < expires_at` ve `expires_at - issued_at <= 15 dakika` olsun. Önerilen üretim TTL'i 10 dakika. Clock skew ve işletim sistemi saati güvenilirliği [V]; bilinmeyen/saat tutarsızlığında kaydı reddet ve yeni adresi kullanma.
6. Her adresi canonical Multiaddr olarak parse et; duplicate/port 0/loopback/unspecified/multicast/broadcast/link-local ve DNS isimlerini reddet. Yalnız desteklenen IP+TCP veya IP+UDP/QUIC v1 biçimleri kabul edilsin. M5a `validate_endpoint_address` bu biçimleri ve IANA'dan türetilen özel amaçlı aralık filtrelerini uygular; RFC1918 IPv4 ve IPv6 ULA geçerli yerel dial adayıdır. IPv6 scope ID kabul edilmez. Özel amaçlı aralıkların gelecekteki registry değişikliklerine göre güncellenmesi ve gerçek dial davranışı [V]. Adres doğrulaması dialing'i sınırlar ama taşıma `PeerId` doğrulamasının yerini tutmaz.
7. Tüm kontrol başarıyla biterse endpoint state ve `highest_seq` tek SQLite transaction'ında kaydedilsin; sonra adresler yalnız mevcut network actor API'sine `learned` kaynağı olarak verilsin. Doğrulama başarısızsa kısmi adres güncellemesi olmasın. Süre dolunca endpoint dial adaylığından çıkarılsın; son geçerli kayda sessizce dönülmesin.

Adresler project key ile şifreli mailbox kaydında cihazlar dışındaki gözlemcilerden gizlenebilir; doğrudan bağlantıdaki peer zaten bu adresleri görür. Alıcı/izleyici sayısını yalnız o cihazla önceden eşleşmiş proje peer'larıyla sınırla. Endpoint almak transfer yetkisi sağlamaz: her normal RPC `authorized` kontrolünü sürdürür.

## 4. Kanal ve servis tasarımı

### 4.1 Kanal sırası ve çalışma sınırları

1. **Mevcut doğrudan bağlantı:** Zaten açık ve transport `PeerId`'si doğrulanmış bağlantıda endpoint RPC/identify eklentisi ile imzalı kayıt gönder. Mesaj proje kapsamını bağlasın; Core alıcıda proje yetkisini tekrar denetlesin. Bu kanal yeni bağlantı kurmaz ve relay üzerinden dosya/proje verisi taşımaz.
2. **Yapılandırılmış proje mailbox'ı:** Yeni endpoint-specific record/route (`kind = endpoint`) ve ayrı pagination/cursor biçimi kullan. Payload proje anahtarıyla XChaCha20-Poly1305 AEAD altında şifrelensin. Yeni wrapper AAD'si yeni domain (`trackmesh-endpoint-mailbox-v1`) kullansın; iç `EndpointRecord` cihaz imzasını da korusun. `Envelope::aad()` ve mevcut `/announcements` şeması aynen kalsın. Mailbox kimlik doğrulaması, TLS, imzalı body ve mevcut ACL davranışı kullanılır; `MailboxAuth` için endpoint'e özgü ayrı scope (ör. `POST endpoint`) ve body hash tanımlansın. Sunucu ciphertext dışında endpoint adreslerini/anahtarını çözemez.
3. **İnsan-okur kod / çözüm sunucusu:** bu M5 tesliminde yok; M6'nın kısa kod seçeneği yalnız tasarım adayıdır.

M5c endpoint RPC yolunu uygular. Network actor gönderimde mevcut direct bağlantı şartını, alımda ise gelen bağlantı kimliğinin direct olmasını denetler. Core'daki `Engine::apply_endpoint_record` doğrudan ve sonraki mailbox kaynağı için ortak kabul fonksiyonudur. Runtime davranışı, eski ikili sürümlerle endpoint variant uyumu ve iki Windows peer ile yeniden bağlanma henüz elle doğrulanmadı.

Önerilen limit: plaintext endpoint kaydı < 16 KiB ve şifreli+JSON/base64 wrapper/HTTP body < 64 KiB. Mevcut mailbox envelope gövde sınırı 128 KiB'dir (`shared/protocol/src/mailbox.rs:8-9`); yeni endpoint türü kendi küçük sınırını tanımlamalı ve servis request-body sınırında da uygulamalı. Bu sayılar tasarım hedefidir, kodda doğrulanmış özellik değildir.

Endpoint mailbox route ayrı yetkilendirme/body validation yapmalı, yazarın `device_id`/public key'ini request auth ve mevcut ACL grant ile eşleştirmeli, yalnız ciphertext saklamalı ve küçük kayıtlar için retention/pruning belirlemeli. Yeni server DB tablosu mevcut snapshot announcement tablosuna karıştırılmamalı. Recipient uygulama tarafında imza, project trust, sıra ve expiry doğrulamasını tekrarlar. Ayrı tür için cursor/pagination ve retention etkileşimi API tasarım onayı gerektirir; mevcut announcement cursor'una gizlice endpoint satırı eklenmemeli. Mailbox yine de bağlantı IP'sini, request zamanını/boyutunu ve proje route bilgisini görebilir; AEAD bunları saklamaz.

### 4.2 Kanal yoksa davranış

- Mailbox URL yoksa doğrudan bağlantı halen açıksa M5 yine o peer'a yeni adresi iletir. Peer'lar bağlantıyı kaybetmişse adres güncellemesi için M5'in garantili fallback'i yoktur.
- Mailbox servisi kurulu ama erişilemiyorsa yayın yerelde bekletilebilir/sonraki session'da retry edilebilir; eski endpoint süresi dolmuşsa onu yeni adres gibi göstermemeli. İstek sonucu mevcut güvenli `UNKNOWN/OFFLINE` durumunda kalır; yeni endpoint varmış gibi iddia edilmez.
- İki cihaz aynı anda adres değiştirir, aralarında açık bağlantı yoktur ve erişilebilir mailbox/rendezvous yoktur: otomatik kurtarma kanalı kalmayabilir. Kullanıcı yeni daveti/elle bağlantı yolunu kullanır. Bu, M5'in açık ürün sınırıdır.
- Relay/rendezvous bağlantı taşımasına yardım edebilir ancak bu tasarım relay'e proje endpoint kaydı veya dosya akışı emanet etmez. Gerçek servis endpoint, DNS, HTTPS sertifika, pinned infrastructure PeerId, ACL provisioning ve reachable advertised address olmadan mailbox/rendezvous kurulmuş varsayılmaz. ACL grant'leri yerel pairing onayından otomatik türemez.

## 5. M5b schema v3 ve sürüm geçişi

**M5b uygulaması:** schema v3, mevcut DB v2 üstüne yalnız iki tablo ekler; `devices`, `trusted_peers`, `project_peers`, snapshots, recovery veya transfer satırlarına dokunmaz.

- `endpoint_local_sequence(device_id PRIMARY KEY, last_seq TEXT NOT NULL)` — cihazın sonraki global endpoint sıra numarasını durable ve atomic üretmek için.
- `peer_endpoints(device_id PRIMARY KEY REFERENCES devices(device_id), highest_seq TEXT NOT NULL, record_json TEXT NOT NULL, expires_at TEXT NOT NULL)` — son kabul edilen signed record ve replay watermark. Expired kayıt dial için kullanılamaz, fakat watermark daha eski bir imzayı engellemek için saklanır.
- Endpoints'i proje izinlerinden bağımsız erişilebilir kılma: yasak. Her yeni kayıt önce `project_peers`/`trusted_peers` bağlamında doğrulanır; endpoint state global device-address bilgisidir ve hiçbir Core/network authorization tablosunu değiştirmez.

Migration v0/v1/v2 sırasını korur; v2'den v3'e iki endpoint tablosu ve `user_version=3` tek transaction'da commit edilir; hata DDL ile version artışını birlikte geri alır. v3 DB eski v2 binary tarafından açılmaz. İlk yeni binary çalıştırılmadan önce uygulama kapatılarak `%LOCALAPPDATA%\TrackMesh` klasörünün yedeği alınmalı; rollback için uygulama kapalıyken yedek geri yüklenmelidir. M5b mevcut tablolardaki verileri güncellemez/yeniden yorumlamaz; v0/v1 için mevcut snapshot chunk backfill adımı aynen korunur.

## 6. M6 adayı — tek kullanımlık, şifreli davet bırakma (tasarım)

**Durum güncellemesi (2026-10-12):** Ürün sahibinin #30 onayıyla M6 yerel olarak uygulandı. `docs/plans/phase1-supabase-backend.md` §4 içindeki migration, signed create / unsigned one-read consume, global/ID limits, URL lookup and deployment details are authoritative. The original proposal below is retained as design history; its not-yet-approved statements are superseded for the M6 implementation. Live deployment and Windows two-PC validation remain `[V]`.

Bu bölüm M6'nın mevcut `ProjectInvite` JSON/kimlik doğrulamasını değiştirmez ve uygulanmış akış değildir. Şu anki davet dosyası native import ile açılır; ProjectInvite proje anahtarını taşır ve pairing transport sahiplik kanıtı + açık inviter onayı gerektirir (`docs/setup/PAIRING_SECURITY.md:1-18`). Davet payload'ı hiçbir zaman React/LLM/agent loglarına veya telemetry'ye gitmemeli.

1. Daveti veren Desktop, mevcut `ProjectInvite` JSON'unu yalnız native Rust tarafında serialize eder; kriptografik rastgele 256-bit anahtar ve AEAD nonce üretir, JSON'u mevcut XChaCha20-Poly1305 ile client-side şifreler. AAD yeni domain (`trackmesh-invite-drop-v1`) + random object ID + expiry'yi bağlar. Servis yalnız şifreli blob ve gerekli retention metadata'sını görür; project key/plaintext asla sunucuya gönderilmez.
2. Bu yalnız ileride kullanıcıca seçilen, kendi projesinin mailbox servisine HTTPS ile kısa ömürlü (<64 KiB, örneğin 15 dk) encrypted blob bırakma adayıdır. ID kriptografik rastgele locator; blob `GET` atomik tek-okumalı, expired/already-consumed kayda genel hata. Tek okumalı endpoint'in yarış/transaction, abuse/rate-limit ve retention temizliği service API/storage işi olur. Mevcut announcement route bu özellikleri sağlamaz.
3. Paylaşım biçimi `trackmesh://invite/<id>#<base64url-256-bit-key>`. URI fragment normal HTTP request path/query/header'a gönderilmez ([RFC 3986 §3.5](https://www.rfc-editor.org/rfc/rfc3986#section-3.5)); uygulama URI'yi doğrudan işler ve tam linki loglamaz. Blob ID tek başına ciphertext'in açılmasına yetmez fakat bir ID'yi ele geçiren kişi tek-okuma blob'unu tüketerek alıcıyı kilitleyebilir; random ID ve TLS bunu çözmez. Bu DoS riski, tek-okuma tasarımı için ayrıca değerlendirilmelidir. Blob servisi ayrıca çağıranın IP'sini, request zamanını/boyutunu ve yapılandırmaya göre proje route bilgisini görebilir.
4. İsteğe bağlı kısa PIN ancak server-side attempt limiting ve endpoint abuse controls ile retrieval rate-limit katmanı olabilir; ID + PIN denemeleri IP/device/object başına sınırlanıp generic hata verilmeli. Düşük entropili PIN'i blob key'i olarak kullanma: ciphertext kopyasını alan saldırgan PIN üzerinde offline brute-force yapabilir. 256-bit link key kriptografik anahtardır; PIN bunu güçlendiren eşdeğer ikinci faktör değildir.
5. Kullanıcıya yalnız “Copy invite link” düğmesiyle pano yazılsın. Windows'ta Win32 `RegisterClipboardFormat` ile `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` (serialized DWORD 0) ve `CanUploadToCloudClipboard` (serialized DWORD 0) biçimleri birlikte işaretlensin; Microsoft bunları history ve cloud sync dışlama işaretleri olarak belgeliyor. Kopyalama sonrası örneğin 60 sn'de yalnız pano hâlâ TrackMesh'in yazdığı içerikse temizle; pano içeriği değiştiyse kullanıcının yeni içeriğine dokunma. Bu işaretler üçüncü taraf clipboard manager, ekran görüntüsü, clipboard API'sine daha önce erişmiş uygulama veya kullanıcı paylaşımını engellemez; gizlilik garantisi değildir. M6 Windows native clipboard entegrasyonu ve OS sürüm davranışı uygulamada [V] kalır.
6. Alıcı `trackmesh://` linkini Desktop native handler içinde parse etsin; HTTP GET'ten gelen blob'u aynı 256-bit key ile native bellekte decrypt edip JSON'u mevcut import yoluna versin. Full link/fragment/payload UI debug, tracing, crash report veya LLM context'e taşınmasın. Decrypt başarısızlığı, expiry, ID tüketilmiş olması veya PIN sınırı genel mesajla gösterilsin. Sonrasında mevcut inviter identity comparison, challenge/proof ve explicit approval akışı aynen yürür; blob'u almak pairing authorization vermez.

### 6.1 Servis seçenekleri

**Karar güncellemesi (2026-10-12):** Önceki “Supabase, Vercel veya GitHub Pages'i varsayılan servis yapma” önerisi, ürün sahibinin M5d endpoint mailbox ve M6 şifreli davet bırakma için Supabase arka ucunu seçmesiyle değiştirilmiştir. Yukarıdaki risk ve gerekçeler silinmemiştir; Supabase'e özgü ciphertext-only tablolar, device-signed authorization, RLS deny-all, Edge Function sınırı, kota/7 günlük inactivity pause ve fallback tasarımı [Aşama 1 Supabase arka ucu tasarım ekinde](phase1-supabase-backend.md) ele alınır. Bu karar yalnız arka uç seçimini kaydeder; ek API/wire, DB migration, ACL provisioning, service deployment veya clipboard/URI entegrasyonunun uygulama onayı yerine geçmez.

| Seçenek | Tasarım sonucu | Neden varsayılan değil |
| --- | --- | --- |
| Projenin kendi mailbox servisi | Tercih edilecek tek aday; yeni küçük encrypted-invite route, atomik one-read, 15 dk retention ve proje ACL gerekir. Mevcut announcement API'de yoktur; ayrıca server API/deployment işi ve onayı gerekir. | Mevcut MVP metadata servisiyle sınırlı kalır, ancak davet-capability blob'u geçici de olsa server'a koymak için ayrıca kapsam/onay gerekir. |
| Supabase Storage/Edge Function | Private bucket/RLS veya Edge Function ile authorization ve rate limit tasarlanabilir. [Private bucket erişimi](https://supabase.com/docs/guides/storage/security/access-control), [signed URL davranışı](https://supabase.com/docs/guides/storage/serving/downloads), [Edge Function limitleri](https://supabase.com/docs/guides/functions/limits). | Desktop'a service-role secret gömülemez; public config tek başına authorization değildir. Signed URL sona erene kadar iptal edilemez bilgisi ve provider account/data processing yeni bağımlılık oluşturur. Sunucu one-read/TTL davranışı ayrıca yazılmalı. Supabase mevcut MVP'de entegre değil. |
| Vercel Blob/Function | Private Blob erişimi Function üzerinden token + uygulama authorization ile yapılabilir; public Blob URL ise linki olan herkese okunabilir. [Vercel private/public erişim](https://vercel.com/docs/vercel-blob), [security](https://vercel.com/docs/vercel-blob/security). | Private Function, secret yönetimi ve tek-okuma/expiry API'si gerekir; public URL modeli uygun değil. Yeni host/provider hesabı, deploy ve gizlilik sınırını büyütür. Bu servis mevcut değil. |
| GitHub Pages | Yalnız statik içerik host eder; içerik yayımlandığında public web'de erişilebilir. [GitHub Pages görünürlüğü](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-github-pages-site). | Tek-okuma, TTL, auth ve atomik tüketim sunmaz; id/key hariç hiçbir davet içeriği oraya konmamalı. Şifreli blob'un statik public kopyası bile public sharing modelidir ve Aşama 1 sınırına uymaz. |

**Önceki MVP kararı önerisi (karar güncellemesine kadar geçerliydi):** Supabase, Vercel veya GitHub Pages'i varsayılan servis yapma. M5 ve mevcut pairing/davet akışı bunlara ihtiyaç duymaz. Tek kullanımlık drop da Aşama 1'de uygulanmasın; bu tasarım cloud/public sharing ve capability-bearing invitation sınırını genişletir. Bu önerinin Supabase kısmı ürün sahibinin 2026-10-12 kararıyla supersede edilmiştir; Vercel/GitHub Pages değerlendirmesi değişmemiştir. Supabase'in server tarafında yalnız şifreli veri saklama, erişim kontrolü, 15 dk hard TTL, atomic one-read, abuse controls, servis loglarında key/payload olmaması ve provider/retention incelemesi şartları [tasarım ekine](phase1-supabase-backend.md) taşınmıştır. Bu koşullar tek başına sonraki API/wire/schema/deployment/UI uygulamalarına onay değildir.

## 7. Tehdit modeli ve doğrulanmamış konular

| Tehdit | Azaltım / kalan sınır |
| --- | --- |
| Eski imzalı adres replay | Durable highest `seq`, `expires_at`, identity-key binding; watermark kaybı DB restore/saat problemiyle eski state'e dönebilir. |
| Başka peer adına sahte adres/PeerId | Trusted public key signature + public-key-derived PeerId + mevcut authenticated transport. İmza doğrulaması proje yetkilendirmesinin yerini tutmaz. |
| Zararlı trusted peer veya özel IP | Yalnız trusted project author'dan gelen güncelleme; adres parser/range allow-list; `PeerId`-authenticated dial. Reddedilecek special-purpose aralık listesi IANA sürümüne göre kesinleşmeli. |
| Mailbox incelemesi/replay/censor | AEAD ciphertext; servis yazar/proje/seq zamanı/record boyutu/erişim zamanını görebilir ve kaydı silebilir/geciktirebilir. İmza, seq/expiry eski veriyi reddeder; availability garantisi vermez. |
| LAN adresi mahremiyeti | Address record yalnız trusted project peer'larına verilir; RFC1918/ULA değerleri kişisel ağ topolojisini açabilir. Public discovery yok. |
| Davet linki/pano sızıntısı | Key yalnız fragment'te, client-side encryption, native işlem, Win32 exclusion marks ve kullanıcı tıklaması. Screenshot, clipboard manager, malware, link paylaşımı ve tek-okuma DoS tamamen önlenemez. |
| Hizmet deploy/ACL hatası | Fail-closed ACL/TLS ve pinned identity, fakat DNS/HTTPS/ACL/retention gerçek deploy'da elle test edilmeden doğrulanmış sayılmaz. |

**[V] Uygulama başlamadan doğrulanacak:** Windows target'ta clipboard Win32 API bağlama/feature gereksinimi; M5 IP/Multiaddr sınıflandırması ve IPv6 scope-id; system clock skew politikasının kullanılabilirliği; mailbox için endpoint page/cursor/retention ve ACL operasyonu; migration backup/downgrade prosedürü; gerçek Windows direct reconnect ve router/NAT durumları. Bu belge bunları test etmedi.

## 8. M5/M6 kilometre taşları ve onay kapıları

1. **M5a — protokol/API onayı:** `EndpointRecord`, `trackmesh-endpoint-v1` signing bytes, 16 KiB/16-address bounds, 10-minute preferred and 15-minute maximum TTL, two-minute future skew, address validation, and duplicate/equivocation behavior are implemented. No dependency, DB schema, mailbox or direct RPC change. Static checks are recorded in `docs/IMPLEMENTATION.md`; Clippy passed in the follow-up worktree review recorded there.
2. **M5b — storage migration:** schema v3 additive tables, durable monotonic seq (`max(last_seq+1, Unix epoch ms)`), watermark retention after expiry and backup/rollback warning are implemented in storage; version bump and both new tables commit atomically. No existing application rows are modified by v2→v3. Static checks are recorded in `docs/IMPLEMENTATION.md`.
3. **M5c — mevcut doğrudan bağlantı:** authenticated peer event ile imzalı endpoint alışverişi, network actor'a mevcut API dışında swarm owner eklememe. Static checks ve iki Windows peer elle doğrulaması.
4. **M5d — mailbox ayrı türü:** yeni client/service route + paging, ciphertext-only persistence, retention ve ACL provisioning. **Kullanıcı onayı şart:** service API/deployment/storage değişikliği. Servis log/ciphertext bounds ve deletion elle doğrulanmalı.
5. **M5e — Core apply/expiry:** `trusted_author` → key/PeerId → signature → seq → time → address → atomic store → existing learned-address interface sırası. Elle stale/replay/wrong-author/expired/malformed ve address-change senaryoları.
6. **M6a — link taşıma kararı:** önce karar ver: yalnız yerel paste/copy ile mevcut invitation mı, yoksa ciphertext drop gerçekten MVP mi? İkinci seçenek için ayrı scope/privacy/service owner onayı gerekir.
7. **M6b — native import/copy UI:** yalnız M6a kabulünden sonra. **Kullanıcı onayı şart:** new URI protocol registration/OS handler, clipboard flags/Windows API bindings, mailbox invite endpoint, link format/expiry/one-read and any PIN/service controls.
8. Her aşama yalnız o aşamaya ait statik kontroller ve hedef makinelerde belirtilen manuel doğrulama ile tamamlanır. Hiçbir aşamada otomatik test/benchmark eklenmez veya çalıştırılmaz.

## 9. Kaynaklar

### Repository

- [Aşama 1 M5/M6 planı](phase1-direct-p2p.md)
- `shared/protocol/src/lib.rs:220-299` — DeviceCard ve Envelope signing/AAD biçimi
- `crates/crypto/src/lib.rs:62-178` — identity signing, verify ve XChaCha20-Poly1305 metadata kullanımı
- `crates/network/src/identity.rs:14-34` — public key / PeerId / DeviceCard binding
- `crates/storage/src/trust.rs:13-72` — trust overwrite, per-project authorized author sorguları
- `crates/storage/src/schema.sql:10-54`, `crates/storage/src/lib.rs:27-40` — mevcut tablolar ve schema v2 migration
- `crates/mailbox/src/http.rs:68-145`, `services/mailbox/src/api.rs:20-160`, `services/mailbox/src/store.rs:20-158` — announcement-only mailbox routes, ACL/body validation, storage
- [Pairing security](../setup/PAIRING_SECURITY.md) — current invitation ownership proof, explicit approval, secret/log boundaries
- [MVP boundary](../MVP.md) — no cloud project files/public sharing; current mailbox stores encrypted metadata

### Official external documentation

- Microsoft, [Clipboard formats — Cloud Clipboard/history controls](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats) — registered format flags and their documented effects. Does not prove third-party clipboard managers comply.
- Supabase, [Storage access control](https://supabase.com/docs/guides/storage/security/access-control), [signed downloads](https://supabase.com/docs/guides/storage/serving/downloads), [Edge Function limits](https://supabase.com/docs/guides/functions/limits).
- Vercel, [Blob private/public access](https://vercel.com/docs/vercel-blob), [Blob security](https://vercel.com/docs/vercel-blob/security).
- GitHub, [Creating a GitHub Pages site and visibility](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-github-pages-site).
- IANA, [IPv4 Special-Purpose Address Space](https://www.iana.org/assignments/iana-ipv4-special-registry) — address policy input; registry status can change.
