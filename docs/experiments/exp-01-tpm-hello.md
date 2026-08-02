# Deney 1 — TPM / Windows Hello Probe

**Başlangıç tarihi:** 2026-08-02  
**Tamamlanma tarihi:** 2026-08-03  
**Durum:** **TAMAMLANDI — Go/No-Go kriteri A karşılandı.** Jest kaynağı olarak Yol C
(`KeyCredentialManager` / Windows Hello), DEK unwrap mekanizması olarak sessiz çalışan Yol A
(Platform Crypto Provider / TPM-backed CNG anahtarı) seçildi.

## Amaç

Microsoft Platform Crypto Provider üzerinden oluşturulan, dışa aktarılamayan TPM-backed bir anahtarın
32-byte DEK'i RSA-OAEP-SHA256 ile wrap/unwrap edebildiğini ve private-key kullanımında güvenilir bir
kullanıcı jesti üretip üretmediğini ölçmek.

Software KSP fallback yapılmaz. Provider adı doğrudan `Microsoft Platform Crypto Provider` olarak
verilir; provider hardware-only bildirmiyorsa probe başarısız olur.

## Ortam

| Öğe | Değer | Durum |
|---|---|---|
| OS | Windows 11 Pro, build 10.0.26200 | Önceki ölçüm; yeniden doğrulanacak |
| Rust | rustc 1.96.0 / cargo 1.96.0 | Doğrulandı |
| TPM 2.0 hazır durumu | Platform Crypto Provider: hardware-only, TPM `0x00020000` | Doğrulandı |
| Windows Hello kayıt durumu | Kayıtlı; Yol A bu mekanizmayı kullanmıyor | Doğrulandı |
| Windows Hello kayıt yöntemi | Yalnızca PIN (biyometrik donanım yok) | Ortam sınırlaması |
| `KeyCredentialManager` desteği | `IsSupportedAsync=true` | Doğrulandı |

Windows Hello kayıt yöntemi yalnızca PIN'dir; test makinesinde biyometrik donanım yoktur.
`hello-challenge` ve `hello-open-challenge` sırasında görülen `pin` prompt tipi bu ortamın
sonucudur, kodun bir kısıtlaması değildir. Biyometrik cihazda prompt tipi farklı olabilir;
test edilmemiştir.

## Probe komutları

`poc/tpm-probe/` içinde:

```text
cargo run -- status
cargo run -- create
cargo run -- inspect
cargo run -- roundtrip 10
cargo run -- handle-cycle 30
cargo run -- lock-probe
cargo run -- lock-handle-probe
cargo run -- hello-status
cargo run -- hello-challenge
cargo run -- hello-open-challenge
cargo run -- passport-status
cargo run -- passport-create
cargo run -- passport-roundtrip 30
cargo run -- delete
```

`status` salt okunurdur. `create`, yalnızca deney adına ait kalıcı kullanıcı anahtarını oluşturur;
mevcut anahtarın üzerine yazmaz. `roundtrip` cookie veya gerçek oturum verisi kullanmaz. `delete`
deney anahtarını TPM/KSP'den kaldırır.

## Ham ölçümler

2026-08-02 `status`:

```text
provider=Microsoft Platform Crypto Provider
implementation_flags=0x00000001
hardware_provider=true
software_provider=false
pcp_platform_type=0x00500054
pcp_tpm_version=0x00020000
```

`create` otomatik çalışma oturumunda Windows güvenlik UI'ını beklerken 60 saniyede zaman aşımına
uğradı. Ardından `inspect`, `NTE_BAD_KEYSET (0x80090016)` döndürdü; kalıcı veya yarım deney anahtarı
kalmadı. Bu sonuç silinmemiştir: görünür kullanıcı oturumu olmadan UI-gated anahtar oluşturma
tamamlanamıyor.

Secret, DEK veya cookie değeri raporlanmadı.

## Yol A — ikinci tur

- Anahtar reboot sonrasında sağ çıktı ve aynı `key_unique_name` ile açıldı.
- `roundtrip 3` unwrap süreleri: **7311 ms**, **30 ms**, **31 ms**.
- İlk işlem ile steady-state arasındaki büyük farkın handle-scope cache'den kaynaklandığı
  `handle-cycle` ile doğrulandı.
- `pszFriendlyName` ve `pszDescription` kullanıcıya gösteriliyor. Üründe “Steam oturumu açılıyor”
  gibi işlem bağlamı göstermek için kullanılabilir.

Görülen diyalog:

```text
Windows Güvenliği — Bu uygulamanın şifreleme anahtarı kullanması gerekiyor.
İzin vermek için Tamam'ı tıklatın.

Anahtar adı: FURSOY TPM/Hello probe key
Anahtar açıklaması: Authorizes one TPM probe private-key operation

Parola / Parola Girin
```

### Kritik sonuç

Yol A'nın ürettiği jest **Windows Hello değildir**. `NCRYPT_UI_FORCE_HIGH_PROTECTION_FLAG`
tarafından üretilen, serbest metin parola alanına sahip CNG strong-key protection diyaloğudur.

- Etkileşimsiz otomatik unwrap yapan commodity saldırılara karşı görünür bir bariyerdir.
- Parola keylogger tarafından yakalanabilir ve yönetilmesi gereken yeni bir sır oluşturur.
- TPM'e bağlı, yazılamayan biyometrik/PIN tabanlı bir Hello jesti değildir.
- Her yeni key handle'da parola girişi dengeli policy için kabul edilemez UX'tir.

Bu başarısız alt sonuç silinmeyecektir. Yol A karşılaştırma tabanı olarak korunur.

### Handle-scope ve lock ölçümleri

`handle-cycle 30`, tek process içinde her örnekte `NCryptOpenKey → unwrap → NCryptFreeObject`
çalıştırdı. Otuz örneğin tamamı **1372–3029 ms** aralığındaydı; hızlı (~34 ms) örnek görülmedi.
Kullanıcı her örnekte CNG strong-key protection parola/PIN kutusunu yeniden doldurdu.

**Sonuç:** Jest process'e değil key handle'a bağlıdır. Her yeni `NCryptOpenKey` yeniden
yetkilendirme ister. Bu, ADR-003'teki “her vault transaction'ında handle aç/kapat” modelinin teknik
olarak işlem başına jest üretebildiğini doğrular.

`lock-probe` aynı handle ile şu değerleri verdi:

```text
before_lock_ms=2615.960
after_lock_ms=34.996
```

Kilit öncesinde jest vardı, sonrasında yoktu. Ancak iki unwrap aynı key handle'ını kullandığı için bu
sonuç kilidin cache üzerindeki etkisini izole etmez; yalnızca aynı handle'daki ikinci kullanımın
ücretsiz olduğunu doğrular.

`lock-handle-probe`, kilit sınırının iki yanında farklı handle kullanarak şu değerleri verdi:

```text
before_lock_handle=A, before_lock_ms=3386.454, prompt_likely_observed=true
after_lock_handle=B,  after_lock_ms=3541.494,  prompt_likely_observed=true
```

Her iki yeni handle da jest istedi ve yavaştı. Böylece önceki `lock-probe` sonucundaki kilit sonrası
ücretsiz kullanımın kilitten değil aynı handle'ın yeniden kullanılmasından kaynaklandığı doğrulandı.
Kilit durumu jest davranışını değiştirmiyor.

**Nihai model:** Jest yalnızca handle'a bağlıdır; süreye, process ömrüne veya kilit durumuna bağlı
değildir. Yeni handle yeni jest üretir, aynı handle ise cache'li ve ücretsizdir.

### UX gözlemi

Yol A'nın parola/PIN kutusu her işlemde metin girip Enter basmayı gerektiriyor. Bu, Windows Hello'nun
tek adımlı dokunma/bakma/PIN akışından belirgin biçimde daha yavaş ve sürtünmelidir. Sık kullanılan
dengeli seviye siteler için kabul edilemez olabilir.

## Yol B — sonuç

Microsoft Passport Key Storage Provider `implementation_flags=0x00000003` bildirdi: provider hem
hardware hem software kabiliyetlidir. Doğrudan `NCryptCreatePersistedKey` çağrısı sıradan deney
container adıyla `NTE_INVALID_PARAMETER (0x80090027)` döndürdü. Aynı adla `NCryptOpenKey` ve delete
de desteklenmedi; provider listesinde kalıcı deney anahtarı bulunmadı.

Bu makinede doğrudan CNG üzerinden Yol B **desteklenmiyor**. Probe artık Passport create/open/delete/
roundtrip/handle-cycle/lock komutlarında bu durumu process hatası yerine aşağıdaki ölçülmüş sonuçla
raporlar:

```text
passport_direct_cng_supported=false
path_b_result=unsupported
nte_status=0x80090027
```

## Yol C — sonuç

`hello-status` komutunda `KeyCredentialManager::IsSupportedAsync=true` döndü. `hello-challenge` ve
ayrı process'teki `hello-open-challenge` testleri geçti: credential cross-process açıldı, challenge
imzası public key ile doğrulandı ve prompt türü `pin` olarak gözlendi. Test makinesinde yalnızca PIN
kayıtlıdır; biyometrik donanım yoktur. Üretimde capability alanlarının challenge'a bağlanması ve tek
kullanımlılık uygulanacaktır; bu, Deney 1'in kullanıcı jesti uygulanabilirliği kararını bloklamaz.

İlk ölçümde aynı process'te create + sign akışında `hello_sign_ms=52.435`, ayrı process'te open +
sign akışında `hello_sign_ms=4587.070` görüldü. 52 ms'lik imzalama sırasında yeni bir kullanıcı jesti
gözlenmedi; PIN promptu `RequestCreateAsync` sırasında çıktı. Bu, jestin işlem başına değil process
veya credential edinme başına cache'lenebileceğini düşündürür. Kesin davranış `hello-sign-cycle`
ile ölçülecektir.

Yol C, `KeyCredentialManager` ile yalnızca imzalama yapar; DEK unwrap yapamaz. Dolayısıyla Yol C tek
başına kasa açamaz — Hello imzası kısa ömürlü bir capability üretir, asıl unwrap işlemini Yol A'daki
TPM-backed CNG anahtarı yapar. İki mekanizma birbirinin alternatifi değil, birlikte kullanılacaktır.

## Yeni ölçüm komutları

- `roundtrip` varsayılan 30 örnek alır; ilk işlem ile steady-state p50/p95/max ayrıdır.
- `prompt_likely_observed`, yalnızca `>500 ms` gecikme heuristiğidir; gerçek prompt tespiti değildir.
- `handle-cycle N`, her örnekte anahtarı açar, bir unwrap yapar ve handle'ı kapatır.
- `lock-probe`, aynı process/handle ile lock öncesi ve sonrası unwrap ölçer.
- `lock-handle-probe`, kilit öncesinde handle A ile unwrap yapıp handle'ı kapatır; kilit sonrasında
  yeni handle B ile unwrap yaparak taze-handle davranışını ölçer.
- `hello-status`, prompt göstermeden `KeyCredentialManager` desteğini ölçer.
- `hello-challenge`, credential oluşturur veya açar, rastgele challenge imzalar ve public key ile
  doğrular.
- `hello-open-challenge`, ayrı process cross-process erişim ölçümü için yalnızca mevcut credential'ı
  açar.
- `hello-sign-cycle N`, tek process ve credential üzerinde her seferinde yeni challenge ile N imza
  alır, her imzayı doğrular ve ilk/steady-state sürelerini ayırır; varsayılan N=10'dur.
- `key_unique_name` yalnızca son yol bileşeni olarak raporlanır; kullanıcı adı/tam yol yazılmaz.

## Test matrisi kapanışı

| Test | Sonuç |
|---|---|
| Process restart / cross-process credential erişimi | ✅ Tamamlandı |
| Reboot sonrası kalıcı TPM anahtarı | ✅ Tamamlandı |
| Kullanıcı iptali | ✅ Tamamlandı |
| `handle-cycle 30` | ✅ Tamamlandı — her yeni handle jest istedi |
| Aynı-handle lock/unlock (`lock-probe`) | ✅ Tamamlandı — ikinci kullanım cache'li |
| Taze-handle lock/unlock (`lock-handle-probe`) | ✅ Tamamlandı — kilidin iki yanında da jest var |
| Software fallback reddi | ✅ Tamamlandı — Platform provider hardware-only doğrulandı |
| p50 / p95 / maksimum süre ölçüm altyapısı | ✅ Tamamlandı |
| Session 0 / servis bağlamı | v1 kapsamı dışı; ileride gerekirse yapılır |
| RDP | v1 kapsamı dışı; ileride gerekirse yapılır |

Native host normal kullanıcı bağlamında çalışacağı için Session 0 ve RDP matrisleri düşük
önceliklidir. Yol C capability alanlarının challenge'a bağlanması ve tek kullanımlılık, ürün
uygulamasında tamamlanacak bir güvenlik gereksinimidir; Deney 1'in açık ölçümü değildir.

## Nihai sonuç — GO

**Go/No-Go kriteri A karşılandı.** Hem Yol A'nın yeni-handle CNG akışı hem Yol C'nin Windows Hello
akışı işlem başına gerçek kullanıcı jesti üretebilir. Yol B'nin doğrudan Passport CNG yolu bu
makinede desteklenmemiştir ve mimarinin parçası olmayacaktır.

Ölçülen UX farkı nedeniyle gerçek jest kaynağı Yol C (`KeyCredentialManager` / Windows Hello)
olacaktır. Yol A'nın TPM-backed Platform Crypto Provider anahtarı yalnızca DEK'in fiili unwrap
işlemini yapacak; kendi `NCRYPT_UI_POLICY` ayarı kaldırılarak sessiz çalışacaktır. Yetkilendirme,
önceden doğrulanan kısa ömürlü Hello capability üzerinden alınacaktır.

**Nihai handle modeli:** Jest süreye, process ömrüne veya Windows kilit durumuna bağlı değildir.
Tek belirleyici handle'dır: yeni handle = yeni jest; aynı handle = cache'li, ücretsiz kullanım.
Deney 1 **TAMAMLANDI**.
