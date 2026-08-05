# Deney 7 — User-mode Watcher / Monitoring Kabulü

**Başlangıç tarihi:** 2026-08-05  
**Sürüm:** `0.3.1`  
**Durum:** UYGULAMA HAZIR — manuel Windows/Chrome ölçümü bekleniyor

## Amaç

Faz 7'nin engelleme iddiası taşımayan user-mode monitoring katmanını ölçmek: yüksek sinyalli
tarayıcı başlatma parametreleri ile mevcut host/extension/reconciliation sinyallerinin redakte,
kurcalama tespitli audit'e ulaştığını ve gerekli olayların kullanıcıya rate-limit'li uyarı olarak
gösterildiğini doğrulamak.

Bu deney cookie hırsızlığını engellediğini, profil dosyası erişimini eksiksiz gözlediğini veya
kararlı bir saldırganı tespit edeceğini **kanıtlamaz**. Watcher aynı kullanıcı bağlamında çalışır;
durdurulabilir/atlanabilir ve false-positive/false-negative üretebilir. Kernel minifilter, ETW tabanlı
genel process sınıflandırması ve profil dizini erişim gözlemi kapsam dışıdır.

## Uygulanan kapsam

- Bir saniyelik `Win32_Process` snapshot polling ile yalnız Chrome
  `--remote-debugging-port` / `--remote-debugging-pipe` var-yok tespiti.
- PID üzerinden `CommandLine` sorgusu; komut satırı, port değeri ve `user-data-dir` hiçbir event,
  extension storage veya audit kaydına taşınmaz.
- Host disconnect, aktif lease sırasında disconnect, reconnect sonucu, reconciliation failure,
  sealed grupta lease-dışı cookie oluşumu ve selector değişimi sinyalleri.
- Native Messaging v3 `monitor.event`, `monitor.alert`, `monitor.poll` sözleşmesi.
- Extension tarafında 128 olaylık bounded `chrome.storage.local` outbox; yeniden bağlantıda
  best-effort teslim ve host event-id deduplication.
- Sabit severity sözlüğü: Yüksek / Orta / Bilgi. Orta ve Yüksek bildirimleri signal+group bazında
  10 dakika rate-limit; Bilgi yalnız audit.
- Chrome notification ve action badge; bildirim metinleri cookie/URL/komut satırı içermez.
- Yalnız debug build'de, ayrı `FCP_DATA_DIR` ile kullanılabilen
  `FCP_MONITOR_RECONCILIATION_FIXTURE=1` kabul fixture'ı; release build'de derlenmez.
- Ayrı `audit-v2-day-*.jsonl` zinciri: monoton sıra, `previous_mac`, HMAC-SHA256 `mac`, başlangıç
  doğrulaması ve DPAPI korumalı HMAC anahtarı/anchor.

Önceki `audit-day-*.log` dosyaları imzasız tarihsel kayıtlardır; v2 tarafından doğrulanmış gibi
yorumlanmaz ve silinmez. V2 zinciri ayrı dosya ailesinde başlar.

## Severity sözleşmesi

| Severity | Sinyaller | Kullanıcı davranışı |
|---|---|---|
| Yüksek | remote-debugging port/pipe, aktif lease sırasında host disconnect, reconciliation failure | Audit + rate-limit'li notification + kırmızı badge |
| Orta | sealed grupta lease-dışı cookie, monitoring outbox overflow | Audit + rate-limit'li notification + turuncu badge |
| Bilgi | pasif host disconnect, reconnect success, selector değişimi, process inspection unavailable | Yalnız audit |

## İlk manuel deneme — başarısız WMI event aboneliği

İlk 0.3.0 denemesi remote-debugging olayını yakalayamadı ve audit yalnız
`process_inspection_unavailable` kaydetti. Aynı native kod yolunun aşamaları normal kullanıcı
token'ıyla ayrı ayrı ölçüldü:

- `ROOT\\CIMV2` bağlantısı: başarılı.
- `SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name='chrome.exe'`: başarılı;
  en az bir gerçek Chrome komut satırı okunabildi.
- `ExecNotificationQuery` ile `Win32_ProcessStartTrace` aboneliği: **başarısız**, HRESULT
  **`WBEM_E_ACCESS_DENIED (0x80041003)`**.

Microsoft sözleşmesine göre `ExecNotificationQuery`, namespace üzerinde gerekli event-query
yetkileri yoksa bu HRESULT'ı döndürür. Native host'u yükseltmek veya makinenin WMI namespace ACL'sini
değiştirmek §9.2 normal-kullanıcı sınırını ihlal edeceği için seçilmedi.

Çözüm event aboneliğini tamamen kaldırıp yönetici istemeyen, salt-okunur `Win32_Process` sorgusunu
bir saniyede bir çalıştırmaktır. Aktif PID'ler her turda uzlaştırılır; aynı PID/sinyal yalnız bir kez
üretilir ve sona eren PID'ler dedup kümesinden çıkarılır. Tam komut satırı yalnız sorgu turundaki
geçici bellekte parser'a verilir, event/audit/storage'a taşınmaz. Polling çok kısa ömürlü, iki sorgu
arasında başlayıp biten process'i kaçırabilir; bu user-mode watcher'ın kabul edilen false-negative
sınırıdır.

Yeni yol normal kullanıcı bağlamında iki gerçek testle doğrulandı: mevcut Chrome process'inden
`CommandLine` okuma **PASS**; ayrı `%TEMP%` profiliyle gerçekten
`--remote-debugging-port=0` taşıyan headless Chrome başlatıp aynı polling+parser yoluyla tespit
**PASS**.

## Otomatik doğrulamalar

- [x] Audit HMAC zinciri round-trip ve veri minimizasyonu
- [x] Satır kurcalama, sıra gerilemesi/yeniden sıralama, kesik son satır ve tam tail silme tespiti
- [x] DPAPI current-user round-trip ve değiştirilmiş blob reddi
- [x] Remote-debugging parser: eşittir/ayrı değer/büyük-küçük harf varyantları
- [x] URL substring, feature adı ve benzer switch false-positive kontrolleri
- [x] 128 kayıtlık bounded queue, event-id dedup ve 10 dakika rate-limit
- [x] Monitoring olayının hedef/dış grup lease state'ini değiştirmemesi
- [x] Extension `tsc` check/build ve monitor davranış testleri
- [x] Normal kullanıcı bağlamında gerçek salt-okunur WMI `Win32_Process` + `CommandLine` sorgusu
- [x] Geçici profil gerçek Chrome process'inde polling ile remote-debugging tespiti

Rust sonucu: otomatik paket ve clippy **PASS**. Ortam-bağımlı polling testleri normal pakette ignored;
Windows normal-kullanıcı bağlamında ayrıca çalıştırılan CommandLine ve gerçek remote-debugging
Chrome testleri **2/2 PASS**. Extension monitor testi: **PASS**;
`npm run check` ve `npm run build`: **PASS**. `cargo clippy -D warnings`: **PASS**.

## Manuel ölçüm hazırlığı

1. Host ve extension `0.3.1` birlikte kurulmalıdır; Native Messaging v2/v3 karışımı bilinçli olarak
   fail-closed olur. Normal kabul için host `Release` olarak kaydedilir.
2. Remote-debugging testi gerçek profil ile yapılmaz. `%TEMP%` altında yeni GUID dizini ve
   `--user-data-dir=<geçici-dizin> --remote-debugging-port=0` kullanan ayrı Chrome açılır.
3. Test boyunca asıl Chrome'da 0.3.1 extension/host bağlantısı açık tutulur; WMI sistemde başlayan
   geçici Chrome process'ini buradan gözler.
4. Reconciliation fixture öncesinde tüm Chrome süreçleri kapatılır. Host `Debug` kaydedilir;
   yalnız fixture Chrome'u başlatan PowerShell sürecinde `FCP_DATA_DIR=<ayrı-geçici-dizin>` ve
   `FCP_MONITOR_RECONCILIATION_FIXTURE=1` tanımlanır. Bu fixture release build'de yoktur.
5. Fixture kapatıldıktan sonra ortam değişkenleri kaldırılır, host yeniden `Release` kaydedilir ve
   normal Chrome başlatılır. Gerçek `%LOCALAPPDATA%\FursoyCookieProtector\vault` yolu fixture
   sürecinde kullanılmaz.

## Manuel kabul matrisi

| # | Kontrol | Beklenen | Gerçekleşen | Sonuç |
|---|---|---|---|---|
| 1 | Normal Chrome başlangıcı/baseline | Remote-debugging uyarısı yok | Ölçülmedi | BEKLİYOR |
| 2 | Geçici profille `--remote-debugging-port=0` | Tek Yüksek olay, notification/badge; audit'te yalnız sabit kod | Ölçülmedi | BEKLİYOR |
| 3 | Aynı sinyal rate-limit | 10 dk içinde ikinci görünür notification yok; occurrence/audit teslimi sürer | Ölçülmedi | BEKLİYOR |
| 4 | Aktif lease sırasında host process sonlandırma | Fail-closed cookie temizliği + Yüksek disconnect uyarısı + reconnect sonucu | Ölçülmedi | BEKLİYOR |
| 5 | Sealed controlled grupta dış cookie oluşumu | Orta lease-dışı-cookie olayı ve otomatik tahliye | Ölçülmedi | BEKLİYOR |
| 6 | Selector cookie değişimi | Bilgi audit olayı; kullanıcı notification'ı yok | Ölçülmedi | BEKLİYOR |
| 7 | Ayrı `FCP_DATA_DIR` reconciliation fixture | Yüksek reconciliation failure; gerçek Wikipedia vault değişmez | Ölçülmedi | BEKLİYOR |
| 8 | Outbox/reconnect | Host yokken olay bounded tutulur; reconnect sonrası bir kez audit edilir | Ölçülmedi | BEKLİYOR |
| 9 | Audit yeniden açılış doğrulaması | Geçerli zincir açılır; fixture kurcalaması fail-closed reddedilir | Ölçülmedi | BEKLİYOR |

## Veri minimizasyonu kontrolü

Manuel test sonunda `audit-v2-day-*.jsonl` satırlarında yalnız sabit event/outcome/detail kodları,
UUID'ler, zaman, sıra ve MAC alanları aranacaktır. Cookie adı/değeri/domain'i, URL, tam process
komut satırı, debug port değeri ve profil yolu bulunmamalıdır.

## Bilinen sınırlar

- WMI erişimi reddedilirse sistem aşama ve `WBEM_E_ACCESS_DENIED (0x80041003)` ayrımını taşıyan
  sabit bir `process_inspection_*` Bilgi kodu kaydeder;
  güvenlik tespiti varmış gibi davranmaz.
- Native host extension bağlantısı yaşadığı sürece çalışır; extension tamamen kaldırılmışsa veya
  hiç bağlantı kuramıyorsa görünür uyarı kanalı garanti edilmez.
- `chrome.cookies.onChanged` yalnız Chrome extension API'sinin gördüğü mutasyonları kapsar.
- Profil klasörüne hangi process'in eriştiği user-mode dosya watcher ile güvenilir biçimde
  ilişkilendirilemediği için bilerek uygulanmamıştır.
- Katman engelleyici değildir; olay gözlemi ile teslim arasında süreç sonlandırılabilir.

## Sonuç

Manuel ölçüm henüz yapılmadı. Sonuç ve GO/NO-GO kararı ham audit/notification gözlemleri alındıktan
sonra doldurulacaktır.
