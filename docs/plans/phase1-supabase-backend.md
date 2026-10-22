# Aşama 1 — Supabase arka ucu tasarımı

**Durum (2026-10-13):** M5d sunucu/istemci, M6 davet drop'u, M7 head kayıtları ve #42 üyelik düzeltmeleri uygulandı. Depoda m5d, m6 ve m7 migration'ları ile dokuz Edge Function bulunur. [IMPLEMENTATION.md](../IMPLEMENTATION.md), canlı Supabase ve Windows Sandbox üzerinde davet tüketimi, açık onay ve host → alıcı aktarımını kaydeder. Ayrı fiziksel ağlarda aktarım, canlı revoke/üç cihaz sınırı ve güvenlik yarışları doğrulanmadı. m7 migration'ının Dashboard sonucu ve `head-publish`/`head-list` deployment'ı **unconfirmed**. Kontrol kanıtlarının tek kaynağı IMPLEMENTATION.md'dir.

Bu ek, Aşama 1 M0–M10 doğrudan-P2P planındaki Supabase arka ucunu açıklar. Kullanıcının kendi Supabase projesi yalnız küçük şifreli davet, endpoint ve head kayıtlarını taşır; proje dosyaları doğrudan cihazlar arasında aktarılır. Desktop'ın ana akışı v2 davet bağlantısıdır. Yerel davet dosyası Advanced yolu olarak korunur; kendi mailbox servisi `future/` altına taşındı. Relay sunucu kodu `future/relay-service` altında, derlenmeden durur; relay desteklenen kuruluma dâhil değildir ve post-1.0'a ertelenmiştir.

## 1. Veri modeli ve gizlilik sınırı

Supabase Postgres kaynaklarında endpoint, davet drop'u ve head kayıtları ile üyelik, nonce ve rate-limit tabloları uygulanmıştır. Kesin kolonlar ve kısıtlar `services/supabase/supabase/migrations/` altındaki migration'larda tanımlıdır.

### `endpoint_records`

Alanlar: `cursor_id`, `project_route` (proje UUID'si), `writer_device_id`, `seq`, `ciphertext`, `expires_at`, `created_at`. Benzersiz anahtar `(project_route, writer_device_id, seq)`; sayfa indeksi proje + `cursor_id`, temizleme indeksi expiry üzerindedir. `ciphertext` yalnız proje anahtarıyla XChaCha20-Poly1305 kullanılarak şifrelenmiş wrapper'dır. Wrapper AAD'si `trackmesh-endpoint-mailbox-v1` olur. İçinde M5 `EndpointRecord` ve imzası bulunur. `project_route`, yazar kimliği, sıra ve zaman sunucu/DB tarafından erişilebilir metadata'dır; yalnız adresler ve imzalı iç kayıt şifreli kalır.

Sunucuda plaintext adres, proje anahtarı veya imza doğrulamasında kullanılan gizli materyal bulunmaz. Edge Function cihaz imzalı isteği ve proje ACL'sini doğrular; alıcı `Engine::apply_endpoint_record` ile M5c'deki aynı güven zinciri, sıra, zaman, adres doğrulama ve atomik yerel kabul sırasını tekrar çalıştırır. Supabase ACL'si yerel `trusted_author`/proje yetkisinin yerine geçmez ve endpoint kaydı transfer yetkisi vermez.

### `invite_drops`

Alanlar: kriptografik rastgele `id` (locator), `ciphertext`, `expires_at`, `created_at`; davet tablosunda proje route veya davet içeriği tutulmaz. Tüketim satırı siler; `consumed` alanı yoktur. HTTP hata yanıtları ID varlığı, expiry ve önceki tüketimi ayırt etmez. Davet blob'u boyutu 64 KiB'den küçük tutulur. Ciphertext, istemcide XChaCha20-Poly1305 ile üretilir; `trackmesh-invite-drop-v1` AAD'si ID ve expiry'yi de bağlar. 256-bit rastgele anahtar yalnız paylaşım URI fragment'inde taşınır; sunucuya gönderilmez. Tek kullanımlık tüketim aşağıdaki §4'teki tek Postgres transaction'ı ile yapılır.

### `head_records`

m7 migration'ı proje/cihaz başına tek şifreli head hint'i ve kalıcı `last_head_seq` watermark'ını ekler. Ciphertext üst sınırı 4 KiB'dir. `head-publish`/`head-list` aynı aktif üyelik ve imzalı HTTP zarfını kullanır. Başarılı revoke hedefin head satırını siler ve üç aktif cihaz sınırında yer açar. Hint dosya içermez; offline bir cihazdan indirilebileceğini veya iki cihazın çevrimiçi eşit olduğunu kanıtlamaz.

## 2. İstek doğrulama, tablo erişimi ve ACL

İstemciye yalnız Supabase Project URL ve yeni biçimli publishable key (`sb_publishable_...`) dağıtılabilir. Bunlar gizli kabul edilmez; API anahtarı kimlik veya proje üyeliği kanıtı değildir. TrackMesh tabloları için doğrudan `anon` ve `authenticated` erişim grant'leri kaldırılır, RLS açılır ve hiçbir istemci politikası tanımlanmaz (deny-all). Edge Function dışında tabloya doğrudan erişim kapalıdır. Function'ın kullandığı secret key (`sb_secret_...`) RLS'yi atlar; yalnız `SUPABASE_SECRET_KEYS` JSON ortam değişkeninin `default` girdisinde tutulur, hiçbir TrackMesh binary'sine, depoya, dağıtım paketine, loga veya istemci yanıtına girmez. SQL erişimi kullanılıyorsa yalnız dar yetkili, kontrollü DB işlemleri tercih edilir; secret kullanımı yalnız Function içindeki doğrulanmış işlemlerden sonra yapılır.

`project-claim`, `member-grant`, `member-revoke`, `endpoint-publish` ve `endpoint-list` için cihaz Ed25519 imzalı istek zarfı kullanılır. Kesin wire/signing biçimi §2.1'de tanımlıdır. Project URL + yeni `sb_publishable_...` key yalnız API projesini seçer; üyelik/kimlik kanıtı değildir. Edge Function JWT doğrulaması kapalıdır çünkü kullanıcı JWT akışı yoktur; imzalı işlevler Ed25519, ilgili ACL ve replay kontrollerini kendileri yapar. İmzasız invite-consume ise drop locator, TTL, tek okuma ve rate-limit kontrolünü uygular. HTTPS zorunludur.

### 2.1 İmzalı istek wire biçimi

İstemci istekleri yalnız `POST /functions/v1/<operation>` kullanır. M5d imzalı işlem adları `project-claim`, `member-grant`, `member-revoke`, `endpoint-publish`, `endpoint-list`; sonraki imzalı işlemler `head-publish`, `head-list` ve `invite-create` olur; URL'deki function adı auth `scope` alanıyla birebir eşleşir. İstek gövdesi UTF-8 JSON'dur ve `x-trackmesh-auth` header'ı mevcut `MailboxAuth` gibi standart padded Base64 ile kodlanmış JSON zarfıdır:

```json
{
  "version": 1,
  "project_route": "lowercase-canonical-uuid",
  "device_id": "lowercase-canonical-uuid",
  "public_key": "64 lowercase hex chars",
  "issued_at": "2026-10-12T12:34:56.789Z",
  "nonce": "lowercase UUIDv4",
  "scope": "endpoint-publish",
  "body_hash": "64 lowercase hex chars",
  "signature": "128 lowercase hex chars"
}
```

`body_hash`, HTTP gövdesinin boşluklar dâhil **ham byte'ları** üzerinde SHA-256'nın küçük harfli hex kodlamasıdır. Mevcut `protocol::hash` BLAKE3 kullandığından bu yeni backend scope'unda BLAKE3 tekrar kullanılmaz. İmza girdisi, UTF-8 ve compact JSON olarak aşağıdaki sıralı tuple'dır; sıra, tip ve tarih gösterimi protokolün parçasıdır:

```text
serde_json::to_vec((
  "trackmesh-supabase-request-v1",
  1_u32,
  project_route,      // UUID, lowercase canonical string
  device_id,          // UUID, lowercase canonical string
  public_key,         // lowercase hex string
  issued_at,          // fixed UTC RFC3339 milliseconds as a string, ending in Z
  nonce,              // lowercase canonical UUIDv4 string
  scope,              // exact function/operation name
  body_hash           // SHA-256 lowercase hex string
))
```

Bu, mevcut `MailboxAuth::signing_bytes`'ın ordered `serde_json` tuple modelini korur; Supabase için yeni domain kullanır. `scope` operation adıdır; operation adı tek bir sabit HTTP POST function path'ine karşılık gelir. UUID'ler lowercase canonical, key/hash/signature hex değerleri lowercase; `nonce` küçük harfli, canonical UUID v4 olmalıdır. `issued_at` tam üç fractional digit ile `YYYY-MM-DDTHH:mm:ss.SSSZ` biçiminde olmalıdır; Rust istemci bu iki biçimi birebir üretmelidir. Timestamp yerel saatle ±120 saniye içinde değilse reddedilir. Body gövdesi 64 KiB üstünde olamaz; auth header 4 KiB üstünde olamaz. Zarf bilinmeyen/eksik alan kabul etmez.

`request_nonces` benzersiz anahtarı `(project_route, device_id, nonce)` olur. Başarılı transaction'da nonce tüketimi ACL kontrolü ve iş değişikliğiyle aynı DB çağrısındadır; en az 10 dakika tutulur (±2 dakika timestamp skew için güvenli replay retention). Aynı nonce farklı operation için de tekrar kullanılamaz. Endpoint payload body alanları: publish `{ "seq": "decimal-u64", "expires_at": "UTC timestamp", "ciphertext": "base64url-no-pad" }`; list `{ "after_cursor": null | "decimal-bigint" }`. Cursor DB'nin monotonik `cursor_id` değeridir; sayfa başına 8 satır. ACL body'leri §2.2'de belirtilir.

İşlem gövdeleri: claim `{ "actor_seq": "1" }`; grant `{ "actor_seq": "decimal-u64", "target_device_id": "uuid", "target_public_key": "64 lowercase hex chars" }`; revoke `{ "actor_seq": "decimal-u64", "target_device_id": "uuid" }`. Grant/revoke sequence'i auth imzasının body hash'iyle imzalanır ve `project_members.last_action_seq` ile artan sırada karşılaştırılır. `project_members.last_endpoint_seq` endpoint satırları TTL nedeniyle silindikten sonra da her üyenin en yüksek kabul edilmiş endpoint sıra numarasını saklar; replay watermark retention'dan bağımsızdır. Başarılı ya da iş kuralı nedeniyle reddedilen imzalı istekte nonce/rate state transaction içinde tutulur; auth, replay, ACL ve yazım ayrıdır/parsiyel değildir.

Başlangıç fixed-window limitleri hem cihaz hem proje bazında uygulanır; cihaz anahtarı imzalı `device_id`, proje anahtarı `project_route`'tur:

| İşlem | Cihaz | Proje |
| --- | ---: | ---: |
| `project-claim` | 3/saat | 10/saat |
| `member-grant`, `member-revoke` | 10/saat | 30/saat |
| `endpoint-publish` | 30/dakika | 120/dakika |
| `endpoint-list` | 60/dakika | 180/dakika |

Ek olarak `project-claim` için Supabase projesinin tüm route'ları boyunca tek bir **global 60/saat** limiti vardır. Supabase resmi belgeleri PostgREST `request.headers` içinden `X-Forwarded-For` okunabildiğini ve Edge loglarında `x-real-ip`/`cf-connecting-ip` header'larının kaydedildiğini gösteriyor; ancak Edge Function handler'ına gelen bu header'ın gateway tarafından client-controlled değerden arındırıldığına dair güvenilirlik garantisi bulamadım. Bu nedenle M5d-server IP başına limit veya IP depolaması yapmaz. Doğrulanmış Edge header kaynağı için `[V]` açık kalır; global claim limiti rastgele anahtar/route ile per-device ve per-project claim limitlerini aşmayı sınırlar.

Global sayaç tüm kullanıcılarca paylaşılır; bir saldırgan saatteki 60 claim hakkını tüketerek meşru claim'leri pencere sıfırlanana kadar geciktirebilir. Beklenen küçük kullanıcı tabanı için bu DoS etkisi kabul edilmiştir. Güvenilir IP kaynağı doğrulanana kadar IP başına limit `[V]` kalır.

Bu uygulama limitleri ilk abuse-control değerleridir; Supabase platform kotası değildir. Limit aşımı, route doluluğu ve diğer iş kuralı reddi aynı generic yanıtı üretir.

### 2.2 Sunucu ACL temsili ve pairing sonrası provisioning

Mevcut pairing/trust state cihazların yerel SQLite depolarındadır; Supabase bunu kendiliğinden bilemez. `project_members(project_route, device_id, public_key, role, status, grant_seq, grant_signature, granted_at, revoke_seq, revocation_signature, revoked_at, last_action_seq, last_endpoint_seq)` sunucu ACL'si olur. Function, isteğin imza anahtarının aynı proje route'unda `active` üye olduğunu kontrol eder. Bir projede en fazla üç aktif cihaz vardır. Revoke edilmiş cihaz yeni endpoint yayımlayamaz veya endpoint listesini okuyamaz. Bu ACL, yalnız metadata servisine erişim iznidir; dosya transfer yetkisi sağlamaz.

**Bootstrap (kullanıcı kararı): TOFU project claim.** Projeyi oluşturan cihaz `project-claim` isteğini kendi Ed25519 imzasıyla gönderir. Function/DB atomik olarak route'ta hiçbir üyelik satırı bulunmadığını kontrol eder ve claim'i yapan device/key çiftini ilk aktif `owner` yapar. Route'ta geçmişte revoke edilmiş satır bile varsa yeni claim reddedilir. Çakışma generic hata döndürür; route varlığını açığa çıkarmaz. Enrollment kodu yoktur. Risk: proje UUID'sini gerçek cihazdan önce öğrenen biri route'u claim edebilir. Gerçek cihaz da aynı generic hata alır; bu durumda ACL kurtarma/route reset tasarlanana kadar elle davet dosyası ve doğrudan peer bağlantısı kullanılabilir. Supabase arka ucu için owner identity reset akışı bu işte yoktur. M5d-server uygulamasındaki SQL `trackmesh_dispatch` bu TOFU claim'i route bazında transaction lock'u altında uygular.

`project-claim` yerelde oluşturulan projelerin claim-first yoludur. #42 sonrası dosya veya bağlantıyla içe aktarılan proje, backend yazılmadan köken işareti alır ve claim göndermez. Eski build ile alınmış projelerin kökeni tahmin edilmez; eski davranış korunur. Yeni üye aktif bir üyenin yerel pairing onayı sonrası grant ile eklenir.

**Üyelik grant/revoke (kullanıcı kararı):** mevcut, aktif herhangi bir üye, yerel pairing challenge/proof ve inviter'ın açık onayından sonra yeni cihaz için `member-grant` imzalı isteği gönderebilir. Grant body'si `actor_seq`, `target_device_id`, `target_public_key` taşır; onaylayan aktif üyenin auth imzası bu body hash'ini kapsar. Bu grant onaylayan cihazın mevcut yerel pairing sonucunu beyan etmesidir; Supabase doğrudan pairing transport'ına bakmaz. Yeni kayıt `member` olur; owner yetkisi grant ile devredilmez. `member-revoke` body'si `actor_seq`, `target_device_id` taşır. Her aktif üye kendini çıkarabilir; başka cihazı yalnız aktif owner çıkarabilir. Her imza sahibinin `actor_seq` değeri proje başına monotonik artmalı ve tabloda kalıcı `last_action_seq` ile atomik olarak ilerlemelidir. Grant/revoke imzası ve signer kimliği üyelik satırında kanıt olarak tutulur. Üçüncü aktif üye sonrası yeni grant reddedilir.

İstemci, projeyi oluşturan cihazda yerel revoke sonrasında imzalı `member-revoke` isteğini kalıcı kuyruğa alır. İçe aktarılan projede yalnız yerel erişim kaldırılır. Grant ve revoke ortak actor sayacıyla artan sıra numarasında gönderilir. Unavailable veya NotDeployed turu durdurur; reddedilen grant yeni sıra alıp kuyrukta kalır ve sonraki işlem denenir. Reddedilen revoke düşürülür ve bellek tanısı tutulur. Revoke proje anahtarını döndürmez; eski dosyalar ve anahtar kaldırılan cihazda kalır.

Her SQL/Edge hata yanıtı genel `request rejected` gövdesi döndürür; var olmayan route, dolu route, yanlış key, revoked member, quota ve geçersiz claim ayrıştırılmaz. Claim, grant/revoke, endpoint publish ve endpoint list aynı imzalı istek/replay modelini kullanır. Uygulama onayı artık verilmiştir: M5d-server SQL şeması/migration, Edge Function API, yeni imza scope'ları, ACL modeli ve deploy edilecek servis kaynakları onay kapsamındadır; Rust istemci M5d-client işidir. Supabase üyeliği yerel pairing onayı sonrası imzalı grant ile otomatik güncellenir; self-hosted altyapı ACL hazırlığı ayrı legacy iştir.

Owner da dahil bir üyenin kendini revoke etmesine izin verilir. Owner kendini revoke ederse aktif member'lar metadata işlemlerini sürdürebilir ve grant edebilir; owner rolü olan aktif cihaz kalmadığından başka üyeyi revoke edemez ve route yeniden claim edilemez. Bu kurtarma sınırı kullanıcıya gösterilmelidir; transfer-owner / owner recovery ayrı tasarım ister `[V]`. M5d-server bu kararı uygular, otomatik owner recovery içermez.

## 3. Endpoint mailbox akışı

Gönderen Core M5d wrapper'ını proje anahtarıyla XChaCha20-Poly1305 altında şifreler; AAD `trackmesh-endpoint-mailbox-v1` (M5 tasarımı). Sonra kendi cihaz Ed25519 anahtarıyla Supabase isteğini imzalar. Edge Function imzayı, timestamp/nonce replay sınırını, aktif `project_members` kaydını, alan/boyut limitini ve iç ciphertext'in metadata tutarlılığını doğrular; ciphertext'i kaydeder. İç adres/plaintext kaydı çözülmez. Route ve auth doğrulaması transaction içinde kayıt ekleme ve nonce tüketimini atomik yapar.

`endpoint_list` yalnız aynı projedeki aktif cihaz üyesine, küçük sayfalarda ve henüz süresi dolmamış ciphertext döndürür. Alıcı her kaydı native Core içinde decrypt eder ve `Engine::apply_endpoint_record` çağırır; M5c'deki trusted author → key/PeerId binding → imza → seq → time → adres → atomik local store sırası korunur. Supabase'den gelen kayıt doğrudan learned dial listesine yazılmaz. Yerel doğrulama başarısız olursa kayıt yok sayılır ve kısmi endpoint state oluşmaz. Expiry sonrası dial adayı kullanılmaz; eski değer sessizce geri gelmez.

Endpoint kayıtlarının retention süresi en fazla iç `expires_at` kadar olmalı; örnek tasarım limiti M5'teki maksimum 15 dakikadır. Liste cursor'u endpoint tablosuna özeldir ve announcement cursor'uyla paylaşılmaz. Supabase görebildiği metadata'ya `project_route`, `writer_device_id`, `seq`, `created_at`, istek IP'si/zamanı/boyutu ve erişim örüntülerini ekleyebilir; şifreleme bunları gizlemez.

`head_records` offline hint'leri cihazın kendi authored snapshot'ını değil, o cihazın projedeki **mevcut local head**'ini bildirir. Bu nedenle cihaz, başka bir eşten aldığı imzalı snapshot'ı kendi writer kimliği ve mevcut imza/AEAD doğrulama biçimiyle yayımlayabilir; record içindeki author, writer'dan farklı olabilir. Client writer imzasını writer cihazının trusted key'i ile doğrular ve `(project, writer)` başına en yüksek `seq` değerini tutar. Wrapper, AAD, imza byte'ları, sunucu sözleşmesi ve tablo şeması değişmez. Eski kayıtlarda `author == writer` olduğundan geriye dönük doğrulama sürer.

## 4. Şifreli davet drop ve atomik tek okuma

M6 yerel olarak uygulanmıştır. Native Rust mevcut imzalı `ProjectInvite` payload'ını XChaCha20-Poly1305 ile şifreler. ID 256 rastgele bitin 43 karakterlik base64url temsilidir; bağımsız çözme anahtarı 256 bittir. AAD byte sırası UTF-8 ile `trackmesh-invite-drop-v1 || 0x00 || id || 0x00 || expires_at` olur. Ciphertext'in base64url temsili 64 KiB'den küçüktür. Süre 15 dakikadır; SQL daha ileri expiry'yi reddeder.

Yeni v2 bağlantı `trackmesh://invite/v2/<ref>/<publishable_key>/<id>#<key>` biçimindedir. Servis referansı, publishable key ve drop ID bağlantıdadır; çözme anahtarı fragment'tedir ve HTTP'ye gönderilmez. Erişilebilir adresler bağlantıdaki açık path'te değil, şifreli imzalı davetin içindedir. Çözülmüş davet ve proje anahtarı Supabase/React/log/ajan bağlamına girmez.

`invite-create` cihaz imzalı isteği kullanır. SQL aktif üyelik, nonce, boyut ve expiry kontrolünü drop eklemeyle tek transaction'da yapar. `invite-consume` alıcı henüz üye olmadığı için imzasızdır; yalnız ID gönderir. `DELETE ... WHERE id=p_id AND expires_at>clock_timestamp() RETURNING` tek kazananı sağlar ve ciphertext'i döndürürken siler. `consumed` kolonu ve `pg_cron` gereksinimi yoktur. RLS deny-all'dır; `anon`/`authenticated` doğrudan erişmez.

ID'yi bilen kişi anahtarı bilmeden önce tüketerek daveti engelleyebilir. Başlangıç limitleri Supabase projesi genelinde create 60/saat, consume 300/saat ve ID başına consume 10/saat'tir. Global sayaçların tüketilmesi meşru kullanımı geciktirebilir; bu kabul edilmiş availability sınırıdır. Güvenilir client-IP kaynağı yoktur; IP limitleri veya IP depolama uygulanmaz. Her RPC transaction'ında en fazla 500 expired drop, 1.000 eski nonce ve 1.000 eski rate bucket temizlenir.

Yeni davetler backend tanımı ve imzayı taşır; alanların sıralı tamamı cihaz anahtarıyla imzalanır. Alıcı, drop'u aldığı servisin host/publishable key'ini çözülen imzalı davetle doğrular. Global varsayılanı yoksa bu servis kaydedilir; farklı mevcut varsayılan değiştirilmez. Eski `trackmesh://invite/<id>#<key>` bağlantısı varsayılan servisi kullanır veya yoksa bir kez URL/key ister. İmzasız tarihsel dosya davetleri kabul edilebilir, ancak backend yapılandıramaz.

İstemci yalnız Project URL ve yeni `sb_publishable_...` key kullanır; legacy JWT anon veya secret key kabul etmez. Edge Function ayrı server-side `SUPABASE_SECRET_KEYS` JSON sözlüğünün `default` secret'ını kullanır. Secret key ve DB parolası istemci/kullanıcıya verilmez. API Keys Dashboard yolu ve deployment adımları [servis README'sindedir](../../services/supabase/README.md).

Canlı host/Windows Sandbox kaydı davet tüketimi, açık onay ve dosya aktarımını gösterir. Sonraki tüketim yarışı, 15 dakikalık expiry, clipboard history dışlama/temizleme ve ayrı fiziksel ağ doğrulaması kayıtlarda yoktur.


## 5. Ücretsiz katman, kota ve erişilememe davranışı

Resmi Supabase belgelerinde 2026-10-12 tarihinde görünen Free plan değerleri:

| Kullanım | Free plan değeri | Tasarım etkisi |
| --- | --- | --- |
| Veritabanı | 500 MB / proje | Endpoint ciphertext ve ACL/nonce/rate-limit satırları küçüktür; TTL cleanup ve büyüme gözlemi şarttır. |
| Dosya Storage | 1 GB | Bu tasarım Supabase Storage kullanmaz; ciphertext Postgres'te. Storage kotası davet upload limiti değildir. |
| Egress | 5 GB | Endpoint polling/pagination ve invite ciphertext indirmesi tüketir; küçük sayfa ve makul refresh sıklığı gerekir. |
| Edge Function çağrısı | 500.000 / ay | Her publish/list/invite upload/consume çağrısı kota tüketir; retry/backoff sınırlanmalı. |
| Edge Function runtime | 150 s worker wall-clock; istek başına 2 s CPU | İşlemler kısa, bounded ve tek DB transaction'ı olmalı; ağır iş/crypto loop yok. İmzaların sınırlı sayıda doğrulanması için uygulama hedefi oluşturulur. |
| API istekleri | Pricing sayfası “unlimited API requests” der; bu sınırsız CPU, egress veya abuse koruması anlamına gelmez. | TrackMesh Function başına/IP/device/project başına uygulama limitleri ve payload sınırları koymalı. |
| Hareketsizlik | Free projeler yetersiz DB etkinliğiyle 7 günlük pencerede otomatik duraklatılabilir; karar eşiği sabit istek sayısı olarak yayımlanmamış. | Proje duraklatılırsa otomatik endpoint ve invite hizmeti kullanılamaz. Ürün arka planda projeyi canlı tutmak için sahte ping üretmez; proje sahibi Dashboard'dan geri yükler veya ücretli plana geçer. |

Kotalar Supabase organizasyonu/proje konfigürasyonu ve ürün değişikliklerine bağlıdır. Bu tablo fiyat/availability garantisi değildir. Functions CPU/wall-clock sınırı ücretsiz katman sayılarını aşmaktan bağımsız runtime limitidir. Free plan'de 1 yıllık restore penceresi sonrası Dashboard'dan geri yükleme garantisi yoktur; davet drop'ları zaten 15 dakika sonra geçersiz olmalıdır.

Supabase timeout, quota, project pause, HTTP 5xx veya ağ hatasında TrackMesh durable/local snapshot, doğrudan açık peer bağlantısı ve manuel davet dosyası akışını çalışır tutar. Endpoint mailbox fallback'i erişilemezse status `UNKNOWN/OFFLINE` kalır; eski endpoint taze gibi sunulmaz. Invite linki kullanılamıyorsa kullanıcı hâlâ mevcut native davet dosyasını seçebilir. Sessiz retry sınırlı exponential backoff ve jitter ile, yalnız kullanıcı akışını bozmadan yapılmalıdır; erişilebilirlik garantisi verilmez.

## 6. İstemci yapılandırması ve bağımlılıklar

Uygulanan tipli arka uç seçenekleri `NONE` (doğrudan yol), `SELF_HOSTED_MAILBOX` (foundation mailbox URL'si) ve `SUPABASE`'dir. Desktop ayarları ve core aynı `ProjectBackendConfig`'i kullanır; `mailbox_url` başka anlamda yorumlanmaz. Dosya daveti ve doğrudan aktarım Advanced yolu olarak korunur.

Supabase URL politikası yalnız HTTPS `*.supabase.co` origin'lerini kabul eder. Kullanıcı bilgisi, özel port, ek path, query veya fragment kabul edilmez. `sb_publishable_...` key istemci ayarıdır; legacy JWT anon, service-role ve secret key reddedilir. Alanlar sır olarak loglanmaz ve hata metninde yankılanmaz.

#42 sonrası `settings.json` sürüm 5'tir; v1–v4 mevcut geçiş yolundan yüklenir. Backend, içe aktarılan proje işareti, bekleyen grant/revoke ve ortak actor sıra sayacı atomik dosya değiştirmeyle saklanır. Bozuk veya bilinmeyen ileri sürüm dosya ezilmez. Eski build v5'i daha yeni sürüm olarak reddeder; manuel deneme öncesi mevcut ayarların yedeği alınmalıdır.

`TRACKMESH_DEFAULT_SUPABASE_URL` ve `TRACKMESH_DEFAULT_SUPABASE_PUBLISHABLE_KEY` yalnız özel build varsayılanlarıdır. Public release kişisel Supabase projesi içermemelidir. Gerçek servis ref/URL/key değerleri belgelere veya depoya yazılmaz.

Rust istemci mevcut reqwest/rustls, XChaCha20-Poly1305 ve Ed25519 altyapısını kullanır. Edge Functions TypeScript/Deno koduyla imzayı kontrol eder ve tek SQL RPC'yi HTTP üzerinden çağırır. WebCrypto Ed25519'un Rust `verify_strict` ile tüm malformed/noncanonical imza koşullarındaki eşdeğerliği ayrıca doğrulanmamıştır.


## 7. Depo yerleşimi ve kurulum akışı

Uygulanan kaynaklar `services/supabase/` altındadır:

- `supabase/config.toml`: dokuz fonksiyonun JWT gateway doğrulamasını kapatır; proje ref veya key içermez.
- `supabase/migrations/20261012000100_trackmesh_m5d.sql`: üyelik, endpoint, nonce/rate-limit ve transaction dispatch.
- `supabase/migrations/20261012000200_m6_invite_drop.sql`: şifreli drop ve tek okuma.
- `supabase/migrations/20261013000100_m7_head_records.sql`: şifreli head kaydı ve head publish/list.
- `supabase/functions/`: `project-claim`, `member-grant`, `member-revoke`, `endpoint-publish`, `endpoint-list`, `head-publish`, `head-list`, `invite-create`, `invite-consume`.
- [README.md](../../services/supabase/README.md): Dashboard/CLI migration ve her fonksiyon için `--no-verify-jwt` deployment adımları.

GitHub entegrasyonu `services/supabase` çalışma dizini ve `master` dalıyla migration'ları push sırasında uygular. m7 içeren master 2026-10-13'te push edildi; Dashboard'da uygulama sonucu kontrol edilmedi. Edge Function deployment migration'dan ayrıdır; `head-publish`/`head-list` için canlı durum **unconfirmed**'dır. Dağıtılmış migration değiştirilmez; düzeltme için yeni migration gerekir.

İstemciye yalnız Project URL ve publishable key verilir. Supabase'in server-side enjekte ettiği `SUPABASE_URL` ve `SUPABASE_SECRET_KEYS` geçersiz/eksikse Functions generic hata ile kapalı kalır. Değerleri istemciye veya loga koyarak bu kontrol aşılmaz. Gerçek RLS/grant, yarış, quota/cleanup ve log redaction doğrulaması deployment işidir; belge yönergesi kanıt değildir.


## 8. Tehdit modeli ve açık doğrulamalar `[V]`

| Tehdit | Tasarım önlemi | Kalan sınır |
| --- | --- | --- |
| Anon key veya doğrudan REST ile tablo erişimi | Grant kaldırma + RLS deny-all; tek giriş Function. | SQL role/grant, exposed schema ve Data API ayarı canlı projede doğrulanmalı. |
| Supabase secret/service-role sızıntısı | Yalnız Function secret; istemci/binary/depoya/loga koymama, rotation prosedürü. | Supabase Function güvenlik/operasyon hesabı ele geçirilirse ciphertext silinebilir/değiştirilebilir ve ACL manipüle olabilir; imzalı iç endpoint kaydı bunu sahte adres olarak kabul etmeyi engeller. |
| Sahte cihaz/proje üyeliği | Body hash/time/nonce imzalı isteği ve cihaz/project ACL kontrolü; mevcut aktif üyenin yerel pairing onayı sonrası imzalı grant. | TOFU claim öncesi proje UUID'sini öğrenen kişi route'u claim ederek hizmeti engelleyebilir; owner/admin kurtarma ve key rotation yok. |
| Replay/equivocation | Scope/path/body hash, dar zaman penceresi ve atomik nonce tüketimi; record seq alıcıda kalıcı. | Nonce table cleanup/kota, yedekten restore ve eşzamanlı transaction semantiği doğrulanmalı. |
| Mailbox/DB operatörünün endpoint içeriğini görmesi | Proje anahtarlı AEAD; iç imza ve `Engine::apply_endpoint_record` doğrulaması. | Route, device ID, seq, zaman, boyut, IP ve erişim paterni metadata'sı görünür; servis kaydı silerek/erteleyerek availability'yi etkiler. |
| Invite drop ID'nin tüketilmesi / link sızıntısı | 256-bit key fragment'te, ciphertext-only, kısa TTL, generic response, one-read transaction ve rate-limit. | ID'yi bilen saldırgan key'i bilmeden de tüketerek denial of service yapabilir; clipboard/link capture tamamen engellenemez. |
| Spam/kota tüketme | Device/project/global/ID limitleri, maksimum body, pagination, retry backoff, TTL cleanup. | Supabase Free sabit request kotası yayımlamıyor; maliyet/availability ve güvenilir IP kaynağı deploymentta yeniden doğrulanmalı. |
| Proje 7 gün sonra duraklar | Client manuel dosya ve direkt bağlantı yollarına düşer; UNKNOWN/OFFLINE, eski endpoint yok. | Free inactivity ölçütü sabit değil; restore Dashboard/owner gerektirir ve uzun duraklamada 1 yıl sınırı var. |
| Kötü niyetli veya yanlış yönetilen ACL | Project route + public key binding; aktif üye onay sonrası grant edebilir; yalnız owner başka üyeyi revoke eder. | Owner kendini revoke ederse başka üyeyi çıkaracak admin kalmayabilir; owner recovery/transfer kararı yok. |

Uygulama/deployment doğrulaması `[V]`: (1) `anon`/`authenticated` SQL grants, RLS deny-all ve Edge Function privilege yolunun canlı Supabase project'te doğrulanması; (2) M5d-client Rust kanonik tuple/signature çıktısının Ed25519 WebCrypto doğrulamasıyla byte-byte eşleşmesi; WebCrypto Ed25519 doğrulamasının Rust `ed25519_dalek::verify_strict` ile aynı canonicality/strictness koşullarını sağladığı ayrıca kanıtlanmadı; malformed/noncanonical imza davranışı staging'de doğrulanmalı; (3) Postgres claim/grant/revoke, nonce/rate-limit transaction yarışları ve üç cihaz sınırı; (4) Supabase project claim TOFU yarışı ve generic conflict; (5) M5d-client URL/key configuration ve secret rotation; (6) Supabase quota/idle pause güncel davranışı; (7) gerçek Windows network outage, fallback UI ve doğrudan kanal davranışı; (8) privacy review, retention, account/billing ownership ve bölge seçimi; (9) Edge Function'a gelen client-IP header'ın spoof edilmeyen kaynağını Supabase resmi doküman/gerçek deployment ile belirleme. M6 global/ID limitleri ve RPC cleanup uygulanmıştır; canlı abuse ve yarış doğrulaması ayrıca yapılmalıdır.

## 9. Kilometre taşları ve kalan doğrulama

1. **M5d-server:** m5d migration, deny-all RLS, tek transaction dispatch ve beş imzalı Edge Function uygulandı. Statik kontrol ve sınırlı canlı host/Sandbox kanıtı IMPLEMENTATION.md'dedir; canlı RLS incelemesi, concurrency ve tam WebCrypto strictness hâlâ açık.
2. **M5d-client:** signed body hash/tuple, function scope, HTTPS/publishable key doğrulaması, sürümlü settings ve endpoint wrapper uygulandı. Canlı kayıtta üyelik lookup'u ve endpoint list kabulü vardır; gerçek ağ değişimi ve ayrı fiziksel ağ akışı doğrulanmadı.
3. **M5e apply/expiry:** `Engine::apply_endpoint_record` ortak trust/signature/seq/time/address yoludur. Bounded cursor/paging ve expiry kontrolü uygulanmıştır; gerçek peer IP değişimi doğrulaması açık.
4. **M6 invite drop:** ayrı m6 migration, `invite-create`/`invite-consume`, fragment anahtarı, 15 dakika hard TTL, 64 KiB limit, atomik silerek tüketme, global/ID abuse limitleri, cleanup, URI ve clipboard yolu uygulanmıştır. Canlı consume/onay/ilk aktarım kayıtlıdır; expiry, ikinci açma, yarış ve clipboard kontrolleri açık.
5. **M7 head ve #42:** m7 migration/fonksiyon kaynakları, offline head hint'i, creator-side revoke, imported köken işareti, sıralı grant/revoke retry ve eksik fonksiyon tanısı uygulanmıştır. Dashboard migration sonucu, head Function deployment ve canlı #42 davranışı doğrulanmadı.
6. **Her aşama:** Otomatik test/benchmark eklenmez veya çalıştırılmaz. Gerçek statik/deployment/manuel kontroller yalnız [IMPLEMENTATION.md](../IMPLEMENTATION.md) içinde kaydedilir.


## 10. Kaynaklar (resmi Supabase; erişim 2026-10-12)

- [Billing and quotas](https://supabase.com/docs/guides/platform/billing-on-supabase) — Free DB 500 MB/proje, file Storage 1 GB, egress 5 GB, Functions 500k; kota planı.
- [Edge Function pricing](https://supabase.com/docs/guides/functions/pricing) — Free 500k çağrı/ay; aşım fiyatlandırması.
- [Edge Function limits](https://supabase.com/docs/guides/functions/limits) — Free worker 150 s, 2 s CPU/request, memory ve platform limitleri.
- [Free project pausing](https://supabase.com/docs/guides/platform/free-project-pausing) — düşük DB aktivitesiyle 7 günlük pencerede duraklatma; eşik sabit değil, Dashboard restore ve 1 yıl penceresi.
- [API keys](https://supabase.com/docs/guides/getting-started/api-keys) — publishable key'in dağıtılabilir oluşu, secret key'in backend'de tutulması.
- [Securing data](https://supabase.com/docs/guides/database/secure-data) — istemci için RLS/least privilege; secret/service role'u asla istemciye vermeme.
- [Row Level Security](https://supabase.com/docs/guides/database/postgres/row-level-security) — RLS ve service role'un RLS bypass özelliği.
- [Edge Function secrets](https://supabase.com/docs/guides/functions/secrets) — production secret'larını Dashboard/CLI ile ayarlama.
- [Edge Functions overview](https://supabase.com/docs/guides/functions) — TypeScript/Deno runtime, CLI ve deployment yolları.
- [Edge Functions Dashboard quickstart](https://supabase.com/docs/guides/functions/quickstart-dashboard) — Dashboard editor ile function oluşturma/deploy etme.
- [Supabase CLI local development](https://supabase.com/docs/guides/cli/local-development) — `supabase init`, local workflow ve migration/deployment başlangıcı.
- [Supabase CLI reference](https://supabase.com/docs/reference/cli) — `db push`, function deploy ve `--no-verify-jwt` seçenekleri.
- [Web Crypto Ed25519 importKey](https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/importKey) — raw Ed25519 public key formatı; Supabase Deno runtime'da ayrıca doğrulanmalıdır.
- [PostgreSQL JSON functions](https://www.postgresql.org/docs/current/functions-json.html) — `jsonb_object_keys` set-returning function; body key-count SQL'i bu fonksiyonla yapılır.
- [Supabase Securing your API](https://supabase.com/docs/guides/api/securing-your-api) — `request.headers` içindeki `X-Forwarded-For` erişim örneği (PostgREST bağlamı; Edge handler header güvenilirliğini tek başına kanıtlamaz).
- [Supabase log field reference](https://supabase.com/docs/guides/observability/log-field-reference) — Gateway loglarına alınabilen `x-real-ip`/`cf-connecting-ip` header adları; spoof-resistant Edge header garantisi vermez.
- [Project backups](https://supabase.com/docs/guides/platform/backups) — plan/backup farkları; Free plan'de backup sınırları.
