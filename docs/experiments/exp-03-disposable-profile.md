# Deney 3 — Disposable Profile Uçtan Uca Oturum Probe

**Başlangıç tarihi:** 2026-08-03
**Tamamlanma tarihi:** 2026-08-03
**Durum:** **TAMAMLANDI — 136/136 PASS, 10/10 restore, §22.3 devam kriterlerinin tamamı karşılandı.**

## Amaç ve sınır

Kendi kontrolümüzdeki `http://localhost:43118` test uygulamasının ürettiği sunucu taraflı dummy
oturumun, cookie snapshot → eviction → restore döngülerinden sonra gerçekten çalışmaya devam edip
etmediğini ölçmek. Gerçek hesap, gerçek kimlik bilgisi veya gerçek oturum cookie'si kullanılmaz.

Bu deney Deney 2'den farklı olarak yalnızca attribute eşitliğine bakmaz. Başarı için korumalı
endpoint cookie kaldırıldıktan sonra `logged_out`, aynı snapshot geri yüklendikten sonra yeniden
`authenticated` dönmelidir.

## Uygulama

- Çalışma alanı: `poc/session-probe/`
- Derleme: yalnızca `tsc`; bundler yok
- Test uygulaması: Node.js HTTP sunucusu, bellekte `Map` tabanlı session store
- Dummy login: sabit, gerçek olmayan test kimlik bilgileri
- Session cookie: sunucunun ürettiği rastgele ID; `HttpOnly`, `SameSite=Lax`, `Path=/`
- Korumalı endpoint: cookie yoksa `missing_cookie`, sunucuda geçersizse `invalid_session`
- Logout: sunucu tarafındaki session kaydını siler ve cookie'yi expire eder
- Harness: toolbar action → `chrome.tabs.create` → tam sayfa; ayrıca gerçek test origin'inde
  inactive bir first-party sekme ve content script
- Varsayılan tekrar: aynı login/session üzerinde `N=10`; döngüler arasında yeniden login yok
- Gizlilik: session cookie değeri her zaman redakte edilir; sabit sentetik cookie adı yalnızca geçici
  metadata tanısında görünebilir

## Ölçüm akışı

1. Eski extension-fetch metoduyla ayrı bir tanı session'ı oluştur; login ve protected kontrolünden
   hemen sonra `chrome.cookies.getAll({url})` çağrısını **name/storeId/partitionKey filtresi olmadan**
   çalıştır. Dönen her cookie'nin değeri dışındaki bütün API alanlarını, özellikle `storeId` ve
   `partitionKey` değerlerini raporla; ardından bu tanı session'ını temizle.
2. Gerçek `http://localhost:43118/` sekmesini inactive olarak aç; bu sekmeye bağlı content script
   hazır olana kadar bekle ve cookie store'u bu web sekmesinin `tabId` değerinden seç.
3. Login ve bütün protected/logout kontrollerini extension sayfasından değil, test sekmesindeki
   content script'in same-origin `fetch` çağrılarıyla çalıştır.
4. Asıl deney session'ı için bir kez login yap ve korumalı endpoint'te `authenticated` durumunu
   doğrula.
5. First-party login sonrasında filtresiz metadata tanısını yeniden al.
6. `chrome.cookies.getAll` ile session cookie snapshot'ı al.
7. `chrome.cookies.remove` ile cookie'yi kaldır ve store'da bulunmadığını doğrula.
8. Korumalı endpoint'in `logged_out/missing_cookie` döndürdüğünü doğrula.
9. Snapshot attribute ve değeriyle `chrome.cookies.set` çağır.
10. Korumalı endpoint'in yeniden `authenticated` döndürdüğünü doğrula.
11. Adım 6–10'u aynı session üzerinde N kez tekrarla.
12. Döngüler bitince gerçek logout endpoint'ini çağır; eski snapshot geri yazılsa bile
   `logged_out/invalid_session` döndüğünü doğrulayarak sunucu tarafı invalidation kontrolünü tamamla.
13. Probe cookie'sini temizle, first-party sekmeyi kapat ve sunucuda aktif test session'ı kalmadığını
    doğrula.

Harness ayrıca yarıda kalan bir çalışmanın dummy session bırakmaması için başlangıçta ve `finally`
temizliğinde probe-only reset endpoint'ini kullanır. Sunucu tarafı invalidation ölçümü bu reset'e
dayanmaz; adım 12'de normal logout endpoint'iyle ayrıca doğrulanır.

## İlk manuel çalışma — başarısız, silinmedi

- **Başlangıç:** `2026-08-03T01:02:19.964Z`
- **Origin:** `http://localhost:43118`
- **Store:** `0`
- **İstenen döngü:** 10
- **Tamamlanan döngü:** 0
- **Kontrol özeti:** 2/4 PASS

| Döngü | Kontrol | Beklenen | Gerçekleşen | Sonuç | Süre (ms) |
|---|---|---|---|---|---:|
| setup | single login | `true` | `true` | PASS | 2.2 |
| setup | session authenticated after login | `authenticated` | `authenticated` | PASS | 0.0 |
| 1 | snapshot cookie count | `1` | `0` | **FAIL** | 1.0 |
| suite | fatal error | suite completes | `cycle 1: expected exactly one session cookie, got 0` | **FAIL** | 0.0 |

### Kanıtlanan durum

- Extension sayfasından `credentials: "include"` ile yapılan login başarılı oldu.
- Aynı extension-fetch bağlamındaki korumalı endpoint cookie'yi gönderdi ve `authenticated` döndü.
- Bunun hemen ardından `url + name + storeId=0` filtreli `chrome.cookies.getAll` hedef cookie için
  sıfır sonuç verdi.
- Chrome veya disposable profil çökmedi; harness kendi invariant kontrolünde hata fırlatarak durdu.

### İlk çalışma anında henüz kanıtlanmayan kök neden

Cookie'nin extension origin'ine bağlı partitioned/izole bir depoya yazılması güçlü bir adaydır;
ancak ilk raporda filtresiz metadata, gerçek `partitionKey` veya cookie'nin döndüğü store bilgisi
yoktur. Store seçimi ya da name/url filtresi uyuşmazlığı da bu raporla elenemez. Bu nedenle
partitioning açıklaması **doğrulanmış gerçek olarak kaydedilmez**.

### Uygulanan düzeltme ve tanı

- Eski extension-fetch login'i yalnızca ayrı bir tanı session'ında korunur. Hemen ardından
  `chrome.cookies.getAll({url: "http://localhost:43118/"})` hiçbir name/storeId/partitionKey filtresi
  olmadan çağrılır; bütün cookie metadata'sı değer redakte edilerek raporlanır.
- Asıl deney login'i ve protected/logout kontrolleri gerçek localhost sekmesine enjekte edilen content
  script üzerinden same-origin/first-party bağlamda çalışır.
- Store seçimi extension harness sekmesine göre değil, localhost test sekmesinin `tabId` değerine göre
  yapılır; eşleşme bulunamazsa fallback yerine açık hata verilir.
- Snapshot/eviction/restore işlemleri tasarlandığı gibi extension'ın `chrome.cookies` API çağrıları
  olarak kalır.

Bu değişiklik test metodolojisini gerçek kullanıcı login bağlamına yaklaştırır. Eski başarısızlığın
partitioning, store veya filtre kaynaklı kesin sınıflandırması ancak yeni rapordaki **legacy
extension-fetch** metadata tanısıyla yapılacaktır.

## İkinci manuel çalışma — tanı ilerledi, first-party reset hatası

- **Döngü:** Henüz başlamadı
- **Kontrol özeti:** 2 PASS / 2 FAIL

| Döngü | Kontrol | Beklenen | Gerçekleşen | Sonuç |
|---|---|---|---|---|
| legacy diagnostic | extension-fetch login | `true` | `true` | PASS |
| legacy diagnostic | extension-fetch protected state | `authenticated` | `authenticated` | PASS |
| diagnostic | legacy extension-fetch unfiltered `getAll({url})` cookie count | `>=1` | `0` | **FAIL** |
| suite | fatal error | suite completes | `first-party reset failed: /api/reset returned HTTP 403: {"error":"origin_not_allowed"}` | **FAIL** |

### Doğrulanmış legacy cookie bulgusu

Ölçülen Chrome ortamında extension sayfasından hedef origin'e yapılan çapraz-origin login isteği
başarılı oldu ve aynı extension-fetch bağlamındaki protected istek session cookie'sini göndererek
`authenticated` döndü. Buna rağmen hemen sonraki
`chrome.cookies.getAll({url: "http://localhost:43118/"})` çağrısı **name, storeId ve partitionKey
filtresi olmadan da sıfır cookie döndürdü**.

Dolayısıyla doğrulanmış sonuç şudur: **extension context'inden çapraz-origin fetch ile oluşturulan bu
cookie, ölçülen ortamda `chrome.cookies` API'sine görünür değildir.** Önceki name/storeId filtreleri
kök neden değildir. Cookie'nin dahili olarak otomatik partitioned/izole bir network storage alanında
tutulması olası açıklamadır; API metadata'sı hiç dönmediği için Chrome'un iç mekanizması doğrudan
ölçülmüş değildir ve kesin mekanizma olarak sunulmaz.

Bu bulgu ürünün hedef kullanım akışına genellenmez. Gerçek kullanıcı session cookie'si extension
sayfasından çapraz-origin login yapılarak değil, sitenin kendi first-party navigasyon/login bağlamında
oluşur. Bu nedenle asıl Deney 3 akışının gerçek localhost sekmesine taşınması doğrulandı; legacy yol
yalnızca negatif tanı sonucu olarak korunur.

> **Nihai düzeltme notu:** Yukarıdaki görünmezlik gözlemi gerçektir; ancak o aşamadaki
> partitioned/izole storage yorumu daha sonraki ölçümle **elendi**. Hem bu cookie hem first-party ve
> `document.cookie` cookie'leri, port içeren host permission nedeniyle `getAll()` sonucundan
> süzülüyordu. Bu bölüm §26.1 gereği ara hipotezin tarihsel kaydı olarak korunur.

### İkinci altyapı hatası ve düzeltme

First-party sekme oluşturulduktan sonra content script'in `/api/reset` POST isteği
`Origin: http://localhost:43118` taşıdı. Sunucunun `originAllowed()` kontrolü yalnızca eksik Origin
header'ını veya sabit extension origin'ini kabul ettiği için istek `403 origin_not_allowed` ile
reddedildi. Bu hata cookie round-trip sonucu değildir; döngü başlamadan önceki test sunucusu allowlist
hatasıdır.

Sunucunun exact allowlist'i aşağıdaki iki origin'i kabul edecek şekilde düzeltildi:

- `chrome-extension://dokhjkpkdknopgnjdmaogjhlelcaiigo`
- `http://localhost:43118`

`originAllowed()` ve `writeCorsHeaders()` aynı allowlist'i kullanır; başka origin'lere izin verilmez.

## Üçüncü manuel çalışma — first-party çelişkili tanı

First-party test sekmesindeki login başarılı oldu ve sonrasında çalıştırılan authenticated kontrolü
`authenticated` döndürdü. Buna karşın login sonrasındaki tamamen filtresiz
`chrome.cookies.getAll({})` çağrısı `all_cookies_count=0` bildirdi. Evict/restore döngüsüne geçilemedi.

Bu ölçüm, önceki cookie görünmezliğinin yalnız extension sayfasındaki çapraz-origin fetch bağlamına
özgü olduğu açıklamasını desteklemez: aynı görünmezlik first-party login sonrasında da ölçüldü.
`authenticated` yanıtının gerçekten ayrı protected isteğine ve bir Cookie header'ına dayanıp
dayanmadığı ile Cookies API görünümünün gecikmeli oluşup oluşmadığı bu çalışma sırasında sunucu
tarafından kanıtlanmadı. Bu nedenle kök neden açık tutulur; başarısız/çelişkili sonuç silinmez.

## Genişletilmiş teşhis — uygulandı, ara tanı

Yeni harness first-party login'in hemen ardından, protected kontrolü ve evict/restore döngüsü
başlamadan önce aşağıdaki ham verileri tek snapshot olarak toplar:

- Login sonrasında filtresiz `chrome.cookies.getAll({})` için hem anlık sonuç hem de 250 ms
  gecikmeli ikinci sonuç; gecikmeli sayı ayrıca `getAll({}) after 250ms delay cookie count`
  satırında gösterilir.
- Sunucunun son 10 `/api/login` ve `/api/protected` isteği için tuttuğu `method`, `path`, Cookie
  header varlığı ve yalnız cookie adları. Değerler bellekte tutulmaz ve raporlanmaz.

Bu ek kayıtlar yarış durumu ile yanlış authenticated sonucu olasılıklarını ayırmak için ham kanıt
üretir. Harness bunlardan otomatik kök neden veya kalıcı davranış kararı çıkarmaz.

### Dördüncü manuel çalışma — `localhost` / `127.0.0.1` karşılaştırması

Deney 2'deki localhost domain-scope sapması ile Deney 3'teki Cookies API görünmezliğinin aynı özel
hostname'den kaynaklanıp kaynaklanmadığını ayırmak için mevcut localhost testi korunarak ayrı bir
`http://127.0.0.1:43118` first-party tanısı eklendi. Sunucu yalnız `::1` ve `127.0.0.1` loopback
adreslerinde dinler; extension izni bu iki sabit origin ile sınırlıdır.

IP varyantı localhost ana akışından önce ayrı bir session ile login → anlık filtresiz `getAll({})` →
250 ms gecikmeli filtresiz `getAll({})` → protected → sunucu Cookie-header kanıtı adımlarını çalıştırır
ve session'ı temizler. Kopyalanan raporda sonuçlar
`LOOPBACK IP DIAGNOSTIC (RAW - NO AUTOMATIC INTERPRETATION)` bölümündedir. Localhost sonuçları mevcut
`EXTENDED DIAGNOSTIC` bölümünde kalır.

Manuel ölçümde iki origin için de sunucu `/api/protected` isteğinde
`FCP-session-probe` Cookie header'ını doğruladı; buna rağmen anlık ve 250 ms gecikmeli tamamen
filtresiz `chrome.cookies.getAll({})` sonuçları 0 kaldı. Aynı sonuç disposable ve normal Chrome
profillerinde gözlendi. Böylece ölçülen ortam için localhost özel-host ve kısa zamanlama hipotezleri
elendi; storeId/URL filtresi kullanılmadığı için bunlar da sonucu açıklamaz. Cookie'nin sunucu
`Set-Cookie`/HttpOnly yolu ile oluşması ve extension tarafından yazılan cookie arasındaki fark henüz
izole edilmemiştir.

### `document.cookie` yazımı karşılaştırması — uygulandı, kaynak hipotezi elendi

Her iki first-party origin'de content script
`document.cookie = "FCP-docwrite-diagnostic=1; path=/"` yazar ve adı kendi `document.cookie`
okumasında arar. Hemen ardından harness'in anlık ve 250 ms gecikmeli filtresiz `getAll({})`
snapshot'larında aynı ad aranır. Rapor aşağıdaki ham alanları ayrı ayrı gösterir:

- `docwrite_diagnostic` / `loopback_ip_docwrite_diagnostic`
- `docwrite_visible_to_cookies_api_immediate`
- `docwrite_visible_to_cookies_api_after_250ms`
- Aynı alanların `loopback_ip_` önekli karşılıkları

Tanı cookie'si snapshot alındıktan sonra content script tarafından silinir ve session döngüsüne
taşınmaz. Ayrıca gerçek login cevabında kullanılan header şablonu değeri redakte edilerek
`session_set_cookie_header_redacted` alanında ham gösterilir. Harness bu üçüncü veri noktasından
otomatik kök neden yorumu üretmez.

1. `chrome.cookies.getAll({})`: hiçbir `url`, `name`, `storeId` veya `partitionKey` filtresi yoktur.
   Dönen her cookie için yalnızca `domain`, `name`, `storeId`, `partitionKey` ve `path` raporlanır;
   cookie değeri alınan API nesnesinde bulunsa da rapora yazılmaz.
2. `chrome.cookies.getAllCookieStores()`: bütün store kayıtlarının `id` ve `tabIds` alanları
   raporlanır.
3. `chrome.tabs.get(testTabId)`: test sekmesinin `id`, gerçek `url`, `windowId` ve `incognito`
   alanları raporlanır.
4. Content script: sayfanın `origin`, `href` ve `document.cookie` içinden yalnız cookie adları
   raporlanır. Session cookie `HttpOnly` olduğu için görünmemesi beklenebilir; harness bu beklentiden
   otomatik kök neden sonucu çıkarmaz.

Kopyalanan düz metin raporda bu veriler
`EXTENDED DIAGNOSTIC (RAW - NO AUTOMATIC INTERPRETATION)` ile
`END EXTENDED DIAGNOSTIC` arasında ayrı bir bölümde bulunur. Bölüm yalnız ham veridir; store,
partitioning veya sorgu hatası hakkında otomatik yorum üretmez.

## Nihai kök neden ve kalıcı düzeltme

İki POC'un ortak hatası manifest host izinlerinin portla sınırlandırılmasıydı:

```json
"host_permissions": ["http://localhost:43118/*"]
```

Cookie'ler port bilgisi taşımaz. Chromium, `chrome.cookies.getAll()` sonucundaki her cookie için izin
kontrolü yaparken cookie'nin scheme ve domain alanlarından `http://localhost/` gibi **portsuz** bir
URL üretir. Portlu manifest kalıbı bu URL ile eşleşmediği için cookie'ler sonuçtan hata vermeden
eleniyordu. Buna karşılık `chrome.cookies.set()` ve URL tabanlı `get()` çağrıları kullanıcının verdiği
`http://localhost:43118/...` URL'sini doğrudan denetlediği için aynı portlu izinle çalışıyordu. Bu API
yolları arasındaki fark, Deney 2'nin kendi yazdığı cookie'leri okuyabildiği halde server veya
`document.cookie` kaynaklı cookie'leri `getAll()` ile göremediği yanılsamasını oluşturdu.

Kalıcı düzeltme portsuz host permission kullanmaktır:

```json
"host_permissions": ["http://localhost/*", "http://127.0.0.1/*"]
```

Content-script eşleşmeleri ve uygulama URL'leri sabit `43118` portunda kalır. Cookie kapsamı zaten
porttan bağımsız olduğundan gerçek ürün manifestinde cookie erişimi gereken host permission'lara
port eklenmeyecektir.

## Elenen ara hipotezler

Başarısız çalışmalar silinmemiştir. Nihai port düzeltmesiyle başarılı olan aynı unpacked extension,
aşağıdaki bütün ara hipotezleri kapatmıştır:

| Hipotez | Durum | Eleme kanıtı |
|---|---|---|
| 250 ms görünürlük yarışı | **Elendi** | Anlık ve gecikmeli `getAll({})` aynı boş sonucu verdi; portsuz izinle anlık okuma çalıştı. |
| `localhost` özel-host davranışı | **Elendi** | `localhost` ve `127.0.0.1` aynı sonucu verdi; ikisi de portsuz izinle görünür oldu. |
| Extension-fetch partitioning/izole storage | **Elendi** | First-party ve extension-fetch yolları aynı permission filtresinden etkileniyordu. |
| `Set-Cookie`/HttpOnly kaynağı | **Elendi** | Server cookie ile non-HttpOnly `document.cookie` cookie'si aynı şekilde etkilenmişti. |
| `document.cookie` kaynağı | **Elendi** | Deney 2 ve Deney 3 docwrite sanity kontrolleri geçerken portlu izinle API görünürlüğü yoktu; portsuz izinle görünürlük sağlandı. |
| `storeId`, URL, name veya domain filtresi | **Elendi** | Tamamen filtresiz `getAll({})` da boştu; sorun aday cookie'lerin son host-permission süzmesiydi. |
| Disposable/normal profil farkı | **Elendi** | Her iki profil aynı ara sonucu verdi; düzeltilen unpacked extension başarılı oldu. |
| Makine genelinde bozuk Cookies API | **Elendi** | Aynı makinedeki Cookie-Editor kontrolü cookie'yi okuyup silebildi; portsuz izinli POC da çalıştı. |
| Statik izin / runtime optional izin farkı | **Elendi** | Her iki izin biçimi portlu kalıpla başarısızdı; nihai statik portsuz izin başarılı oldu. |
| Popup / tam sayfa harness / arka plan / aktif sekme | **Elendi** | Görünür popup ve aktif sekme tanıları sonucu değiştirmedi; özgün tam sayfa harness portsuz izinle geçti. |
| Manifest `key` alanı | **Elendi** | Key kaldırılması sonucu değiştirmedi; çalışan Cookie-Editor manifestinde de key vardı; nihai probe sabit key ile geçti. |
| Web Store / unpacked kurulum farkı | **Elendi** | Aynı unpacked Deney 3 extension'ı yalnız permission kalıbı düzeltilerek 136/136 geçti. |
| Extension ID çakışması | **Elendi** | Deney 2 extension'ı yüklü değilken de hata sürdü; nihai sabit ID ile başarı sağlandı. |
| Site-access veya enterprise policy kısıtı | **Elendi** | Site erişimi açıktı, policy yoktu; portsuz manifest değişikliği tek başına sorunu çözdü. |

## Nihai manuel ölçüm — başarılı

**Ortam:** Windows 11 Pro build `10.0.26200`, Google Chrome `150.0.0.0`, kontrollü loopback test
uygulaması, gerçek hesap veya gerçek oturum yok.

| Sonuç | Ölçüm |
|---|---:|
| Kontrol özeti | **136/136 PASS** |
| İstenen / tamamlanan döngü | **10/10** |
| Başarılı restore | **10/10** |
| Restore başarı oranı | **%100** |
| Yanlış logout | **0/10** |
| Yanlış logout oranı | **%0** |
| Döngü içinde sunucu session invalidation | **0** |
| Restore sonrası güvenlik alarmı | **0** |
| Kalıcı profil bozulması | **0 — manuel gözlemsel doğrulama** |
| Logout invalidation kontrolü | **PASS — stale restore → `logged_out/invalid_session`** |

Tek dummy login ile oluşturulan aynı session üzerinde bütün döngüler tamamlandı; döngüler arasında
yeniden login yapılmadı. Her eviction sonrasında korumalı endpoint `logged_out/missing_cookie`, her
restore sonrasında `authenticated` döndürdü. Kontrol adımında logout endpoint'i session'ı sunucu
tarafında geçersiz kıldı; eski snapshot yeniden yazıldığında cookie tarayıcıya dönmesine rağmen
sunucu `invalid_session` verdi. Böylece test yalnız cookie varlığını değil, gerçek server-backed
oturum davranışını doğruladı.

Kullanıcı manuel gözleminde test boyunca yalnız beklenen localhost test sekmesi açılıp kapandı;
Chrome çökmesi, profil bozulması, beklenmeyen popup veya garip davranış görülmedi.

## Ölçülecek metrikler ve §22.3 kriterleri

| Metrik | Devam kriteri | Ölçülen sonuç | Durum |
|---|---:|---:|---|
| Restore başarı oranı | ≥ %99 | **%100 (10/10)** | **Karşılandı** |
| Yanlış logout oranı | ≤ %0,1 | **%0 (0/10)** | **Karşılandı** |
| Kalıcı profil bozulması | 0 | **0, gözlemsel** | **Karşılandı** |
| Restore sonrası hesap güvenlik alarmı | 0 | **0** | **Karşılandı** |
| Sunucu session invalidation | Döngülerde 0 | **0** | **Karşılandı** |
| Logout invalidation kontrolü | PASS | **stale restore → `invalid_session`** | **PASS** |

Varsayılan 10 döngüde §22.3 oranlarını karşılamak için restore sonucu 10/10, yanlış logout 0/10
olmalıdır. Daha hassas oran ölçümü gerekirse döngü sayısı artırılır; gerçek site login'i
tekrarlanmaz.

## Ölçüm durumu

**Deney 3 tamamlandı ve §22.3 devam kriterlerinin tamamı karşılandı.** Dört başarısız/çelişkili ara
çalışma ve elenen hipotezler yukarıda korunmaktadır. Nihai 10/10 sonucu kontrollü loopback uygulaması
için gerçek server-backed session evict/restore uyumluluğunu kanıtlar; gerçek sitelerde rotation,
device binding, güvenlik alarmı veya cookie dışı storage uyumluluğunu tek başına kanıtlamaz. Bu
alanların ölçümü sonraki deneylerin kapsamındadır.
