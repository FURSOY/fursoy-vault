# Deney 4 — Duty Cycle Ölçümü

**Başlangıç tarihi:** 2026-08-03  
**Tamamlanma tarihi:** 2026-08-03  
**Durum:** **TAMAMLANDI — geçerli 5 dakikalık ölçüm başarılı; §22.4 devam kriteri karşılandı.**

## Amaç ve sınır

Kendi kontrolümüzdeki `http://localhost:43118` dummy oturumunda session cookie'sinin browser
store'da açık kaldığı süreyi gerçek Chrome olaylarına dayalı otomatik inject/evict davranışı altında
ölçmek. Gerçek hesap, gerçek kimlik bilgisi veya gerçek oturum cookie'si kullanılmaz.

Bu deney yalnız sentetik localhost oturumunun duty cycle davranışını ölçer. Sonuç tek başına gerçek
bir sitenin uyumluluğunu, bütün günlük kullanımı veya üretim güvenlik garantisini kanıtlamaz.

## Uygulama

- Çalışma alanı: `poc/session-probe/`
- Derleme: yalnızca `tsc`; bundler yok
- Test uygulaması: Deney 3'teki Node.js sunucu, bellek içi session store ve dummy login
- Harness: toolbar action → `chrome.tabs.create` → `duty.html` tam sayfası
- Durable deney durumu: MV3 service worker askıya alınmalarına dayanması için
  `chrome.storage.session`; yalnız sentetik cookie snapshot'ı tutulur ve ölçüm bitince snapshot
  temizlenir
- Süre sonlandırma: `chrome.alarms`
- Idle algılama: gerçek `chrome.idle.onStateChanged` olayı
- Sekme algılama: `chrome.tabs.onUpdated`, `onActivated`, `onRemoved` ve
  `chrome.windows.onFocusChanged`

Manifest host izinlerinde Deney 3'te bağlayıcı hale gelen portsuz biçim korunur:
`http://localhost/*` ve `http://127.0.0.1/*`. Port içeren host permission kullanılmaz.

## Ölçüm tanımları

Ölçüm penceresi harness'in `start` komutundan süre alarmı veya manuel `stop` olayına kadar geçen ve
Chrome'un açık olduğu gözlenen süredir. Tarayıcı kapalı geçen duvar saati paydaya eklenmez.

```text
exposure_duty_cycle = cookie_present_ms / browser_open_time_ms

active_exposure =
    cookie mevcut
    AND ilgili sekme odaktaki Chrome penceresinde aktif
    AND chrome.idle durumu active

unnecessary_exposure =
    cookie mevcut
    AND active_exposure koşulu sağlanmıyor

unnecessary_exposure_ratio = unnecessary_exposure_ms / browser_open_time_ms
```

Her durum geçişinden önce önceki durumun süresi tek zaman çizelgesinde biriktirilir. Bu nedenle
yuvarlama farkları dışında şu invariant beklenir:

```text
cookie_present_ms = active_exposure_ms + unnecessary_exposure_ms
```

## Otomatik davranış

1. Harness first-party localhost sekmesini aktif açar, önceki dummy session'ı temizler ve yalnız bir
   kez dummy login yapar.
2. Login sonrası service worker cookie snapshot'ını alır ve başlangıç cookie-varlık durumunu kaydeder.
3. Yapılandırılan aktif kullanım fazı boyunca cookie açık kalır.
4. Harness son ilgili sekmeyi kapatır. Gerçek `chrome.tabs.onRemoved` dinleyicisi
   `last_tab_closed` olayını üretir, cookie'yi kaldırır ve yokluğunu yeniden sorgulayarak doğrular.
5. Kapalı-sekme fazında cookie yok kalır.
6. Harness ilgili sekmeyi yeniden açar. `tabs.onUpdated` dinleyicisi saklanan snapshot'ı inject eder;
   content script üzerinden taze `/api/protected` isteğiyle oturumun `authenticated` olduğu
   doğrulanır.
7. Kullanıcı mouse/klavye kullanmadan bekler. Yapılandırılan eşikte gerçek `chrome.idle` olayı
   `idle_start` üretir ve otomatik eviction çalışır.
8. `chrome.alarms` ölçümü seçilen sürede sonlandırır. Harness nihai metrikleri ve olayları düz metin
   rapora dönüştürür.

Idle fazı sentetik bir fonksiyon çağrısıyla taklit edilmez. Manuel ölçümde kullanıcının sistemle
etkileşmemesi gerekir; rapordaki `idle_start_count` bunun gerçekten oluşup oluşmadığını gösterir.

## Olay sözlüğü

| Olay | Anlamı |
|---|---|
| `inject` | Snapshot browser store'a yazıldı ve protected health check başarılı oldu |
| `last_tab_closed` | Son ilgili origin sekmesi kapandı |
| `idle_start` | `chrome.idle` durumu `idle` veya `locked` oldu |
| `eviction` | Remove tamamlandı ve cookie yokluğu yeniden doğrulandı |
| `reconciliation` | Service worker başlangıcı, ölçüm başlangıcı veya beklenmedik store farkı uzlaştırıldı |
| `failed_eviction` | Remove çağrısı ya da kaldırma-sonrası yokluk doğrulaması başarısız oldu |
| `site_cookie_recreated` | Extension'ın beklediği remove/set dışında cookie yeniden ortaya çıktı |

Ek bağlam olayları `measurement_started`, `login_detected` ve `measurement_stopped` olarak tutulur.
Cookie değeri olay tablosuna veya kopyalanan rapora yazılmaz.

## Manuel ölçüm yapılandırması

Başlangıç varsayılanları:

| Alan | Varsayılan |
|---|---:|
| Toplam süre | 5 dakika |
| Aktif kullanım fazı | 30 saniye |
| Son sekmenin kapalı kaldığı faz | 20 saniye |
| Idle eşiği | 30 saniye |

Toplam süre 1–30 dakika arasında kullanıcı tarafından değiştirilebilir. Harness, aktif + kapalı +
idle fazlarının ölçüm penceresine sığmadığı bir yapılandırmayı başlatmaz.

## İlk manuel çalışma — geçersiz ölçüm

İlk çalışma bir ürün veya harness hatası nedeniyle değil, manuel uygulama hatası nedeniyle erken
idle'a girdi. Kullanıcı fiziksel olarak bilgisayardan **aktif kullanım fazı sırasında** uzaklaştığı
için gerçek `chrome.idle` eşiği planlanan faz geçişiyle aynı zamanda tetiklendi; koşu beklenen
aktif → last-tab-close → yeniden açma/inject sırasını tamamlayamadı ve `inject_count=0` ile yarım
kaldı.

Bu koşu duty-cycle sonucu olarak kullanılmaz ve aşağıdaki nihai metriklere katılmaz. Bununla birlikte
§26.1 gereği metodoloji hatasının tarihsel kaydı olarak silinmez. Geçersiz koşunun sonucu ile ikinci
koşunun doğrulanmış sonucu birbirinden ayrı tutulur.

## İkinci manuel çalışma — geçerli ve başarılı

- **Başlangıç:** `2026-08-03T16:25:02.783Z`
- **Bitiş:** `2026-08-03T16:30:02.790Z`
- **Sonlandırma:** `duration_elapsed`
- **Ölçüm penceresi:** `300007 ms` — tam 5 dakika
- **Idle eşiği:** 30 saniye
- **Manuel gözlem:** Çökme, profil bozulması veya beklenmedik davranış görülmedi

### Ölçülmüş sonuçlar

| Metrik | Ölçülen değer | Durum |
|---|---:|---|
| `browser_open_time_ms` | `300007 ms` | Ölçüldü |
| `cookie_present_ms` | `42033 ms` | Ölçüldü |
| `exposure_duty_cycle` | **%14,011** | §21.3 genel `<%15` başlangıç hedefini karşıladı |
| `active_exposure_ms` | `41998 ms` | Ölçüldü |
| `unnecessary_exposure_ms` | **`35 ms`** | Ölçüldü |
| `unnecessary_exposure / browser_open_time` | **%0,012** | Ana optimizasyon hedefi; §22.4 karşılandı |
| Cookie'nin bulunmadığı süre | `257974 ms` (%85,989) | Türetilmiş |
| Inject | `1`, başarılı | PASS |
| Son ilgili sekme kapanışı | `1` | PASS |
| Idle başlangıcı | `1` | PASS |
| Eviction | `2/2` başarılı | PASS |
| Başarısız eviction | `0` | PASS |
| Reconciliation | `2` | Ölçüldü |
| Site tarafından yeniden oluşturulma | `0` | PASS |
| Kalıcı profil bozulması | `0` | Kullanıcı gözlemiyle doğrulandı |
| Restore sonrası güvenlik alarmı | Uygulanamaz — dummy hesap | Sentetik kapsam |

Süre invariantı tam karşılandı:

```text
active_exposure_ms + unnecessary_exposure_ms
= 41998 + 35
= 42033 ms
= cookie_present_ms
```

Cookie'nin açık olduğu sürenin %99,917'si gerçek aktif kullanıma, yalnız %0,083'ü gereksiz
maruziyete karşılık geldi.

### Ölçülmüş olay akışı

1. `t=0`: Tek dummy login tamamlandı ve session cookie snapshot'ı alındı.
2. Yaklaşık 31 saniye ilgili sekme aktif kullanıldı.
3. Son ilgili sekme kapandı; `last_tab_closed` olayı otomatik eviction çalıştırdı ve cookie yokluğu
   doğrulandı.
4. Yaklaşık 20 saniyelik kapalı-sekme fazında cookie store'da bulunmadı.
5. İlgili sekme yeniden açıldı; snapshot otomatik inject edildi ve session restore başarılı oldu.
6. Yaklaşık 11 saniye sonra gerçek `chrome.idle` olayı algılandı; ikinci otomatik eviction başarılı
   oldu.
7. Ölçümün kalan yaklaşık dört dakikasında cookie geri gelmedi;
   `site_cookie_recreated_count=0` kaldı.
8. `chrome.alarms` ölçümü planlanan beş dakikada `duration_elapsed` nedeniyle sonlandırdı.

### §21 yorumu

`exposure_duty_cycle=%14,011`, bu beş dakikalık sentetik akışta cookie'nin browser-open süresinin
yaklaşık %14'ünde store'da bulunduğunu gösterir. Bu değer §21.3'teki gün boyu Chrome açıkken genel
`<%15` başlangıç hedefini karşılar; kritik hesap `<%2` ve dengeli hesap `<%10` hedeflerini tek başına
karşılamaz. Ancak bu iki eşik farklı ve daha uzun gerçek kullanım dağılımları için tanımlanmıştır;
tek beş dakikalık sentetik koşudan onlar için genelleme yapılmaz.

Ürünün §21'de belirtilen **ana optimizasyon hedefi** toplam duty cycle değil,
`unnecessary_exposure / browser_open_time` oranıdır. Ölçülen **%0,012** oranı, cookie'nin ilgili
sekme kullanılmıyorken yalnız 35 ms açık kaldığını gösterir. Son sekme kapanışı ve idle
tetikleyicileri kullanılmayan cookie'yi ölçüm çözünürlüğü içinde neredeyse anında tahliye etmiştir.
Bu nedenle §22.4 devam kriteri **karşılandı**.

## Uygulanan manuel çalıştırma yöntemi

1. `poc/session-probe/` altında `npm run check` ve `npm run build` çalıştır.
2. Aynı dizinde `npm run serve` ile yalnız loopback üzerinde çalışan test sunucusunu başlat.
3. `chrome://extensions` içinde mevcut session-probe unpacked uzantısını yeniden yükle.
4. Toolbar ikonuna tıkla; açılan tam sayfada süre/faz değerlerini seç.
5. **Run duty-cycle simulation** düğmesine bas.
6. Harness idle fazına geçtiğini söylediğinde ölçüm tamamlanana kadar mouse veya klavye kullanılmadı.
7. Sonuç tablosunda en az bir `last_tab_closed`, `inject`, `idle_start` ve bunlara karşılık gelen
   başarılı `eviction` olayını doğrula.
8. **Copy report as text** ile ham raporu kopyala; Chrome/profil davranışına ilişkin manuel gözlemi
   ayrıca kaydet.

## Sonuç

Deney 4 **GO** ile tamamlandı. Geçerli ikinci koşuda `failed_eviction_count=0`, iki gerçek tetikleyici
üzerinden `2/2` başarılı eviction, `1/1` başarılı yeniden inject ve sıfır kendiliğinden cookie
oluşumu ölçüldü. Cookie toplam sürenin %14,011'inde açıktı; bunun browser-open zamanına göre yalnız
%0,012'si gereksiz maruziyetti. Kullanıcı beş dakika boyunca çökme, kalıcı profil bozulması veya
başka beklenmedik davranış gözlemlemedi.

Sonuç kontrollü localhost dummy oturumuna aittir. Gerçek siteler, uzun günlük kullanım dağılımları,
çoklu profil/incognito ve partitioned cookie davranışı ayrıca doğrulanmadan bu sonuca dahil edilmez.
