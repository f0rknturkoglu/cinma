# Cinma — Özellik Envanteri ve Modül Rehberi (`#01` – `#80`)

Bu belge Cinma'da aktif olarak çalışan tüm özellikleri **sıralı numara**, **ilgili ayar / API anahtarı** ve **sorumlu Rust + Swift dosyaları** ile listeler.  
Özellik bazlı iyileştirme, hata düzeltme veya refactor isterken doğrudan numarayla referans verebilirsin *(örneğin: `"#42 ve #45'e UI polish yap"`, `"#71 için birim test ekle"`)*.

> **Mimari Kural Özeti:** Tek süreç (`Single-Process`), sıfır WebView, sıfır telemetri, yerel oynatmada `localhost` sunucusu yok (`mpv_stream_cb_add_ro` — `cinma://blob/<id>`), gizli bilgiler macOS Keychain'de, durumlar SQLite (`WAL`) üzerinde tutulur.

---

## A. Ana Sayfa, Keşif, Kütüphane ve Meta Veri (`#01`–`#18`)

| # | Özellik | Özet / Ayar Anahtarı | Rust Katmanı | Swift / AppKit Katmanı |
|---|---|---|---|---|
| **#01** | **SQLite İlk Açılış & Önbellekli Ana Ekran** | Önce SQLite'daki *İzlemeye Devam Et* + önbellekli popüler raflar çizilir, ağ arkadan güncellenir. | `crates/core/src/addons.rs`, `crates/db/` | `app/Cinma/HomeBrowserViewController.swift` |
| **#02** | **Hero Banner & Görünürlük Duraklatma** | Hero döngüsü pencere gizli/arkada olduğunda durur (`0%` boşta CPU). | — | `app/Cinma/HomeBrowserViewController.swift` |
| **#03** | **İzlemeye Devam Et Düzeni & Film/Dizi Bölme** | `cwAtTop`, `cwHideCaughtUp` (`>= %85`), `splitContinueWatching` (Film/Dizi ayrı satır). | `crates/core/src/subtitles.rs`, `library.rs` | `app/Cinma/HomeBrowserViewController.swift` |
| **#04** | **Poster Görünümü & Rozet Özelleştirme** | `posterScalePct` (`80–125`), `posterRadiusPt` (`4–24`), `hidePosterTitles`, `showImdbBadge`, `showTypeBadge`, `showWatchedBadge`, `showLibraryBadge`. | `crates/core/src/subtitles.rs` | `app/Cinma/PosterCards.swift` |
| **#05** | **Dinamik Eklenti Katalog Satır Sınırı** | `maxAddonRails` (`0–15`) ve kataloglarda `hideWatchedInCatalogs` / `hideUnreleased` filtreleri. | `crates/core/src/addons.rs`, `subtitles.rs` | `app/Cinma/HomeBrowserViewController.swift`, `PosterCards.swift` |
| **#06** | **Ana Sayfaya Katalog Sabitleme (`Pinned Shelves`)** | Herhangi bir eklenti kataloğu + tür kombinasyonunu Ana Sayfa'ya özel raf olarak sabitle/kaldır (`pin_shelf`, `unpin_shelf`). | `crates/core/src/phase9/`, `crates/ffi/src/lib.rs` | `app/Cinma/DiscoverViewController.swift`, `HomeBrowserViewController.swift` |
| **#07** | **Keşfet: Eklenti Kataloğu, Tür Çipleri & Sayfalama** | Katalog seçimi, dinamik tür pilleri, artımlı arama (`skip`) ve birleşik arama sonuçları. | `crates/core/src/addons.rs`, `crates/features/stremio/` | `app/Cinma/DiscoverViewController.swift` |
| **#08** | **TMDB Yayın Platformu Filtreleri** | Yalnızca tıklandığında TMDB `/discover/{movie\|tv}` sorgulayan *Netflix, Disney+, Apple TV+, Prime Video, Max, Crunchyroll, MUBI* filtreleri. | `crates/features/metadata/src/tmdb.rs`, `crates/core/src/metadata.rs` | `app/Cinma/DiscoverViewController.swift` |
| **#09** | **TMDB + Fanart.tv + Kitsu Meta Veri & Dil Kontrolü** | `metadataLanguage`, `preferOriginalTitles`, `tmdbImageLang` (`auto/en/tr/null`), ClearLogo ve *"Nerede İzlenir"* sağlayıcıları. | `crates/features/metadata/`, `crates/core/src/phase9/` | `app/Cinma/TitleDetailViewController.swift` |
| **#10** | **Film Serisi (`Collection`) & Oyuncu/Yönetmen Sayfası** | `title_collection` rafı ve tıklanabilir oyuncu/yönetmen çiplerinden biyografi + kredi modalı (`person_profile`). | `crates/features/metadata/src/tmdb.rs`, `crates/core/src/phase9/` | `app/Cinma/PersonAndQuickPlay.swift`, `TitleDetailViewController.swift` |
| **#11** | **Başlık Detayı: Spoiler Koruması & Bölüm Düzeni** | `hideSpoilers` (izlenmemiş thumb blur + özet gizleme), `episodeSortAsc`, `episodeCompactLayout`, sağ tık toplu *"Buraya Kadar İzlendi"*. | `crates/core/src/library.rs`, `subtitles.rs` | `app/Cinma/TitleDetailViewController.swift` |
| **#12** | **Kütüphane Arama, Tür Filtresi & Sıralama** | Kütüphanede anlık arama kutusu, *All / Movies / Series* segmenti ve *Recent / A→Z / Z→A* sıralaması. | `crates/core/src/library.rs` | `app/Cinma/LibraryCalendarViewControllers.swift` |
| **#13** | **Yerel Medya Kütüphanesi Tarayıcısı** | İsteğe bağlı recursive tarama (`depth <= 6`, `<512 KB` eleme, `SxxExx`/yıl ayrıştırma), `scan_local_folders`, `FSEvents` daemon yok. | `crates/core/src/local_library.rs` | `app/Cinma/LibraryCalendarViewControllers.swift`, `Dalga3Views.swift` |
| **#14** | **Doğrudan WebDAV Ağ Kaynakları** | Zurg, TorBox WebDAV, Nextcloud, NAS için `PROPFIND Depth: 1` tarama; şifre Keychain'de (`webdav.{id}.password`). | `crates/core/src/webdav.rs`, `keychain.rs` | `app/Cinma/Dalga3Views.swift`, `LibraryCalendarViewControllers.swift` |
| **#15** | **Takvim & Yerel Yeni Bölüm Bildirimleri** | Kütüphane dizilerinin TMDB yayın tarihleri, `weekStartsMonday`, uygulama açıkken yerel `UserNotifications` zamanlaması. | `crates/features/metadata/`, `crates/core/src/` | `app/Cinma/LibraryCalendarViewControllers.swift`, `CalendarNotifications.swift` |
| **#16** | **Profil Sistemi & Yedekleme / Geri Yükleme** | Çoklu profil izolasyonu, atomik SQLite anlık görüntü yedeği (`VACUUM INTO`) dışa/içe aktarma (`rollback` korumalı, ayarlar dahil; gizli anahtarlar Keychain'de kalır), Stremio hesap içe aktarma. | `crates/core/src/security_and_backup.rs`, `integrations.rs` | `app/Cinma/SheetsAndSettings.swift` |
| **#17** | **macOS Spotlight & AppIntents Kısayolları** | Kütüphaneyi isteğe bağlı `CoreSpotlight` ile dizinleme ve Siri / Shortcuts için *Open Title / Play Next* `AppIntents`. | — | `app/Cinma/SpotlightAndIntents.swift` |
| **#18** | **Eklenti Yöneticisi & `stremio://` Derin Bağlantı** | Eklenti manifest kurma/kaldırma, `addonTimeoutS` (`3–60s`), `confirmStremioDeeplink` kurulum onayı. | `crates/core/src/addons.rs`, `crates/features/stremio/` | `app/Cinma/SheetsAndSettings.swift`, `AppDelegate.swift` |

---

## B. Akış Sıralama, Quick Play ve Debrid (`#19`–`#28`)

| # | Özellik | Özet / Ayar Anahtarı | Rust Katmanı | Swift / AppKit Katmanı |
|---|---|---|---|---|
| **#19** | **Paralel Akış Toplama & İlerleme Durumu** | Tüm akış eklentileri paralel sorgulanır, gelen akışlar artımlı render edilir (*n of m addons answered*). | `crates/core/src/addons.rs` | `app/Cinma/TitleDetailViewController.swift` |
| **#20** | **Açıklamalı Puanlama & Akıllı Sıralama** | Çözünürlük, HDR/DV, donanımsal AV1 (`sysctlbyname`), ses, seeder, dil puanlaması; `streamSort` (`score/seeds/size_desc/size_asc`). | `crates/features/ranker/src/lib.rs` | `app/Cinma/TitleDetailViewController.swift` |
| **#21** | **Filtreler, Kelime Engelleme & Ham Dosya Adı** | `4K / 1080p / <10GB / HDR` pilleri, `trackBlockWords`, `pickerShowFilename` toggle'ı. | `crates/features/ranker/`, `core/src/subtitles.rs` | `app/Cinma/TitleDetailViewController.swift` |
| **#22** | **Dizi Bazlı Kaynak Hatırlama (`bingeGroup`)** | `rememberLastStream` & `keepSourceNextEpisode` ile son seçilen eklenti/`bingeGroup` sonraki bölümde öne alınır. | `crates/core/src/subtitles.rs` | `app/Cinma/TitleDetailViewController.swift`, `PlayerViewController.swift` |
| **#23** | **Tek Tıkla Hızlı Oynat (`Quick Play`) + Sağ Tık Menüsü** | `quickPlayEnabled`: Poster, Hero veya Detay'dan en iyi/önbellekli akışı doğrudan başlatır (`Option+Tık` seçici açar); poster sağ tık menüsü. | `crates/core/src/phase9/` | `app/Cinma/PersonAndQuickPlay.swift`, `HomeBrowserViewController.swift`, `TitleDetailViewController.swift` |
| **#24** | **6 Debrid Sağlayıcı & Anlık Önbellek Kontrolü** | **Real-Debrid, AllDebrid, Premiumize, TorBox, EasyDebrid, Debrid-Link**; Keychain anahtarı, `[Instant Debrid]` öne çıkarma. | `crates/features/debrid/src/lib.rs`, `core/src/phase9/` | `app/Cinma/IntegrationsView.swift`, `TitleDetailViewController.swift` |
| **#25** | **Yalnızca Debrid / Doğrudan Akış Modu** | `debridOnlyMode`: Önbellekte olmayan ham P2P torrent akışlarını seçiciden gizler. | `crates/features/ranker/`, `core/src/addons.rs` | `app/Cinma/SheetsAndSettings.swift` |
| **#26** | **Sezon Paketi / Toplu Torrent İnceleyici** | Çok dosyalı torrent/magnet içinde `list_files` + `SxxExx` ayrıştırma ve seçilen bölümleri tek tıkla sıraya alma (`queue_torrent_batch`). | `crates/features/torrent/src/lib.rs`, `core/src/downloads.rs` | `app/Cinma/Dalga3Views.swift`, `TitleDetailViewController.swift` |
| **#27** | **Harici Oynatıcı Devri (`mpv / IINA / Infuse / VLC`)** | `externalPlayerEnabled` + `externalPlayerPreset` (`auto/iina/infuse/vlc/custom`) + `externalPlayerPath` (bir `.app`) ile tek tık dışa aktarma; URL şeması veya Launch Services ile açılır, Cinma'nın alt süreci olmaz. | `crates/core/src/subtitles.rs` | `app/Cinma/TitleDetailViewController.swift`, `SheetsAndSettings.swift` |
| **#28** | **Stall Watchdog (Takılmada Otomatik Kurtarma)** | `autoNextStreamOnStall` + `stallTimeoutS` (`5–30s`): takılmada yeniden dener ve sonraki akışa geçer. | — | `app/Cinma/PlayerViewController.swift` |

---

## C. Oynatıcı (`libmpv`), HDR, Ses ve Mac Donanım Deneyimi (`#29`–`#53`)

| # | Özellik | Özet / Ayar Anahtarı | Rust Katmanı | Swift / AppKit Katmanı |
|---|---|---|---|---|
| **#29** | **Uyarlanabilir OpenGL / macOS EDR HDR Yüzeyi** | SDR'de 8-bit `BT.709`, HDR'de 16-bit float `PQ / macOS EDR` (`macEdrEnabled`, `forceHdrSurface`, `forceYuv420p`). | `crates/player-bridge/src/lib.rs` | `app/Cinma/VideoLayer.swift`, `PlayerViewController.swift` |
| **#30** | **Canlı HDR Ton Eşleme & Ölçekleyici** | Canlı menüden `hdrToneMapping` (`auto/bt.2390/reinhard/hable/clip`), `hdrTargetPeak`, `mpvScalePreset`, `mpvHwdecMode`, `mpvBufferMb`, `mpvExtraOptions`. | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#31** | **Adaptive CAS Shader & Canlı Video EQ** | `shaderPreset` (`sharpen`) ve `mpv` donanım matrisi üzerinden `eqBrightness`, `eqContrast`, `eqGamma`, `eqSaturation` (Gece Gamma Boost önayarı dahil). | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#32** | **En/Boy Oranı & Kırpma Modları** | Canlı değiştirilebilen `cropMode`: `fit`, `fill`, `16:9`, `21:9`, `4:3`. | `crates/player-bridge/src/lib.rs` | `app/Cinma/PlayerViewController.swift`, `PlayerMenus.swift` |
| **#33** | **Basılı Tutarak Geçici `2.0×` Hızlandırma** | `holdToBoostEnabled` & `holdBoostSpeedPct` (`150–300%`): Space veya basılı tık `>260 ms` tutulduğunda geçici hız + üstte pill. | `crates/core/src/subtitles.rs` | `app/Cinma/PlayerViewController.swift` |
| **#34** | **Özelleştirilebilir Hız, Atlama & Kare-Kare Adım** | `defaultSpeedPct`, `exactSeek`, `seekBackStepS` / `seekForwardStepS` (`3–60s`), `frame_step` (`.`), `frame_back_step` (`,`), `A-B` döngüsü. | `crates/player-bridge/src/lib.rs`, `core/src/commands.rs` | `app/Cinma/PlayerViewController.swift` |
| **#35** | **Sıfır Ağ Maliyetli Seek-Bar Thumbnail & Chapter İşaretçileri** | Yerel/tamponlu kareden üretilen seek önizleme ve `seekBarChapters` çubuk çizgileri. | `crates/player-bridge/src/lib.rs` | `app/Cinma/PlayerHelpers.swift`, `PlayerViewController.swift` |
| **#36** | **Yayın Ayrıntıları (`Globe` Popover) & P2P Hız Çipi** | Canlı indirme/yükleme hızı, peer sayısı, buffer %, dosya adı, `InfoHash`, `hwdec`, düşen kareler; `showP2pSpeedChip`. | `crates/core/src/playback.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#37** | **CoreAudio Ses Cihazı Seçici & Ses Politikaları** | `audio-device-list` menüsü, kulaklık kopunca otomatik duraklatma (`pauseOnAudioOutputChange`), `volumeBoostMax` (`100–200%`), `audioNormalize`, `downmixStereo`, `volumeHud`. | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#38** | **MacBook Trackpad Jestleri & Force Touch Haptik** | `trackpadGesturesEnabled` (iki parmak yatay sarma / dikey ses) ve `hapticFeedbackEnabled` (`NSHapticFeedbackManager` chapter/ses eşiği hissi). | `crates/core/src/subtitles.rs` | `app/Cinma/PlayerViewController.swift` |
| **#39** | **Oyun Kumandası (`GameController`) Desteği** | Bağlı gamepad'lerle oynatıcı (`A/B/X/Y/D-Pad/L1/R1/L2/R2`) ve arayüz (`Tab/D-Pad/Onay/Geri`) kontrolü; `gamepadSupportEnabled`, sıfır boşta yük. | `crates/core/src/subtitles.rs`, `crates/ffi/src/lib.rs` | `app/Cinma/GamepadController.swift`, `AppDelegate.swift` |
| **#40** | **Duraklatılan Karede `VisionKit` Live Text OCR** | Oynatıcı duraklatıldığında bellekteki kareden (`VKImageAnalyzer`) metin seçme, kopyalama ve çevirme (`T` kısayolu). | `crates/player-bridge/src/lib.rs` | `app/Cinma/PlayerViewController.swift`, `PlayerHelpers.swift` |
| **#41** | **Intro / Recap / Outro Atlama & Manuel Dizi İşareti** | AniSkip + IntroDB (7 gün SQLite TTL) + chapter tespiti, `autoSkipIntro`/`Recap`/`Outro`, ve **dizi bazlı manuel Intro/Outro bitişi kaydetme**. | `crates/features/metadata/src/skip_segments.rs`, `core/src/phase9/` | `app/Cinma/PlayerViewController.swift`, `PlayerMenus.swift` |
| **#42** | **Anime Filler Tespit Rozeti & Otomatik Atlama** | Jikan v4 (`/anime/{mal_id}/episodes`, 7 gün SQLite önbellekli) üzerinden `"Filler"` bölüm rozeti ve `autoSkipAnimeFillers`. | `crates/features/metadata/src/skip_segments.rs`, `core/src/phase9/` | `app/Cinma/TitleDetailViewController.swift`, `SheetsAndSettings.swift` |
| **#43** | **Sonraki Bölüm Prefetch & Otomatik Geçiş** | `%88` ilerlemede sonraki bölümü önceden ısıtma, `autoplayNext`, `autoplayCountdownS`, `nextEpisodeLeadS`, `stillWatchingAfterEpisodes`. | `crates/core/src/playback.rs` | `app/Cinma/PlayerViewController.swift` |
| **#44** | **Sıfır Kopyalı Yüzen PiP Mini Oynatıcı & Oto-PiP** | `VideoLayer` taşıyan `MiniPlayerPanel` ve başka tam ekran Space/uygulamaya geçildiğinde otomatik PiP (`autoPipOnOcclusion`). | `app/Cinma/VideoLayer.swift` | `app/Cinma/MiniPlayerPanel.swift`, `PlayerViewController.swift`, `AppDelegate.swift` |
| **#45** | **Çoklu Monitör Karartma & Pencere Kuralları** | `blackoutOtherScreens`, `playerAlwaysOnTop`, `pauseMinimized`, `pauseUnfocused`, `confirmOnLeave`, `screenLockEnabled`, `resumeBehavior`. | `crates/core/src/subtitles.rs` | `app/Cinma/PlayerViewController.swift` |
| **#46** | **Bölüm Sonu / Zamanlı Uyku Zamanlayıcısı** | Süreli (`customSleepMinutes`) veya **"Bölüm / Film Bitince Durdur"** uyku zamanlayıcısı. | — | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#47** | **Ekran Görüntüsü (`Screenshot`)** | Altyazılı veya `screenshotIncludeSubtitles` kapalıyken yalnızca temiz video karesini PNG olarak kaydetme. | `crates/player-bridge/src/lib.rs` | `app/Cinma/PlayerViewController.swift` |
| **#48** | **Oynatıcıya Sürükle & Bırak (`Drag & Drop`)** | Pencereye video dosyası bırakınca oynatma, oynatıcıya `.srt/.ass/.ssa/.vtt` bırakınca anında altyazı ekleme. | — | `app/Cinma/MainShellViewController.swift`, `PlayerViewController.swift` |
| **#49** | **Tam Ekran Saat, Bitiş Saati & Başlık Sırası** | `fullscreenClock`, `fullscreenClockEndTime`, `playerTitleSeriesFirst` (Dizi adı önde/sonda) ve `NowPlaying` medya tuşu/AirPods entegrasyonu. | `crates/core/src/subtitles.rs` | `app/Cinma/PlayerHelpers.swift`, `PlayerViewController.swift` |
| **#50** | **Kısayol Komut Kayıt Defteri (`COMMANDS`)** | Özelleştirilebilir klavye kısayolları ve Ayarlar → Shortcuts sekmesinde canlı tuş yakalama (`Esc` iptal). | `crates/core/src/commands.rs` | `app/Cinma/MainMenu.swift`, `SheetsAndSettings.swift`, `PlayerViewController.swift` |

---

## D. Altyazı Boru Hattı ve Stil Kontrolü (`#51`–`#58`)

| # | Özellik | Özet / Ayar Anahtarı | Rust Katmanı | Swift / AppKit Katmanı |
|---|---|---|---|---|
| **#51** | **Eklenti + OpenSubtitles REST v1 Altyazı Toplama** | `enableAddonSubtitles`, `enableOpenSubtitles`, OpenSubtitles hesap girişi + kalan kota ve 64 KiB video hash araması. | `crates/features/subtitles/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `IntegrationsView.swift` |
| **#52** | **Türkçe `CP1254` / `UTF-16` Otomatik Dönüşüm & SDH Temizliği** | Bozuk karakterli `windows-1254` / `ISO-8859-9` dosyaları UTF-8'e çevirme, `hideSdh` (`[efekt]` temizleme), `forcedWhenNativeAudio`. | `crates/features/subtitles/src/lib.rs` | `app/Cinma/SheetsAndSettings.swift` |
| **#53** | **Çift Altyazı (`Dual Subtitles`)** | `dualSecondaryLang` + `secondaryPos` ile üstte ikinci dil altyazısı (`secondary-sid`). | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#54** | **Diyalog İçi Arama (`Scene Search`) & Replik Atlama** | Aktif `.srt/.ass` dosyasını bellekte ayrıştırıp kelime arama (`⌘F`) ile replik saniyesine zıplama; önceki/sonraki replik atlama. | `crates/features/subtitles/`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |
| **#55** | **HDR Altyazı Parlaklığı Sınırı (`sub-hdr-peak`)** | `subHdrPeakNits` (`0 = Auto` veya `150–400 nits`): HDR içerikte beyaz altyazının göz almasını engeller. | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `SheetsAndSettings.swift` |
| **#56** | **Siyah Bar (`Letterbox`) Altyazı Yerleştirme** | `subUseMargins` (`sub-use-margins` + `sub-ass-force-margins`): `21:9` filmlerde altyazıyı alt siyah boşluğa indirir. | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `SheetsAndSettings.swift` |
| **#57** | **Canlı SRT/ASS Görünüm & HUD Yukarı Kaydırma** | Yazı tipi, boyut, `%50–150` ölçek, `assOverride`, `assScaleWithWindow`, konum, saydamlık, arka kutu opaklığı, hizalama, renk, çerçeve ve `liftAboveControls` + Ayarlar canlı önizleme. | `crates/player-bridge/src/lib.rs`, `core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `SheetsAndSettings.swift` |
| **#58** | **Zaman Kaydırma (`Sub/Audio Delay`) & Tercih Kalıcılığı** | Video bazlı altyazı/ses gecikme kaydı (`video_offsets`) ve bölüm geçişlerinde aktif tercih koruma. | `crates/core/src/subtitles.rs` | `app/Cinma/PlayerMenus.swift`, `PlayerViewController.swift` |

---

## E. İndirme Yöneticisi, P2P Torrent, Arşiv ve Uzman Araçları (`#59`–`#68`)

| # | Özellik | Özet / Ayar Anahtarı | Rust Katmanı | Swift / AppKit Katmanı |
|---|---|---|---|---|
| **#59** | **Birleşik İndirme Kuyruğu & Canlı Bellek İlerlemesi** | Torrent + HTTP/1.1 + `yt-dlp` kuyruğu, çökme sonrası otomatik devam ve SSD yıpratmadan `LiveProgressMap` üzerinden akıcı UI güncellemesi. | `crates/queue/src/lib.rs`, `crates/blobstore/src/lib.rs`, `core/src/downloads.rs` | `app/Cinma/DownloadsViewController.swift` |
| **#60** | **Tek Kopyalı Torrent Akışı & İndirirken "Play Now"** | Okuma imleci öncelikli `librqbit` akışı (`head+tail` ön ısıtma), `.cinma-partial/<hash>` sahneleme, indirirken anında oynatma ve bitince hard-link taşıma. | `crates/features/torrent/src/lib.rs`, `core/src/downloads.rs` | `app/Cinma/DownloadsViewController.swift` |
| **#61** | **Çok Parçalı HTTP/1.1 & `yt-dlp` + Süreç İçi Remux** | Dinamik `Range` bölümlü HTTP indirme ve kısa ömürlü `yt-dlp -J` + `libavformat` video/ses birleştirme (`~/Movies/Cinma`, `~/Music/Cinma`). | `crates/features/http/`, `core/src/trailers.rs`, `downloads.rs` | `app/Cinma/DownloadsViewController.swift`, `SheetsAndSettings.swift` |
| **#62** | **Sıkıştırmasız Arşiv İçi Akış (`ZIP / RAR4 / RAR5`)** | Store-mode ZIP/RAR torrentlerini diske açmadan doğrudan `ArchiveByteSource` üzerinden oynatma. | `crates/blobstore/src/lib.rs` | — *(şeffaf oynatma yolu)* |
| **#63** | **Seeding Politikası, Hız Sınırları & VPN Kill Switch** | Oran/dakika bazlı seeding kapatma, upload/download sınırları, BSD `utun*` arayüz kilidi + `vpn_kill_switch`. | `crates/core/src/disk.rs`, `phase9/`, `features/torrent/` | `app/Cinma/SheetsAndSettings.swift` |
| **#64** | **`.torrent` Dosya Sahipliği & Pano Magnet/URL Yakalayıcı** | Çift tıklanan `.torrent` açma (`Info.plist`), `clipboardCatcherEnabled`, `⌘U` ile Hızlı URL/Magnet penceresi (*Oynat / İndir*). | `crates/core/src/downloads.rs` | `app/Cinma/AppDelegate.swift`, `app/Config/Info.plist` |
| **#65** | **İndirme Bitti Bildirimi & Menü Çubuğu Modu** | `notifyOnDownloadComplete` (`UserNotifications`) ve `menubarEnabled` ile pencere kapalıyken menü çubuğunda indirme kontrolü. | `crates/core/src/downloads.rs` | `app/Cinma/MenuBarController.swift`, `AppDelegate.swift` |
| **#66** | **Saat Bazlı İndirme Zamanlayıcısı** | `downloadScheduleEnabled`, `startHour`, `endHour` (gece yarısı devri destekli): izinli saat dışında kuyruğu duraklatır, pencere açılınca devam ettirir. | `crates/core/src/downloads.rs` | `app/Cinma/AppDelegate.swift`, `SheetsAndSettings.swift` |
| **#67** | **İsteğe Bağlı Yerel Unix Kontrol Soketi (`cinma.sock`)** | Varsayılan kapalı (`localApiSocketEnabled = false`); açıldığında satır bazlı JSON-RPC (`status`, `tools/list`, `tools/call`, `downloads.add_*`) sunar. | `crates/core/src/local_api.rs` | `app/Cinma/SheetsAndSettings.swift` |
| ~~**#68**~~ | ~~**Disk Kullanım Özeti & Önbellek Temizleme**~~ | **Kaldırıldı (sahip kararı, 2026-11-18, `b0f1641`).** İzleme dosyaları oynatma bitince silindiği ve indirmeler kullanıcı klasörüne taşındığı için yönetilecek önbellek kalmadı (ADR 0016). Numara, eski referanslar bozulmasın diye boş bırakıldı. | — | — |

---

## F. Entegrasyonlar, Tema, Tasarım & Sistem (`#69`–`#77`)

| # | Özellik | Özet / Ayar Anahtarı | Rust Katmanı | Swift / AppKit Katmanı |
|---|---|---|---|---|
| **#69** | **Chromecast & DLNA Yayınlama** | Süreç içi mDNS + SSDP keşfi; yalnızca yayın aktifken yaşayan geçici LAN HTTP `Range` sunucusu. | `crates/features/casting/src/lib.rs`, `core/src/playback.rs` | `app/Cinma/PlayerMenus.swift`, `IntegrationsView.swift` |
| **#70** | **Trakt, Simkl, AniList, MAL & Letterboxd Senkronu** | Trakt cihaz kodu & çift yönlü çekme; `cinma://oauth` PKCE (Simkl/AniList/MAL); çevrimdışı SQLite outbox (otomatik eşitleme), `malCountRewatches`. *(Atıl `pauseScrobbleOnPause` ayarı `b0f1641` ile kaldırıldı.)* | `crates/features/tracking/src/lib.rs`, `core/src/integrations.rs`, `phase9/` | `app/Cinma/IntegrationsView.swift`, `AppDelegate.swift` |
| **#71** | **Canlı TV / IPTV (`M3U`, `Xtream`, `XMLTV EPG`)** | Kaynak yönetimi, kanal arama/favori, anlık/sonraki EPG programı ve `mpv` üzerinden canlı yayın. | `crates/features/iptv/src/lib.rs`, `core/src/phase9/` | `app/Cinma/LiveTvViewController.swift`, `IntegrationsView.swift` |
| **#72** | **Discord Rich Presence** | Süreç içi `$TMPDIR/discord-ipc-*` bağlantısı; `discordHideTitle`, `discordShowWhenPaused`, `discordShowWhenBrowsing` gizlilik kontrolleri. | `crates/features/discord/src/lib.rs`, `core/src/phase9/` | `app/Cinma/IntegrationsView.swift`, `PlayerViewController.swift` |
| **#73** | **Tema Vurgusu, Cam Yoğunluğu & Sekme Gizleme** | `themeAccent` (*Amber/Crimson/Cyan/Emerald/Violet*), `reduceGlassEffect`, `backdropDimPct` (`20–90%`), `hiddenNavTabs`. | `crates/core/src/subtitles.rs` | `app/Cinma/DesignSystem.swift`, `MainShellViewController.swift` |
| **#74** | **Ayarlar Arama Çubuğu & Sekmeli Ayar Düzeni** | `General / Playback / Subtitles / Downloads / Shortcuts / Backup` sekmeleri ve 80+ ayarda canlı arama (`searchableSettings`). | — | `app/Cinma/SheetsAndSettings.swift` |
| **#75** | **Çift Dilli Yerelleştirme (`EN` / `TR`)** | `uiLanguage` (`system/tr/en`), `Localizable.xcstrings` içinde **~950** etkin anahtar, eksik Türkçe çeviri yok (2026-11-19). | — | `app/Cinma/Localizable.xcstrings`, `SheetsAndSettings.swift` |
| **#76** | **Fork Family Uyumu & macOS 27 Liquid Glass AppIcon** | `AppIcon.icon` (Icon Composer 3 katmanlı cam + Light/Dark/Tinted `Assets.car` + `AppIcon.icns`). | — | `app/Cinma/AppIcon.icon/`, `app/Config/Cinma.xcconfig` |
| **#77** | **Otomatik Karşılaştırma & Ölçüm Araçları** | Soğuk açılış, `phys_footprint` RAM/CPU ölçüm betiği ve playback TTFF/seek test koşucusu. | — | `tools/measure.sh`, `tools/measure-playback.sh`, `DevAutomation.swift` |

---

## G. Faz 10 — Sertleştirme & Yayın Bekleyenler (`#78`–`#80`)

| # | Başlık | Durum / Not | Kaynak |
|---|---|---|---|
| **#78** | **Gerçek Ağ & Cihaz Sahip Testleri** | Sandbox dışı ortamda gerçek Torrentio swarm hızı, `yt-dlp` mux, gerçek Chromecast/DLNA cihazları ve canlı OAuth hesap doğrulaması. | [docs/ROADMAP.md](docs/ROADMAP.md#L41-L72) |
| **#79** | **`libmpv` + FFmpeg Uygulama İçi Gömme (`ADR 0015`)** | Homebrew ABI güncellemelerinden bağımsızlaştırmak için minimal ikililerin `Contents/Frameworks` altına alınması. | [docs/ROADMAP.md](docs/ROADMAP.md#L258-L265) |
| **#80** | **Rakip Kıyaslama & Bellek/Crash Sertleştirmesi** | `tools/measure.sh` ile Stremio v5 / IINA / Flux kıyası, torrent sonrası bellek (`mimalloc/jemalloc` deneyi) ve `yt-dlp`/casting `kill -9` testi. | [docs/ROADMAP.md](docs/ROADMAP.md#L258-L265) |
