# Aşama 1 — Doğrudan P2P uygulama planı

**Durum (2026-10-13):** Aşama 1 M0–M10 doğrudan-P2P planıdır. M0–M9'un aşağıda belirtilen bölümleri uygulanmış veya incelenmiştir; M3 PCP ertelenmiş, M8 gerçek internet erişim kanıtı ve M10 ayrı ağ kabulü açık kalmıştır. Gerçek kontrol kanıtları yalnız [IMPLEMENTATION.md](../IMPLEMENTATION.md) içindedir. Foundation phases 1–8, `PHASE*.md` belgelerindeki önceki temeldir; #NN review/UX görev numarasıdır.

**Varsayımlar** `[V]` ile işaretlidir: kod veya kütüphane davranışı bu planı yazarken çalıştırılarak doğrulanmadı (ilk plan hazırlanırken Windows, router ve ikinci ağ çalıştırma doğrulaması yoktu). Her `[V]` ilgili kilometre taşının başında doğrulanır.

## 1. Kapsam

İçinde: LAN/doğrudan QUIC+TCP (Noise/Yamux korunur); kullanıcı onaylı UPnP IGD (PCP ertelendi); erişilebilirlik gözleminin eşleme sonucundan ayrı gösterimi; ağ değişiminde yeniden deneme/yenileme; imzalı endpoint güncellemesi; isteğe bağlı Supabase v2 davet bağlantısı ve head hint'i; açılışta peer/snapshot durum yenileme.

Dışında: relay/VPS/DCUtR doğrulaması ve desteklenen transfer relay fallback'i, Windows File Watcher, DAW'dan bağımsız dosya izleme, herkese açık DHT, BitTorrent, bulutta proje depolama, DAW açıkken otomatik Receive. Relay/DCUtR/rendezvous kodu build'den çıkarılıp `future/` altına alındı; relay üründe yoktur ve [post-1.0'a ertelenmiştir](post-1.0.md#relay).

Değişmezler (her kilometre taşında): imzalı kimlik/sabit `PeerId`, proje yetkisi (`store.authorized`), `gate.rs` pairing penceresi, snapshot/transfer kilit ve journal sırası, yerel SQLite, `SAVE_OBSERVED` dışında dosya izleme yok (manual Share ve ilk onay snapshot'ı ayrıca vardır), bulutta yalnızca şifreli metadata, davet/anahtar/sır log'a veya ajan bağlamına girmez, **otomatik test/benchmark eklenmez veya çalıştırılmaz** (yalnızca `cargo fmt/check/clippy`, `npm typecheck/build`, manuel doğrulama).

## 2. Kod tabanı gerçekleri (bu inceleme sırasında okunan)

| Konu | Bulgu | Plan etkisi |
| --- | --- | --- |
| Dinleme | `behaviour.rs:93-94` `0.0.0.0` UDP(QUIC)+TCP, tek `config.port` (43101). | Eşlemenin yerel portu bilinir; TCP ve UDP ayrı kayıt. |
| Dış adres | Başlangıç adresleri ve M3 `Command::SetExternalAddresses` ile Gateway kaynaklı dinamik adresler ayrı tutulur. | Public UPnP adresi eklenir; başarısızlık/rıza iptalinde yalnız Gateway adresi çekilir. |
| Adres kaynağı | `DeviceCard.addresses` **imzalı** (`signing_bytes`), `trusted_peers.card_json` içinde; `Store::trust` aynı `device_id` için kartı **koşulsuz üzerine yazar** (`trust.rs:36`). Kartta zaman/sıra yok. | Eski kart yeniden oynatılarak adres gerilemesi mümkün → M5'te yeni imzalı tip + monotonik sıra şart. `DeviceCard` alanları değiştirilmez (`deny_unknown_fields`, imza biçimi sabit). |
| Dial | `discovery.rs::tick` 6 sn'de bir; kart ve süreli `learned` adresleri, QUIC/TCP denemeleri. Legacy circuit kodu vardır. | M4 ağ değişiminde `direct_attempts` sıfırlar; M5 imzalı endpoint süreli adaya dönüşür. Relay desteklenen yol değildir. |
| Panel adresi | Davet adresleri listen/advertised public adayları ve doğrulanmış manuel public endpoint'ten oluşur. | Elle yönlendirme Settings → Advanced içindedir; eşleme tek başına erişim kanıtı değildir. |
| Mailbox | Foundation `Envelope` snapshot duyurusudur. Supabase endpoint ve head kayıtları ayrı şifreli türlerdir; M6 davet drop'u ayrı tek-okuma RPC kullanır. | Eski Envelope AAD/imzası korunmuştur; Supabase proje dosyası taşımaz. |
| Settle | `scan_after_save`: ~2 sn sessizlik, 60 sn sınır, 3 deneme; watcher değil. | P5 çoğunlukla mevcut; yalnız doğrulama. |
| Proje yetkisi | İstek yetkisi hem `command` hem gelen istekte `authorized` ile kontrol. | Yeni RPC'ler de proje/cihaz yetkisi ister. |

## 3. Kütüphane ve Windows API kararı (M0; 2026-10-12)

M0'ın tarihsel web incelemesi crate metadata'sı, docs.rs belgeleri/kaynakları, upstream depoları ve Microsoft Learn birincil belgeleriyle yapıldı. Bu inceleme Windows hedef derlemesi veya router davranışı değildir.

| Alan | Karar ve kanıt | Durum |
| --- | --- | --- |
| UPnP IGD | M2 adayı: [`igd-next`](https://crates.io/crates/igd-next), crates.io/docs.rs'teki `0.18.0`; [upstream README](https://github.com/dariusc93/rust-igd#feature-flags), [0.18.0 manifest](https://docs.rs/crate/igd-next/0.18.0/source/Cargo.toml.orig), [async Tokio API](https://docs.rs/igd-next/0.18.0/igd_next/aio/index.html). Upstream README, varsayılan senkron I/O'dan ayrı `aio_tokio` özelliğini (Tokio + Hyper) belgeliyor; manifest MIT lisansı ve optional Tokio bağımlılığını doğruluyor. Gateway API'si TCP/UDP protokol parametresi, sonlu lease, `PortInUse`, `OnlyPermanentLeasesSupported` ve kendi kaydını silme işlemlerini sunuyor. | Tokio/MIT/API doğrulandı. Windows hedef derlemesi **[V]**: bu M0'da MSVC hedef derlemesi veya resmi Windows CI kanıtı doğrulanmadı; M2'de bağımlılık eklenmeden önce kontrol edilmeli. Bakım sinyali: yayımlanmış `0.18.0` ve upstream commit/release geçmişi var; devamlı bakım/SLA garantisi yok. |
| PCP | M3 adayı: [`crab_nat`](https://crates.io/crates/crab_nat), `0.8.1`; yalnız [`pcp` API'si](https://docs.rs/crab_nat/0.8.1/crab_nat/pcp/index.html) kullanılmalı. [Manifest](https://docs.rs/crate/crab_nat/0.8.1/source/Cargo.toml) MIT ve Tokio `^1.52.3` bağımlılığını gösteriyor; bu aralık repo kilidindeki Tokio `1.53.2` ile uyumlu, fakat yeni bağımlılık çözülmedi. [upstream README](https://github.com/ryco117/crab_nat#usage) async Tokio UDP kullanımını ve `pcp` ile `natpmp` modüllerini ayrı kullanmayı belgeliyor. `PortMapping::new` PCP başarısızsa NAT-PMP'ye geçer; o kolaylık API'si bu plan için kullanılmayacak. | Tokio/MIT/API doğrulandı. Windows hedef derlemesi **[V]**: bu incelemede resmi Windows CI/hedef sonucu yok; M3 öncesi kontrol edilmeli. Bakım sinyali: docs.rs sürüm geçmişinde `0.8.1` (2026-07-06), `0.8.0` (2026-07-05), `0.7.6` (2026-04-13) var; upstream özellik notları PCP `ANNOUNCE` ve seçeneklerinin desteklenmediğini açıkça listeliyor. Yakın yayımlar sürekli bakım garantisi değildir. |
| `pcp-client`, `port-control-client` | Seçilmedi. [`pcp-client`](https://docs.rs/pcp-client/latest/pcp_client/) public mapping API'si UDP ile sınırlı; TCP+UDP gereksinimini karşılamıyor. [`port-control-client`](https://docs.rs/port-control-client/latest/port_control_client/) docs, gateway discovery desteğini Linux/macOS olarak sınırlar ve Windows için UPnP'yi belirtir. | Kapsam/Windows uygunluğuyla elendi; diğer bakım/lisans/Windows derleme iddiaları bu seçim için gerekli görülmedi. |
| `libp2p-upnp` | Seçilmedi. [rust-libp2p](https://docs.rs/libp2p/latest/libp2p/upnp/tokio/index.html) bunu swarm `NetworkBehaviour` olarak sunuyor. TrackMesh'in kullanıcı onayı, TCP+UDP için ayrı kayıt sahipliği ve eşlemeden ayrı durum raporu gereksinimini karşılayıp karşılamadığı bu incelemede doğrulanmadı. Bu nedenle önceki "yalnız TCP" iddiası **[V]** olarak kaldırıldı; doğrulanmamış kapsam iddiası seçim gerekçesi değil. | Dışarıda bırakıldı; ayrı istemci seçildi. |
| NAT-PMP | Uygulanmayacak. RFC 6886 kendisini RFC 6887 PCP tarafından superseded edilmiş ve PCP'yi uyumlu paket biçimi üzerine önemli geliştirmeler ekleyen standart-track protokol olarak tanımlar. M0 kapsamı mevcut UPnP IGD + ayrı PCP tanı durumlarıdır; üçüncü protokol eklemek aynı temel eşleme akışını tekrarlar ve M1–M4 kapsamını büyütür. `crab_nat` kullanılacaksa yalnız `pcp` modülü çağrılır; NAT-PMP otomatik fallback'i devre dışı kalır. | Kapsam kararı; router desteği hakkında iddia değildir. |
| Arayüz ve gateway | Windows IP Helper [`GetAdaptersAddresses(AF_INET, GAA_FLAG_INCLUDE_GATEWAYS)`](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses) ile arayüz, IPv4 ve gateway adayları; [`GetIpForwardTable2(AF_INET)`](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getipforwardtable2) ile `0.0.0.0/0` varsayılan rota satırları. Birden fazla rota/arayıüz olabilir; ilk gateway'i varsayılan sayma. `GetAdaptersAddresses` senkron ve maliyetli olabileceğinden M1 çağrıları `spawn_blocking` içinde yapılmalı. | API seçildi; Rust binding/derleme ve çoklu arayüz davranışı **[V]**. [`IP_ADAPTER_ADDRESSES`](https://learn.microsoft.com/en-us/windows/win32/api/iptypes/ns-iptypes-ip_adapter_addresses_xp), [`MIB_IPFORWARD_ROW2`](https://learn.microsoft.com/en-us/windows/win32/api/netioapi/ns-netioapi-mib_ipforward_row2). |
| Windows Firewall | Salt okunur durum için [`INetFwPolicy2`](https://learn.microsoft.com/en-us/windows/win32/api/netfw/nn-netfw-inetfwpolicy2): etkin profil bitmask'i, etkinlik, varsayılan eylem ve kural koleksiyonu; [`INetFwRule`](https://learn.microsoft.com/en-us/windows/win32/api/netfw/nn-netfw-inetfwrule) alanlarıyla TCP/UDP, yön, eylem, port, adres/arayüz kapsamını gözlemle. [`CoInitializeEx`](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coinitializeex) ve COM sorguları aynı blocking thread'de; kural oluşturma/değiştirme yok. [`INetFwProducts::get_Count`](https://learn.microsoft.com/en-us/windows/win32/api/netfw/nf-netfw-inetfwproducts-get_count) yalnız kayıtlı üçüncü taraf ürünleri sayar; sıfır başka paket filtreleri olmadığını kanıtlamaz. | API seçildi; COM projection/etkili politika yorumlama ve Windows doğrulaması **[V]**. Gerçek internet erişilebilirliği firewall tablosundan çıkarılmaz. |

`GetAdaptersAddresses` ve `GetIpForwardTable2` M1'de doğrudan Windows API ile, PowerShell'e shell-out yapmadan okunur. `INetFwPolicy2` salt-okunur incelemesi M8 kapsamındadır; M1'de firewall API'sine çağrı yapılmaz. Windows IP Helper uygulaması `crates/core/src/network_observation.rs` içindedir. Açılış gözlemi Desktop `spawn_blocking` işçisinde; M4 periyodik gözlemi PeerRuntime izleyicisinde `spawn_blocking` ile alınır, hiçbir IP Helper çağrısı actor veya async executor thread'inde yapılmaz. Windows API yüzeyi M1'de Windows hedefiyle kontrol edilir.

## 4. Ortak durum modeli

Mevcut ağ durum alanları `crates/network/src/status.rs` içindedir. Windows arayüz/gateway gözlemi `crates/core/src/network_observation.rs`, süreli UPnP işçisi `crates/network/src/portmap.rs` tarafından sahiplenilir. Gözlem, eşleme ve peer bağlantısı ayrı tutulur.

- PortMappingStatus açık rıza, TCP/UDP sonucu, dış IP sınıfı ve bounded gerekçe taşır. Varsayılan `NOT_ATTEMPTED`'dır; PCP istek göndermez.
- ReachabilityStatus `UNVERIFIED` kalır. Yetkili public-address inbound olayının peer/transport/zamanı gözlem olabilir; ayrı ağdan gelindiğini kanıtlamaz. `VERIFIED_INBOUND` gerçek çalışma davranışı uygulanmış sayılmaz ve Desktop dalı kaldırılmıştır.
- `MAPPED` internet erişimi değildir. Aynı LAN veya hairpin trafik proof olarak kullanılamaz.
- Firewall gözlemi etkin profil bitmask'ini ve TCP/UDP candidate izin/engel sonuçlarını gösterir. Üçüncü taraf ürün kaydı yokluğu başka filtrelerin yokluğunu kanıtlamaz; okunamayan veri `UNKNOWN` olur. Kural değiştirme yapılmaz.
- Online peer karşılaştırması ve ACK fresh eşitliğin kaynağıdır. Offline head hint'i yalnız son bilinen bilgidir. TransferState/ApplyPhase ve wire etiketleri korunur.


## 5. Kilometre taşları

Her başlık: **Amaç / Dosyalar / Görevler / Korunacaklar / Kontroller / Elle doğrulama / Riskler**. "Kontroller" yalnız derleme/statik; hiçbir kilometre taşı test dosyası eklemez.

### M0 — Doğrulama ve uyumluluk notu (kod yok)

- **Durum:** inceleme tamamlandı; runtime kanıtı değildir.

- **Amaç:** `[V]` maddelerini kapatmak, kütüphane seçimini kesinleştirmek.
- **Görevler:** `AGENTS.md`, `crates/network/AGENTS.md`, `crates/core/AGENTS.md`, `docs/MVP.md` yeniden oku. Aday UPnP/PCP crate'lerinin Windows derlenebilirliğini, tokio entegrasyonunu, lisansını, bakım durumunu resmi kaynaktan doğrula (varsayım yok). Windows ağ/firewall durum API'lerini seç. Sonucu bu dosyanın §3'üne işle.
- **Not:** Kullanıcının ağı (TP-Link VX220-G2u, UPnP açık, WAN IPv4 = görünen IPv4) **ön bilgidir, test sonucu değildir**; port erişilebilirliği doğrulanmadı.
- **Elle doğrulama:** yok. **Çıktı:** güncellenmiş §3, `IMPLEMENTATION.md`'ye yalnız gerçekten yapılan inceleme.

### M1 — Gözlemlenebilir ağ durumu (eşleme yok)

- **Durum:** uygulandı; statik kontroller kayıtlı, gerçek router/ağ doğrulaması açık.

- **Amaç:** Arayüz ve core için erişilebilirlik modelini eklemek; davranış değişmez.
- **Dosyalar:** `crates/core/src/network_observation.rs` (Windows IP Helper okuması), `crates/core/src/lib.rs`; `apps/desktop/src-tauri/src/runtime.rs` (`DesktopStatus`), `src/api.ts`, `App.tsx` (Connection setup panelinde salt-okunur satırlar). Gözlem, Desktop başlangıcındaki `spawn_blocking` işinde alınır; network actor dışında kalır.
- **Görevler:** `GetAdaptersAddresses` ile IPv4 arayüzü/adresleri ve gateway adayları; `GetIpForwardTable2` ile varsayılan route/gateway'ler. Dışarıdan görünen IP için M1'de sorgu yapılmadığından `WAN IP = visible IP? UNKNOWN` ve kaynak açıkça “ölçüm yok” olarak etiketlenir; elle girilen IP ölçüm değildir. UPNP_IGD ve PCP'nin TCP/UDP `Mapping` satırları `NOT_ATTEMPTED` kalır. Router ve firewall çağrısı yoktur; Firewall COM okuması M8'e aittir.
- **Korunacaklar:** `NetworkStatus` JSON alanları (ekleme yalnız ek alan), elle genel IPv4 akışı.
- **Kontroller:** `cargo fmt --all -- --check`, `cargo check -p trackmesh-network -p trackmesh-core -p trackmesh-desktop --locked --offline`, `cargo clippy … -D warnings`, `npm run typecheck && npm run build`.
- **Elle:** panelde yeni alanların görünmesi (Windows).
- **Risk:** VPN/çoklu arayüzde yanlış arayüzün seçilmesi → gösterilen değerler "gözlem" etiketli, karar vermez.

### M2 — UPnP IGD eşlemesi (onaylı, süreli)

- **Durum:** uygulandı; statik kontroller kayıtlı, gerçek router/ağ doğrulaması açık.

- **Amaç:** Kullanıcı ilk kullanımda açıkça onaylarsa TCP ve UDP için ayrı süreli eşleme.
- **Dosyalar:** `crates/network/Cargo.toml` (`igd-next 0.18`, `default-features = false`, `aio_tokio`), `crates/network/src/portmap.rs`, `crates/core/src/settings.rs`, `crates/core/src/peer.rs`, Desktop connection setup/API. Onay ayarı ilk M2'de sürüm 1, güncel #42 sonrası sürüm 5 `settings.json` içindedir; eksik, bozuk veya ileri sürüm kapalı kalır; `snapshot::atomic_replace` ile atomik değiştirilir. DB şeması değişmez; ayar deposu `[V]` kapandı.
- **Görevler:** (a) onay yoksa `NOT_ATTEMPTED`, hiçbir router isteği gitmez; (b) keşif ve mapping listesi sorguları süreyle sınırlı; (c) yalnız tek bir gözlemlenen IPv4 arayüz/gateway çifti varsa devam et, belirsizlikte `UNKNOWN`; (d) gerçek dinlenen TCP ve QUIC/UDP portlarını ayrı ayrı eşle, açıklama `TrackMesh` + kısa cihaz kimliği, bir saat lease ve yarı sürede yenile; (e) görünür port kaydı varsa yalnız aynı açıklama/iç istemci/port ile sahipliğini doğrula, diğerine dokunma; (f) kapatırken veya onay geri alınınca yalnız sahip olunan kayıtları sil; (g) router external IPv4 özel/CGNAT ise durumu belirt, `MAPPED` erişilebilirlik kanıtı değildir.
- **Korunacaklar:** swarm tek sahip aktör; ayrı sahipli async eşleme işçisi actor döngüsünü bloklamaz. M2 eşlenen endpoint'i yayınlamaz; adres değişikliği M3 işidir. Sır/davet/router yanıtları loglanmaz.
- **Elle (Windows+TP-Link):** UPnP açık/kapalı, zaten kullanılan port, yönlendirici yeniden başlatma, lease bitişi, uygulama kapanınca silinme.
- **Riskler:** router UPnP'yi yalan söyleyebilir ("başarılı" ama erişilemez) → durum `MAPPED`+`UNVERIFIED`; çift NAT/CGNAT'ta IGD yalnız ilk cihazı eşler; bazı cihazlar sabit lease süresi istemez (0 → sonsuz) — sonlu lease zorunlu, reddedilirse yeniden dene.

### M3 — Dinamik dış adres + PCP (PCP ertelendi)

- **Durum:** dinamik adres bölümü uygulandı; PCP ertelendi.

- **Amaç:** Eşleme sonucunu çalışan swarm'a yansıtmak; UPnP başarısız/uygunsuzsa ayrı PCP yolu sonraki iş olarak değerlendirilmişti.
- **Dosyalar (uygulanan dinamik adres bölümü):** `crates/network/src/handle.rs` (`Command`), `actor.rs`, `discovery.rs`, `portmap.rs`, Desktop connection setup.
- **Dinamik adres:** `Command::SetExternalAddresses(Vec<Multiaddr>)`; aktör yalnız Gateway kaynaklı eski/yeni farkını `add_external_address`/`remove_external_address` ile uygular. Başlangıç MANUAL adresleri ve izinli altyapı eşlerinden alınan PEER_OBSERVED adresler kaynak etiketleriyle korunur. Yalnız UPnP protokolü `MAPPED` ve router external IPv4'ü PUBLIC ise ilgili gerçek TCP/QUIC port adresi eklenir; yenileme başarısızlığı, rıza iptali ve kapanışta kaldırılır. Ham dış IP status/log'a yazılmaz. PCP gözlem satırı ayrı kalır ve `NOT_ATTEMPTED` gösterir.
- **PCP ertelendi:** Kullanıcının M3 karar eşiği, elle uygulamanın yaklaşık 300 satırı aşmaması ve RFC yorumunda belirsizlik kalmamasıydı. Salt okunur keşif, tam request/response ayrıştırma, TCP/UDP nonce sahipliği, lease yenileme/silme ve worker yaşam döngüsünün toplamını yaklaşık 280–380 satır tahmin etti; kullanıcı eşiğine göre bu uygulama “kolay” değil. Yeni crate eklenmedi. Resmi [RFC 6887](https://www.rfc-editor.org/rfc/rfc6887) MAP paket alanları, yanıt doğrulaması, hata kodları ve lifetime/silme kuralları için esas alındı; PCP kodu yazılmadı. PCP `NOT_ATTEMPTED` kalır ve hiçbir PCP isteği gönderilmez. NAT-PMP uygulanmaz; eski NAT-PMP uyumluluğunu eklemek bu ertelenen PCP uygulamasının yerine geçmez.
- **Korunacaklar:** Mevcut `external_addresses` başlangıç davranışı.
- **Elle:** Windows/router ile UPnP public endpoint ekleme/çekme ve başlangıç MANUAL adresinin korunmasını doğrula. PCP bu kilometre taşında uygulanmadığı için destekli/desteksiz gateway senaryoları `NOT_ATTEMPTED` olarak kalır; PCP wire davranışı doğrulanmadı.
- **Risk:** PCP pek çok ev router'ında yoktur; beklenti "UPnP veya hiçbiri" olabilir.

### M4 — Başlangıç/ağ değişimi yeniden değerlendirmesi

- **Durum (2026-10-12):** uygulandı; statik kontroller geçti, gerçek Windows ağ geçişi/router davranışı **unverified**.
- **Amaç:** Açılışta ve ağ değişiminde doğrudan denemelerin hızlanması, lease yenilemesi.
- **Dosyalar:** `crates/core/src/peer.rs` ayrı on saniyelik monitor; `network_observation.rs` zaten mevcut senkron IP Helper okuması `spawn_blocking` içinden çağrılır; `discovery.rs` (`network_changed()`), `actor.rs` (yalnız sayaç sıfırlama), `portmap.rs` watch hedef supervisor'ı ve lease yenilemesi.
- **Görevler:** sıralanmış/tekrarsız IPv4 arayüz-adres ve varsayılan-gateway imzası değişince Desktop gözlemi güncellenir, `direct_attempts` sıfırlanır ve eşleme hedefi yeniden değerlendirilir. İzleyici ayrıca dinleme adres/port değişimini fark edip mapping hedefini yeniden kurar. Lease yenilemesinde router dış IP'si tekrar okunur; başarısız protokol `EXPIRED` ve gerekçesiyle gösterilir, Gateway kaynaklı dış adres listesi güncellenir/boşaltılır. İzleme yalnız uzun ömürlü mapping-enabled PeerRuntime'larda başlar; one-shot CLI komutları etkilemez.
- **M1/M4 sınırı:** router observation/UPnP mapping worker'ın önceden var olan, açık onaylı M2 işlemi dışında ağ değişikliği izleyicisi router'a, firewall'a veya Windows Firewall COM'a istek yapmaz. M4 mapping hedef değişiminde worker bounded sahiplik kontrolü/kaldırma ve gateway adreslerini geri çekme sonrasında yenilenir; onay bayrağı değiştirilmez.
- **Kontrol:** repository'de statik check kayıtları `docs/IMPLEMENTATION.md` M4 girdisinde; Windows Wi-Fi/VPN geçişi, gerçek router WAN değişimi ve gerçek inbound bağlantı **unverified**.
- **Korunacaklar:** altı saniyelik tick, QUIC tercihi, TCP fallback, doğrudan > relayed, `connection_limits`.
- **Elle:** Wi-Fi değiştirme, modem yeniden başlatma, VPN aç/kapa.

### M5 — Güvenli endpoint güncellemesi (JSON'suz yeniden bağlanma)

- **Amaç:** IP/port değişince güvenilir peer'in yeni adresini imzalı, süreli, sıralı kayıtla duyurmak.
- **Durum (2026-10-13):** M5a/b/c ve M5d Supabase sunucu/istemci uygulanmıştır. Canlı host/Sandbox kaydında üyelik ve endpoint list kabulü vardır; gerçek IP değişimi ve ayrı ağ transferi açık. #42 creator revoke, imported köken işareti ve sıralı grant/revoke retry ekler.
- **Dosyalar:** `shared/protocol` (endpoint tipi ve yeni, proje kapsamlı RPC varyantları; mevcut varyantlar ve `trackmesh-endpoint-v1` imza biçimi değişmez), `crates/crypto` (imza), `crates/storage` (M5b additive schema v3), `crates/core/src/endpoints.rs` (ortak kabul/yayın), `crates/network` (actor RPC ve süreli adaylar). Mailbox desteği M5d kapsamıdır.
- **Tasarım:**
  - Kayıt: `{device_id, peer_id, addresses[≤16], issued_at, expires_at (kısa), seq (monotonik u64)}` cihaz anahtarıyla imzalı.
  - Alıcı: cihaz projede güvenilir mi (`trusted_author`), `peer_id`/anahtar bağı, `seq > saklanan`, süre sınırı, adres biçimi/özel-aralık doğrulaması; geçerse `learned` adres olarak eklenir. **Kart ve yetki yeniden yazılmaz** (böylece `Store::trust` üzerine-yazma gerilemesi yaşanmaz).
  - Kanal öncelikleri: (1) M5c'de var olan doğrudan bağlantı üzerinden proje kapsamlı endpoint RPC (yeni bağlantı kurulmaz; relay reddedilir), (2) M5d'de mailbox'ta şifreli kayıt (proje anahtarıyla AEAD, mevcut AAD biçimine dokunmadan ayrı tür), (3) kısa insan-okur kod için süreli çözüm sunucusu — yalnız tasarım notu.
  - Sunucu ağ trafiği veya proje dosyası relay etmez; yalnız şifreli küçük kayıt (<64 KiB sınırı).
- **Korunacaklar:** İmzalı kimlik, proje yetkisi, davet ve pairing akışı; sunucuda çözülmüş manifest/anahtar yok.
- **Riskler:** mailbox yoksa iki taraf da aynı anda IP değiştirirse hiçbir kanal kalmaz → UI "adres bilinmiyor, davet yenile" gösterir; replay → `seq`+`expires_at`; uzun URL daveti ID paylaşımıyla yetki vermez (davet hâlâ sahiplik kanıtı ister).

### M6 — Kullanıcı dostu ilk eşleştirme (JSON'suz UX)

- **Durum (2026-10-13):** bağlantı/URI/native clipboard ve şifreli tek okuma uygulanmıştır. Host/Windows Sandbox consume/onay/ilk aktarım kaydı vardır; expiry, ikinci tüketim ve clipboard cleanup doğrulaması açık.
- **Akış:** Link folder → Invite collaborator → bağlantıyı aç/yapıştır → Accept → otomatik cihaz isteği → açık Approve. İlk Invite/Accept süreli UPnP onayını sorar.
- **Bağlantı:** `trackmesh://invite/v2/<ref>/<publishable_key>/<drop-id>#<key>`. Servis referansı/publishable key/drop kimliği URI'de, bağımsız çözme anahtarı fragment'tedir. Supabase'deki drop 15 dakika geçerli, bir kez okunabilen şifreli ProjectInvite içerir; plaintext JSON ağda veya logda dolaşmaz.
- **Korunacaklar:** çözme/import native Rust'tadır; payload/anahtar React ve ajan bağlamına girmez. Link yalnız açık kullanıcı eylemiyle panoya konur. Dosya daveti Advanced, manuel kart legacy yoludur; CLI link tüketmez.
- **Onay:** beklenen public kimlik doğrulanır. İlk snapshot yoksa ve inviter klasöründe dosyalar varsa Approve ilk snapshot'ı oluşturur; Supabase membership grant'i kuyruğa alınır.
- **Sonraki tasarım:** QR veya insan-okur kısa kod arayüzü uygulanmamıştır. V2 drop ID zaten vardır; yeni kısa-kod özelliğinin kanıtı değildir.


### M7 — Otomatik bağlantı ve snapshot durumu

- **Durum:** açılış yenilemesi, online peer/snapshot durumları ve offline head hint'leri uygulanmıştır. Head Function deployment'ı ve yeni ayrı-ağ akışı doğrulanmadı.
- **Akış:** bilinen adreslerden yeniden bağlantı denenir. Eşitlik bağlı peer'in ACK'inden gelir; metadata tek başına eşitlik veya dosya mevcudiyeti değildir.
- **İlk alma kararı:** Desktop yalnız boş, hiç sürüm almamış, recovery/transfer bulunmayan hedefe onay sonrası ilk sürümü otomatik alır. Bu genel arka plan ön-indirme veya sürekli sync değildir.
- **Sonraki alma:** Get collaborator's version ve DAW-kapalı onayı gerekir. Divergent klasörde açık Use …'s version eski yerel değişiklikleri snapshot'lar ve dosyaları backup'ta korur; Keep mine yerel sürümü paylaşır. Otomatik merge veya sessiz overwrite yoktur.
- **Share:** Share current version yerel snapshot ve paylaşılabilir sürümü hazırlar; alıcının teslim sonucu ayrı gösterilir. Dosyalar için yetkili kaynak online olmalıdır.


### M8 — Gerçek erişilebilirlik kanıtı ve Windows Firewall durumu

- **Durum:** salt-okunur Firewall ve yetkili inbound olay gözlemi uygulanmıştır; gerçek farklı-ağ erişim kanıtı hâlâ açık.
- **Uygulama:** INetFwPolicy2/INetFwRule COM işlemleri blocking worker'dadır. Etkin profil, aday TCP/UDP kuralları ve üçüncü taraf ürün kaydı gösterilir; kural veya profil değiştirilmez.
- **Sınır:** public remote adresli yetkili doğrudan istek olayına rağmen Reachability UNVERIFIED kalır. Adres verisi ayrı-ağ provenance'ını kesinleştirmez; hairpin/aynı LAN yanlış doğrulanmış sayılmaz. Ham uzak IP status/log'a girmez.
- **Elle:** gerçek Windows policy/çoklu profil/COM hata davranışı ve ayrı-ağ inbound denemesi yapılmalıdır. Statik derleme bu runtime davranışlarını kanıtlamaz.


### M9 — DAW Save → snapshot (doğrulama, kod az)

- `SAVE_OBSERVED` dışında izleme yok; `scan_after_save` korunur. Yalnız: aynı içerik için gereksiz snapshot üretilmediğini (`snapshots.rs` unchanged-content reuse) ve yayın yeniden denemesini **incele ve belgele**; boşluk bulunursa ayrı küçük iş.
- **M9 review sonucu (2026-10-12):** Kod incelemesinde aynı manifest dosyaları için parent snapshot'ın yeniden kullanıldığı, başarısız metadata publish'in PENDING kaldığı ve uzun ömürlü mailbox session'larında 30 saniyelik interval tick'lerinde yeniden denendiği doğrulandı (ağ işlemleri bir turu uzatabilir); sunucu aynı snapshot ID + aynı envelope için idempotent ACK döndürür. Eşit snapshot sırasında daha önceki PENDING kaydın yayını aynı gönderimde denenebilir. SAVE_OBSERVED dışı watcher yok. Kanıt ve sınırlar [IMPLEMENTATION.md](../IMPLEMENTATION.md) içindeki M9 review kaydındadır; runtime/DAW doğrulaması yapılmadı, boşluk bulunmadı.

### M10 — İki cihazla manuel kabul (kod yok)


**Açık:** ayrı fiziksel internet ağlarında relay olmadan transferin kabulü tamamlanmadı. Kayıtlı host/Windows Sandbox denemeleri aynı fiziksel host/NAT üzerindedir. Gerçek DAW, WAN/IP değişimi, CGNAT, revoke/üç cihaz, expiry ve eksik head Function kontrolleri son manuel denemede yapılır. Sonuçlar yalnız IMPLEMENTATION.md'ye gerçekten çalıştırılan kapsamla yazılır.


## 6. Manuel doğrulama matrisi (Windows, iki PC)

| # | Senaryo | Beklenen |
| --- | --- | --- |
| 1 | Aynı LAN, UPnP kapalı | Doğrudan QUIC/TCP, Reachability `UNVERIFIED` (LAN erişimi ayrı ağ kanıtı değildir); eşleme `NOT_ATTEMPTED` |
| 2 | Farklı ev/ISS; A genel IPv4 + UPnP onaylı, B yalnız çıkış | B→A doğrudan aktarım denenir; inbound gözlemi varsa da Reachability `UNVERIFIED` kalır |
| 3 | UPnP kapalı | `UNSUPPORTED/DENIED`, net gerekçe, transfer garanti edilmez |
| 4 | PCP destekli/desteksiz gateway | PCP ertelenene kadar her ikisinde `NOT_ATTEMPTED`; PCP paketi gönderilmez |
| 5 | Port başka uygulamada | Çakışma `DENIED/FAILED`, başkasının mapping'i dokunulmaz |
| 6 | Windows Firewall engeli / farklı profil | Eşleme `MAPPED`, erişilebilirlik `UNVERIFIED`; rapor profili gösterir |
| 7 | Dış IP değişimi, modem yeniden başlama | Yeni imzalı endpoint, JSON gerekmeden yeniden bağlanma |
| 8 | Uygulamayı yeniden başlat | İlk davet sonrası tekrar davet istenmez |
| 9 | Yanlış kimlik / eski `seq` | Reddedilir |
| 10 | DAW Save, yarım/kilitli dosya | Snapshot yalnız settle sonrası; yayın offline retry |
| 11 | Büyük aktarım kesintisi | Resume + BLAKE3 doğrulama |
| 12 | Başarısız doğrudan bağlantı | UI gerekçeyi gösterir; relay yok, aktarım garanti edilmez |

## 7. Başarı koşulları

En az bir cihaz erişilebilirken **relay olmadan** doğrulanmış doğrudan transfer; kullanıcı her açılışta IP yazmaz/JSON göndermez; UPnP "garantili" sunulmaz; PCP ertelenmiştir; bulutta ses/çözülmüş manifest/sır yok; başarısızlık nedeni görünür.

## 8. Açık kararlar

1. **Karar verildi (M0/M3):** UPnP için `igd-next` kullanılıyor. M0'da PCP crate'i araştırılmış olsa da M3'te kullanıcı eşiği nedeniyle PCP ertelendi; `crab_nat` bağımlılığı eklenmedi ve PCP durumu `NOT_ATTEMPTED`. Windows ağ ve firewall API seçimleri §3'te; runtime/router uyumluluğu **[V]**.
2. **M2/M6/#42 ayar kararı:** kullanıcı rızası, backend ve grant/revoke kuyruğu TrackMesh veri dizininde sürümlü settings.json ve atomic replace ile tutulur. Güncel sürüm 5, v1–v4 yükleme yolu korunur. SQLite değerleri bu ayarlar için değiştirilmez.
3. **M5 kararı:** Supabase endpoint publish/list ayrı kayıt ve fonksiyon kanalıdır; foundation snapshot Envelope biçimi/AAD'si değişmez.
4. Sürekli kısa kod çözücü sunucu (M6) — Aşama 1'de uygulanmaz, yalnız tasarlanır.
5. **M4 kararı:** Windows bildirim API'si veya yeni crate/Windows feature'ı eklenmedi. PeerRuntime'da 10 saniyelik iptal edilebilir izleyici, blocking IP Helper worker'ı ve sıralı watch hedefi kullanılır; actor'daki mevcut altı saniyelik tick değişmez. NetworkChanged yalnız `direct_attempts` sayaçlarını temizler. `SetExternalAddresses` mevcut kaynak ayrımıyla kullanılır; router/WAN ve Windows çalışma davranışı **[V]** kalır.

## 9. Codex yürütme protokolü (alt-ajan destekli)

**Bu bölüm bir operatör istemidir; ilerleme kanıtı değildir.** Codex'in alt-ajan (sub-agent) desteği olup olmadığı bu planı yazarken doğrulanmadı `[V]`. Önce Codex'e sorulur; destek yoksa aynı iş sırayla, tek ajanla yürütülür. Model adları kullanıcının Codex hesabına bağlıdır. Codex bildirdi (ajan başlatılmadan): `gpt-6.1-sol` (`max`) güçlü işler ve bağımsız inceleme, `gpt-6-luna` (`medium`) hafif uygulama için seçildi; `gpt-6-astra` kullanılmaz, `ultra` gerekmez. Model gücü sürüm numarasından kesin sıralanamadı; seçici açıklaması esas alınır.

### 9.1 Başlatma sorusu (önce bunu yapıştır)

> Bu repoda `AGENTS.md` ve `docs/plans/phase1-direct-p2p.md` dosyalarını oku. Kod yazma. Önce şunu yanıtla: (1) Bu oturumda alt-ajan başlatabiliyor musun, bunları hangi modelle ve hangi akıl yürütme seviyesiyle çalıştırabiliyorsun? (2) Alt-ajanlar aynı çalışma ağacını mı paylaşıyor yoksa ayrı worktree mi alıyor? (3) Alt-ajan çıktısını nasıl geri alıyorsun? Varsayma; yalnızca gerçekten yapabildiğini söyle. Yapamıyorsan "alt-ajan yok, sıralı yürütürüm" de.

Yanıta göre 9.2 (destek var) veya 9.3 (yok) kullanılır.

### 9.2 Alt-ajan destekli yürütme istemi

> `AGENTS.md`, ilgili crate-local `AGENTS.md` dosyaları ve `docs/plans/phase1-direct-p2p.md` dosyalarını oku. Sen orkestratörsün; **en yüksek akıl yürütme seviyesinde** çalış. Planı **tek seferde değil, bir kilometre taşı (M) bir seferde** uygula; sıradaki M'yi bitirip raporlamadan sonrakine geçme. Her M için:
>
> 1. **Dal:** `AGENTS.md` kuralı: yerel `codex/phase-N-description` dalı; doğrulama sonrası `master`'a `--no-ff` yerel merge; force push yok; faz dalları yerel kalır.
> 2. **Keşif alt-ajanları (salt-okunur, paralel olabilir):** ilgili dosyaları/`[V]` maddelerini ayrı alt-ajanlara dağıt (ör. kütüphane Windows desteği, mevcut çağrı yolları). Alt-ajan **dosya yazmaz**; bulgu + dosya:satır döndürür.
> 3. **Uygulama:** değişikliği **tek yazıcı** yapar (sen veya tek bir `gpt-6.1-sol` (`max`) alt-ajan). Aynı dosyalara paralel yazan iki ajan olmasın. M1, M6 ve M7 aynı `App.tsx`/`runtime.rs` dosyalarına dokunur; bunları paralel yürütme.
> 4. **Bağımsız inceleme:** uygulayıcıdan farklı bir alt-ajan (`gpt-6.1-sol` (`max`), yüksek akıl yürütme), diff'i `AGENTS.md` değişmezlerine karşı eleştirel okur: imzalı kimlik, `authorized`, gate penceresi, snapshot/transfer kilit ve journal sırası, SQLite, sır/davet loglama, `SAVE_OBSERVED` dışı izleme yok, otomatik test/benchmark eklenmedi. Bulguları düzelt, sonra tekrar incelet.
> 5. **Kontroller (yalnız statik):** `cargo fmt --all -- --check`, `cargo check … --locked --offline`, `cargo clippy … -D warnings`, `npm run typecheck && npm run build`. **Test veya benchmark ekleme/çalıştırma yok.** Çalışmayan veya çalıştıramadığın kontrolü "geçti" diye yazma.
> 6. **Kayıt:** `docs/IMPLEMENTATION.md`'ye yalnız gerçekten yaptığın kontrolleri `verified/unverified` ile ekle. `docs/CODEMAP.md`'yi modül değiştiyse güncelle. Rakip durum belgesi oluşturma.
> 7. **Rapor:** görev, değişen dosyalar, korunan değişmezler, çalışan kontroller, **Windows/router olmadan doğrulanamayanlar**, benim elle yapacağım adımlar, kalan riskler, sonraki somut adım.
>
> **Dur ve sor** (varsayma): şema/migration gerekiyorsa (M2, M5), protokol/imza biçimi değişiyorsa, bağımlılık eklenecekse, beklenmedik şekilde ağ davranışı doğrulanamıyorsa. Ağ/NAT/Firewall davranışını doğrulanmış gibi sunma.

Model/seviye eşlemesi (öneri): M0, M2, M3, M4, M5, M8 ve **bağımsız inceleme** → `gpt-6.1-sol` (`max`), en yüksek akıl yürütme. M1, M6, M7, M9 uygulaması → `gpt-6-luna` (`medium`) olabilir, ama **inceleme yine `gpt-6.1-sol` (`max`)**.

Sıra ve bağımlılık: M0 → M1 → M2 → M3 → M4 → M8; M5 M1'den sonra başlayabilir; M6 ve M7 M1'den sonra, ancak birbirleriyle sıralı. M9/M10 sonda.

### 9.3 Alt-ajan yoksa

Aynı 9.2 adımları tek ajanla sırayla uygulanır: önce keşif, sonra uygulama, sonra **ayrı bir oturumda** (yeni sohbet, `gpt-6.1-sol` (`max`)) diff incelemesi. Aynı bağlamın kendi işini onaylaması bağımsız inceleme sayılmaz.
