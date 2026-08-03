# Deney 2 — Cookie Attribute Probe

**Başlangıç tarihi:** 2026-08-03  
**Tamamlanma tarihi:** 2026-08-03  
**Durum:** **TAMAMLANDI — manuel Chrome ölçümü 40/43 PASS.** Genel round-trip uyumluluğu
yüksek; `localhost` domain-scope davranışında iki alan sapması ve CHIPS yazımında bir başarısızlık
ölçüldü.

## Amaç ve sınır

Gerçek cookie'lere dokunmadan, yalnızca sabit `localhost:43117` hedefinde sentetik
`FCP-probe-*` gölge cookie'leriyle `chrome.cookies` API attribute round-trip uyumluluğunu ölçmek.
Prefix kuralları, prefix'i koruyan `__Host-FCP-probe` ve `__Secure-FCP-probe` isimleriyle ayrıca
ölçülür.

Bu deney **oturumun çalışacağını kanıtlamaz**. Yalnızca API round-trip uyumluluğunu test eder;
server-side rotation, CSRF state, device binding ve cookie dışı storage bağımlılıkları bu deneyin
kapsamında değildir.

## Uygulama

- Çalışma alanı: `poc/cookie-probe/`
- Derleme: yalnızca `tsc`
- Test hedefi: `http://localhost:43117`
- Uzantı izni: yalnızca sabit localhost hostunun HTTP/HTTPS şemaları ve `cookies`
- Harness: toolbar action üzerinden `chrome.tabs.create` ile açılan tam sayfa
- Probe yaşam döngüsü: yaz → geri oku → karşılaştır → `finally` içinde sil
- Rapor çıktısı: arayüzde tablo ve panoya kopyalanabilir düz metin

## Ölçüm ortamı

| Öğe | Değer |
|---|---|
| Ölçüm başlangıcı | `2026-08-03T00:26:36.387Z` |
| OS | Windows 11 Pro, build `10.0.26200` |
| Chrome | `150.0.0.0` |
| User agent | `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36` |
| Extension ID | `dokhjkpkdknopgnjdmaogjhlelcaiigo` (manifest `key` alanıyla sabit) |
| Test origin | `http://localhost:43117` (aynı localhost/port için HTTP + HTTPS izni) |
| Cookie store | `store_id=0` — tek normal profil, incognito değil |

## Ham ölçüm

**Özet:** 40/43 kontrol geçti.

| Vaka | Attribute | Beklenen | Gerçekleşen | Sonuç |
|---|---|---|---|---|
| host-only session cookie | hostOnly | `true` | `true` | PASS |
| host-only session cookie | domain | `localhost` | `localhost` | PASS |
| host-only session cookie | session | `true` | `true` | PASS |
| host-only session cookie | storeId | `0` | `0` | PASS |
| host-only session cookie | url | `http://localhost:43117/` | `http://localhost:43117/` | PASS |
| host-only session cookie | cleanup | absent | absent | PASS |
| domain persistent cookie | hostOnly | `false` | `true` | **FAIL** |
| domain persistent cookie | domain | `.localhost` | `localhost` | **FAIL** |
| domain persistent cookie | session | `false` | `false` | PASS |
| domain persistent cookie | expirationDate | `1785720396` (±2s) | `1785720396` | PASS |
| domain persistent cookie | url | `http://localhost:43117/` | `http://localhost:43117/` | PASS |
| domain persistent cookie | cleanup | absent | absent | PASS |
| path and HttpOnly cookie | path | `/probe/deep` | `/probe/deep` | PASS |
| path and HttpOnly cookie | httpOnly | `true` | `true` | PASS |
| path and HttpOnly cookie | sameSite | `strict` | `strict` | PASS |
| path and HttpOnly cookie | url | `http://localhost:43117/probe/deep` | `http://localhost:43117/probe/deep` | PASS |
| path and HttpOnly cookie | cleanup | absent | absent | PASS |
| Secure cookie | secure | `true` | `true` | PASS |
| Secure cookie | url | `https://localhost:43117/` | `https://localhost:43117/` | PASS |
| Secure cookie | cleanup | absent | absent | PASS |
| SameSite=unspecified | sameSite | `unspecified` | `unspecified` | PASS |
| SameSite=unspecified | cleanup | absent | absent | PASS |
| SameSite=lax | sameSite | `lax` | `lax` | PASS |
| SameSite=lax | cleanup | absent | absent | PASS |
| SameSite=strict | sameSite | `strict` | `strict` | PASS |
| SameSite=strict | cleanup | absent | absent | PASS |
| SameSite=no_restriction with Secure | sameSite | `no_restriction` | `no_restriction` | PASS |
| SameSite=no_restriction with Secure | secure | `true` | `true` | PASS |
| SameSite=no_restriction with Secure | url | `https://localhost:43117/` | `https://localhost:43117/` | PASS |
| SameSite=no_restriction with Secure | cleanup | absent | absent | PASS |
| CHIPS partition key | write/read | round-trip succeeds | `chrome.cookies.set returned no cookie` | **FAIL** |
| CHIPS partition key | cleanup | absent | absent | PASS |
| `__Host-` prefix | prefix | `__Host-FCP-probe` | `__Host-FCP-probe` | PASS |
| `__Host-` prefix | hostOnly | `true` | `true` | PASS |
| `__Host-` prefix | domain | `localhost` | `localhost` | PASS |
| `__Host-` prefix | path | `/` | `/` | PASS |
| `__Host-` prefix | secure | `true` | `true` | PASS |
| `__Host-` prefix | url | `https://localhost:43117/` | `https://localhost:43117/` | PASS |
| `__Host-` prefix | cleanup | absent | absent | PASS |
| `__Secure-` prefix | prefix | `__Secure-FCP-probe` | `__Secure-FCP-probe` | PASS |
| `__Secure-` prefix | secure | `true` | `true` | PASS |
| `__Secure-` prefix | url | `https://localhost:43117/` | `https://localhost:43117/` | PASS |
| `__Secure-` prefix | cleanup | absent | absent | PASS |

## Analiz

### Doğrulanan davranışlar

Aşağıdaki alanlar ölçülen Windows 11 / Chrome 150 / tek normal profil ortamında tam uyumlu round-trip
etti:

- Host-only cookie ve session/`expirationDate` ilişkisi
- Kalıcı cookie için `session=false` ve `expirationDate`
- `path` ve `httpOnly`
- `secure` ve secure cookie için HTTPS URL üretimi
- Dört `sameSite` değeri: `unspecified`, `lax`, `strict`, `no_restriction`; sonuncusunda
  `secure=true`
- `storeId=0` değerinin yazma/okuma round-trip'i
- `__Host-` için `secure=true`, `path=/`, domain verilmemesi ve `hostOnly=true`
- `__Secure-` için `secure=true`
- Bütün probe vakalarında cleanup sonrası cookie'nin bulunmaması

### `localhost` domain-scope sapması

**Doğrulanmış gerçek:** `domain: "localhost"` verilerek yazılan kalıcı cookie,
`hostOnly=true` ve `domain="localhost"` olarak geri döndü. Beklenen `hostOnly=false` ve
`domain=".localhost"` değerleri gözlenmedi. Session ve expiration alanları yine doğru round-trip
etti.

**Doğrulanmamış açıklama:** Chrome, kayıtlı bir public suffix/TLD'si olmayan özel `localhost`
hostunu domain-scoped cookie için özel ele alıp isteği host-only cookie'ye düşürüyor olabilir. Bu
deney gerçek bir eTLD+1 üzerinde çalıştırılmadığı için `.example.com` benzeri domain'lerde aynı
davranışın tekrarlanıp tekrarlanmayacağı bilinmiyor. Bulgular bu nedenle test ortamına özgü bir
kısıt olarak kaydedildi; genel domain-cookie uyumsuzluğu sonucu çıkarılmadı.

### CHIPS / `partitionKey` başarısızlığı

**Doğrulanmış gerçek:** `partitionKey.topLevelSite=http://localhost:43117` ile partitioned cookie
yazma isteğinde `chrome.cookies.set` cookie döndürmedi; probe bunu
`chrome.cookies.set returned no cookie` olarak raporladı. Cookie oluşmadı ve cleanup kontrolü geçti.

**Doğrulanmamış açıklama:** CHIPS yazımı gerçek bir top-level site / üçüncü-taraf iframe bağlamı
gerektiriyor olabilir veya extension, `chrome.cookies.set` üzerinden doğrudan top-level bağlamdan
istenen partition key'i yazamıyor olabilir. Bu ihtimaller mevcut ölçümle birbirinden ayrılamaz.
Başarısızlık silinmez veya başarı olarak yorumlanmaz; PLAN.md içinde **Q18** olarak açık kalır ve
gerçek üçüncü-taraf bağlamlı kontrollü bir testle kapanacaktır.

## Nihai sonuç

Cookie attribute round-trip uyumluluğu ölçülen kapsamda genel olarak yüksektir: **40/43 PASS**.
Başarısız üç kontrolün ikisi aynı `localhost` domain-scope sapmasına, biri CHIPS cookie yazımının
başarısız olmasına aittir. Host-only, path/HttpOnly, Secure, dört SameSite değeri,
session/expirationDate ilişkisi, normal profil `storeId=0` ve prefix kuralları doğrulandı.

Bu sonuç **oturumun canlı çalışacağını kanıtlamaz**. Server-side rotation, CSRF state, device
binding, `localStorage` / `IndexedDB` bağımlılıkları ve gerçek bir oturumun evict/restore sonrasında
yaşaması yalnızca disposable profile uçtan uca deneyinde doğrulanabilir.
