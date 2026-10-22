# Aynı Wi-Fi üzerinde iki bilgisayarla ilk deneme

A ilk gönderen, B ilk alıcıdır. Bu yönerge güncel Desktop bağlantı akışını kullanır. Henüz güncel bir trial paketi yoktur; aynı build incelenmiş master'dan hazırlanır. VST3 veya arkadaş bilgisayarında Rust/Node gerekmez. Bu deneme internet erişilebilirliği kanıtı değildir.

## Sırayla yapın

1. İki cihazda güncel Desktop'ı açın. Normal aynı Wi-Fi/LAN kullanın; guest/client isolation bağlantıyı engelleyebilir.
2. A küçük bir proje kopyasını **Add project** ile eklesin. B'nin projesi kendi yerel veri profilinden ayrı bir klasörde oluşturulsun.
3. A kendi Supabase servis ayarını kontrol edip **Invite collaborator** seçsin. İlk Invite/Accept, **Router setup** ile UPnP iznini sorar. Aynı LAN için public adres/port yönlendirmesi gerekmez.
4. A 15 dakikalık tek kullanımlık bağlantıyı özel kanaldan B'ye iletsin.
5. B Windows'ta açsın veya **Paste invite link** kullansın; **Invite link** penceresinde oluşturulacak klasörün üst dizinini kontrol edip **Accept** seçsin; **Choose another folder** üst dizini değiştirir. Ajan bağlamına link veya davet içeriği vermeyin.
6. A proje kartındaki **Someone opened your invite link** kutusundaki public kimlikleri B ile karşılaştırsın ve **Approve** seçsin.
7. A'nın klasöründe dosyalar ve henüz sürüm yoksa onay ilk snapshot'ı oluşturur. B'nin boş, daha önce sürüm almamış klasörüne ilk sürüm otomatik kurulur. İki uygulama açık kalsın.
8. Dosyalar geldikten sonra B **Advanced → Connection → Check connection** ile fresh karşılaştırmayı kontrol etsin. Ardından DAW'da açılış ve sample'lar ayrıca incelensin.
9. Sonraki sürümlerde A **Share current version**, B DAW kapalı onayıyla **Get collaborator's version** kullanır.

Servis olmadan deneme gerekiyorsa [ortak rehberdeki Advanced dosya daveti yolunu](README.md#advanced-ve-legacy-yollar) izleyin. Kart dosyası geri gönderme normal akışın parçası değildir.

## Takılırsanız

Onay süresi dolduysa A **Open approval window**, B **Ask for approval** seçsin. Profil/UUID'yi sıfırlamayın. Windows Firewall kontrolü (`Win + R → wf.msc`), TCP/UDP ve router adımları tek yerde [Bağlantı kurulamazsa](README.md#bağlantı-kurulamazsa) bölümündedir.

Başarıyı onay, tamamlanan dosya kurulumu, fresh Check connection ve DAW açılışıyla ayrı ayrı değerlendirin. Kaynak yeni sürüm paylaşmışsa güncel eşitlik değişebilir. Bu sayfa yönergedir; gerçek deneme ve kullanılan build yalnız [IMPLEMENTATION.md](../IMPLEMENTATION.md) içinde kaydedilir.
