# TrackMesh: iki bilgisayarda kurulum ve ilk deneme

Collaborator projede çalışırken ana kart ve VST3 durum alanı bir uyarı gösterir; iki taraf da çalışıyorsa kaydedilen değişikliklerin sürümleri ayırabileceğini belirtir. Plugin oturumu ana sinyaldir; plugin yokken sınırlı, dakikalık dosya-tarihi kontrolü yaklaşık bir uyarı sağlar ve yanlış/gecikmiş uyarılar mümkündür. Share current version önce onay ister; otomatik DAW paylaşımı onay istemez. Get engellenmez, collaborator'ın yakında yeni sürüm paylaşabileceği belirtilir. Offline veya presence desteklemeyen eski cihaz için uyarı gösterilmez. Plugin'in başlattığı TrackMesh, DAW kapandıktan sonra da Windows tepsisinde kalır; ilk kez aynı tek-seferlik açıklamayı gösterir. Tepsiden açın; çıkmak için Quit TrackMesh kullanın. Açık kaldığı sürece collaborator Get yapabilir.

Bu rehber Windows Desktop'ın güncel bağlantı akışını anlatır. Henüz güncel bir deneme paketi yoktur; gerektiğinde incelenmiş `master` üzerinden yeni paket hazırlanır. Eski paketler bağlantı akışını içermez. İki cihaz aynı güncel build'i kullansın. Hazır executable için Rust/Node gerekmez; Desktop WebView2 ister.

Küçük bir proje kopyası ve boş alıcı klasörü kullanın. VST3 ilk aktarım için gerekli değildir. Bu sayfa yönergedir; gerçek kontrollerin sonuçları yalnızca [IMPLEMENTATION.md](../IMPLEMENTATION.md) içindedir.

## Rolünü seç

| Rol | Rehber | İlk işi |
| --- | --- | --- |
| Host / A / ilk gönderen | [host/README.md](host/README.md) | Klasörü bağlar, davet bağlantısını paylaşır ve cihazı onaylar. |
| Peer / B / ilk alıcı | [peer/README.md](peer/README.md) | Bağlantıyı açar, oluşturulacak klasörün üst dizinini kontrol eder ve onayı bekler. |
| Paketle çalışan terminal ajanı | [peer/CLAUDE.md](peer/CLAUDE.md) | Advanced CLI dosya daveti, `join`, `status`, `request-pairing`, `check` ve `receive` kullanır. |

Host merkezi sunucu veya kalıcı master değildir. İlk bağlantıyı B, A'ya başlatır. Kurulu bağlantı iki yönde kullanılabilir. Bağlantı kesildikten sonra A'nın B'ye bağımsız ulaşması için B'nin erişilebilir yolu da gerekir.

## Güncel Desktop akışı

1. A **Add project** seçer, proje adı ve klasörü girer ve ekler.
2. A proje kartında **Invite collaborator** seçer. İsteğe bağlı davet servisi için kullanıcının kendi Supabase projesi ayarlanmış olmalıdır. A bunu **Advanced → This PC → Default invite service** bölümünde yapılandırabilir; proje ayarı **Advanced → (proje) → Invite service** altındadır.
3. İlk Invite veya Accept işlemi **Router setup** onayını sorar. **Allow (recommended)** süreli UPnP eşlemesine izin verir; **Not now** eşleme yapmaz. Tercih sonradan **Settings → Connection → Set up my router automatically (UPnP)** ile değişir. Eşleme başarısı internet erişimi kanıtı değildir.
4. A bağlantıyı özel kanaldan B'ye gönderir. V2 bağlantı 15 dakika geçerlidir ve bir kez tüketilir. Servis referansı, publishable key ve drop kimliğini taşır; ayrı çözme anahtarı fragment içindedir. Davet, Supabase'de küçük bir şifreli drop olarak tutulur.
5. B bağlantıyı Windows'ta açar veya **Paste invite link** kullanır. **Invite link** penceresinde oluşturulacak proje klasörünün üst dizinini kontrol edip **Accept** der. **Choose another folder** üst dizini değiştirir; native uygulama burada proje adına göre boş bir alt klasör oluşturur. Yeni v2 bağlantı için URL/key girmesi gerekmez. Davet içeriğini veya bağlantıyı ajan bağlamına yapıştırmayın.
6. B'nin imzalı cihaz isteği otomatik gider. A, proje kartındaki **Someone opened your invite link** kutusundaki public Device/Identity değerlerini B ile karşılaştırıp **Approve** seçer; beklenmeyen isteği **Reject** eder. Yalnızca isim yeterli değildir.
7. A'nın klasöründe dosyalar ve henüz bir sürüm yoksa onay ilk snapshot'ı oluşturur. B'nin boş, daha önce sürüm almamış klasörüne ilk sürüm otomatik alınır. İki uygulama da açık kalsın; işlem bitene kadar DAW'da hedefi açmayın.
8. Sonraki değişikliklerde kaynak **Share current version** seçer. Alıcı DAW projesini kapatır ve **Get <isim>'s version** seçer; TrackMesh bir şey indirmeden önce tek pencerede eklenen, değişen ve silinen dosyaları gösterir ve projenin DAW'da kapalı olmasını ister. **Advanced → Connection → Check connection** güncel bağlantı ve karşılaştırmayı kontrol eder.

Arayüz dili **Settings → Language** ile İngilizce veya Türkçe seçilir; ilk açılışta Windows dili Türkçe ise Türkçe gelir. Bu rehberdeki düğme adları İngilizce arayüzü anlatır (örneğin Türkçe arayüzde **Share current version** = **Güncel sürümü paylaş**, **Get <isim>'s version** = **<isim>'in sürümünü al**).

Davet penceresi 15 dakikadır. Onay geç kaldıysa A **Open approval window**, B **Ask for approval** kullanır. Aynı projede pencereyi yeniden açmak bekleyen istekleri korur; başka proje için açmak önceki onaysız istekleri temizler. Pencere kapanışı, süre sonu veya restart onaysız istekleri kalıcı bir kuyruğa yazmaz. Onaylı cihaz her açılışta tekrar onay istemez.

Projeyi oluşturan cihaz, başka bir projede zaten birlikte çalıştığı bir cihazı bağlantı göndermeden ekleyebilir: kartta **Add <isim>**. Teklif doğrudan o cihaza gider; orada Projects sayfasının üstünde **Accept** / **Decline** çıkar. Accept projeyi Projects konumuna ekler, eşleşme ayrıca onay istemeden tamamlanır ve ilk sürüm kendiliğinden gelir. İki uygulama da açık olmalıdır; teklif bir saat geçerlidir ve uygulama kapanınca düşer. Proje başına üç cihaz sınırı geçerlidir.

Oluşturan cihaz bir üyeyi çıkarırsa, davet servisine ulaşabilen üyenin kartı bunu söyler (**Removed**); Share ve Get kapanır, dosyalar klasörde kalır. Projeden ayrılan ya da çıkarılan cihaz, oluşturan cihazın yeni daveti veya **Add <isim>** teklifiyle geri eklenebilir; proje eski klasörüne döner.

Divergence varsa **Use <name>'s version** açık onayla karşı tarafın sürümünü kurar; önce, paylaşılmamış yerel değişikliklerin karşı tarafın da alabileceği yeni bir sürüm olarak kaydedileceği sorulur, sonra snapshot alınır ve önceki dosyalar backup'ta korunur. **Keep mine** yerel sürümü paylaşır. Otomatik birleştirme yoktur.

Get'in indireceği yeni veri 1 GB'ı (1.000.000.000 byte) aşarsa indirme başlamadan onay istenir; doğrulanmış yerel parçalar bu miktara dahil değildir. İptal edilmiş Get aynı snapshot ile Resume getting üzerinden sürdürülebilir. Uygulama eski sürümlerin chunk verisini otomatik temizler; senin ve collaborator'ların son yerel sürümleri, yarım Get/recovery için gereken parçalar ve imzalı geçmiş korunur. `.trackmesh-…` yedekleri yalnız **Settings → Storage** içinden, boyutları görülüp topluca seçilerek silinir; her projenin son yedeği ve bitmemiş recovery verisi korunur.

## VST3 plugin kullanımı

TrackMesh VST3'ü DAW'da herhangi bir track'e ekleyip TrackMesh'te bağlanmış projeyi listeden seçin; davetle gelen proje de aynı listede görünür. Seçimi DAW projesine kaydedin. Daha önce kaydedilmiş UUID zaten bağlıysa plugin doğrudan açılır; yanlış seçimi **Change project** ile değiştirin. DAW standart state-dirty bildirimini desteklemiyorsa seçim değiştirilmez ve açıklama gösterilir. **Share current version** kaydın oturmasını bekleyip Desktop ile aynı paylaşım akışını kullanır. **Get <isim>'s version…** yalnız TrackMesh'i açıp ilgili kartı gösterir: DAW projesini kaydedip kapatın, ardından TrackMesh'te Get seçin. Plugin otomatik indirme veya açık DAW projesine kurulum yapmaz. Plugin, TrackMesh penceresinde seçilen dili kullanır (Settings → Language); sağ üstte iş arkadaşının bağlı olup olmadığını, altında son sürümünün yerel saatini gösterir.

Plugin'de **Share when I save in the DAW** proje bazında varsayılan olarak kapalıdır; mevcut projelerde de DAW kaydı kendiliğinden sürüm paylaşmaz. Kapalıyken kaydettikten sonra **Share current version** seçin. Aynı ayar plugin checkbox'ından veya **Settings → Share when I save in the DAW** bölümünden değiştirilebilir. Açıkken her gözlenen DAW kaydı dosyaların oturması beklendikten sonra paylaşılır; bazı DAW'ların autosave işlemleri de buna dahildir. Eski agent bu ayarı desteklemiyorsa plugin checkbox'ı göstermez.

## Davet servisi ve erişim sınırları

Supabase yalnız küçük şifreli invite drop, endpoint kaydı ve head hint'i tutar. Dosyalar ve sample'lar servise yüklenmez. Proje anahtarı yalnız şifreli davetin içindedir; açık anahtar ve çözülmüş davet servise gönderilmez. Servis kaydı tek başına NAT engelini aşmaz ve offline kaynağın dosyalarını indirmez.

Kendi mailbox sunucusu desteği kaldırıldı; uygulama metadata yayınlamaz ve yayın bekleyen sürüm göstermez. Eski davetteki mailbox adresi yok sayılır.

**Approve** yerel yetkiyi verir ve üyelik grant'ini otomatik kuyruğa alır. Projeyi oluşturan cihazdaki **Remove access** (Advanced → Folder, ID and access), yerel erişimi kaldırır ve Supabase üyelik revoke'unu kuyruğa alır. İçe aktarılan projede başka cihazı servis üyeliğinden çıkarma isteği gönderilmez. Revoke anahtarı değiştirmez: çıkarılan cihaz daha önce aldığı dosyaları ve anahtarı elinde tutar. Yeni import'lar sahiplenme isteği göndermez; #42 öncesi import'ların kökeni geriye dönük tahmin edilmez.

Servis problemi ve eksik fonksiyon adları proje kartındaki **Advanced → (proje) → Invite service** altında görünür. URL olarak `https://<project-ref>.supabase.co` ve yeni biçimli `sb_publishable_...` kullanılır. Secret key veya veritabanı parolası istemciye verilmez. [Supabase kurulum belgesi](../../services/supabase/README.md) deployment adımlarını anlatır. Public release kişisel Supabase projesi içermemelidir; derleme varsayılanları yalnız özel build içindir.

## Panel, çevrimdışı bilgi ve ağ değişimi

`Collaborator` yerelde bilinen cihazı gösterir. Bu kayıt, bağlantı veya dosya teslimi değildir. **Waiting for … to approve this device…** onay beklenmesini gösterir. Seçili cihazın bağlantısı ve güncel **Advanced → Connection → Check connection** sonucu ayrı bilgilerdir. Offline head hint'i son bilinen sürümü gösterir; dosyalar için yetkili kaynak online olmalıdır.

Uygulamayı açmak davet almayan birine bildirim veya public kişi listesi oluşturmaz. Public rehber, browser deployment, offline pairing kuyruğu, kalıcı Windows arka plan servisi ve sürekli senkronizasyon yoktur. Relay ve hole punching (DCUtR) yoktur; bağlantı yalnız doğrudan TCP/QUIC ile kurulur. Relay istemci ve sunucu kodu `future/` altında, derlenmeden durur ve post-1.0'a ertelenmiştir. Eski davetteki relay adresi yok sayılır.

UPnP ve imzalı endpoint yayını, değişen erişilebilir adresleri güncellemeye çalışır. Gerçek WAN/ağ geçişi davranışı doğrulanmış sayılmaz. Elle yönlendirme kullanılıyorsa yeni public IPv4/dış portu Settings'te güncelleyin. Yeni ağ erişilemiyorsa önce aşağıdaki sırayı izleyin. Profil, proje UUID'si ve anahtarları sıfırlamayın. Dosya davetinin adresleri gerçekten eskidiyse güncel dosyayı aynı profil/proje/hedefe native import edin; sıradan timeout için yeniden import yapmayın.

Açık A–B bağlantısı iki yöndeki yetkili istekleri taşır. B kaynak olacaksa **Share current version** yapıp uygulamasını açık tutar; A sonraki sürümü Get ile alır. CLI network komutu tamamlanınca kendi bağlantısını kapatır. Açık Desktop veya `serve` oturumu olmadan o cihaz kalıcı online kaynak değildir.

## TrackMesh açılmıyorsa

Desktop, Microsoft Edge WebView2 Runtime ister. Güncel Windows 11'de ve güncellenmiş Windows 10'da zaten kuruludur. Eksikse kurulum dosyası indirmeye çalışır; bunun için internet gerekir. İndirme olmazsa (internetsiz bilgisayar, Windows Sandbox) pencere açılmaz ya da boş kalır. O zaman önce Microsoft'un sitesinden WebView2 **Evergreen Standalone Installer** kurulup TrackMesh yeniden açılır.

## Bağlantı kurulamazsa

Her seferinde tek koşulu değiştirip **Advanced → Connection → Check connection** ile tekrar deneyin. İlk uzak bağlantıda alıcı normal dışarı erişim kullanır; alıcıda port yönlendirmesi bu yönün önkoşulu değildir. En az bir cihazın erişilebilir yolu gerekir.

1. Doğru proje/cihaz ve aynı build kullanılıyor mu, A açık mı, cihaz onaylandı mı, paylaşılabilir sürüm var mı kontrol edin. Bekleyen onay için **Open approval window** / **Ask for approval** kullanın.
2. Aynı LAN'da guest Wi-Fi/client isolation erişimi kesebilir. Aynı normal ağda deneyin. İnternet port yönlendirmesi aynı LAN için gerekmez.
3. İlk onayda UPnP'ye izin verilmişse **Advanced → (proje) → Connection** satırlarını ve **Technical details** gözlemlerini kontrol edin. `MAPPED` yalnız router eşlemesi demektir; `UNVERIFIED` internet erişimi ispatı değildir.
4. Kurulum dosyası `trackmesh-desktop.exe` için `TrackMesh` adlı gelen bağlantı izin kuralını kendisi ekler. `wf.msc → Gelen Kuralları` içinde bu kuralın açık olduğunu kontrol edin; yoksa TrackMesh'i yeniden kurun. Kurulumsuz geliştirme sürümünde gönderende `Win + R → wf.msc → Gelen Kuralları → Yeni Kural → Bağlantı Noktası` ile uygulamanın yerel dinleme portuna ayrı TCP ve UDP izinleri verin. Aktif Private/Public ağ profilini kontrol edin: kural her profilde açıktır; Windows ağı Public sayıyorsa Desktop açılışta riskleri anlatan bir uyarı gösterip bu ağda devam edilsin mi diye sorar ([SECURITY.md](../SECURITY.md)). Ev ağınızsa Windows Ayarları'ndan Private yapın. Gözlem paneli kural oluşturmaz; firewall'ı tamamen kapatmayın.
5. UPnP uygun değilse router'da elle yönlendirme yapın. A'nın router'a bağlı LAN adresini panelden belirleyin ve DHCP rezervasyonu yapın. TCP `<external-port> → <PC-LAN-IP>:<local-port>` ve UDP için aynı eşlemeyi oluşturun; varsayılan yerel port 43101'dir. **Advanced → This PC → Manual port forwarding** içinde **Public IPv4** ve **External port** değerlerini **Save** ile kaydedin. Bu alanlar router kuralını kendileri oluşturmaz.
6. TP-Link arayüzünde modeline göre **Advanced → NAT Forwarding → Virtual Servers / Port Forwarding** bölümünü açın. TCP/UDP kurallarını doğru iç IP/portla etkinleştirip kaydedin. Başka kullanıcıya ait kuralı değiştirmeyin. Menü kilitliyse ISP/ağ yöneticisinden erişim isteyin. [Üretici rehberi](https://www.tp-link.com/uk/support/faq/785/) model farklarını açıklamak için kullanılabilir.
7. Dış port çakışıyorsa boş bir dış port seçin; yerel dinleme portu aynı kalabilir. UI'daki dış port router kuralıyla aynı olsun. Yeni public IP veya portu Advanced → This PC → Manual port forwarding içinde güncelleyin.
8. Router WAN adresi özel/CGNAT aralığındaysa yerel kural upstream NAT'ı açmaz. ISP'den erişilebilir public IPv4 veya erişilebilir başka ağ isteyin. Supabase bu engeli çözmez; transfer garantisi yoktur.

| Belirti | Sonraki somut adım |
| --- | --- |
| Invite kullanılamıyor / eksik fonksiyon | Kartta çıkan uyarıdaki düğmeyle **Advanced → Invite service** bölümüne gidin; **Advanced → This PC → Default invite service** ayarını da kontrol edin; deployment için servis belgesine bakın. |
| Link kullanılmış veya süresi dolmuş | A yeni bağlantı oluştursun. Decrypt/import edilmiş projeyi sıradan retry için silmeyin. |
| Onay penceresi kapalı / onay bekleniyor | A pencereyi yeniden açsın, B Ask for approval seçsin ve kimlik karşılaştırmasını tamamlasın. |
| Kaynak offline / sürüm yok | Kaynak uygulamayı açıp sürümü paylaşsın; hint dosya teslimi değildir. |
| Yerel değişiklik / divergence | Açık Use …'s version veya Keep mine kararını verin; mevcut dosyaları silmeyin. |
| Aktarım PAUSED | Aynı hedef/snapshot için Resume getting kullanın. |
| RECOVERY_REQUIRED | DAW kapalıyken kartta **Recover** kullanın; journal, staging ve backup'ı koruyun. |
| Aynı profilde başka owner açık | İşin/kurulumun güvenli bitmesini bekleyin; apply sırasında process'i öldürmeyin. |

## Advanced ve legacy yollar

Davet servisi kullanılamıyorsa **Advanced → Manual invitation (without invite service)** içinde **Export invitation with current addresses** kullanılabilir. Public adresli dosya için aynı bölümde **Save invitation file with public addresses** vardır. Bunlar mevcut UI etiketleridir. Dosya daveti ayrı özel kanaldan paylaşılır; native import da otomatik istek ve açık onay gerektirir.

CLI'nin davet bağlantısı tüketme akışı yoktur. Dosya daveti ile `join`, güvenli `status` ve `request-pairing` kullanır; ardından `check`/`receive` yapılır. [Paket ajan şablonu](peer/CLAUDE.md) komutları ve tek-owner kuralını anlatır. Manuel imzalı kart alışverişi **Legacy file pairing** yoludur.

## Denemeyi bitirme ve paket bakımı

Aktarım/kurulum bitince dosyaları ve fresh Check connection sonucunu kontrol edin; DAW'da açılış/sample kontrolü ayrı manuel iştir. Uygulama kapanırken kabul edilmiş işlerin bitmesini bekleyin. Recovery verisini temizlik olarak silmeyin.

Pencerenin **X** düğmesi uygulamayı Windows sistem tepsisine gizler; ilk seferinde açıklama gösterilir. TrackMesh tepside çalışırken mevcut bağlantı ve sürüm sunumu devam eder. Tepsi ikonuna tıklamak veya **Open TrackMesh** pencereyi geri getirir. Penceredeki ve tepsideki **Quit TrackMesh**, hangi collaborator'ların sürümlerini alamayacağını göstererek onay ister; onaydan sonra kabul edilmiş işler tamamlanıp uygulama çıkar. Bilgisayarın uyuması veya ağın kesilmesi tepside çalışırken de erişimi durdurabilir.

Sorun bildirirken build/commit, Windows sürümü, aynı LAN mı ayrı ağ mı, son tamamlanan adım ve güvenli hata metni yeterlidir. Davet, anahtar, `secrets`, veritabanı veya kullanıcı proje dosyası göndermeyin.

Paket kökündeki `CLAUDE.md`, [peer/CLAUDE.md](peer/CLAUDE.md) şablonundan hazırlanır; repo geliştirme talimatı pakete kopyalanmaz. Aynı build'in EXE/DLL'leri, taşınabilir rehberleri, public build metadata ve checksum'ları eklenir. Özel davet paketten ayrı kalır. Daha önce gönderilmiş ZIP sessizce değiştirilmez. Build adımları [DEVELOPMENT.md](../DEVELOPMENT.md) içindedir.
