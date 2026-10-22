# Host bilgisayarı: ilk gönderen A

Host ilk aktarımın kaynağıdır. Ortak sıra ve bağlantı sorunları [kurulum girişinde](../README.md) tutulur. İki tarafta aynı build'in `TrackMesh_<sürüm>_x64-setup.exe` kurulum dosyasını kullanın; uygulama ve VST3 plugin birlikte kurulur. Kurulum dosyası [DEVELOPMENT.md](../../DEVELOPMENT.md) adımlarıyla incelenmiş master'dan hazırlanır; kurmak için Rust/Node gerekmez.

## Klasörü bağla ve bağlantıyı paylaş

1. Başlat menüsünden TrackMesh'i açın. Küçük proje kopyasını **Add project** ile seçip ekleyin. Yerel veri profili ve DAW klasörü ayrı olsun.
2. Kendi Supabase projenizin URL ve publishable key'ini **Advanced → This PC → Default invite service** içinde ayarlayın. Secret key veya DB parolası girilmez. Gerekirse **Advanced → (proje) → Invite service** ayarlarını kontrol edin.
3. Proje kartında **Invite collaborator** seçin. İlk kullanımda **Router setup** sorusu gelir; **Allow (recommended)** süreli UPnP eşlemesine izin verir, **Not now** izin vermez.
4. Oluşan v2 bağlantıyı B'ye özel kanaldan gönderin. 15 dakika geçerlidir ve bir kez tüketilir. B yeni bağlantı için URL/key yazmaz. Bağlantıyı, fragment anahtarını veya davet içeriğini ajan/log bağlamına vermeyin.
5. B bağlantıyı kabul edince proje kartındaki **Someone opened your invite link** kutusunda isteği görün. Public Device/Identity değerlerini B ile karşılaştırıp **Approve** seçin. Beklenmeyen isteği **Reject** edin.

Onay yerel erişimi verir ve Supabase üyelik grant'ini kuyruğa alır. Klasörde dosyalar ve henüz snapshot yoksa ilk sürüm oluşturulur. Boş, daha önce sürüm almamış B klasörüne ilk sürüm otomatik gider. Kaynak açık ve erişilebilir kalsın; onay tek başına dosya teslimi değildir.

Onay gecikirse **Open approval window** kullanın ve B'den **Ask for approval** isteyin. Aynı projenin penceresini yeniden açmak bekleyen istekleri korur; farklı projeninki önceki onaysız istekleri temizler. Onaylı cihaz yeniden pencere istemez.

## Sonraki sürümler ve revoke

Projeyi kaydedin, ilk denemede DAW'ı kapatın ve **Share current version** seçin. B DAW projesini kapatıp onay kutusuyla **Get <isim>'s version** kullanır. İşlem tamamlanınca **Advanced → Connection → Check connection** güncel karşılaştırmayı kontrol eder. Snapshot oluşturma, metadata yayını ve B'nin dosyaları alması ayrı sonuçlardır.

B de kaynak olabilir: açık bağlantı iki yönde çalışır. Bağımsız bağlantı kurabilmek ve IP değişimleri için [ortak bağlantı rehberini](../README.md#bağlantı-kurulamazsa) izleyin. Relay yoktur; en az bir taraf doğrudan erişilebilir olmalıdır.

**Advanced → Folder, ID and access → Remove access** bu PC'deki erişimi kaldırır. Projeyi oluşturan cihaz Supabase üyelik revoke'unu da kuyruğa alır. Anahtar değişmez; B daha önce aldığı dosyaları ve anahtarı elinde tutar. Hata veya eksik servis fonksiyonu proje kartında uyarı olarak çıkar ve **Advanced → (proje) → Invite service** altında görünür.

## Advanced dosya daveti

Bağlantı servisi kullanılamıyorsa **Advanced → Manual invitation (without invite service)** içinde **Export invitation with current addresses** seçin. Public adresli dosya için **Save invitation file with public addresses** kullanılır. Bunlar güncel UI etiketleridir; ana akış bağlantıdır.

Dosyayı paketten ayrı özel paylaşın. B native import ile otomatik isteği gönderir; siz yine kimliği doğrulayıp Approve seçersiniz. Elle IP/port, TCP/UDP ve TP-Link adımları yalnız [ortak sorun giderme bölümündedir](../README.md#bağlantı-kurulamazsa). Manuel kart alışverişi Legacy file pairing'dir.

Fresh eşitlik ve gelen dosyalar kontrol edilmeden denemeyi tamamlandı saymayın. Gerçek sonuçların kaydı [IMPLEMENTATION.md](../../IMPLEMENTATION.md) içindedir; bu rehber doğrulama kanıtı değildir.
