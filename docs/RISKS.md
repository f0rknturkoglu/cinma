# Risk ve Karar Kayıt Defteri (Phase 1–7 Toplu İnceleme İçin)

Bu dosya, `/goal` akışı boyunca karşılaşılan teknik riskleri, sandbox/donanım sınırlarını ve sahip (owner) onayı bekleyen mimari kararları tek yerde toplar.

## 1. Ağ ve Gerçek Swarm Riskleri
- **Gerçek Torrent Swarm TTFF & Seek (Faz 1 / Faz 3):** Agent sandbox'ı dış TCP bağlantıları proxy'lediği için BitTorrent swarm testleri yalnızca yerel loopback seeder üzerinden (`loopback.rs`) doğrulandı. `librqbit` parametreleri (`9657b75`: `max_request_window=16`, `4s` connect timeout, fallback trackers, head+tail warmup) gerçek ağda henüz yeniden ölçülmedi.
  - Doğrulama komutu: `tools/measure-playback.sh "https://torrentio.strem.fun/manifest.json" movie tt6751668 5`
- **Canlı Çevrimiçi API'ler (TMDB, OpenSubtitles, Trakt, Simkl, AniList, MAL, Stremio Import, Chromecast/DLNA):** Tüm HTTP/OAuth/SOAP istemcileri yerel mock sunucularla test edildi; gerçek üçüncü parti sunuculara karşı uçtan uca kimlik doğrulama ve hız sınırı davranışları gerçek ağda doğrulanmalıdır.

## 2. Medya Motoru, Render ve Paketleme Riskleri
- **Dolby Vision P5 Renk Doğruluğu (ADR 0004):** Mevcut `vo=libmpv` OpenGL `CAOpenGLLayer` yolu HDR10/HLG'yi doğru işler (4K HDR10 CPU `7.3%`), ancak `libplacebo` olmadığı için Dolby Vision Profile 5 tam renk dönüşümü yapamaz. Gerçek P5 örneği hatalı renk verirse ADR 0004 Çözüm D1 (`macvk` + özel `libmpv` derlemesi) tetiklenmelidir.
- **FFmpeg / libmpv Runtime Bağımlılığı ve Hardened Runtime İmzalama (ADR 0005, ADR 0011, ADR 0015):** Uygulama başlatma süresi (`< 250 ms`) için `libmpv.2.dylib`, `libavformat.63`, `libavcodec.63`, `libswscale.10`, `libavutil.61` çalışma zamanında `dlopen` edilir. Farklı Homebrew major FFmpeg sürümü yüklü makinelerde küçük resim ve `yt-dlp` içi muxing devre dışı kalır (yerel küçük resimler `AVFoundation`'a düşer). Dağıtım paketlemesinde `BUNDLE_LIBMPV=1 tools/package-release.sh` ile `Contents/Frameworks/` içine bağımlılıkların `@loader_path` üzerinden gömülmesi desteklenir; harici Homebrew dylib yükleme yolu için `Cinma.entitlements` içinde `com.apple.security.cs.disable-library-validation` bırakıldı. Apple Developer ID dağıtımı/Notarization aşamasında `BUNDLE_LIBMPV=1` ile tüm dylib'ler aynı ekip sertifikasıyla imzalanarak `disable-library-validation` kaldırılabilir.
- **PiP (Resim İçinde Resim) Yönü (Faz 4 Spike):** `AVPictureInPictureController` doğrudan `CAOpenGLLayer` desteklemez (`AVSampleBufferDisplayLayer` gerektirir). İstenirse Faz 4/7 sonrası çerçevesiz AppKit yüzen mini-pencere (`NSPanel`) veya frame kopyalayıcı eklenebilir.

## 3. İndirme, Bellek ve Entegrasyon Riskleri
- **Torrent Sonrası Bellek Parçalanması (Açık Konu 3):** `librqbit` oturumu kapandıktan sonra heap sayfalarının macOS VM'e hızlı dönmesi için Darwin `malloc_zone_pressure_relief` çağrısı ve disk politikası eklendi; uzun indirme oturumları sonrası gerçek swarm altında profil takibi önerilir.
- **Menü Çubuğu ve Arka Plan Aktivitesi (Faz 5):** Ana pencere kapatıldığında (`menubar.enabled = true`) menü çubuğu ayak izi kapalı pencere senaryosunda **`28 MB`** (`0.0 %` CPU) ölçüldü (`≤ 25 MB` hedefinin 3 MB üzerinde, AppKit/Swift runtime tabanından kaynaklı).
- **AirPlay Fizibilitesi (Faz 6 Araştırması):** macOS'ta `AVRoutePickerView` / sistem AirPlay video çıkışı `AVPlayer` gerektirir; özel `libmpv` + OpenGL yüzeyi doğrudan AirPlay Mirroring/Screen Extension dışında AirPlay video stream protokolüne (`HLS` + FairPlay/HAP) bağlanamaz. Yalnızca ses çıkışı CoreAudio üzerinden AirPlay hoparlörlere otomatik çalışır; TV aktarımı için LAN Chromecast + DLNA uygulandı.
- **Rakip Karşılaştırma Referans Ölçümü (Faz 7):** `Cinma` Faz 5–7 Release ölçümleri kaydedildi (`181.8 ms` ilk kare, `30.0 MB` boşta, `28 MB` menü çubuğu, `0.00 %` CPU). Stremio / Infuse / IINA ile aynı makinede canlı yan yana benchmark tablosu harici uygulamalar kurulu/ölçümlü olmadığı için kullanıcı ortamında manuel tamamlanabilir.

## 4. Güvenlik & Şema Migrasyonları
- **Önerilen ADR'ler (Onay Bekleyen):** `ADR 0011` (FFmpeg `dlopen` / GL thread / OpenSubtitles), `ADR 0012` (HTTP/1.1 segmented & `yt-dlp` mux), `ADR 0013` (Disk/seeding politikası & menü çubuğu), `ADR 0014` (Casting LAN sunucusu & Tracking outbox + `0005`/`0006` migrasyonları), `ADR 0015` (Paketleme & imzalama).
- **Profil PIN KDF:** `profile.pin_hash` alanı `ring::pbkdf2` (`PBKDF2-HMAC-SHA256`, 100.000 iterasyon, 16-byte rastgele tuz) kullanır. Düz metin saklanmaz.
