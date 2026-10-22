# Peer bilgisayarı: ilk alıcı B

Peer ilk sürümü alan cihazdır. Ortak akış ve ağ sorunları [kurulum girişinde](../README.md) tutulur. Gönderenden aynı build'in `TrackMesh_<sürüm>_x64-setup.exe` kurulum dosyasını alın ve çalıştırın; yönetici izni ister. İmzasız olduğu için Windows tanınmayan uygulama uyarısı gösterebilir (**More info → Run anyway**) ya da ayarlarına göre kurulumu engelleyebilir. Uygulama `Program Files\TrackMesh`, VST3 plugin `Common Files\VST3` altına kurulur; WebView2 yoksa kurulum indirir. Rust/Node veya repo gerekmez. Eski ZIP'ler güncel bağlantı akışını içermez.

## Davet bağlantısını kabul et

1. A'nın özel kanaldan gönderdiği bağlantıyı Windows'ta açın veya uygulamada **Paste invite link** kullanın. Bağlantıyı ajan sohbetine yapıştırmayın.
2. **Invite link** penceresinde proje adına göre oluşturulacak alt klasörün üst dizinini kontrol edin. **Choose another folder** üst dizini değiştirir; **Accept** burada boş proje klasörünü oluşturur. Yeni v2 bağlantı servis bilgisini taşır; URL/key girmeniz gerekmez. 15 dakika geçerlidir ve bir kez tüketilir.
3. İlk Accept işlemi **Router setup** onayını sorar. **Allow (recommended)** süreli UPnP eşlemesine izin verir; **Not now** izin vermez. Bu tercih Settings'te değiştirilebilir.
4. İmzalı cihaz isteğiniz A'ya otomatik gider. A ile Device/Identity değerlerini karşılaştırın; A **Approve** seçsin. Kart dosyası geri göndermeniz gerekmez.
5. Klasör boşsa ve daha önce sürüm almamışsa onaydan sonra ilk sürüm otomatik indirilip kurulur. İki uygulama açık kalsın; tamamlanmadan DAW'da hedefi açmayın.

Onay bekleniyorsa A **Open approval window**, siz **Ask for approval** kullanın. Klasörü yeniden link etmeyin veya yeni UUID oluşturmayın. Onaylı cihaz her yeni bağlantıda tekrar onay istemez.

`Collaborator` listesinde A'yı görmek yerel kimlik kaydıdır. Online bağlantı, hazır sürüm ve tamamlanan dosya kurulumu ayrı durumlardır. Offline hint dosyaları indirmek için yeterli değildir; A online ve erişilebilir olmalıdır.

## Sonraki sürümü al

A **Share current version** ile sürümü paylaşsın. DAW projesini kapatın ve **Get <isim>'s version** seçin; TrackMesh bir şey indirmeden önce tek pencerede eklenen, değişen ve silinen dosyaları gösterir ve projenin DAW'da kapalı olmasını ister. Gerekirse **Advanced → Connection → Check connection** ile bağlantıyı kontrol edin. Aktarım PAUSED olursa aynı snapshot için **Resume getting**, recovery gerekiyorsa DAW kapalıyken kartta **Recover** kullanın.

Divergence varsa **Use <name>'s version** açık onayla karşı tarafın sürümünü kurar ve eski yerel dosyaları backup'ta korur. **Keep mine** yerel sürümü paylaşır. İki tarafın değişiklikleri otomatik birleştirilmez; mevcut proje/veri/backup klasörlerini silerek engeli aşmayın.

Son fresh Check connection sonucunu ve gelen dosyaları kontrol edin. DAW'da açılış/sample kontrolü ayrıca yapılır. A bu sırada yeni sürüm paylaşırsa eşitlik sonucu değişebilir.

## Advanced dosya daveti ve CLI

Dosya daveti gelirse **Advanced → This PC → Open an invitation file** ile hedef klasörü seçip **Open invitation file…** kullanın. Dosya payload'ını açmayın; native uygulama işler. Manuel imzalı kart alışverişi Legacy file pairing yoludur.

CLI bağlantı tüketmez ve Desktop'ın otomatik ilk alma akışını yürütmez. [CLAUDE.md](CLAUDE.md) paket şablonu native `join`, güvenli `status`, `request-pairing`, `check`, `receive` ve son check adımlarını anlatır. Aynı profilde CLI network işi ile Desktop/`serve`/plugin owner birlikte çalışmaz. CLI network komutu bitince bağlantısı kapanır; sonraki kaynak rolü için aynı profille Desktop veya açık serve oturumu gerekir.

## Hata ve yetki sınırları

Ağ/IP/firewall ayarları yalnız [ortak bağlantı sorunları bölümünde](../README.md#bağlantı-kurulamazsa) anlatılır. Supabase adres güncellemesi NAT engelini aşmaz; relay yoktur.

Davetle alınan proje #42 sonrası köken işareti taşır ve Supabase sahiplenme isteği göndermez. Başka cihazı Revoke yaparsanız mesaj yerel erişimin kaldırıldığını, servis üyeliğini yalnız projeyi oluşturan cihazın kaldırabileceğini söyler. Revoke daha önce alınmış dosyaları veya proje anahtarını silmez.

A'ya build/commit, Windows sürümü, son tamamlanan adım ve güvenli hata metnini bildirin. Davet, anahtar, secrets, veritabanı veya proje dosyaları göndermeyin. Gerçek kontroller [IMPLEMENTATION.md](../../IMPLEMENTATION.md) içinde kaydedilir.
