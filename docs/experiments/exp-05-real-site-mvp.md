# Deney 5 — Gerçek Site Tek-Grup MVP Uyumluluğu

**Başlangıç tarihi:** 2026-08-04  
**Tamamlanma tarihi:** 2026-08-04  
**Durum:** TAMAMLANDI — BAŞARILI (`0.1.11` manuel kabul testi)

## Amaç ve sınır

Faz 5'te kontrollü dummy oturumla doğrulanan TPM/Hello + vault + native host + MV3 extension
zincirini düşük-riskli gerçek bir site olan `https://tr.wikipedia.org/` üzerinde, kullanıcının mevcut
hesabıyla tek bir login → evict/restore → logout akışında ölçmek.

Bu test otomatik veya tekrarlı login/logout yapmaz. Kimlik bilgileri extension'a, native host'a,
rapora veya loglara verilmez. Kullanıcı Wikipedia'nın kendi sayfasında yalnız bir kez manuel login olur;
§29.1 anti-abuse sınırı gereği başarısızlık halinde otomatik tekrar denenmez.

## Resmî kaynak araştırması

MediaWiki oturum belgeleri, başarılı yerel login sonrasında `<wikiID>Session`, `<wikiID>UserID`,
`<wikiID>UserName` ve yalnız “oturumu açık tut” seçilirse `<wikiID>Token` cookie'lerini tarif eder.
Wikimedia CentralAuth için bunlara ek olarak parent domain üzerinde `centralauth_Session`,
`centralauth_User` ve isteğe bağlı `centralauth_Token` bulunur; ilk ikisinin kayıp yerel cookie'leri
yeniden üretebildiği belirtilir.

Kaynaklar:

- [MediaWiki login/session debug kılavuzu](https://www.mediawiki.org/wiki/Manual:How_to_debug/Login_problems)
- [MediaWiki SessionManager ve AuthManager](https://www.mediawiki.org/wiki/Manual:SessionManager_and_AuthManager)
- [CookieSessionProvider API dokümantasyonu](https://doc.wikimedia.org/mediawiki-core/master/php/classMediaWiki_1_1Session_1_1CookieSessionProvider.html)

`tr.wikipedia.org` için wiki kimliği/prefix'i `trwiki` olarak uygulanmıştır. Gerçek profil ölçümünde
değerler okunmadan/loglanmadan yalnız ad, domain, path, HttpOnly, Secure, SameSite ve session metadata'sı
service-worker konsolundaki `FCP Wikipedia selector diagnostic` kaydıyla doğrulanacaktır. Aşağıdaki
zorunlu küme oluşmazsa enrollment başlamaz ve deney durdurulur; yeni cookie adı varsayılıp kapsam
genişletilmez.

## Account-group selector sözleşmesi

| Selector | Cookie adı | Domain | Path | Enrollment |
|---|---|---|---|---|
| `local_session` | `trwikiSession` | `tr.wikipedia.org` | `/` | Zorunlu |
| `local_user_id` | `trwikiUserID` | `tr.wikipedia.org` | `/` | Zorunlu |
| `local_user_name` | `trwikiUserName` | `tr.wikipedia.org` | `/` | Zorunlu |
| `local_token` | `trwikiToken` | `tr.wikipedia.org` | `/` | İsteğe bağlı; “oturumu açık tut” |
| `central_session` | `centralauth_Session` | `.wikipedia.org` | `/` | Zorunlu |
| `central_user` | `centralauth_User` | `.wikipedia.org` | `/` | Zorunlu |
| `central_token` | `centralauth_Token` | `.wikipedia.org` | `/` | İsteğe bağlı; “oturumu açık tut” |

`centralauth_LoggedOut` bir logout marker'ıdır ve auth restore kümesine alınmaz. Tercih, dil,
analytics, anti-abuse veya diğer Wikimedia cookie'leri de selector dışındadır.

## Implementasyon

- Extension sürümü: `0.1.11`
- İlgili sekmeler: CentralAuth parent-domain cookie'si bütün aileyi etkilediği için HTTPS
  `wikipedia.org` ve `*.wikipedia.org`; manuel hedef/health sekmesi `https://tr.wikipedia.org`
- Host izinleri: `https://tr.wikipedia.org/*`, `https://wikipedia.org/*`,
  `https://*.wikipedia.org/*`
- Snapshot: yedi exact selector için ayrı `chrome.cookies.getAll({url,name})` sorgusu, ardından
  name + normalized domain + exact path filtresi ve kimlik bazlı deduplication
- Enrollment: zorunlu beş selector mevcut olmalı; değer dahil yalnız bellek içi imza üç saniye
  değişmeden kalmalı; değer hiçbir log/storage kaydına yazılmaz
- Eviction: snapshot'taki bütün eşleşen cookie'ler vault'a doğrulanmış yazımdan sonra tek tek kaldırılır
  ve selector kümesinin tamamının yokluğu doğrulanır
- Inject: vault'taki her kayıt selector allowlist'ine tekrar doğrulanır, tüm küme yazılır ve metadata
  kimliklerinin birebir round-trip ettiği kontrol edilir
- Health check: tr.wikipedia.org sayfasının MAIN world bağlamında aynı-origin
  `/w/api.php?action=query&meta=userinfo` çağrısı; kullanıcı adı raporlanmadan yalnız anon/id durumu
  `authenticated` veya `logged_out` sonucuna çevrilir
- Content script yoktur; health check yalnız gerektiğinde `chrome.scripting.executeScript` ile çalışır

## İlk manuel koşu — geçersiz sıra ve doğrulanmış hata

Kullanıcı planlanan evict/restore sırasını tamamlamadan Wikipedia'nın gerçek “Çıkış yap” işlemini
kullandı. Bu anti-abuse ihlali veya tekrarlı login değildir; tek login içindeki manuel sıra sapmasıdır.
Koşu gerçek-site uyumluluk sonucu olarak kullanılmaz, fakat §26.1 gereği silinmez.

Extension `chrome.cookies.onChanged` içinde `info.removed=true` olaylarını yok sayıyordu. Site logout'u
auth cookie kümesini kaldırdığı halde encrypted vault eski server session'ını korudu. Sonraki boş
snapshot yolu vault'u güncellemeden lease'i `SEALED` yaptı; yeniden açılış stale cookie kümesini tekrar
inject etmeye çalıştı.

Redakte native audit bunu doğruladı:

- Son başarılı zincir: `enrollment success` → `eviction success` → `inject success` → `eviction success`.
- Logout sonrasında ilk iki restore denemesi `inject authorized` → `inject failed: health_logged_out`
  oldu; yani Wikipedia eski session'ı gerçekten reddetti.
- Akış terminal duruma geçmediği için incelenen devam penceresinde toplam 14 inject authorize edildi;
  üçü `health_logged_out`, on biri `cookie_roundtrip_failed` ile bitti ve 33 gereksiz eviction tamamlandı.
- Tarayıcı çökmedi; lease/vault tutarsızlığı ve tekrar Hello UX'i oluştu.

`0.1.11` düzeltmesi:

1. Extension'ın kendi remove işlemleri 30 saniyelik, değer içermeyen cookie-identity suppression kaydıyla
   ayrılır.
2. Dış `removed` olayı 750 ms sonra yeniden okunur; zorunlu auth kümesi eksikse overwrite/rotation değil
   `external_logout` kabul edilir.
3. `session.invalidate` native mesajı encrypted vault dosyasını siler, lease'i durable
   `UNINITIALIZED` yapar ve `session.invalidated` ile doğrular; bu yol Hello göstermez.
4. Restore health sonucu `logged_out`/`invalid_session` ise host aynı invalidation'ı otomatik uygular;
   stale session ikinci kez Hello ile sunulmaz.
5. Logout ile last-tab eviction yarışırsa invalidation, sıralı portta `evict.result` sonrasına ertelenir.
6. Vault silme ile lease metadata yazımı arasında process ölürse startup, eksik vault'u authoritative
   kabul edip stale lease'i `UNINITIALIZED` olarak onarır.

## Manuel ölçüm kapıları

1. Login öncesi selector kümesi boş olmalı.
2. Tek manuel login sonrası değer-redakte selector diagnostic zorunlu beş kaydı göstermeli.
3. Enrollment Hello göstermeden tamamlanmalı ve oturum açık kalmalı.
4. F5 yeni Hello üretmemeli; kullanıcı giriş yapmış kalmalı.
5. Son ilgili sekme kapanınca sessiz eviction tamamlanmalı.
6. Yeniden açılışta yalnız bir inject Hello çıkmalı; onay sonrası kullanıcı giriş yapmış görünmeli.
7. Idle eviction sessiz tamamlanmalı; sonraki aktivasyonda yalnız bir inject Hello çıkmalı.
8. Nihai manuel logout sonrası F5 gereksiz Hello üretmemeli.
9. Chrome çökmesi, profil bozulması, güvenlik alarmı veya tekrarlı login/logout olmamalı.

## Ölçüm sonucu

İlk geçersiz koşuda bulunan external-logout/stale-vault hatası `0.1.11` ile düzeltildikten sonra deney
temiz durumdan, tek bir manuel Wikipedia login'i kullanılarak yeniden çalıştırıldı. Nihai koşu bütün
kabul kapılarını geçti:

- Enrollment sırasında zorunlu beş selector — `local_session`, `local_user_id`, `local_user_name`,
  `central_session`, `central_user` — değerleri loglanmadan doğru yakalandı. Böylece hem yerel
  `tr.wikipedia.org` hem `.wikipedia.org` CentralAuth cookie'lerinden oluşan çoklu-cookie account group
  gerçek bir sitede doğrulandı.
- Tek seferlik login sonrası enrollment sessiz tamamlandı; Hello gösterilmedi ve oturum açık kaldı.
- F5 yeni bir Hello üretmedi ve oturum korunmuş kaldı.
- Son ilgili sekmeler kapatıldığında last-tab eviction sessiz tamamlandı.
- Site yeniden açıldığında yalnız bir inject Hello gösterildi; onaydan sonra restore edilen cookie kümesi
  geçerli oturumu geri getirdi.
- `30 s` test idle eşiğinde eviction sessiz tamamlandı; dönüşte yalnız bir inject Hello gösterildi.
- Wikipedia'nın kendi “Çıkış yap” işlemi Hello göstermeden `session.invalidate` akışını tetikledi; encrypted
  vault silindi ve lease `UNINITIALIZED` durumuna geçti.
- Nihai logout sonrasında F5 ve sekmeleri kapatıp yeniden açma gereksiz Hello üretmedi; kullanıcı
  `logged_out` kaldı ve stale-session tekrar döngüsü oluşmadı.

Bilinen UX sınırı: başarılı reopen/inject sonrasında açık Wikipedia sayfası oturum durumunu kendiliğinden
yenilemedi; kullanıcının görünür sayfa durumunu güncellemek için bir kez F5 yapması gerekti. Cookie restore
ve native health doğrulaması başarılıdır; bu bulgu güvenlik başarısızlığı değil, ayrı ele alınacak bir
sayfa-yenileme UX borcudur.

| Kontrol | Beklenen | Gerçekleşen | Sonuç |
|---|---|---|---|
| Selector metadata doğrulaması | Zorunlu 5; varsa opsiyoneller | Zorunlu 5 selector doğru yakalandı | PASS |
| Sessiz enrollment | Hello yok, authenticated | Hello yok; oturum açık kaldı | PASS |
| F5 | Hello yok, authenticated | Hello yok; oturum açık kaldı | PASS |
| Last-tab eviction | Hello yok, tüm selector'lar yok | Sessiz eviction tamamlandı | PASS |
| Reopen inject | Tek Hello, authenticated | Tek Hello; restore başarılı, görünür sayfa için F5 gerekti | PASS |
| Idle eviction | Hello yok, tüm selector'lar yok | Sessiz eviction; dönüşte tek inject Hello | PASS |
| External logout invalidation | Hello yok, vault yok, lease uninitialized | Vault otomatik silindi; lease `UNINITIALIZED` oldu | PASS |
| Final logout + F5 + reopen | Gereksiz Hello yok, logged_out | Hello yok; logged-out kaldı, tekrar döngüsü yok | PASS |

## Sonuç ve karar

**PASS.** Faz 5 tek-grup MVP zinciri, kontrollü test uygulamasından sonra §29.1 sırasındaki ikinci kapı
olan düşük-riskli gerçek site `tr.wikipedia.org` üzerinde de uçtan uca doğrulandı. Yerel ve CentralAuth
cookie'lerinden oluşan beş zorunlu selector enrollment, last-tab/idle eviction ve Hello-gated inject
boyunca birlikte korunup geri yüklendi.

İlk koşunun external logout bulgusu gerçek bir ürün hatasıydı: site tarafından silinen cookie'lerin
vault'taki stale oturumu geçersiz kılmaması tekrar Hello/restore döngüsü yaratıyordu. `session.invalidate` /
`session.invalidated` protokolü, extension'ın kendi silmeleri ile dış silmeleri ayıran suppression kaydı ve
restore health başarısızlığında tek-seferlik invalidation ile sorun giderildi; nihai koşu düzeltmeyi de
doğruladı. Sonraki gerçek hedeflere genelleme yapılmaz; daha yüksek riskli siteler ayrıca ve açık kullanıcı
onayıyla test edilmelidir.
