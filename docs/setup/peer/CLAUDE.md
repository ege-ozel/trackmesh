# TrackMesh: arkadaş bilgisayarında ilk sürümü alma

Bu belge dağıtım paketinde kök `CLAUDE.md` olarak kullanılır ve repo checkout'u gerektirmez. Henüz güncel bir deneme paketi yoktur; gerektiğinde incelenmiş master'dan yeni paket hazırlanır. Eski paketler güncel davet bağlantısı akışını içermez. Güncel pakette aynı build'in `trackmesh-desktop.exe`, `trackmesh-agent.exe`, gereken app-local Visual C++ DLL'leri ve taşınabilir rehberler bulunur. EXE/DLL'leri birlikte tutun. Arkadaş bilgisayarında Rust/Node gerekmez. Desktop [Microsoft WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) ister; CLI WebView2 kullanmaz.

## Önce Desktop bağlantı akışı

Gönderen **Add project** ile projeyi ekler, **Invite collaborator** seçer ve v2 bağlantıyı özel kanaldan paylaşır. Alıcı Windows'ta açar veya **Paste invite link** kullanır; **Invite link** penceresinde oluşturulacak proje alt klasörünün üst dizinini kontrol edip **Accept** seçer; **Choose another folder** üst dizini değiştirir. Yeni bağlantı servis bilgisini taşır, URL/key girişi gerekmez. 15 dakika geçerlidir ve bir kez tüketilir. İlk Invite/Accept **Router setup** ile süreli UPnP için **Allow (recommended)** / **Not now** sorusunu gösterir.

İmzalı cihaz isteği otomatik gider. Gönderen public Device/Identity değerlerini alıcıyla karşılaştırıp **Approve** seçer. İlk sürüm boş, daha önce sürüm almamış klasöre otomatik kurulur. Sonraki sürümlerde kaynak **Share current version**, alıcı DAW kapalı onayıyla **Get collaborator's version** kullanır. CLI bu link tüketme ve otomatik ilk alma akışını içermez.

Davet servisi isteğe bağlıdır; kullanıcının kendi Supabase projesi yalnız küçük şifreli invite drop, endpoint kaydı ve head hint'i tutar. Dosyalar cihazlar arasında aktarılır. Onay üyelik grant'ini kuyruğa alır; projeyi oluşturan cihazdaki Revoke üyelik revoke'unu kuyruğa alır. Davetle alınan projeler #42 sonrası köken işareti taşır ve sahiplenme isteği göndermez. Revoke proje anahtarını değiştirmez; çıkarılan kişi aldığı dosyaları ve anahtarı elinde tutar.

## Bu ajan için Advanced CLI yolu

CLI kullanmayı seçen kullanıcı gönderenden ayrı özel davet dosyasını alsın. Daveti, bağlantıyı, anahtarları veya proje dosyalarını ajan sohbetine eklemesin; yalnız dosya ve boş hedef klasörün yerel yollarını versin. Dosyayı native `join` okur. Manuel kart alışverişi yalnız legacy seçenektir.

Paket klasöründe PowerShell açın. Claude Code kurulu ve giriş yapılmışsa `claude` çalıştırın; MD talimat dosyasıdır. Ajanı davet içeriğini okumadan `join`/`status`/`request-pairing` ile ilerletin. Gönderen aynı güncel build'in Desktop'ını ve 15 dakikalık onay penceresini açık tutsun. A **Approve** seçtikten ve kaynak sürüm hazır olduktan sonra kullanıcı hedefi ve DAW'ın kapalı olduğunu bildirsin; ajan `check`, `receive`, son `check` çalıştırsın.

Davet import'u yerel proje ve gönderen kimliğini kaydeder. Bu kayıt online bağlantı veya dosya teslimi değildir. Onaylı bir cihaz her bağlantıda yeni pencere istemez. CLI network işi yalnız komut boyunca online kalır; aynı profilde iki owner açılmaz. Kaynak daha sonra alıcı olabilir; kurulu bağlantı iki yönlüdür.

Yerel SQLite trust, history ve transfer durumunu tutar. Public rehber, browser deployment, offline pairing kuyruğu, kalıcı arka plan servisi ve otomatik merge yoktur. Relay yoktur; eski davetteki relay adresi yok sayılır. Supabase offline kaynağın dosyalarını taşımaz veya NAT engelini kaldırmaz.


## Claude için görev ve sınırlar

Bu Windows kullanıcısına küçük bir test projesini terminalden aldır. Kullanıcıya Türkçe ve kısa konuş. Her aşamadaki somut sonucu bildir; bağlantı hatasında tüm akışı baştan başlatma.

- Davet dosyasını araçla/metin olarak okuma; `Get-Content`, JSON parse, clipboard, attachment veya log ile payload/project key'i açığa çıkarma. Dosyayı yalnızca native CLI'nin `join --invite-file` argümanı olarak geçir.
- `secrets`, device/project keys ve veritabanını okuma/paylaşma. Davet çıktısı üreten `invite` komutunu bu alma akışında kullanma. Kimlikleri güvenli `status` çıktısından öğren.
- Bilgisayardaki mevcut proje/veri klasörlerini silme, sıfırlama veya otomatik conflict merge yapma. Hedefi kullanıcı seçsin; ilk deneme için boş klasör kullan. Hedef/data/paket klasörleri birbirinden ayrı olsun.
- İlk denemede ayrı `%LOCALAPPDATA%\TrackMeshPeerTrial` veri profili kullan; profil zaten varsa kimliği koru. Komutlar Windows kullanıcısının kendi hesabında çalışsın; başka hesap altında DPAPI anahtarları açılamaz.
- CLI `request-pairing`/`check`/`receive` çalışırken aynı profilde Desktop, `serve` veya plugin owner açma. Tek network runtime lease vardır. Önceden açık owner varsa onu zorla öldürme; güvenli kapanış/işlerin bitmesini bekle veya kullanıcıyla ayrı bir profil seç.
- Receive öncesinde hedef klasör ve DAW'ın kapalı olduğu kullanıcı tarafından belirtilmiş olsun. Belirtildiyse aynı onayı tekrar sorma. Apply sırasında process'i zorla sonlandırma.
- Firewall/router ayarlarını otomatik değiştirme, admin/UAC veya servis kurma. Gerekirse aşağıdaki rehberle kullanıcıyı yönlendir. Claude'un normal komut izinlerini atlatma.
- Tamamlandı demek için Receive'in başarıyla bitmesini ve son fresh check sonucunu gör. Timeout/PENDING/last-known durumunu başarı sayma. Kalan engeli açıkça bildir.

## Terminal akışı — PowerShell

Bu bloklar yönergedir; her komutun sonucunu kontrol ederek ilerle. Hata varsa sonraki bağımlı adımı çalıştırma. Claude farklı shell kullanıyorsa PowerShell komutlarını olduğu gibi bash'e yapıştırmasın. Native komutlardan sonra `$LASTEXITCODE` kontrol et.

### A. Executable'ları ve yolları hazırla

Paket klasöründe:

```powershell
$trialAgent = (Resolve-Path -LiteralPath '.\trackmesh-agent.exe').Path
$trialData = Join-Path $env:LOCALAPPDATA 'TrackMeshPeerTrial'
& $trialAgent --help
& $trialAgent join --help
& $trialAgent request-pairing --help
& $trialAgent receive --help
```

Kullanıcıdan `$trialInvite` (özel davetin tam yolu) ve `$trialFolder` (boş hedef klasörün tam yolu) için değer al. Klasör yoksa belirtilen yerde oluştur; varsa boş olduğunu dosya isimlerini/metadata'yı kontrol ederek doğrula. Dolu klasör seçilmişse ilk deneme için başka boş klasör iste; içerik silme. Sadece davet dosyasının varlığını kontrol et, içeriğini açma.

### B. Daveti native olarak import et

```powershell
& $trialAgent --data-dir $trialData --device-name 'Peer trial PC' join --invite-file $trialInvite --folder $trialFolder
```

Mevcut join çıktısı manuel kart önerir; yeni akışta aşağıdaki request-pairing kullanılır. Kart dosyası üretmen/göndermen gerekmez. Proje zaten aynı hedefe import edilmişse kimliği koru; sıradan retry için tekrar join veya New UUID yapma. Gönderenin adresi gerçekten değişmiş ve güncel davet alınmışsa aynı proje/hedefe native join ile adres güncellemesi yapılabilir.

### C. Proje ve gönderen kimliğini güvenli status'tan seç

```powershell
$trialStatus = & $trialAgent --data-dir $trialData status | ConvertFrom-Json
```

`projects[].project.project_id` proje UUID'si, o projenin `trusted_peers[].device_id` alanı gönderenin cihaz UUID'sidir. Birden fazla proje/peer varsa adları/UUID'leri kullanıcıyla doğrula; ilk kaydı rastgele seçme. Uygun kayıtlardan `$trialProjectId` ve `$trialSenderId` değişkenlerini doldur. Davet dosyasından bu alanları okumak gerekmez.

### D. Eşleştirme isteğini otomatik gönder

Gönderen kendi Desktop'ında daveti export etmiş (pencereyi açar) veya **Open approval window** seçmiş olsun. Uygulaması açık kalsın. Her komutun sonucunu kontrol et:

```powershell
& $trialAgent --data-dir $trialData request-pairing $trialProjectId --device-id $trialSenderId --connect-timeout 30
```

Native CLI imzalı kartı ve tek kullanımlık davet sahipliği kanıtını doğrudan gönderir; anahtar/proof çıktısını agent okumaz. Gönderen gelen Device/Identity değerlerini beklediği kişiyle doğrulayıp **Approve** seçer. Alıcı bağlantı için 30 saniyeye, istek sonrası onay için 60 saniyeye kadar bekleyebilir. `pairing_state: approved` yetki verildiğini, `pending` yalnızca isteğin teslim edildiğini gösterir. Pending'i dosya teslimi veya onay olarak raporlama. Daha geç onayda aynı komutu tekrar çalıştır; kaynak sürümü paylaşmasını bekle. Offline/kapalı pencere/timeout'ta önce bu koşulları düzelt; profil veya projeyi sıfırlama. Eski executable komutu tanımıyorsa yeni paketi iste.

### E. Bağlantıyı dene ve receive ile al

```powershell
& $trialAgent --data-dir $trialData check $trialProjectId --device-id $trialSenderId --connect-timeout 30
```

30 saniyeye kadar bağlantı beklenebilir. Başarılı check'te `remote_snapshot` yoksa gönderenden önce snapshot iste. Divergence/unknown ancestry gibi engeller varsa bu ilk denemede ilerleme; yerel veriyi silerek aşmaya çalışma. Her başarılı check eşitlik anlamına gelmez; boş hedef henüz dosyaları almamıştır.

Kaynak hazır, hedef doğru ve DAW kapalıysa:

```powershell
& $trialAgent --data-dir $trialData receive $trialProjectId --device-id $trialSenderId --connect-timeout 30
& $trialAgent --data-dir $trialData check $trialProjectId --device-id $trialSenderId --connect-timeout 30
& $trialAgent --data-dir $trialData status
```

Receive bitmeden diğer komutu başlatma. Başarıyı son check'in `verified_equal` değeriyle değerlendir; false ise snapshot kimliklerini/bağlantı sonucunu bildir. Sonradan kaynak değişmiş olabilir. İlk Receive sırasında ağ kesilirse status'taki PAUSED session/snapshot ID'sini kullanarak aynı hedefi `receive ... --snapshot-id <UUID>` ile sürdür. Recovery gerekiyorsa DAW kapalıyken `recover <project-UUID>` kullan; backup/journal verisini koru.

## Bağlantı olmadıysa kısa kontrol sırası

Önce gönderen açık mı, doğru proje/peer seçildi mi, onay penceresi açık mı, cihaz onaylandı mı ve kaynak sürüm var mı kontrol edin. Onay geciktiyse pencere yeniden açılır ve aynı `request-pairing` denenir. Profil/UUID'yi sıfırlamayın.

Her seferinde tek koşulu düzeltip check yapın. Ayrıntılı IP, TCP/UDP, Windows Firewall ve TP-Link adımları paketteki ortak kurulum README'sinin **Bağlantı kurulamazsa** bölümünde tutulur; bulunmuyorsa gönderenden bu rehberi isteyin. Ajan router/firewall kuralı veya servis kurmaz.

Gerçekten eskimiş dosya daveti için gönderenden güncel adresli dosya isteyip aynı profil/proje/hedefe native join yapabilirsiniz. Sıradan timeout için yeniden import yapmayın. En az bir uç erişilebilir olmalıdır; relay yoktur. Gerçek ayrı-ağ aktarımı henüz doğrulanmış sayılmaz.


## Desktop'ı daha sonra açmak

CLI işi tamamlandıktan sonra aynı deneme profilini göstermek için:

```powershell
Start-Process -FilePath '.\trackmesh-desktop.exe' -ArgumentList @('--data-dir', ('"' + $trialData + '"')) -WindowStyle Hidden
```

Kullanıcı açıkça Desktop penceresi görmek istiyorsa normal çift tıklama yerine bu profil seçimiyle aç; aksi halde çift tıklama varsayılan `%LOCALAPPDATA%\TrackMesh` profilini gösterir ve deneme projesi görünmeyebilir. CLI alma akışı için Desktop açmak şart değildir. GUI kontrol araçların yoksa butonlara bastığını iddia etme.

`\\?\D:\...` görünen yollar Windows'un canonical/uzun yol biçimidir; aynı `D:\...` klasörünü ifade eder. Veri/proje yolunu düzeltmek için veritabanını elle değiştirme.
