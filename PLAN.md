# FURSOY Cookie Protector — Proje Planı ve Teknik Karar Kaydı

> Bu belge projenin **ana hafızasıdır**. Projeyi hiç görmemiş bir geliştirici bu belgeyi
> okuyarak doğru bağlamla devam edebilmelidir. Konuşma geçmişine bağımlılık kabul edilmez.
>
> Belge **yaşayan bir dokümandır**. Her önemli çalışma sonunda [Son Durum](#30-son-durum),
> [Sonraki Kesin Adım](#31-sonraki-kesin-adım) ve [Karar Günlüğü](#32-karar-günlüğü)
> bölümleri gözden geçirilir. Günlük tarzı uzun kayıt tutulmaz; belge güncel ve okunabilir kalır.

**Son güncelleme:** 2026-08-08
**Durum:** Tasarım tamamlandı. **Faz 1–4 deneylerinin tamamı GO; Faz 5 tek grup uçtan uca MVP
kontrollü uygulama ve düşük-riskli gerçek site manuel kabul testlerini tam geçti.** Q16 Aday A ile kapatıldı; vault v1, crypto temeli, Native Messaging v1 sözleşmesi ve
inject'e özel tek kullanımlık Hello capability/replay ledger'ı, gerçek vault transaction/lease dispatcher'ı,
Native Messaging host'u, ürün extension'ı ve kontrollü oturum uygulaması kodlandı. Dikey dilim
kontrollü oturumda `0.1.9`, `tr.wikipedia.org` üzerinde çoklu-cookie oturumuyla `0.1.11` sürümünde
TPM/Hello ile uçtan uca doğrulandı. Faz 5.1 navigasyon-öncesi unlock gate prototipi `0.1.12` manuel
testini tam geçti ve F5 gereksinimini kaldırarak Q21'i kapattı. **Faz 6 çoklu-grup/policy/reconciliation
implementasyonu `0.2.0` manuel iki-grup kabulünü 12/12 PASS ile tamamladı; Q4, Q12 ve Q19 kapandı.**
**Faz 7 `0.3.1` izleme katmanının çekirdek döngüsü 2026-08-06 manuel oturumunda doğrulandı; kabul
matrisi hâlâ tamamlanmadı** (bkz. [§31](#31-sonraki-kesin-adım)). [ADR-020](#adr-020--korunan-site-kullanıcı-tarafından-eklenir-ve-tüm-çerezler-kasalanır)
ile selector tabanlı profil/login-tespit modeli terk edilmiş, yerine **kullanıcının siteyi elle
eklediği ve o sitenin tüm çerezlerinin kasalandığı** model kabul edilmiştir. **ADR-020'nin her iki
dilimi de (tüm-çerez kasalama + kullanıcının kendi sitesini eklemesi, Q24) uygulanmış ve
2026-08-06'da manuel doğrulanmıştır; Faz 8 bu haliyle tamamlanmış sayılır** — bu belgenin önceki
sürümlerinde dilim 2'nin "açık" göründüğü yerler (özet, yol haritası, §31) çelişkiliydi ve bu
güncellemeyle düzeltildi. **2026-08-07/08 oturumunda [ADR-021](#adr-021--windows-hello-imzalama-arka-ucu-webauthndlle-taşınmıştır)
ile Windows Hello imzalama arka ucu `KeyCredentialManager`'dan `webauthn.dll` platform authenticator
API'sine taşındı**: onay penceresinin tarayıcının arkasında açılması sorunu (harici pencere
manipülasyonuyla düzeltilemeyen, dokümante edilmemiş bir Windows sınırlaması olduğu doğrulandı)
kalıcı olarak çözüldü, karşılığında `hello_cache_ms`/tekrar-sormama rahatlığı kayboldu (bkz. ADR-021
"Kabul edilen sınırlar"). Aynı oturumda `cookie_roundtrip_failed` sağlık kontrolündeki gerçek bir
hata (sitenin kendi eklediği çerezleri hata sanması) da düzeltildi.

---

## İçindekiler

1. [Proje Özeti](#1-proje-özeti)
2. [Problem Tanımı](#2-problem-tanımı)
3. [Ürün Konumlandırması](#3-ürün-konumlandırması)
4. [Hedef Kullanıcılar](#4-hedef-kullanıcılar)
5. [Hedef Dışı Kullanım Alanları](#5-hedef-dışı-kullanım-alanları)
6. [Tehdit Modeli](#6-tehdit-modeli)
7. [Güvenlik Sınırları](#7-güvenlik-sınırları)
8. [Temel Mimari](#8-temel-mimari)
9. [Bileşenler](#9-bileşenler)
10. [Veri Modeli](#10-veri-modeli)
11. [Anahtar Hiyerarşisi](#11-anahtar-hiyerarşisi)
12. [Vault Formatı](#12-vault-formatı)
13. [Lease Modeli](#13-lease-modeli)
14. [Policy Seviyeleri](#14-policy-seviyeleri)
15. [Crash ve Reconciliation Modeli](#15-crash-ve-reconciliation-modeli)
16. [Native Messaging Protokolü](#16-native-messaging-protokolü)
17. [Site / Account-Group Profilleri](#17-site--account-group-profilleri)
18. [Cookie Attribute Uyumluluğu](#18-cookie-attribute-uyumluluğu)
19. [TPM / Hello Deney Planı (Deney 1)](#19-tpm--hello-deney-planı-deney-1)
20. [Browser Deney Planı (Deney 2–4)](#20-browser-deney-planı-deney-24)
21. [Ölçülecek Metrikler](#21-ölçülecek-metrikler)
22. [Go / No-Go Kriterleri](#22-go--no-go-kriterleri)
23. [Bilinen Riskler](#23-bilinen-riskler)
24. [Açık Teknik Sorular](#24-açık-teknik-sorular)
25. [Yol Haritası](#25-yol-haritası)
26. [Repo Çalışma Kuralları](#26-repo-çalışma-kuralları)
27. [Commit ve Git Kuralları](#27-commit-ve-git-kuralları)
28. [Repo İz Bırakmama Kuralları](#28-repo-i̇z-bırakmama-kuralları)
29. [Test ve Güvenlik Kuralları](#29-test-ve-güvenlik-kuralları)
30. [Son Durum](#30-son-durum)
31. [Sonraki Kesin Adım](#31-sonraki-kesin-adım)
32. [Karar Günlüğü](#32-karar-günlüğü)

---

## 1. Proje Özeti

FURSOY Cookie Protector, Windows 11 üzerinde çalışan bir **Session Exposure Manager**'dır.

Kritik tarayıcı oturum artefaktlarını (öncelikle cookie'ler) **kullanılmadıkları zaman**
tarayıcı profilinden çıkarır, TPM'e bağlı şifreli bir kasada saklar ve yalnızca ilgili
hesap aktif kullanılırken kısa süreli bir *lease* ile tarayıcıya geri enjekte eder.

Amaç, oturum artefaktlarının browser store içinde **açıkta kaldığı süreyi minimuma indirmek**tir.
Başarı ölçütü matematiksel olarak tam koruma değil, **ölçülebilir maruziyet azaltımıdır**.

**Bir cümlede:** Cookie hırsızlığını engelleyen bir araç değil; kritik oturumların
açıkta kalma süresini ölçen ve asgariye indiren bir araç.

---

## 2. Problem Tanımı

### 2.1 Cookie'ler neden bu kadar kolay çalınıyor

Chromium tabanlı tarayıcılarda oturum cookie'leri
`%LOCALAPPDATA%\<Vendor>\<Browser>\User Data\<Profile>\Network\Cookies`
yolundaki SQLite veritabanında tutulur. Değerler AES-256-GCM ile şifrelidir; ancak bu
şifreleme anahtarı `Local State` dosyasında saklanır ve DPAPI ile korunur.

DPAPI koruması **kullanıcı bazlıdır**. Aynı kullanıcı hesabında çalışan herhangi bir
process `CryptUnprotectData` çağırarak anahtarı çözebilir. Commodity infostealer'ların
yaptığı tam olarak budur: iki dosyayı kopyala, anahtarı çöz, cookie'leri çöz, dışarı gönder.

Chrome 127+ ile gelen **App-Bound Encryption (ABE)**, anahtarı SYSTEM yetkisiyle çalışan bir
elevation servisi ile sarmalayarak çağıran binary'nin yolunu doğrular. Bu barı yükseltmiştir,
ancak stealer'lar bunu tarayıcıyı `--remote-debugging-port` ile kendileri başlatıp CDP
üzerinden cookie okuyarak veya tarayıcı process'ine enjekte olarak aşmaktadır.

Firefox'ta `cookies.sqlite` cookie değerleri için OS seviyesinde bir şifreleme uygulamaz.

### 2.2 Kapsam yalnızca cookie değil

Bazı platformlar oturumu cookie dışındaki artefaktlarda tutar (`localStorage`, `IndexedDB`,
LevelDB tabanlı depolar). Veri modeli bu artefaktları ileride kapsayacak şekilde tasarlanır,
ancak **v1 yalnızca cookie destekler**.

### 2.3 Sektörün doğru çözümü ve bu projenin konumu

Bu problemin yapısal çözümü **DBSC (Device Bound Session Credentials)**'tır: oturum anahtarı
TPM'e bağlanır ve çalınan cookie başka bir cihazda geçersiz olur. DBSC **sunucu tarafı destek**
gerektirir ve istemci tarafından tek başına uygulanamaz.

Bu proje, DBSC yaygınlaşana kadarki boşluğu dolduran bir **istemci tarafı azaltım aracıdır**.
DBSC'nin yerine geçmez.

---

## 3. Ürün Konumlandırması

**Ürün adı / kategori:** Session Exposure Manager

### Kullanılabilecek ifadeler

- Session exposure azaltır
- Kritik oturumları kullanılmadıkları zaman kilitler
- Tarayıcı profilinde kalıcı session artefaktı bulunmasını azaltır
- Commodity infostealer saldırı yüzeyini küçültür
- TPM-backed vault kullanır

### Kullanılması yasak ifadeler

- Cookie çalınmasını tamamen engeller
- Malware'e karşı tam koruma
- Hacklenemez
- Oturumlar hiçbir zaman çalınamaz
- Windows Hello tüm saldırıları engeller

Ürün arayüzü ve dokümantasyonu, cookie **aktif lease sırasında** korumanın Chrome'un
sunduğunun üzerine çok sınırlı çıktığını açıkça belirtmelidir. Yanlış güven hissi
yaratılması, bu projede bir **hata** olarak kabul edilir.

---

## 4. Hedef Kullanıcılar

1. **Birincil:** Kendi makinesinde yüksek değerli oturumlar taşıyan bireysel kullanıcı
   (oyun platformları, geliştirici hesapları, ana e-posta, sosyal medya).
2. **İkincil:** Tehdit modelini anlayan, maruziyet azaltımının kısmi olduğunu kabul eden
   teknik kullanıcı.
3. **Üçüncül (ileride):** Küçük ekipler; yönetilen policy dağıtımı ile.

---

## 5. Hedef Dışı Kullanım Alanları

- Kurumsal EDR / antivirus yerine geçmek
- Zaten enfekte olmuş bir makineyi temizlemek veya güvenli hale getirmek
- Malware tespiti veya kaldırma (v1'de watcher bile kapsam dışı)
- Şifre yöneticisi işlevi
- Adli analiz / incident response aracı
- Sunucu tarafı oturum güvenliği
- Mobil platformlar
- Cookie **aktifken** çalışan malware'e karşı savunma

---

## 6. Tehdit Modeli

### 6.1 Öncelikli hedeflenen saldırılar (bu proje bunlara karşı tasarlanır)

| # | Saldırı | Neden hedeflenir |
|---|---------|------------------|
| T1 | Tarayıcı kapalıyken cookie veritabanının kopyalanması | Kasadayken profilde veri yok |
| T2 | Chrome profil klasörünün taranması | Aynı sebep |
| T3 | `Cookies`, `Local State`, LevelDB gibi standart yolları hedefleyen commodity infostealer | Şablon davranış, kasadaki veriyi bulamaz |
| T4 | CDP / remote debugging üzerinden cookie okuma | Kilitli durumda store boş döner |
| T5 | Çalınan disk, profil yedeği, senkronize klasör | Kasa TPM'siz açılamaz |
| T6 | Tek seferlik, fırsatçı, kalıcılık kurmayan malware | Maruziyet penceresi dışında çalışma olasılığı yüksek |

### 6.2 Tam olarak engellenemeyecek saldırılar (açıkça kabul edilir)

| # | Saldırı | Neden engellenemez |
|---|---------|--------------------|
| N1 | Cookie aktif lease sırasında browser store'dayken çalışan malware | Cookie o an gerçekten oradadır |
| N2 | Chrome process injection | Tarayıcı içi, bizim sınırımızın dışı |
| N3 | Browser veya extension runtime belleği okuma | JS heap sıfırlanamaz |
| N4 | Zararlı tarayıcı uzantısı — cookie aktifken | İzinli uzantı `chrome.cookies` çağırabilir |
| N5 | Bu projeye özel geliştirilmiş hedefli malware | Aynı kullanıcı yetkisi, obscurity yok |
| N6 | Admin / SYSTEM / kernel yetkili saldırgan | Tüm istemci savunmalarının üzerinde |
| N7 | Host process belleğini hedefleyen gelişmiş saldırı | Transaction penceresinde anahtar canlıdır |

### 6.3 Saldırgan yetenek varsayımı

Saldırgan **kullanıcı ile aynı Windows yetkisinde** kod çalıştırabilir. Bu varsayım altında:

- Aynı kullanıcı altında yapılan hiçbir yazılımsal kimlik doğrulama **kesin sınır değildir**.
- Tek gerçek sınır, **TPM'e bağlı non-exportable anahtar** ve **kullanıcı jesti**dir.
- İkinci gerçek sınır, **zaman**dır: artefaktın açıkta olmadığı süre.

---

## 7. Güvenlik Sınırları

Bunlar bağlayıcı tasarım ilkeleridir. Bir tasarım kararı bunlarla çelişiyorsa karar yanlıştır.

1. Cookie veya session artefaktı kullanılmıyorsa browser store içinde bulunmamalı.
2. Cookie yalnızca ilgili hesap grubu aktif kullanılırken **kısa süreli lease** ile enjekte edilmeli.
3. Şu tetikleyicilerde tahliye zorunlu: ilgili son sekmenin kapanması, kullanıcı idle,
   Windows kilitlenmesi, lease süresinin dolması.
4. Hata durumlarında tercih edilen sonuç **logout**'tur.
5. Şüpheli durumda cookie verilmez.
6. **Recovery veya escrow anahtarı bulunmaz.**
7. TPM kaybı / anakart değişimi / TPM clear / Windows yeniden kurulumu, korunan tüm
   oturumların kaybı anlamına gelebilir.
8. Bu **kabul edilebilir**: cookie yeniden üretilebilir bir kimlik bilgisidir, kullanıcı
   tekrar giriş yapar.
9. Kriptografik anahtar kaybı bir **güvenlik açığı** oluşturmamalı, yalnızca kullanılabilirlik
   kaybı oluşturmalı.
10. Obscurity temel güvenlik mekanizması olarak kullanılmaz.
11. Aynı kullanıcı yetkisindeki malware'e karşı tek gerçek sınır TPM/Hello tabanlı kullanıcı
    jesti ve kısa maruziyet süresidir.
12. Cookie aktifken koruma seviyesinin Chrome'un sunduğunun üzerine çok sınırlı çıktığı kabul edilir.
13. Kullanıcı jesti yalnız cookie plaintext'inin browser store'a **açıldığı** `SEALED → UNLOCKING →
    LEASED` / inject yönünü yetkilendirir. Enrollment, eviction, lock ve reconciliation cookie'yi
    açığa çıkarmayan veya maruziyeti azaltan fail-closed işlemlerdir; Hello beklemez.

### 7.1 Bellek hijyeni kademelendirmesi

Efor eşit dağıtılmaz; sızıntının etkisine göre kademelenir:

| Veri | Sızarsa etki | Hijyen seviyesi |
|------|--------------|-----------------|
| TPM-backed KEK | Hiç bellekte olmamalı (TPM içinde kalır) | Zorunlu — asla dışa aktarılmaz |
| Grup DEK | O grubun **tüm** kasası | **Katı** — transaction scope, `zeroize` |
| Cookie plaintext (host tarafı) | Tek oturum, lease penceresi | Makul özen — `zeroize`, kopya minimizasyonu |
| Cookie plaintext (extension JS heap) | Tek oturum, lease penceresi | **Best-effort** — JS string'leri sıfırlanamaz |

Extension tarafı bir **tavan** oluşturur: `chrome.cookies.set` çağrısı için değer bir JS
string'inde bulunmak zorundadır ve JS string'leri immutable ve GC'lidir. Host tarafında
aşırı hijyen yatırımı yapmak bu tavanı yükseltmez.

---

## 8. Temel Mimari

### 8.1 Hedef platform

- **OS:** Windows 11 (geliştirme referansı: 11 Pro, build 26200)
- **Tarayıcı:** Chromium tabanlı; **v1'de yalnızca Google Chrome**
- Edge ve Brave sonraki aşamada değerlendirilir
- **Firefox v1 kapsamı dışıdır** (kendi cookie partitioning modeli ayrı çalışma gerektirir)

### 8.2 Teknoloji tercihleri

| Katman | Teknoloji | Gerekçe |
|--------|-----------|---------|
| Native host | Rust | Buffer/anahtar yaşam döngüsü kontrolü, `zeroize`, doğrudan FFI |
| Browser extension | TypeScript (MV3) | Tek gerçekçi seçenek |
| Masaüstü UI (gerekirse) | Ayrı, düşük yetkili yardımcı process (WinUI vb.) | Host'u UI karmaşıklığından ayırmak |
| Windows API | `windows` crate | NCrypt FFI ve WinRT'yi tek bağımlılıkta kapsar |
| Kriptografi | Windows CNG/NCrypt + bakımlı Rust kripto kütüphaneleri | Anahtar TPM'de, AEAD userland'de |
| Buffer temizliği | `zeroize`, sabit boyutlu byte buffer | Managed kopyalardan kaçınma |

> Her bağımlılık eklenmeden önce gerekçesi bu belgeye yazılır. Deney 1 bağımlılıkları aşağıdadır.

### 8.2.1 Deney 1 bağımlılıkları

- `windows 0.62.2`: Microsoft Platform Crypto Provider'a ait NCrypt/CNG API'lerini resmi Rust
  bağlamaları üzerinden çağırmak için. Yalnızca ihtiyaç duyulan Win32 kriptografi özellikleri açılır.
- `zeroize 1.9.0`: Rastgele üretilen DEK ve çözülmüş DEK buffer'larını kapsam sonunda güvenilir
  biçimde sıfırlamak için.

Deney 1 bir Rust binary'sidir. Tekrarlanabilir derlemeler ve incelenebilir bağımlılık çözümlemesi için
`Cargo.lock` repoda tutulacaktır; böylece **Q10 kapanmıştır**.

### 8.2.2 Faz 5 native host bağımlılıkları

- `aes-gcm 0.10.3`: vault payload'ını AES-256-GCM ile authenticated encryption altında tutmak;
  detached 16-byte tag ve caller-supplied AAD kullanmak için.
- `serde 1` / `serde_json 1`: katı Native Messaging JSON sözleşmesi, encrypted vault payload'ı ve
  secretsiz lease metadata serileştirmesi için.
- `uuid 1`: protokol message/operation/lease kimlikleri ve sabit account-group kimliği için.
- `windows 0.62.2`: Deney 1 ile aynı NCrypt, KeyCredentialManager, WinRT ve atomik Windows dosya
  API'lerini kullanmak için.
- `zeroize 1.9.0`: DEK ve çözülmüş plaintext buffer'larını transaction sonunda temizlemek için.

`native-host/Cargo.lock` repoda tutulur. Ürün crate'i POC crate'ine bağımlı değildir; Deney 1'de
doğrulanan primitive'ler üretim modüllerine ayrıştırılarak taşınır.

### 8.3 Üç katmanlı yaşam süresi ayrımı (merkezi tasarım kararı)

Bu projenin en önemli tasarım kararı, birbirine karıştırılan üç şeyin **ayrılmasıdır**:

| Katman | Ne | Ömrü |
|--------|-----|------|
| 1 | **Kullanıcı jesti** (yalnız unlock/inject Windows Hello onayı) | Policy'ye göre cache'lenebilir |
| 2 | **Grup DEK'inin TPM ile unwrap edilmesi** | **Cache'lenmez** — tek vault transaction |
| 3 | **Cookie'nin browser store'da bulunması** | Lease ile sınırlı (dakikalar) |

Sonuç: kullanıcı günde 1–3 Hello görür, ancak cookie günün küçük bir yüzdesinde açıktadır ve
host belleğinde bekleyen uzun ömürlü bir anahtar yoktur.

Güvenlik/UX takası **1. katmanda** yaşanır; 2. ve 3. katmanda taviz verilmez.
Katman 1 yalnız sealed verinin browser store'a çıkarılmasına uygulanır. Katman 3'ü kısaltan
eviction/lock işlemi kullanıcı yokken de tamamlanabilmelidir; bu yönde jest zorunluluğu yoktur.

### 8.4 Akış diyagramı (kavramsal)

```
Kullanıcı hesap grubuna ait bir sekme açar
        ↓
Extension → Host: lease.request(group_id)
        ↓
Host: policy kontrolü → gerekiyorsa Windows Hello jesti
        ↓
Host: TPM KEK ile grup DEK'ini unwrap et
        ↓
Host: AEAD ile grup kaydını çöz → cookie seti
        ↓
Host: DEK'i zeroize et
        ↓
Host → Extension: cookies.inject(lease_id, cookies)
        ↓
Extension: chrome.cookies.set(...) → health check
        ↓
[ LEASE AKTİF — maruziyet penceresi ]
        ↓
Tetikleyici: son sekme kapandı | idle | Windows lock | lease expiry
        ↓
Extension: cookie snapshot → Host: cookies.evict(lease_id, snapshot)
        ↓
Host: Hello göstermeden TPM-backed DEK unwrap → yeni nonce ile şifrele → atomik yaz → doğrula → DEK zeroize
        ↓
Host → Extension: evict.confirmed
        ↓
Extension: chrome.cookies.remove(...) → doğrula
        ↓
[ SEALED ]
```

> **Kritik sıralama:** Cookie, vault'a güvenli şekilde yazıldığı **doğrulanmadan** browser
> store'dan silinmez (bkz. [§29](#29-test-ve-güvenlik-kuralları)).

---

## 9. Bileşenler

### 9.1 Browser Extension (TypeScript, MV3)

**Sorumluluklar**

- Korunan hesap gruplarına ait sekmeleri izlemek
- Login ve session durumu değişikliklerini gözlemlemek — **ADR-020 ile kaldırılacaktır**;
  yeni modelde enrollment kullanıcı jestiyle başlar, extension oturum semantiği yorumlamaz
- `chrome.cookies` API üzerinden HttpOnly dahil cookie yönetmek
- Inject ve eviction işlemlerini koordine etmek
- İlgili hesap grubuna ait **son sekmenin** kapanmasını takip etmek
- Native host ile doğrulanmış protokol üzerinden iletişim kurmak
- Site profillerini uygulamak
- Restore sonrası sağlık kontrollerini çalıştırmak
- Lease sürelerini bağımsız olarak zorlamak (host çökse bile)

**Bilinen sınırlar**

- JS string'leri güvenli şekilde sıfırlanamaz
- Cookie plaintext'i extension JS heap'inde kısa süreli bulunur
- Bu risk kapatılamaz; hijyen best-effort'tur
- MV3 service worker'ı boşta sonlandırılabilir → zamanlayıcı stratejisi kritiktir
  (bkz. [Q5](#24-açık-teknik-sorular))

**Bağlayıcı manifest kuralı — cookie host izinlerinde port kullanılmaz**

- Gerçek ürünün `host_permissions` / `optional_host_permissions` cookie erişim kalıplarında
  `:443`, `:43118` veya başka bir port **ASLA belirtilmez**; `https://example.com/*`,
  `http://localhost/*` gibi portsuz kalıplar kullanılır.
- Cookie'ler port taşımaz. Chromium `chrome.cookies.getAll()` sonucunu host permission ile süzerken
  cookie scheme+domain alanlarından portsuz URL üretir; portlu kalıp bu URL ile eşleşmez ve cookie
  hata vermeden sonuçtan elenir.
- Uygulama ve content-script URL'leri gerektiğinde sabit portla sınırlandırılabilir; bu kural yalnız
  cookie görünürlüğünü yetkilendiren host permission kalıpları içindir.
- Ayrıntılı ölçüm ve karar: Deney 3 raporu ve [ADR-015](#adr-015--cookie-host-permission-kalıpları-portsuz-olacaktır).

### 9.2 Native Host (Rust)

**Sorumluluklar**

- TPM-backed anahtar oluşturmak ve kullanmak
- Grup bazlı DEK yönetmek
- Vault kayıtlarını şifrelemek / çözmek
- Atomik yazma yapmak
- Audit log tutmak (cookie değeri **içermeden**)
- Lease state tutmak (diskte expiry metadata ile)
- Crash reconciliation yapmak
- Software KSP fallback'i **reddetmek**
- Hata durumunda fail-to-logout davranmak

**Açıkça sorumlu olmadıkları**

- Malware tespiti (v1)
- Kendi process'ini korumak (mümkün değil — PPL erişimi yok)
- UI göstermek (ayrı yardımcı process)
- **Browser cookie store'unu doğrudan okumak veya cookie silmek** (bkz. §9.2.2)

#### 9.2.1 Yaşam döngüsü sınırı — host kalıcı bir lease enforcer DEĞİLDİR

Standart Chrome Native Messaging modelinde host process'i **Chrome tarafından, extension'ın
`chrome.runtime.connectNative()` çağrısı üzerine** başlatılır. Port kapandığında (extension
service worker'ı sonlandığında, extension devre dışı bırakıldığında, Chrome kapandığında veya
çöktüğünde) host'un yaşam döngüsü de sona erebilir.

Bunun doğrudan sonucu:

- Host, extension'dan **bağımsız ve kalıcı** bir lease zorlayıcısı olarak tasarlanamaz.
- "Host lease süresini arka planda takip eder" varsayımı standart NMH modelinde **geçersizdir**.
- Chrome kapalıyken host çalışmaz; dolayısıyla Chrome kapalıyken tahliye yapamaz.
- Windows lock bildirimi host'a ulaşsa bile, extension erişilemez durumdaysa host cookie'yi
  kaldıramaz (bkz. §9.2.2).

**Sonuç:** v1'de lease'in birincil zorlayıcısı **extension**'dır; host yalnızca bağlıyken
ikincil zorlayıcıdır. Crash-state güvenliği, kalıcı bir process'e değil,
**başlangıç reconciliation'ına** dayanır (bkz. [§15](#15-crash-ve-reconciliation-modeli)).

> **Açık mimari karar:** Kalıcı bir Windows user agent (oturum açılışında başlayan, Chrome'dan
> bağımsız yaşayan bir kullanıcı servisi/tray process'i) bu sınırı kaldırabilir — Chrome kapalıyken
> de lease expiry takibi, lock tahliyesi ve reconciliation tetikleme yapabilirdi. Ancak yeni bir
> kalıcı saldırı yüzeyi, kurulum/autostart karmaşıklığı ve ayrı bir güncelleme yolu getirir.
> Bu karar **verilmemiştir**; bkz. [ADR-013](#adr-013--kalıcı-windows-user-agent-açık-karar) ve
> [Q15](#24-açık-teknik-sorular).

#### 9.2.2 Erişim sınırı — host cookie store'a dokunamaz

Host, browser cookie store'unu doğrudan okuyamaz ve cookie silemez. ADR-002 gereği cookie
erişimi yalnızca `chrome.cookies` API'si üzerinden, yani **extension tarafından** yapılır.

Bu nedenle:

- Browser store snapshot'ı **extension** tarafından alınır.
- Cookie kaldırma işlemi **extension** tarafından yapılır.
- Host yalnızca vault tarafını (şifreleme, doğrulama, atomik yazma) yürütür ve extension'a
  ne yapması gerektiğini söyler.
- **Extension devre dışı veya kaldırılmışsa host tek başına tahliye yapamaz.**

### 9.3 Watcher / Monitoring — **v1 kapsamı dışı**

İleriki aşamalarda izlenebilecekler:

- Şüpheli process başlangıçları
- `--remote-debugging-port` ile tarayıcı başlatma
- Browser profil klasörüne erişimler
- Beklenmeyen cookie değişimleri
- Host / extension bağlantı kayıpları
- Lease dışı cookie oluşumu
- Reconciliation hataları

**Kernel minifilter v1'de yapılmayacaktır** (EV sertifika + attestation imzalama maliyeti,
uyumluluk riski).

---

## 10. Veri Modeli

### 10.1 Account Group — kasalama birimi

Kasalama birimi **tek origin değil, hesap grubudur**. Sebep: bir hesap birden çok domain
kullanabilir, authentication domain'i ile uygulama domain'i farklı olabilir, OAuth redirect'leri
vardır, partitioned cookie'ler top-level site bağlamına göre değişir, ve service worker /
`localStorage` / `IndexedDB` bağımlılıkları bulunabilir. Kısmi tahliye **hem korumayı hem
oturumu** bozar.

```text
account_group
├── id                     (UUID, kalıcı)
├── display_name
├── domains[]              (eTLD+1 veya tam host listesi)
├── cookie_selectors[]     (isim pattern'i, domain, path filtreleri)
├── partition_rules        (CHIPS / partitionKey davranışı)
├── storage_artifacts[]    (v1'de boş; ileride localStorage/IndexedDB)
├── unlock_policy          (kritik | dengeli | izleme)
├── lease_policy           (süreler, idle eşiği)
├── eviction_triggers[]    (last_tab_closed | idle | lock | expiry | manual)
├── health_checks[]        (restore sonrası doğrulama tanımı)
└── compatibility_version  (profil şeması sürümü)
```

**Gerçek-site doğrulaması (2026-08-04):** Deney 5'te bu model yalnız şema düzeyinde kalmadı.
`tr.wikipedia.org` yerel session/user cookie'leri ile `.wikipedia.org` CentralAuth cookie'lerinden oluşan
beş zorunlu `cookie_selectors[]` kaydı aynı account group içinde enrollment, snapshot, eviction ve inject
boyunca birlikte doğrulandı. Sonuç, çoklu-cookie grup modelini bu düşük-riskli site için doğrular; başka
sitelerin selector kümelerine veya partitioned cookie desteğine kendiliğinden genellenmez.

> **ADR-020 etkisi (2026-08-06):** Yukarıdaki şema **mevcut uygulamayı** tanımlar ve tarihsel kayıt
> olarak korunur. ADR-020 kabul edildiğinden `cookie_selectors[]` ve `health_checks[]` alanları yeni
> modelde kaldırılacak, yerlerine kullanıcının eklediği adresten türeyen tek bir **kapsam alanı**
> (kayıtlı domain / eTLD+1) gelecektir. Kasalama birimi hâlâ account group'tur; değişen şey grubun
> içeriğinin elle küratörlükle değil kullanıcı jestiyle belirlenmesidir. Yeni şema ADR-020 uygulanırken
> yazılacaktır.

### 10.2 Lease kaydı (diskte, plaintext metadata)

```text
lease
├── lease_id               (UUID)
├── group_id
├── granted_at             (monotonic + wall clock)
├── expires_at
├── last_activity_at
├── state                  (sealed | unlocking | leased | evicting | degraded)
├── injected_cookie_refs[] (isim HASH'i + domain, DEĞER YOK)
└── sequence               (monoton, replay kontrolü için)
```

> Lease metadata **plaintext'tir** ve cookie değeri içermez. Amacı crash sonrası
> reconciliation'ı mümkün kılmaktır.

### 10.3 Disk yerleşimi (öneri)

```text
%LOCALAPPDATA%\FursoyCookieProtector\
├── vault\
│   ├── manifest.json          (grup indeksi, şema sürümü, KEK tanımlayıcı)
│   └── groups\<group_id>.fcpv (AEAD ile şifreli cookie payload)
├── leases\<lease_id>.json     (plaintext metadata, değer yok; stale lease kayıtları dahil)
├── audit\audit-YYYYMMDD.log   (append-only, redakte)
└── config\profiles\*.json     (account-group profilleri)
```

> **FCPV v1 kararı:** Wrapped DEK yalnız `groups\<group_id>.fcpv` authenticated başlığındadır.
> `manifest.json` içine kopyalanmaz; manifest yalnız grup indeksi, format sürümü ve KEK kimliği gibi
> secretsiz genel metadata taşır (bkz. [§12.0](#120-tek-doğruluk-kaynağı-ilkesi-bağlayıcı)).

`%LOCALAPPDATA%` seçilmiştir çünkü OneDrive / bilinen bulut senkronizasyonu kapsamında değildir.
Kasa şifreli olsa da senkronize edilmesi gereksiz kopya üretir.

---

## 11. Anahtar Hiyerarşisi

```text
TPM-backed, non-exportable KEK   (Microsoft Platform Crypto Provider)
├── Google account-group DEK      (wrapped)
├── Steam account-group DEK       (wrapped)
├── GitHub account-group DEK      (wrapped)
└── Discord account-group DEK     (wrapped)
```

**Kurallar**

- KEK **asla** TPM dışına çıkmaz; yalnızca wrap/unwrap işlemi yapılır.
- Her hesap grubu için **ayrı DEK**. Bir grubun açılması diğerlerini açmaz.
- Her vault kaydı için **benzersiz nonce**; nonce **asla** tekrar kullanılmaz.
- AEAD zorunlu (tercihen AES-256-GCM; alternatif değerlendirmesi [Q2](#24-açık-teknik-sorular)).
- DEK yalnızca **tek vault transaction süresince** bellekte bulunur.
- Global ve uzun ömürlü açık master key **kullanılmaz**.
- Software KSP fallback **sessizce yapılmaz**; reddedilir ve kullanıcıya bildirilir.

### 11.1 DEK'in gerçek yaşam süresi

DEK "birkaç milisaniye" yaşamaz. Bir transaction şu adımların **tamamını** kapsar:

1. Şifreli vault kaydını oku
2. DEK'i unwrap et
3. AEAD ile payload'ı çöz
4. Cookie'leri protokol üzerinden extension'a taşı
5. Geçici buffer'ları temizle
6. (Eviction'da) cookie snapshot'ını al
7. Yeni nonce ile tekrar şifrele
8. Dosyayı atomik yaz
9. Tüm plaintext ve key buffer'larını sıfırla

Hedef: **DEK'in ömrü tek bir vault transaction'ıdır**, daha uzun değil.

---

## 12. Vault Formatı

> **DONDURULDU — FCPV v1.** Deney 1, TPM-backed/non-exportable RSA-2048 KEK ve
> RSA-OAEP-SHA256 ile 32-byte DEK wrap/unwrap yolunu doğruladı. Q16 için **Aday A** seçildi:
> wrapped DEK yalnızca ilgili grup dosyasının authenticated başlığında bulunur. Manifest içinde
> wrapped DEK bulunması format ihlalidir.

Her hesap grubu ayrı dosyada tutulur. Ayrı dosya seçimi atomik yazmayı kolaylaştırır ve
bozulmanın etki alanını tek grupla sınırlar.

### 12.0 Tek doğruluk kaynağı ilkesi (bağlayıcı)

**Wrapped DEK yalnızca TEK bir yerde saklanır.** Aynı sarmalanmış anahtarın hem
`manifest.json` içinde hem grup dosyasında bulunması yasaktır: iki kopya birbirinden ayrışır,
rotasyon ve kurtarma mantığını belirsizleştirir ve hangisinin geçerli olduğu sorusunu doğurur.

Değerlendirilen iki yerleşim ve sonuç:

| Aday | Wrapped DEK nerede | Artı | Eksi |
|------|--------------------|------|------|
| **A — grup dosyası içinde — SEÇİLDİ** | `<group_id>.fcpv` başlığında | Grup dosyası kendi kendine yeter; atomik yazma tek dosyada biter | KEK rotasyonu grup dosyalarını tek tek yeniden yazmayı gerektirir |
| **B — manifest içinde — REDDEDİLDİ** | `manifest.json` | KEK rotasyonu tek dosyada biter | Manifest ile grup dosyası arasında çapraz tutarlılık ve iki-dosyalı atomiklik gerekir |

Tek grup Faz 5 ve ilerideki çoklu grup modeli için tek dosyalı atomiklik, manifest merkezli rotasyon
kolaylığından daha değerlidir. KEK rotasyonu gerektiğinde her grup dosyası bağımsız atomik transaction
ile yeniden yazılır. Manifest grup indeksi ve genel şema metadata'sı olarak kalır.

### 12.1 Kayıt düzeni — dondurulmuş FCPV v1

```text
offset  alan                 boyut     not
------  -------------------  --------  ----------------------------------------
0       magic                4         "FCPV"
4       format_version       2         u16 LE; sabit 1
6       header_len           2         u16 LE; v1 için sabit 318
8       group_id             16        UUID
24      aead_alg_id          2         u16 LE; 1 = AES-256-GCM
26      wrap_alg_id          2         u16 LE; 1 = RSA-2048-OAEP-SHA256
28      kek_key_id           16        KEK tanımlayıcı (rotasyon için)
44      nonce                12        benzersiz, asla tekrar kullanılmaz
56      wrapped_dek_len      2         u16 LE; v1 için sabit 256
58      wrapped_dek          256       TPM KEK ile sarmalanmış 32-byte DEK
314     ciphertext_len       4         u32 LE; üst sınır 4 MiB
318     ciphertext           değişken  AES-GCM ciphertext
..      tag                  16        GCM authentication tag
```

**AAD (Additional Authenticated Data)**, offset 0–317 arasındaki 318-byte başlığın tamamıdır:
`magic || format_version || header_len || group_id || aead_alg_id || wrap_alg_id || kek_key_id ||
nonce || wrapped_dek_len || wrapped_dek || ciphertext_len`. Başlık, algoritmalar, tek yetkili
wrapped DEK ve uzunluk alanı GCM tag'ine bağlıdır. Bilinmeyen sürüm/algoritma, yanlış sabit uzunluk,
truncation ve trailing data fail-closed reddedilir.

### 12.2 Atomik yazma protokolü

```text
1. <group_id>.fcpv.tmp dosyasına yaz
2. flush + FlushFileBuffers
3. Geri oku ve AEAD doğrulaması yap        ← doğrulama başarısızsa iptal
4. ReplaceFile / MoveFileEx ile atomik değiştir
5. Dizini flush et
6. Sonucu audit log'a yaz
```

**Doğrulama adımı (3) atlanamaz.** Cookie, vault'a yazıldığı doğrulanmadan browser store'dan
silinmez.

### 12.3 Bozulma davranışı

- AEAD authentication hatası **sessizce geçilmez**.
- Vault bozulmuşsa cookie plaintext'i kurtarmaya **çalışılmaz**.
- Grup `degraded` işaretlenir, kullanıcıya bildirilir, sonuç logout'tur.

---

## 13. Lease Modeli

### 13.1 Durum makinesi

```text
              iptal / jest reddi / unwrap hatası / inject başarısız
        ┌──────────────────────────────────────────────────┐
        │                                                  │
        ▼                                                  │
   ┌─────────┐                                       ┌─────┴─────┐
   │ SEALED  │────────── lease.request ─────────────►│ UNLOCKING │
   └─────────┘                                       └─────┬─────┘
        ▲                                                  │ jest + unwrap OK
        │ evict.confirmed + store'dan silme doğrulandı      │ + inject + health OK
        │                                                  ▼
   ┌────┴─────┐                                       ┌─────────┐
   │ EVICTING │◄─────────── tetikleyici ──────────────│ LEASED  │
   └────┬─────┘                                       └─────────┘
        │ tahliye başarısız / tutarsızlık / reconciliation hatası
        ▼
   ┌──────────┐
   │ DEGRADED │  → kullanıcıya bildir, koruma aktif GÖSTERİLMEZ
   └──────────┘
```

**`UNLOCKING` → `SEALED` geçişi neden güvenlidir:** bu aşamada henüz browser store'a hiçbir
cookie yazılmamıştır. Jest iptali, unwrap hatası veya inject başarısızlığı durumunda kasada
değişiklik yoktur; doğru sonuç doğrudan `SEALED`'e dönmektir. `EVICTING` yalnızca
**gerçekten enjekte edilmiş** cookie'lerin geri alınması için kullanılır.

`DEGRADED` durumuna ayrıca başlangıç reconciliation'ının başarısız olması veya
tamamlanamaması durumunda da girilir (bkz. [§15](#15-crash-ve-reconciliation-modeli)).

**Yönsel yetkilendirme kuralı:** `SEALED → LEASED` inject geçişi cookie plaintext'ini açığa
çıkardığı için Windows Hello capability zorunludur. `LEASED → EVICTING → SEALED`, enrollment ve
reconciliation ters yöndedir; Hello istemez. Host TPM-backed KEK ile gereken unwrap/encrypt
transaction'ını sessiz yürütür, DEK'i transaction sonunda zeroize eder ve doğrulanmış vault yazımı
olmadan extension'a cookie silme izni vermez.

### 13.2 Tahliye tetikleyicileri

| Tetikleyici | Açıklama |
|-------------|----------|
| `last_tab_closed` | Hesap grubuna ait son sekme kapandı |
| `idle` | Kullanıcı etkileşimi policy eşiğini aştı |
| `lock` | Windows oturumu kilitlendi → **best-effort** anında tahliye (bkz. §13.2.1) |
| `expiry` | Lease süresi doldu |
| `manual` | Kullanıcı talebi ("şimdi kilitle") |
| `host_disconnect` | Extension host bağlantısını kaybetti → extension kendi başına tahliye eder |

Bu tetikleyicilerin hiçbiri Hello onayı beklemez. Özellikle `idle` ve `lock`, kullanıcı tanım gereği
etkileşimde değilken çalışır; prompt beklemek fail-open maruziyet üretir ve yasaktır.

#### 13.2.2 `SEALED` durumunda ortaya çıkan cookie — ADR-020 ile değişecek kural

Mevcut uygulama, grup `SEALED` iken o gruba ait bir cookie oluşursa bunu `lease_outside_cookie_created`
Orta severity izleme olayı sayar, kullanıcıya bildirim gösterir ve yeniden tahliye tetikler. Tek bir
oturum selector'ı için bu makuldü.

ADR-020 tüm çerezleri kapsama aldığından bu kural **sürdürülemez**: analytics, prefetch, consent bandı
ve arka plan istekleri kilitli site için sürekli cookie üretir; her biri bildirim ve tahliye döngüsü
doğururdu. Yeni kural:

- Gruba ait **açık ilgili sekme yoksa** → cookie arka plandan gelmiştir → **sessizce silinir**;
  bildirim üretilmez, izleme olayı yükseltilmez.
- Gruba ait **açık ilgili sekme varsa** → kullanıcı o sitede etkileşimdedir ve navigasyon gate'i
  (§Faz 5.1, `webNavigation` blocking olmadığı için garanti değildir) kaçırmış olabilir → cookie
  silinmez, normal unlock akışına girilir.

Bu ayrım oturum semantiği yorumlamaz; yalnızca "kullanıcı orada mı" sorusunu sorar ve zaten tutulan
ilgili-sekme listesini kullanır. Gerekçe ve alternatifler için [ADR-020](#adr-020--korunan-site-kullanıcı-tarafından-eklenir-ve-tüm-çerezler-kasalanır).

#### 13.2.1 Windows lock: garanti değil, best-effort

**Lock anında tahliye garantisi verilemez.** Cookie'yi store'dan yalnızca extension kaldırabilir
(§9.2.2) ve extension yalnızca Chrome çalışıyor ve erişilebilir durumdayken bunu yapabilir.

Uygulanacak davranış:

1. Lock algılandığında **best-effort immediate eviction** denenir: host bağlıysa extension'a
   tahliye komutu gönderilir, extension cookie'leri kaldırır ve doğrular.
2. Extension veya Chrome erişilemiyorsa (kapalı, çökmüş, service worker sonlanmış, extension
   devre dışı) tahliye **yapılamaz**.
3. Bu durumda diske **stale lease** kaydı yazılır: hangi grubun hangi lease altında açık kaldığı,
   hangi zamanda tespit edildiği.
4. Stale lease, **ilk yeniden bağlantıda** reconciliation ile kapatılır.
5. Stale lease var olduğu sürece ilgili grup için koruma **aktif gösterilmez**.

Bu, kilit ekranı arkasında cookie'nin store'da kalabileceği anlamına gelir. Metriklerde bu süre
`unnecessary_exposure` içinde sayılır ve gizlenmez.

### 13.3 Zorlayıcı modeli

**Birincil zorlayıcı: extension.** Lease süresini ve tahliye tetikleyicilerini Chrome içinde
extension takip eder, çünkü cookie'yi kaldırabilen tek bileşen odur (§9.2.2) ve host standart
NMH modelinde kalıcı değildir (§9.2.1).

**İkincil zorlayıcı: host — yalnızca bağlıyken.** Host, diskteki lease expiry metadata'sını
takip eder ve süresi dolan lease için extension'a tahliye komutu gönderir. Port kapalıysa
bu katman **yoktur**.

**Üçüncü katman: başlangıç reconciliation'ı.** Yukarıdaki ikisi de düştüğünde (Chrome zorla
kapatıldı, extension devre dışı bırakıldı, makine kapandı) güvenlik, kalıcı bir process'e değil
**bir sonraki bağlantıda çalışan reconciliation'a** dayanır.

Ek bir watchdog process'i **eklenmez**: o da öldürülebilir ve yeni bir saldırı yüzeyi getirir.
Kalıcı bir Windows user agent bu tabloyu değiştirir ancak henüz karar verilmemiştir
([ADR-013](#adr-013--kalıcı-windows-user-agent-açık-karar)).

---

## 14. Policy Seviyeleri

Takas **jest cache süresinde** yapılır; DEK cache'i hiçbir seviyede yoktur.

| Seviye | Jest (Hello) cache | Cookie lease | Idle eşiği | Lock davranışı |
|--------|--------------------|--------------|------------|----------------|
| **Kritik** | Yok veya birkaç saniye | Sekme ömrü / 2–5 dk | Kısa (1–2 dk) | Best-effort tahliye + stale lease |
| **Dengeli** | 5–30 dk | Son sekme + 2–10 dk | 5–10 dk | Best-effort tahliye + stale lease |
| **Kullanışlı** | Windows kilidine kadar | Sekme / idle bazlı | 10–15 dk | Best-effort tahliye + jest cache temizle |
| **İzleme** | — | — | — | Kasalama yok, yalnızca log |

> "Best-effort tahliye" ifadesi bilinçlidir: lock anında tahliye **garanti edilemez**
> (bkz. [§13.2.1](#1321-windows-lock-garanti-değil-best-effort)). Jest cache'inin temizlenmesi
> ise lock'ta her seviyede yapılır — bu host'un tek başına yapabileceği bir işlemdir.

**Kritik** örnekleri: banka, ana e-posta, şifre yöneticisi, Steam, GitHub admin erişimleri.
**Dengeli** örnekleri: Discord, GitHub (normal), Google, sosyal medya, cloud panelleri.

> Başlangıç varsayımı olarak **8 saatlik jest cache'i kullanılmaz** — fazla uzundur.
> Yukarıdaki süreler başlangıç değerleridir ve Deney 4 sonuçlarına göre ayarlanacaktır.

> **Faz 6 ölçüm notu:** Tablodaki Hello cache süresi, uygulamanın aynı grup için aynı
> `KeyCredential` handle'ını yeniden kullanabileceği üst sınırdır; Windows Hello UI'sının bu süre
> boyunca kesinlikle gösterilmeyeceği anlamına gelmez. `0.2.0` kabulünde Dengeli grup 10 dakikalık
> pencere içinde host audit'inde `hello_cached` yolunu kullandığı halde Windows yeniden prompt
> gösterdi. Last-tab eviction cache'i temizlememiştir; OS credential/UI cache ömrü ayrıca ölçülecektir.

> **ADR-021 notu (2026-08-08):** Yukarıdaki tablo, artık kullanılmayan `KeyCredentialManager`
> arka ucunun davranışını tarihsel olarak yansıtır. Yeni `webauthn.dll` arka ucu durum tutmuyor
> (stateless); jest cache süresi **fiilen etkisiz**, tüm seviyeler her yeniden girişte Hello
> istiyor (ölçüldü: art arda çağrılarda gözlenebilir bir hızlanma yok). `hello_cache_ms` alanı
> koddan kaldırılmadı — olası bir uygulama-seviyesi önbellekleme katmanı için saklı tutuluyor —
> ama şu an hiçbir davranışı etkilemiyor. Bkz. [ADR-021](#adr-021--windows-hello-imzalama-arka-ucu-webauthndlle-taşınmıştır).

---

## 15. Crash ve Reconciliation Modeli

### 15.1 Çökme senaryoları

| Senaryo | Sonuç | Değerlendirme |
|---------|-------|---------------|
| Host, cookie **kasadayken** çöker | Kullanıcı logout olur | **Güvenli** — kabul edilir |
| Host, cookie **store'da açıkken** çöker | Cookie store'da kalır | **Güvensiz** — reconciliation zorunlu |
| Chrome zorla sonlandırılır | Kapanış event'i gelmez, host da ölür | **Yalnızca kapanış event'ine güvenilmez** |
| Windows lock, extension erişilemez | Tahliye yapılamaz | Stale lease → ilk bağlantıda reconciliation (§13.2.1) |
| **Extension devre dışı / kaldırılır** | Host cookie'yi göremez ve silemez | **Reconciliation mümkün değildir** — grup `degraded` kalır (§15.3) |

### 15.2 Reconciliation prosedürü (zorunlu) — ortak host + extension işlemi

Reconciliation **host'un tek başına yapabileceği bir işlem değildir.** Host browser cookie
store'unu okuyamaz ve cookie silemez (§9.2.2); bu adımları extension yürütür. Bu nedenle
reconciliation değerlendirmesinin tetikleyicisi "host başlangıcı" değil, **host ile extension
arasındaki bağlantının kurulması**dır (`handshake`). Handshake yalnız durable host durumunu ve
lease kimliği/expiry'sini bildirir; browser gözlemi yapılmadan host durumunu `EVICTING`'e taşımaz.
Extension güncel ilgili sekmeleri ve cookie varlığını okuduktan sonra tek bir eylem seçer: sağlıklı
`LEASED` + açık ilgili sekme + mevcut cookie lease'i sürdürür; son sekme yoksa gerçek
`last_tab_closed`, eksik/tutarsız browser durumu veya durable geçiş/degraded state varsa
`startup_reconciliation` başlatılır. Böylece soğuk MV3 worker handshake'i kendisini uyandıran
`tabs.onRemoved` olayını gölgelemez.

```text
 1. Host başlatılır (extension connectNative ile bağlanır)
 2. Handshake tamamlanır
 3. Host: vault manifest'i ve lease kayıtlarını okur; handshake.ack ile durable state + lease metadata bildirir
 4. Extension: güncel ilgili sekmeleri ve cookie varlığını chrome.tabs/chrome.cookies ile okur
 5. Extension: tutarsız/stale durum varsa startup_reconciliation başlatıp browser store snapshot'ı alır
 6. Extension → Host: cookies.snapshot
 7. Host: kasada olması gerekip store'da bulunan cookie'leri tespit eder
 8. Host: cookie'leri vault'a yazar
 9. Host: yazmayı DOĞRULAR (§12.2 adım 3)          ← başarısızsa iptal, grup degraded
10. Host → Extension: evict.confirmed
11. Extension: cookie'leri store'dan kaldırır ve kaldırmayı doğrular
12. Extension → Host: evict.result
13. Host: stale lease kaydını kapatır, sonucu audit log'a yazar
14. Ancak bundan sonra ilgili grup için koruma "aktif" gösterilir
```

### 15.3 Extension yoksa: reconciliation mümkün değildir

Extension devre dışı bırakılmış, kaldırılmış veya bağlanamıyorsa yukarıdaki adımların 5, 6, 11
ve 12'si yürütülemez. Bu durumda:

- Reconciliation **yapılamaz** — host'un elinde yalnızca lease metadata'sı vardır, browser
  store'un gerçek durumu bilinmez.
- İlgili grup **`degraded` durumunda kalır**.
- Koruma **aktif gösterilmez**; kullanıcıya açıkça bildirilir.
- Kasa açılmaz; yeni lease verilmez.
- Tahliye, extension **yeniden bağlandığı anda** (handshake sonrası, herhangi bir lease
  verilmeden **önce**) yapılır.

> Bu, ürünün kabul edilmiş bir sınırıdır: extension'ı kaldıran veya devre dışı bırakan bir
> saldırgan, o an store'da açık olan cookie'lerin orada kalmasını sağlayabilir. Sistem bunu
> gizlemez — durumu `degraded` olarak gösterir ve maruziyeti metriklerde sayar.

### 15.4 Kurallar

- Reconciliation tamamlanmadan koruma **aktif gösterilmez**.
- Reconciliation değerlendirmesi host başlangıcında **ve** her extension yeniden bağlantısında çalışır;
  doğrulanmış sağlıklı `LEASED` gözlemi gereksiz tahliye/inject döngüsü üretmeden sürdürülür.
- Reconciliation, o bağlantıda **herhangi bir yeni lease verilmeden önce** tamamlanmalıdır.
- Lease expiry metadata'sı diske **inject işleminden önce** yazılır; böylece çökme anında
  hangi cookie'nin açıkta olduğu bilinir.
- Reconciliation başarısız olur veya tamamlanamazsa grup `degraded` işaretlenir.
- `degraded` gruptan çıkış yalnızca başarılı bir reconciliation ile mümkündür.

---

## 16. Native Messaging Protokolü

### 16.1 Taşıma katmanı kısıtları

Chrome Native Messaging taşıma katmanı sabittir: **4-byte little-endian uzunluk öneki + UTF-8
JSON gövde**. Kendi binary framing'imizi taşıma seviyesinde uygulayamayız; şema bu JSON'un
**içinde** tanımlanır.

Boyut limitleri Chrome tarafından dayatılır (host→extension yönünde belirgin şekilde daha
düşüktür). Büyük hesap gruplarında chunking gerekebilir — bkz. [Q1](#24-açık-teknik-sorular).

### 16.2 Mesaj zarfı

```json
{
  "v": 1,
  "conn_nonce": "<bağlantı başına rastgele>",
  "seq": 42,
  "id": "<uuid>",
  "type": "lease.request",
  "payload": { }
}
```

### 16.3 Mesaj tipleri

Faz 5 v1 dikey diliminde etkin mesajlar aşağıdaki tablonun `handshake`, `handshake.ack`,
`lease.request`, `lease.grant`, `lease.deny`, `cookies.inject`, `inject.result`, `evict.request`,
`cookies.snapshot`, `evict.confirmed` ve `evict.result` satırlarıdır. `vault.status`, `lease.renew`,
ayrı `reconcile.*`, `heartbeat` ve çift yönlü `audit.event` mesajları Faz 6'ya ertelenir. Ancak
reconciliation davranışı ertelenmez: tek grup başlangıç reconciliation'ı handshake sonrasında,
mevcut snapshot/eviction mesajlarıyla yeni lease'den önce çalışır.

| Tip | Yön | Amaç |
|-----|-----|------|
| `handshake` | E→H | Protokol sürümü, conn_nonce tesisi |
| `handshake.ack` | H→E | Kabul, host yetenekleri, vault durumu |
| `vault.status` | E→H | Grup listesi ve durumları |
| `lease.request` | E→H | Grup için lease talebi |
| `lease.grant` | H→E | lease_id, expiry |
| `lease.deny` | H→E | Gerekçe (jest iptali, policy, degraded) |
| `cookies.inject` | H→E | Cookie seti (lease_id'e bağlı) |
| `inject.result` | E→H | Başarı + health check sonucu |
| `lease.renew` | E→H | Aktivite bildirimi ile uzatma |
| `evict.request` | H→E veya E→H | Tahliye başlat |
| `cookies.snapshot` | E→H | Tahliye için mevcut cookie durumu |
| `evict.confirmed` | H→E | Vault yazımı doğrulandı; `cookie_disposition=retain_leased|remove` ile enrollment/tahliye ayrımı |
| `evict.result` | E→H | İstenen disposition'ın uygulanma sonucu ve kalan cookie sayısı |
| `reconcile.request` | H→E | Reconciliation başlat; beklenen açık cookie referansları |
| `reconcile.report` | H→E | Reconciliation sonucu ve grup durumları |
| `heartbeat` | E→H | Canlılık + SW ömrü |
| `audit.event` | çift yön | Redakte olay kaydı |

### 16.3.1 Windows Hello capability binding — bağlayıcı

Hello challenge JSON değildir; aşağıdaki sabit canonical binary dizidir:

```text
"FCPHCAP1"
|| account_group_id[16]
|| operation[1]                 # 1=inject; evict capability yoktur
|| expiry_unix_ms[u64 LE]
|| monotonic_sequence[u64 LE]
|| nonce[32]
```

- Host sequence ve 32-byte nonce'u CSPRNG ile üretir; extension capability alanı seçemez.
- İmza doğrulaması beş alanın tamamını ve bekleyen lease state-machine geçişini eşleştirir.
- Capability ömrü Faz 5'te en fazla 60 saniyedir; expired veya aşırı ileri expiry reddedilir.
- Sequence sıfır olamaz ve kalıcı high-water mark'tan büyük olmalıdır. Kullanılmış nonce'lar replay
  penceresinde tutulur.
- Bekleyen payload ile signed payload byte-for-byte aynı olmalıdır. Operation veya başka tek bir alan
  değişirse doğrulama reddedilir.
- Doğrulanan sequence/nonce, inject TPM DEK unwrap çağrısından **önce** atomik lease-ledger yazımıyla
  tüketilir. Aynı capability ikinci kez kullanılamaz; persistence başarısızsa unwrap yapılmaz.
- `operation` alanı explicit domain binding için korunur fakat v1'de yalnız `inject` kabul eder.
  `evict` değeri deserialize edilmez. Enrollment/eviction/reconciliation capability ledger'a girmez.

`protocol/messages.rs` capability payload/operation sözleşmesinin, `crypto/hello.rs` canonical
challenge imzalama/doğrulamasının, `lease/state_machine.rs` ise durable reserve/consume ve replay
reddinin tek sorumlusudur.

### 16.4 Minimum host sertleştirme

Yalnızca **gerçek değeri olan ve düşük maliyetli** önlemler uygulanır:

- Single instance
- Zarf içinde şema sürümü ve katı schema validation
- Connection nonce (frame replay ve bağlantılar arası cevap taşınmasını önler)
- Monoton sequence number
- Lease ID bağlama (her cookie işlemi bir lease'e bağlıdır)
- Replay kontrolü
- Maksimum mesaj boyutu
- Audit log
- Atomik vault yazma
- Crash reconciliation
- Extension heartbeat
- Fail-closed / fail-to-logout davranışı

### 16.5 Güvenlik sınırı olarak **kabul edilmeyenler**

| Önlem | Neden sınır değil |
|-------|-------------------|
| Parent process doğrulama | `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` ile PPID sahtelenebilir |
| Rastgele binary adı | Manifest ve registry enumerate edilebilir |
| Rastgele registry yolu | Aynı |
| Obscurity | İlke gereği reddedilir |
| Diskte statik shared secret | Aynı kullanıcı malware'i okuyabilir |
| Yalnızca extension ID kontrolü | Manifest okunabilir, host doğrudan çalıştırılabilir |

Bunlar **protokol hijyenidir**, güvenlik sınırı değildir. Belgede ve kodda böyle etiketlenir.

> Native host, Chrome tarafından ayrı bir process olarak başlatılır ve stdio üzerinden konuşur.
> Ancak host binary'si **aynı kullanıcıdaki başka bir process tarafından doğrudan
> çalıştırılabilir**. Host bunu varsayarak davranmalı; "benimle konuşuluyorsa yetkilidir"
> varsayımı yapılmamalıdır. Yetkilendirmenin tek gerçek kaynağı TPM/Hello jestidir.

---

## 17. Site / Account-Group Profilleri

> **Bu bölümün tamamı ADR-020 ile terk edilmiştir (2026-08-06).** Aşağıdaki 17.1–17.3 mevcut
> `0.3.1` uygulamasının davranışını tanımlar ve tarihsel kayıt olarak korunur (§26.1). Yeni model
> için [§17.4](#174-adr-020-sonrası-model-kullanıcı-tanımlı-koruma) ve
> [ADR-020](#adr-020--korunan-site-kullanıcı-tarafından-eklenir-ve-tüm-çerezler-kasalanır).

### 17.1 Yaklaşım (terk edildi — mevcut uygulamanın davranışı)

Grupları **yalnızca statik domain listesiyle** tanımlamak yetmez. Kimlik sağlayıcı
yönlendirmeleri, iframe'ler ve partitioned cookie'ler top-level site bağlamına göre değişir.

Bu nedenle profiller iki kaynaktan üretilir:

1. **Elle küratörlük** — ilk 15–20 yüksek değerli hedef için manuel doğrulama
2. **Ampirik türetme** — login/logout sırasında hangi cookie'lerin gerçekten değiştiğini
   gözlemleyip grubu buradan çıkarma

**Neden terk edildi:** Her hedef site için selector araştırması gerektirir ve ölçeklenmez;
kullanıcı kendi sitesini ekleyemez. Ayrıca 2026-08-06 manuel oturumunda ampirik/login-tespit
tarafının pratikte kırılgan olduğu ölçüldü (bkz. §30 Faz 7 bulguları).

### 17.2 Profil yaşam döngüsü (terk edildi)

- Her profilin `compatibility_version` alanı vardır.
- Profil doğrulanmadan **kritik** seviyeye alınamaz.
- Doğrulanmamış profiller varsayılan olarak **izleme** seviyesindedir.
- Health check başarısız olan profil otomatik olarak izleme seviyesine düşürülür.

### 17.3 Öncelikli hedef listesi (terk edildi)

Kendi kontrolümüzdeki test uygulaması → düşük riskli test hesabı → sonra gerçek hedefler.
Google, Steam, banka ve ana e-posta **erken testlerde kullanılmaz**.

> Test sırası kuralı **geçerliliğini korur** (§29.1); terk edilen şey küratörlü profil listesi
> fikridir, test disiplini değil.

### 17.4 ADR-020 sonrası model — kullanıcı tanımlı koruma

**Durum:** Karar verildi, **uygulanmadı**.

- Korunacak site **kullanıcı tarafından eklenir** (ayarlar ekranı veya sayfa üzerinden
  "bu siteyi korumaya al" eylemi). Ekleme anı serbesttir: kullanıcı sitenin içinde de olabilir,
  dışında da.
- Ekleme anında oturum durumu **sorgulanmaz**. "Giriş yapılmış mı", "bu bir login mi" gibi bir
  değerlendirme yapılmaz.
- Kapsam, eklenen adresin **kayıtlı domaini (eTLD+1)** olarak türetilir. `tr.wikipedia.org`
  eklendiğinde `wikipedia.org` ve alt domainleri kapsanır.
- Tahliye anında o kapsamdaki **tüm çerezler** kasalanır; isim/selector filtresi yoktur.
- Unlock anında aynı küme geri yazılır.
- Selector listesi, `required_for_enrollment` işareti ve site-özel `health_check` tanımları
  **kaldırılır**.

**Kabul edilen sınır — farklı eTLD+1'deki SSO çerezleri.** `auth.wikimedia.org` örneğinde olduğu
gibi, oturum farklı bir kayıtlı domaindeki çerezlere bağlı olabilir. Bunlar kapsam dışında kalır.
Bu **işlevi bozmaz** (o çerezler tarayıcıda kalmaya devam eder), ancak **korumayı eksik bırakır**:
kapsanmayan domaindeki çereze erişen bir saldırgan oturumu kısmen yeniden kurabilir. Kullanıcı
isterse o domaini ayrıca ekleyebilir. Bu sınır ürün metninde gizlenmez.

---

## 18. Cookie Attribute Uyumluluğu

### 18.1 Bilinen sınır

**`chrome.cookies` API'si ile keyfî, izole, geçici bir cookie store oluşturulamaz.**
Store'lar tarayıcının yarattığı bağlamlardır (profil, incognito). API yalnızca mevcut
store'ları listeler.

Bu nedenle doğrulama iki ayrı katmanda yapılır:

| Katman | Yöntem | Neyi kanıtlar |
|--------|--------|---------------|
| Attribute round-trip | **Gölge probe** — gerçek cookie'ye dokunmadan aynı attribute'larla `FCP-probe-*` yaz, geri oku, karşılaştır, sil | Yalnızca API round-trip uyumluluğu |
| Uçtan uca oturum | **Disposable Chrome profili + test hesabı** üzerinde evict/restore | Oturumun gerçekten yaşadığı |

> **Attribute'ların birebir eşleşmesi oturumun çalışacağını kanıtlamaz.** Server-side rotation,
> CSRF state, device binding, `localStorage` / `IndexedDB` bağımlılıkları bozulabilir.

### 18.2 Attribute eşleme tablosu

| Alan | `get` çıktısı | `set` girdisi | Not |
|------|---------------|---------------|-----|
| `name` | var | var | — |
| `value` | var | var | — |
| `domain` | var | opsiyonel | **`hostOnly` ile birlikte ele alınır**; Chrome 150 `localhost` ölçümünde domain verilmesine rağmen host-only'ye düştü, gerçek eTLD+1 davranışı doğrulanmadı |
| `hostOnly` | var | **yok** | Türetilir: `domain` verilirse `false`, verilmezse `true` |
| `path` | var | var | — |
| `secure` | var | var | — |
| `httpOnly` | var | var | Uzantı izinliyse okunur ve yazılır |
| `sameSite` | `no_restriction`/`lax`/`strict`/`unspecified` | aynı | `no_restriction` için `secure=true` zorunlu |
| `session` | var | **yok** | `expirationDate` verilmezse session cookie olur |
| `expirationDate` | var (session değilse) | opsiyonel | — |
| `partitionKey` | var (CHIPS) | var | Chrome 150 `localhost` ölçümünde `set` cookie döndürmedi; bağlam gereksinimi [Q18](#24-açık-teknik-sorular) olarak açık |
| `storeId` | var | var | Tek normal profilde `storeId=0` round-trip doğrulandı; çoklu profil / incognito doğrulanmadı |
| `url` | — | **zorunlu** | `domain` + `path` + `secure`'dan üretilir |

### 18.3 Prefix kuralları

| Prefix | Gereksinim |
|--------|------------|
| `__Host-` | `secure=true`, `path=/`, **`domain` attribute'u yok** (yani `hostOnly=true`) |
| `__Secure-` | `secure=true` |

Gölge probe isim değiştirdiği için prefix kurallarını doğrudan test edemez; bu kurallar
`__Host-FCP-probe` ve `__Secure-FCP-probe` gibi **prefix'i koruyan** probe isimleriyle ayrıca
test edilir.

---

## 19. TPM / Hello Deney Planı (Deney 1)

**Konum:** `poc/tpm-probe/` (oluşturuldu; Deney 1 tamamlandı)
**Dil:** Rust
**Neden ilk:** Tamamen yerel, hesap riski yok, site bağımlılığı yok, en küçük kod parçası ve
**en sert kapı**. Sonuç ürünün iddiasını doğrudan belirler.

### 19.1 Karşılaştırılacak üç yol

**Yol A — Doğrudan CNG**

```text
Microsoft Platform Crypto Provider
+ TPM-backed RSA/ECC key
+ NCRYPT_UI_POLICY
+ NCryptEncrypt / NCryptDecrypt (veya uygun wrap/unwrap)
```

Ölçülecek: her kullanımda gerçekten kullanıcı doğrulaması alınabiliyor mu?

**Ölçülmüş sonuç (2026-08-02/03):** Yol A, TPM-backed ve non-exportable RSA anahtarla
RSA-OAEP-SHA256 wrap/unwrap yapıyor; anahtar reboot sonrasında aynı kimlikle kullanılabiliyor.
Ancak `NCRYPT_UI_FORCE_HIGH_PROTECTION_FLAG` tarafından gösterilen kullanıcı jesti **Windows Hello
değildir**. Serbest metin parola isteyen CNG strong-key protection diyaloğudur. İlk unwrap 7311 ms,
aynı process/handle içindeki sonraki iki unwrap 30–31 ms ölçüldü. Bu yol etkileşimsiz saldırıya karşı
görünür bir bariyer oluşturur fakat keylogger'a açık yeni bir sır ve dengeli policy için kabul
edilemez UX üretir. Yol A, üretimde kendi UI policy'si kaldırılmış sessiz DEK unwrap mekanizması
olarak kullanılacaktır.

**Handle-scope sonucu (2026-08-03):** `handle-cycle 30` örneklerinin tamamı 1372–3029 ms sürdü ve
her yeni `NCryptOpenKey` handle'ında CNG parola/PIN diyaloğu yeniden dolduruldu; hızlı örnek yoktu.
Jest process'e değil handle'a bağlıdır. Bu sonuç ADR-003'ün her vault transaction'ında handle
aç/kapat modelinin teknik olarak işlem başına yetkilendirme ürettiğini doğrular. Aynı handle'ı kilit
sınırı boyunca koruyan `lock-probe` 2615.960 ms / 34.996 ms ölçtü. Taze-handle
`lock-handle-probe` ise kilit öncesinde handle A için 3386.454 ms, kilit sonrasında handle B için
3541.494 ms ölçtü ve iki tarafta da jest gözlendi. Böylece kilidin davranışı değiştirmediği ve tek
belirleyicinin handle olduğu doğrulandı.

**Yol B — Microsoft Passport Key Storage Provider**

Windows Hello anahtarları CNG üzerinden bu KSP ile de görünür. Ölçülecek: bu KSP'deki bir
anahtar **kullanım başına gerçek Hello jesti** veriyor mu? Verirse Yol A ve C'nin ikisini
birden karşılar ve capability katmanı gereksizleşir.

**Ölçülmüş sonuç (2026-08-03):** Provider açılıyor ve `implementation_flags=0x3` bildiriyor;
ancak doğrudan `NCryptCreatePersistedKey` çağrısı sıradan uygulama container adıyla
`NTE_INVALID_PARAMETER (0x80090027)` döndürüyor. Aynı adla open/delete de desteklenmiyor. Bu
makinede doğrudan CNG üzerinden Yol B **desteklenmiyor**; bu negatif sonuç hata gibi gizlenmez ve
probe tarafından `path_b_result=unsupported` olarak raporlanır.

**Yol C — `KeyCredentialManager` challenge**

```text
KeyCredential.RequestSignAsync(challenge)
        ↓
Hello onayı
        ↓
imzalı capability
        ↓
host tek bir vault operasyonuna izin verir
```

Capability alanları:

```text
account_group_id
operation          (inject)
expiry
monotonic_sequence
nonce
```

Bu modelde Hello doğrudan DEK çözmez; jest **kısa ömürlü bir yetkilendirme** üretir, DEK unwrap
işlemini ayrı bir TPM-backed CNG anahtarı yapar. Capability'nin yukarıdaki alanlara bağlanması
zorunludur; aksi halde jest cache'i açıkken malware farklı bir inject geçişini tetikleyebilir.

**Ölçülmüş sonuç (2026-08-03):** `hello-challenge` ve ayrı process'teki
`hello-open-challenge` başarılıdır; credential cross-process açılabilmiş, challenge imzası public
key ile doğrulanmış ve prompt türü PIN olarak gözlenmiştir. Test makinesinde yalnızca PIN kayıtlıdır;
biyometrik donanım yoktur. Hesap grubu/operasyon/expiry/sequence/nonce alanlarına bağlı tek
kullanımlık capability katmanı üretimde uygulanacaktır. Deney 1 kriter A ile karşılandığından bu
uygulama işi Go/No-Go kararını açık bırakmaz.

> Üç yol da `windows` crate ile **tek binary'de** test edilebilir (NCrypt FFI + WinRT aynı crate).

### 19.2 Test edilecekler

- TPM 2.0 mevcut mu, hazır mı
- Provider adıyla zorunlu tutulabiliyor mu
- Software KSP fallback reddediliyor mu
- Anahtar gerçekten non-exportable mı
- Provider ve anahtar özellikleri geri okunabiliyor mu
- 32-byte DEK wrap/unwrap çalışıyor mu
- Prompt her kullanımda çıkıyor mu
- Prompt cache var mı, süresi nedir
- Process restart sonrası davranış
- Windows lock/unlock sonrası davranış
- Reboot sonrası davranış
- Kullanıcı iptalinde davranış ve kasanın kaldığı durum
- TPM olmadığında davranış (fallback **reddedilmeli**)
- Session 0 / servis bağlamından UI gösterilebiliyor mu
- RDP altında davranış
- Gecikme: ortalama, p50, p95, maksimum
- Buffer temizliği doğrulaması
- Hata kodları

### 19.3 Çıktı

`docs/experiments/exp-01-tpm-hello.md` — tarih, ortam bilgisi (Windows build, TPM üretici/versiyon,
Hello kayıt durumu), yöntem, ham ölçümler, sonuç ve karar.

**Başarısız deneyler silinmez**; neden başarısız oldukları yazılır.

---

## 20. Browser Deney Planı (Deney 2–4)

### Deney 2 — Cookie attribute probe

Gerçek cookie **silinmeden önce**, aynı attribute'larla probe cookie yazılır ve geri okunur.

Doğrulanacak alanlar: `hostOnly`, `domain`, `path`, `secure`, `httpOnly`, `sameSite`,
`expirationDate`, `partitionKey`, `storeId`, prefix kuralları.

Bu test **oturumun çalışacağını kanıtlamaz**; yalnızca API round-trip uyumluluğunu test eder.

**Ölçülmüş sonuç (2026-08-03):** Windows 11 Pro build `10.0.26200` ve Chrome `150.0.0.0`
üzerinde **40/43 PASS**. Host-only, path/HttpOnly, Secure, dört SameSite değeri,
session/expirationDate, normal profil `storeId=0` ve prefix kuralları doğrulandı. `localhost`
domain cookie isteği host-only olarak geri döndü; bunun gerçek eTLD+1 domain'lere genellenip
genellenemeyeceği doğrulanmadı. CHIPS `partitionKey` yazımı cookie döndürmedi ve [Q18](#24-açık-teknik-sorular)
olarak açık kaldı. Ayrıntılar `docs/experiments/exp-02-cookie-attributes.md` içindedir.

### Deney 3 — Disposable profile uçtan uca

**Konum:** `poc/session-probe/`
**Rapor:** `docs/experiments/exp-03-disposable-profile.md`

- Ayrı Chrome profili
- Test hesabı
- Önce **kendi kontrolümüzdeki test uygulaması**, sonra düşük riskli site
- Adımlar: cookie snapshot → eviction → oturumun kapandığını doğrula → restore →
  oturumun geri geldiğini doğrula
- Rotation ve background request gözlemi
- **Aynı oturum üzerinde** tekrar eden evict/restore döngüleri

**İlk manuel çalışma (2026-08-03):** Extension sayfasından yapılan login ve protected fetch'i
başarılı olmasına rağmen ilk döngüde `url + name + storeId=0` filtreli `chrome.cookies.getAll`
session cookie'yi bulamadı; 0/10 döngü tamamlandı ve harness kontrollü hata ile durdu. Chrome/profil
çökmedi. İlk rapor partitioning'i kanıtlamaz çünkü filtresiz metadata yoktur; store veya filtre
uyuşmazlığı da elenmemiştir.

**Düzeltme:** Eski extension-fetch yolu ayrı bir tanı session'ında filtresiz `getAll({url})`
metadata'sı üretir. Asıl login ve protected/logout kontrolleri gerçek localhost sekmesindeki content
script üzerinden first-party bağlamda çalışır; store ID bu web sekmesinin `tabId` değerinden seçilir.
Snapshot/eviction/restore yine extension `chrome.cookies` API'sindedir. Kesin kök neden yeni tanı
raporu gelene kadar açık kalır.

**İkinci manuel çalışma (2026-08-03):** Legacy extension-fetch login ve protected kontrolü yeniden
geçti; filtresiz `chrome.cookies.getAll({url})` ise 0 döndürdü. Böylece ölçülen ortamda extension
context'inden çapraz-origin fetch ile oluşan cookie'nin Cookies API'ye görünmediği doğrulandı;
name/storeId filtresi kök neden değildir. Otomatik partitioned/izole storage mekanizması olasıdır
ancak metadata dönmediği için iç mekanizma doğrudan ölçülmedi. Asıl first-party akış daha sonra
`/api/reset` çağrısında `403 origin_not_allowed` ile durdu: sunucu yalnız extension origin'ini kabul
ediyordu. Exact allowlist'e `http://localhost:43118` eklendi; extension origin'i korunuyor.

**Üçüncü manuel çalışma (2026-08-03):** First-party login ve onu izleyen authenticated kontrolü
geçti, fakat tamamen filtresiz `chrome.cookies.getAll({})` yine 0 döndürdü. Bu sonuç görünmezliği
yalnız çapraz-origin/extension partitioning açıklamasına bağlayan hipotezi desteklemez. Protected
yanıtının Cookie header kanıtı ve gecikmeli Cookies API görünümü henüz ölçülmediğinden kök neden açık
tutulur.

**Dördüncü manuel çalışma (2026-08-03):** Hem `localhost` hem `127.0.0.1` first-party protected
isteklerinde sunucu gerçek `FCP-session-probe` Cookie header'ını doğruladı; iki origin'de de anlık ve
250 ms gecikmeli filtresiz `getAll({})` 0 kaldı. Localhost özel-host ve kısa yarış durumu hipotezleri
ölçülen ortamda elendi. Sıradaki tanı, aynı sayfadan yazılan non-HttpOnly `document.cookie` değerinin
Cookies API görünürlüğünü izole eder.

**Nihai manuel çalışma (2026-08-03):** Kök neden manifest host permission kalıplarındaki porttu.
Cookie port taşımadığı ve `getAll()` her aday cookie için portsuz scheme+domain URL'si üzerinden izin
kontrolü yaptığı için `http://localhost:43118/*` kalıbı bütün adayları sessizce eliyordu. `set()` ve
URL tabanlı `get()` verilen portlu URL'yi doğrudan kullandığından Deney 2'de çalışmış ve yanlış bir
“yalnız extension'ın kendi yazdığı cookie görünür” izlenimi oluşturmuştu. İzinler
`http://localhost/*` ve `http://127.0.0.1/*` olarak düzeltildi. Aynı unpacked, sabit-key extension ile
**136/136 kontrol PASS**, **10/10 restore**, restore başarı oranı **%100**, yanlış logout **%0** ve
güvenlik alarmı **0** ölçüldü. Server-side logout sonrası stale cookie restore kontrolü doğru biçimde
`invalid_session` verdi; manuel gözlemde kalıcı profil bozulması veya beklenmeyen davranış görülmedi.
Ara hipotezler ve başarısız çalışmalar §26.1 gereği Deney 3 raporunda korunur.

> **Gerçek hesaplarda yüzlerce login/logout yapılmayacaktır.** Yoğun login döngüleri anti-abuse
> sistemlerini tetikler ve hesap kilitlenmesine/banlanmasına yol açar. Döngü, login değil
> **evict/restore** üzerinde kurulur; login yalnızca oturum gerçekten öldüğünde tekrarlanır.

### Deney 4 — Duty cycle

Extension şunları loglar: inject zamanı, son ilgili sekmenin kapanması, idle başlangıcı,
eviction, reconciliation, başarısız eviction, cookie'nin site tarafından kendiliğinden yeniden
oluşturulması.

Gerçek kullanım senaryosu simüle edilir ve [§21](#21-ölçülecek-metrikler) metrikleri hesaplanır.

---

## 21. Ölçülecek Metrikler

### 21.1 Ana metrik

```text
exposure_duty_cycle =
    cookie'nin browser store içinde bulunduğu süre
    / ilgili browser profilinin AÇIK OLDUĞU süre
```

> Payda **duvar saati değil, tarayıcının açık kaldığı süredir.** Aksi halde gece tarayıcı
> kapalıyken metrik kendiliğinden güzelleşir ve hiçbir şey ölçülmemiş olur.

### 21.2 Ayrıştırılmış maruziyet

```text
active_exposure =
    cookie açıkken hesap grubuna ait bir sekmenin gerçekten aktif kullanıldığı süre

unnecessary_exposure =
    cookie açıkken hesap grubuna ait hiçbir sekmenin aktif olmadığı süre
```

**Ana optimizasyon hedefi:**

```text
unnecessary_exposure / browser_open_time
```

Gerekçe: site aktif kullanılırken cookie'nin açık olması sistemin **doğal sınırıdır**. Esas
başarısızlık, kullanılmıyorken açık kalmasıdır.

### 21.3 Hedefler (başlangıç)

| Seviye | `exposure_duty_cycle` hedefi |
|--------|------------------------------|
| Kritik hesap | < %2 |
| Dengeli hesap | < %10 |
| Gün boyu Chrome açıkken (genel) | < %15 |

### 21.4 Kalite metrikleri

- Yanlış logout oranı **(hedef: ≤ %0,1)**
- Restore başarı oranı **(hedef: ≥ %99)**
- Tahliye başarısızlık oranı
- Reconciliation başarı oranı
- Hello prompt sayısı (günlük)
- Ortalama ve p95 unlock gecikmesi
- Ortalama inject süresi
- Ortalama eviction süresi
- Crash sonrası açık cookie süresi
- Cookie'nin site tarafından yeniden oluşturulma oranı
- Kalıcı profil bozulması **(hedef: 0)**
- Restore sonrası hesap güvenlik alarmı **(hedef: 0)**

---

## 22. Go / No-Go Kriterleri

### 22.1 Deney 1 (TPM/Hello) — ✅ TAMAMLANDI

**Go/No-Go: KARŞILANDI (kriter A).** Hem Yol A (handle-scoped, parola/PIN tabanlı) hem Yol C
(`KeyCredentialManager`, Windows Hello tabanlı) işlem başına gerçek kullanıcı jesti üretebiliyor.
`lock-handle-probe`, kilit sınırının iki yanında açılan yeni handle'ların ikisinin de jest istediğini
doğruladı. Jest süreye, process ömrüne veya kilit durumuna değil yalnızca handle'a bağlıdır: yeni
handle = yeni jest; aynı handle = cache'li, ücretsiz kullanım.

**Ürün kararı:** Jest kaynağı Yol C (`KeyCredentialManager` / Windows Hello) olacaktır. Yol A'nın
Platform Crypto Provider TPM-backed CNG anahtarı yalnızca DEK'in fiili unwrap işlemini yapacak;
kendi `NCRYPT_UI_POLICY` ayarı kaldırılarak sessiz çalışacaktır. Yetkilendirme CNG parola kutusundan
değil, öncesinde doğrulanan kısa ömürlü Hello capability üzerinden alınacaktır. Üretimde her vault
transaction'ı yeni bir CNG key handle açacak, unwrap işlemini yapacak ve handle'ı kapatacaktır.

Aşağıdakilerden **en az birinin** sağlanması gerekiyordu; **A karşılandı:**

- **A)** Kritik policy'de her unwrap için güvenilir kullanıcı jesti uygulanabiliyor, **veya**
- **B)** Hello challenge ile tek kullanımlık, hesap grubuna bağlı capability üretilebiliyor.

### 22.2 Hiçbiri sağlanmazsa

Proje **iptal edilmez**; ürünün iddiası küçülür:

```text
TPM-bound unattended vault
+ kısa cookie lease
+ ölçülen exposure reduction
```

Bu durumda "her erişimde Hello-gated vault" iddiası **çıkarılır**. Elde kalan koruma
(cookie'lerin günün büyük kısmında TPM'e bağlı bir kasada olması ve profil klasöründe
bulunmaması) hâlâ hedef tehditlerin çoğunu karşılar.

### 22.3 Deney 3 (uçtan uca) — ✅ KARŞILANDI

- [x] Restore başarı oranı ≥ %99 — **%100 (10/10)**
- [x] Yanlış logout ≤ %0,1 — **%0 (0/10)**
- [x] Kalıcı profil bozulması = 0 — **manuel gözlemde 0**
- [x] Restore sonrası hesap güvenlik alarmı oluşmaması — **0**

Ek kontrol: server-side logout sonrasında eski cookie yeniden yazıldığında korumalı endpoint
`logged_out/invalid_session` döndürdü. Sonuç kontrollü loopback uygulaması içindir; gerçek site
uyumluluğu ayrıca ölçülmeden genellenmez.

Karşılanmazsa: ilgili site profili **izleme** seviyesine düşürülür, mimari değişmez.
Birden çok hedefte sistematik başarısızlık varsa cookie-only yaklaşımı yeniden değerlendirilir.

### 22.4 Deney 4 (duty cycle) — ✅ KARŞILANDI

- [x] `unnecessary_exposure / browser_open_time` anlamlı ölçüde düşürüldü — **%0,012**
- [x] Son ilgili sekme kapanışında otomatik eviction — **1/1 başarılı**
- [x] Idle başlangıcında otomatik eviction — **1/1 başarılı**
- [x] Başarısız eviction — **0**
- [x] Site kaynaklı kendiliğinden cookie oluşumu — **0**

Geçerli beş dakikalık ölçümde `exposure_duty_cycle=%14,011`, `active_exposure=41998 ms` ve
`unnecessary_exposure=35 ms` ölçüldü. Cookie store'da açık kaldığı 42033 ms'nin yalnız 35 ms'si
ilgili sekme aktif kullanılmıyorken geçti. Bu sonuç kontrollü localhost dummy oturumuna aittir;
gerçek site ve uzun günlük kullanım dağılımına kendiliğinden genellenmez.

`unnecessary_exposure / browser_open_time` anlamlı ölçüde düşürülemiyorsa ürünün temel değer
önerisi doğrulanmamış demektir; tahliye tetikleyicileri yeniden tasarlanır.

---

## 23. Bilinen Riskler

### 23.1 Teknik sınırlamalar (kesinleşmiş)

- `chrome.cookies` API ile keyfî geçici cookie store oluşturulamaz
- Attribute round-trip doğrulaması tek başına oturumun çalışacağını garanti etmez
- `hostOnly`, `sameSite`, `partitionKey`, `__Host-`, `__Secure-` dikkatli eşlenmeli
- Bir cookie geri yazılabildi diye server-side session geçerli kalmayabilir
- Rotation, CSRF state, device binding veya başka storage bağımlılıkları olabilir
- Cookie plaintext'i extension JS belleğinde kısa süreli bulunacaktır ve sıfırlanamaz
- Native Messaging Host aynı kullanıcı tarafından doğrudan çalıştırılabilir
- ✅ **Çözüldü (2026-08-07) — eşzamanlı iki host process'i audit zincirini bozuyordu.** Audit
  yazıcısının tek-instance kilidi yoktu; iki host aynı HMAC zincirine yazdığında zincir
  `audit sequence regression or gap detected` ile bozuluyor ve host **sonraki her açılışta
  fail-closed çıkıyordu**. Bozulma tek yönlüydü ve elle müdahale (audit dizinini kenara alma)
  gerektiriyordu; 2026-08-06'da iki kez gözlendi. Artık host, veri dizini üzerinde paylaşımsız bir
  `host.lock` dosyasıyla **açılışta tek-instance kilidi** alır ve ikinci instance audit'e hiç
  ulaşmadan temiz biçimde reddedilir. Böylece yıkıcı, elle kurtarma gerektiren hata, extension'ın
  zaten yeniden denediği sıradan bir bağlantı hatasına dönüşür. Tetikleyici taraf da kapatıldı:
  extension'ın `connect()` fonksiyonu `client` atanmadan önce `await` yaptığı için iki eşzamanlı
  çağrı iki port (ve iki host) açabiliyordu; artık tek seferlik giriş koruması vardır.
- **Standart NMH host'u kalıcı değildir**: `connectNative` portu kapanınca host yaşam döngüsü
  sona erebilir; host extension'dan bağımsız bir lease enforcer olarak tasarlanamaz (§9.2.1)
- **Host browser cookie store'unu okuyamaz ve cookie silemez**; snapshot ve kaldırma işlemleri
  extension'a bağımlıdır (§9.2.2)
- Extension devre dışı veya kaldırılmışsa reconciliation yapılamaz; grup `degraded` kalır (§15.3)
- Windows lock anında tahliye **garanti edilemez**; best-effort + stale lease modeli uygulanır (§13.2.1)
- Parent process doğrulama gerçek güvenlik sınırı değildir (PPID spoofing mümkündür)
- Diskte tutulan shared secret aynı kullanıcı malware'ine karşı anlamlı sınır değildir
- Rastgele host adı veya registry gizleme güvenlik mekanizması değildir
- Chrome store'da açık cookie mevcutken CDP veya izinli extension tarafından okunabilir
- TPM anahtarı host process'ini veya cookie plaintext'ini korumaz
- `NCRYPT_UI_POLICY` her kullanımda biyometri/PIN prompt garantisi vermeyebilir
- Prompt caching davranışı Windows sürümü, provider ve policy'ye göre değişebilir
- **Bu nedenle TPM/Hello davranışı teorik kabul edilmeyecek, ölçülecektir**

### 23.2 Ürün riskleri

| Risk | Etki | Azaltım |
|------|------|---------|
| UX sürtünmesi kullanıcıyı aracı kaldırmaya iter | Koruma %0'a düşer | Jest cache ≠ lease ayrımı; policy seviyeleri |
| Yanlış logout | Güven kaybı, kaldırma | Health check, fail-safe (kasalamama), profil doğrulama |
| Yaygınlaşma → araca özel bypass yazılması | Obscurity avantajı yok olur | Obscurity zaten sınır kabul edilmiyor; TPM bağlaması kalır |
| Kullanıcı korumayı aktif sanarken değil | Yanlış güven | Reconciliation tamamlanmadan aktif gösterilmez; degraded durumu görünür |
| TPM kaybı → tüm oturumlar gider | Kullanılabilirlik | Ürün metninde açıkça belirtilir; escrow bilinçli olarak yok |

### 23.3 Belirsizlik altındaki riskler

Bkz. [§24](#24-açık-teknik-sorular). Bunlar deneyle kapatılacaktır, tartışmayla değil.

---

## 24. Açık Teknik Sorular

Bu bölüm **canlıdır**. Cevaplanan sorular ilgili bölüme taşınır ve burada "kapandı" olarak
işaretlenir.

| # | Soru | Etki | Nasıl kapanır |
|---|------|------|---------------|
| **Q1** | Chrome native messaging'in host→extension mesaj boyutu limiti nedir ve büyük hesap grupları için chunking gerekli mi? | Protokol tasarımı | Deney 1'e ek küçük ölçüm |
| **Q2** | KEK için RSA-OAEP mi ECC+ECDH mi? Platform Crypto Provider hangisinde per-use jest veriyor? | Anahtar hiyerarşisi | Deney 1 |
| **Q3** | ✅ Tam kapandı — Per-use kullanıcı jesti mümkün: Yol A'da her yeni handle, Yol C'de Hello capability işlemi ile doğrulandı; süre, process ve kilit durumu belirleyici değil. | Ürün iddiası | Deney 1 handle-cycle + lock-handle-probe + Hello challenge |
| **Q4** | ♻️ Kapandı, sonra **ADR-020 ile yürürlükten kaldırıldı** — Faz 6 cevabı (domain/navigation/selector kümeleri sürümlü `account-groups.json` içinde elle tanımlanır) `0.3.1`'de çalışır durumdadır fakat ölçeklenmediği için terk edildi. Yeni cevap: kapsam kullanıcı jestiyle eklenir ve eklenen adresin eTLD+1'i olarak türetilir; selector listesi yoktur (§17.4). | Profil modeli | Faz 6 config validator + config-digest handshake; ADR-020 ile revize |
| **Q5** | MV3 service worker'ı boşta sonlandırılıyor. Lease zamanlayıcısını ne zorlayacak? `chrome.alarms` granülaritesi yeterli mi? Açık native messaging port'u SW ömrünü ne kadar uzatıyor? | **Tahliye hassasiyeti — kritik** | Küçük extension deneyi (Deney 2'ye eklenebilir) |
| **Q6** | Host, Chrome tarafından başlatılan bir process olarak Windows lock bildirimini nasıl alacak? (`WTSRegisterSessionNotification` pencere handle'ı ister; gizli mesaj penceresi mi kurulacak?) Host kalıcı olmadığı için bu bildirim yalnızca port açıkken anlamlıdır (§9.2.1). | Lock tahliyesinin best-effort yolu | Deney 1'e ek |
| **Q7** | Cookie tahliye edildikten sonra hâlâ çalışan bir service worker veya background fetch cookie'yi yeniden oluşturuyor mu? | Duty cycle doğruluğu | Deney 3/4 metriği |
| **Q8** | 🟡 Kısmen ölçüldü — Tek normal Chrome profilinde `storeId=0` yazma/okuma round-trip'i doğrulandı. Çoklu profil ve incognito store kimlikleri ile geçiş davranışı henüz doğrulanmadı. | Kapsam | Çoklu profil / incognito desteği ele alınmadan önce ayrı ölçüm |
| **Q9** | ✅ Kapandı — Manifest `key` alanıyla unpacked extension ID'si `dokhjkpkdknopgnjdmaogjhlelcaiigo` olarak sabitlendi ve manuel reload/test akışında kullanıldı. | Kurulum / native host manifest | Deney 2 |
| **Q10** | ✅ Kapandı — Rust binary için `Cargo.lock` repoda tutulacak. | Repo hijyeni | §8.2.1 |
| **Q11** | Mevcut lisans GPL-3.0 (repoda hazır). Bu bilinçli bir tercih mi, teyit edilmeli. | Dağıtım | Kullanıcı teyidi |
| **Q12** | ✅ Kapandı — Hash/tuz gereksinimi kaldırıldı: audit DTO'su cookie adı, domain'i, değeri veya serbest hata metni kabul etmez. Yalnız group UUID, bounded event/outcome/detail code ve operation UUID yazılır; cookie korelasyonu gerekiyorsa selector sayısı kullanılır. | Log gizliliği; isim sözlüğü saldırısı yüzeyi yok | Faz 6 grup-bazlı audit şeması ve otomatik schema testi |
| **Q13** | ✅ Kapandı — Windows Hello kayıtlıdır; Yol A bu mekanizmayı kullanmıyor ve parola tabanlı CNG strong-key protection diyaloğu gösteriyor. Yol C prompt türü yalnızca PIN kayıtlı test ortamında PIN olarak ölçüldü; biyometrik cihaz test edilmedi. | Policy | Deney 1 ikinci tur + Yol C |
| **Q14** | ✅ Kapandı — Platform Crypto Provider hardware-only ve TPM sürümü `2.0` bildirdi. `Get-Tpm` yönetici istediği için doğrulama doğrudan CNG provider özellikleriyle yapıldı. | Deney 1'in ön koşulu | Deney 1 `status` ölçümü |
| **Q15** | **Kalıcı bir Windows user agent eklenecek mi?** Standart NMH host'u kalıcı değildir (§9.2.1); Chrome kapalıyken lease expiry takibi, lock tahliyesi ve reconciliation tetikleme yapılamaz. Kalıcı agent bunu çözer ancak yeni saldırı yüzeyi, autostart ve güncelleme yükü getirir. | **Mimari — lease zorlama modeli** | [ADR-013](#adr-013--kalıcı-windows-user-agent-açık-karar); Deney 4 duty cycle sonuçları karar girdisi olacak |
| **Q16** | ✅ Kapandı — **Aday A:** wrapped DEK yalnız `<group_id>.fcpv` authenticated başlığında saklanır; manifest'te bulunmaz. FCPV v1 RSA-2048-OAEP-SHA256 için 256-byte wrapped DEK ile donduruldu. | Vault formatı donduruldu | Deney 1 + Faz 5 vault v1 implementasyonu ([§12.0](#120-tek-doğruluk-kaynağı-ilkesi-bağlayıcı)) |
| **Q17** | Extension kaldırıldığında veya devre dışı bırakıldığında kullanıcı `degraded` durumdan nasıl haberdar edilecek? Host kalıcı değil ve UI'ı yok; extension da yoksa bildirim kanalı kalmıyor. | Kullanıcının yanlış güven hissine kapılmaması | Q15 kararına bağlı; kalıcı agent varsa çözülür |
| **Q18** | CHIPS/partitioned cookie'ler yalnızca gerçek üçüncü-taraf bağlamında mı yazılabiliyor; extension bağlamından `chrome.cookies.set` ile doğrudan `partitionKey` verildiğinde neden cookie dönmüyor? Chrome 150 `localhost` ölçümünde yazım sessizce başarısız oldu. | Partitioned cookie restore uyumluluğu | Kontrollü top-level site + üçüncü-taraf iframe deneyi; extension ve sayfa bağlamlarını karşılaştır |
| **Q19** | ✅ Kapandı — Kritik/Dengeli/Kullanışlı idle eşikleri sırasıyla `1/5/15 dk`; Chrome'un global 1 dk sinyali sonrası grup-bazlı alarm ve tahliye anında `idle.queryState` doğrulaması kullanılır. Manuel ölçümde Kritik ~70 sn'de tahliye olurken Dengeli leased kaldı; 5+ dk'da Dengeli de sessiz tahliye oldu. Faz 5'teki `30 s` test değeri kaldırıldı. | Policy bazlı idle ayrımı gerçek iki-grup akışında doğrulandı | Faz 6 `0.2.0`, exp-06 Faz E |
| **Q20** | Sistem idle sinyali, medya oynatma veya görünür sayfadaki pasif ama gerçek kullanımı nasıl ayırt edecek? YouTube/video gibi senaryolarda klavye-fare olmaması yanlış erken tahliye üretebilir. | Aktif pasif kullanımda oturumun gereksiz kilitlenmesi | Faz 6'da tab visibility, audible/media state ve site aktivitesini güvenlik sınırını gevşetmeden değerlendiren policy tasarla |
| **Q21** | ✅ Kapandı, **2026-08-07'de revize edildi** — `webNavigation.onBeforeNavigate`, `sealed + yeni ana-frame navigasyonu` durumunda tam hedef URL'yi saklayıp sekmeyi `unlock.html` ara sayfasına yönlendirir; Hello ve cookie round-trip başarısından sonra sekme path/query korunarak hedefe döner. `leased` navigasyonlara dokunulmaz. **Revizyon:** inject artık ayrı bir düğme jesti beklemez, ara sayfa açılır açılmaz başlar (bkz. aşağıdaki not). İptal/hata durumunda ara sayfada kalınır ve düğmeyle tekrar denenir. | Restore UX'i; F5 gereksinimi kaldırıldı | Faz 5.1 `0.1.12` Wikipedia manuel testi: ana akış, leased, Hello reddi/tekrar ve idle senaryoları PASS |
| **Q22** | 🟡 Blocker değil — Uygulama `hello_cached`/aynı `KeyCredential` handle yolunu 10 dakikalık Dengeli pencere içinde doğru seçtiği halde Windows Hello UI yeniden gösterildi. Last-tab eviction cache'i temizlemiyor; uygulama penceresi OS'nin promptsuz credential cache süresini garanti etmiyor. Gerçek OS prompt-cache süresi nedir? | Beklenenden fazla prompt; güvenlik fail-safe kalır | Ayrı süre ölçümü: aynı process/handle ile artan aralıklarda prompt gözlemi |
| **Q23** | **Başarısız restore sonrası grup yeniden enroll olmuyor.** `0.3.1`'de kontrollü uygulama grubu `restore_rejected` ile invalidate olduktan sonra, kullanıcı siteye taze login yapıp 10+ dakika beklediği halde audit'e hiç `enrollment` kaydı düşmedi; grup kalıcı olarak `uninitialized` kaldı. Extension'ın reload edilmesi durumu düzeltir. `waitForStableEnrollmentCookies` mi boş dönüyor, yoksa state makinesi mi takılı kalıyor? | **Koruma sessizce durur** — kullanıcı korunduğunu sanır, hiçbir uyarı çıkmaz | ADR-020 login-tespit yolunu kaldırdığı için bu kod yolu yeniden yazılacaktır; yine de kök neden yazılı olarak doğrulanmalı ki yeni modelde tekrarlanmasın. Repro: §30 Faz 7 bulguları |
| **Q24** | ✅ Kapandı — Host config'in tek sahibidir ve config'i handshake'te extension'a gönderir; extension kendi kopyasını taşımaz, aldığını doğrular ve yalnız offline fail-closed tahliye için cache'ler. Ekleme/silme `group.add`/`group.remove` ile hosta gider, host UUID atar ve açılıştaki validator'dan geçirip atomik yazar. | Grup ekleme akışı; config-digest fail-closed sözleşmesi korundu | ADR-020 dilim 2 uygulaması |
| **Q25** | Kullanıcı-eklenen bir sitenin koruması, extension yeniden kurulduğunda **sessizce** durur: optional host izinleri kurulumla gider, host'taki site listesi kalır. Şu an bu durum tespit edilip kullanıcıya gösteriliyor ve tek düğmeyle onarılıyor. Daha iyi bir yol var mı — ör. izin kaybını extension başlangıcında proaktif bildirim olarak yükseltmek? | Kullanıcı korunduğunu sanırken korunmuyor olabilir | Faz 8 kabul testinde izin-kaybı senaryosunun ayrı bir madde olarak koşulması |

---

## 25. Yol Haritası

| Faz | İçerik | Çıktı | Kapı |
|-----|--------|-------|------|
| **Faz 0** | Plan ve karar kaydı | `PLAN.md` | ✅ Tamamlandı |
| **Faz 1** | Deney 1 — TPM/Hello probe (Rust) | `poc/tpm-probe/`, `docs/experiments/exp-01-*.md` | ✅ Tamamlandı — §22.1 kriter A karşılandı |
| **Faz 2** | Deney 2 — Cookie attribute probe (extension) + Q5, Q8, Q9 | `poc/cookie-probe/`, exp-02 raporu | ✅ Tamamlandı — 40/43 PASS; Q9 kapandı, Q8 kısmi, Q5 ve yeni Q18 açık |
| **Faz 3** | Deney 3 — Disposable profile uçtan uca | `poc/session-probe/`, exp-03 raporu | ✅ Tamamlandı — 136/136 PASS, 10/10 restore; §22.3 karşılandı |
| **Faz 4** | Deney 4 — Duty cycle ölçümü | exp-04 raporu | ✅ Tamamlandı — §22.4 karşılandı; unnecessary exposure %0,012 |
| **Faz 5** | Tek grup, uçtan uca MVP (vault + host + extension) | Çalışan dikey dilim | ✅ **TAMAMLANDI** — kontrollü uygulama `0.1.9`, düşük-riskli gerçek site `tr.wikipedia.org` `0.1.11`; TPM/Hello + vault + host + extension ve çoklu-cookie group zinciri doğrulandı |
| **Faz 5.1** | Navigasyon-öncesi kullanıcı kontrollü unlock gate | `webNavigation` yakalama + `unlock.html` + tam URL'ye dönüş | ✅ **TAMAMLANDI** — `0.1.12` manuel test tam geçti; ilk görünür yükleme authenticated, F5 gereksinimi yok |
| **Faz 6** | Çoklu grup, policy seviyeleri, reconciliation sertleştirme | `0.2.0`, exp-06 kabul raporu | ✅ **TAMAMLANDI** — otomatik kontroller ve iki-grup manuel kabul matrisi 12/12 PASS; Q4/Q12/Q19 kapandı |
| **Faz 7** | Watcher / monitoring katmanı | `0.3.1`, exp-07 kabul raporu | ✅ **Fiilen tamamlandı** (2026-08-08) — #1–4, #7–9 doğrulandı; #6 ADR-020 ile anlamsızlaştı (selector kaldırıldı); #5 (sealed grupta dış cookie / boş-kavanoz) kullanıcı tarafından denendi ama audit log'da ayrı bir `scope_empty` kaydı gözlenmedi, resmi kanıt zayıf. "Known broken" (2026-08-05 WIP notu) kök nedeni bulunup düzeltildi ve canlı doğrulandı: bkz. aşağıdaki oturum notu |
| **Faz 8** | **Kullanıcı tanımlı koruma (ADR-020)** — tüm-çerez kasalama, selector/login-tespit yollarının kaldırılması, site ekleme UI'ı | Config şeması v2 + yeni protokol sözleşmesi + kabul raporu | ✅ **Dilim 1 ve dilim 2 uygulandı ve doğrulandı** (2026-08-06); Q24 kapandı. Tam kabul matrisi koşulmadı |
| **Faz 9** | Edge / Brave desteği | v0.4 | — |

Kernel minifilter ve Firefox desteği yol haritasında **yoktur**; ileride ayrı değerlendirilir.

---

## 26. Repo Çalışma Kuralları

### 26.1 Dokümantasyon

- `PLAN.md` yaşayan belgedir ve projenin ana hafızasıdır.
- Her önemli mimari karar belgeye işlenir.
- Deney sonuçları **tarih ve ortam bilgisiyle** kaydedilir.
- **Varsayımlar ile doğrulanmış gerçekler ayrılır.**
- Başarısız deneyler silinmez; neden başarısız oldukları yazılır.
- Açık sorular açıkça listelenir ([§24](#24-açık-teknik-sorular)).
- Gelecekteki bir geliştirici geçmiş konuşmaya ihtiyaç duymadan devam edebilmelidir.
- Gereksiz günlük tarzı uzun kayıt tutulmaz; belge güncel ve okunabilir kalır.

**Plan güncelleme kontrol listesi** (her önemli çalışma sonunda):
son durum · tamamlanan işler · yeni kararlar · değişen varsayımlar · test sonuçları ·
açık sorunlar · sonraki adım · bilinen regresyonlar · güvenlik etkileri

### 26.2 Değişiklik disiplini

- Kullanıcının istemediği dosyalara dokunulmaz.
- Kapsam dışı refactor yapılmaz.
- **Bağımlılık eklemeden önce gerekçe yazılır.**
- Güvenlik bağımlılıkları güncel ve aktif bakımlı olmalıdır.
- Lockfile commit kararı proje tipine göre bilinçli verilir ([Q10](#24-açık-teknik-sorular)).

### 26.3 Kod yorumları

- Yorumlar yalnızca **teknik gerekçe** için yazılır.
- Kod kendini açıklıyorsa yorum yazılmaz.
- Gereksiz açıklayıcı yorum eklenmez.
- Güvenlik açısından kritik kararlar yorumla veya ADR ile açıklanır.

---

## 27. Commit ve Git Kuralları

- Kullanıcı **açıkça** "commit at" / "commit oluştur" / "değişiklikleri commit et" veya eşdeğer
  net bir talimat vermedikçe **commit atılmaz**.
- Kullanıcı yalnızca kod yazılmasını veya değişiklik yapılmasını isterse commit oluşturulmaz.
- Commit mesajı kullanıcı tarafından belirlenmediyse, commit öncesinde kapsam özetlenir.
- Açık izin olmadan: **push yapılmaz, branch oluşturulmaz, PR açılmaz, tag/release oluşturulmaz.**
- Otomatik commit, amend, squash veya rebase yapılmaz.
- Var olan commit geçmişi yeniden yazılmaz.

---

## 28. Repo İz Bırakmama Kuralları

Local development-environment artifacts must not be committed unless explicitly requested.
This includes personal settings, session exports, generated-attribution trailers, tool-specific
notes, caches, metadata, and temporary working files.

**Ignore stratejisi:**

- Yerel araç dosyaları `.git/info/exclude` içine yazılır (yerel, commit edilmez).
- **`.gitignore` içine yazılmaz** — `.gitignore` repoda görünür ve iz bırakır.

**Mevcut durum (2026-08-02):**

- Local development settings are excluded globally.
- Editor-specific local folders are listed in `.git/info/exclude`.
- **Takip edilen (tracked) dosyalar:** `.gitattributes`, `LICENSE` — yalnızca bu ikisi.
- **`PLAN.md` untracked'dır**; henüz commit edilmemiştir (bkz. [§27](#27-commit-ve-git-kuralları):
  açık talimat olmadan commit atılmaz).

---

## 29. Test ve Güvenlik Kuralları

### 29.1 Test kuralları

- Gerçek ana hesaplar ilk testlerde kullanılmaz.
- Sıra: **kontrollü test uygulaması → düşük riskli test hesabı → gerçek hedefler**.
- **İlerleme (2026-08-04):** kontrollü uygulama kapısı Faz 5 `0.1.9` ile, düşük-riskli test hesabı
  kapısı Deney 5 `0.1.11` ile `tr.wikipedia.org` üzerinde tamamlandı. Daha yüksek riskli/gerçek hedefler
  henüz test edilmedi ve her biri açık kullanıcı onayı gerektirir.
- Google, Steam, banka ve ana e-posta hesapları erken testlerde kullanılmaz.
- Anti-abuse tetikleyecek yoğun login/logout döngülerinden kaçınılır.
- Testler **aynı session üzerinde evict/restore** şeklinde yapılır.
- Gerçek cookie değerleri test raporlarına yazılmaz.
- Test sonuçları tekrarlanabilir olmalıdır (ortam bilgisi raporda).
- **`fcp-host.exe` elle çalıştırılmaz.** Host'u Chrome başlatır ve sahibi Chrome'dur
  (`connectNative`). Chrome veya extension bağlıyken ikinci bir host process'i elle başlatmak audit
  HMAC zincirini bozar ve sistemi fail-closed kilitler (§23.1). Zorunlu bir tanı gerekiyorsa önce
  **tüm** Chrome ve host process'leri kapatılır, tanı ayrı `FCP_DATA_DIR` altında yapılır.
  Bu kural 2026-08-06'da iki kez ihlal edildiği ve her ikisinde de zincir bozulduğu için yazılmıştır.

### 29.2 Güvenlik kuralları

- Software crypto fallback **sessizce yapılmaz**.
- Güvenlik özelliği çalışmıyorsa **fail-open davranılmaz**.
- Koruma aktif değilse kullanıcıya **açıkça bildirilir**.
- TPM kullanılmıyorsa kullanılıyor gibi gösterilmez.
- Hello prompt oluşmadıysa kullanıcı doğrulandı kabul edilmez.
- Cookie, vault'a güvenli şekilde yazılmadan browser store'dan silinmez.
- **Vault write sonrası doğrulama yapılmadan kaynak cookie kaldırılmaz.**
- Kriptografik nonce tekrar kullanılmaz.
- AEAD authentication hataları sessizce geçilmez.
- Vault bozulmasında cookie plaintext'i kurtarmaya çalışılmaz.
- Crash sonrası reconciliation yapılmadan koruma aktif gösterilmez.

### 29.3 Veri hijyeni

- Secret, token, cookie, gerçek hesap bilgisi veya hassas log repoya yazılmaz.
- Test fixture'larında gerçek session artefaktı bulunmaz.
- Kullanıcı verileri loglanmaz.
- **Audit loglar cookie değerlerini içermez.**
- Cookie adları ve domain'leri audit DTO'suna hiç alınmaz; hash/tuz da tutulmaz
  ([Q12](#24-açık-teknik-sorular)).
- Debug çıktıları production build'de kapalıdır.

---

## 30. Son Durum

**Tarih:** 2026-08-06

**Kilometre taşı:** Faz 1–4 kapsamındaki dört deney **GO** sonucu verdi ve Faz 5 tek grup uçtan uca
MVP hem kontrollü uygulamada (`0.1.9`) hem düşük-riskli gerçek site `tr.wikipedia.org` üzerinde
(`0.1.11`) manuel kabul testini **tam geçti**. TPM/Hello, şifreli vault, Native Messaging host, MV3
extension, gerçek server-side session ve yerel + CentralAuth çoklu-cookie account group zinciri birlikte
doğrulandı. Faz 5.1 `0.1.12` navigasyon-öncesi unlock gate manuel testi de tam geçti: sealed oturumda
gerçek site görünür biçimde commit edilmeden ara sayfa gösterildi, Hello yalnız düğme jestiyle başladı ve hedef path/query ilk görünür
yüklemede authenticated açıldı. Q21 kapandı. **Faz 6 `0.2.0` iki-grup manuel kabulü de 12/12 PASS
ile tamamlandı:** Wikipedia (`balanced`) ve Controlled Session App (`critical`) bağımsız enrollment,
last-tab, idle, unlock/ret, external logout ve reload reconciliation akışlarında birbirini etkilemedi.
Q4, Q12 ve Q19 kapandı. §29.1 test sırasının ilk iki kapısı tamamlandı; yüksek riskli gerçek hedefler
test edilmedi.

### Tamamlananlar

- Tehdit modeli, güvenlik sınırları, mimari ve deney planı kararlaştırıldı.
- Bu belge (`PLAN.md`) oluşturuldu ve bir revizyondan geçti (aşağıya bkz.).
- `.git/info/exclude` içine yerel araç klasörü kayıtları eklendi.
- `poc/tpm-probe/` altında Deney 1 Rust binary'si başlatıldı.
- Platform Crypto Provider adıyla açıldı; hardware-only (`0x1`) ve TPM 2.0 (`0x00020000`)
  özellikleri geri okunarak software fallback olmadığı doğrulandı.
- Probe derleme ve test doğrulamasından geçti (`cargo check --locked`, `cargo test --locked`).
- Deney raporu `docs/experiments/exp-01-tpm-hello.md` altında tamamlandı.
- Yol A reboot ve üçlü roundtrip testi tamamlandı: anahtar aynı unique identity ile sağ çıktı;
  unwrap süreleri 7311 ms, 30 ms ve 31 ms ölçüldü.
- Yol A isteminin Windows Hello değil, parola tabanlı CNG strong-key protection diyaloğu olduğu
  kullanıcı gözlemiyle doğrulandı.
- Yol C (`hello-challenge` / `hello-open-challenge`), Yol B Passport komutları, `handle-cycle`,
  `lock-probe`, ayrıştırılmış timing metrikleri ve redaksiyon kodlandı.
- `KeyCredentialManager::IsSupportedAsync` mevcut unpackaged probe process'inde `true` döndürdü.
- Yol C challenge ve cross-process open/sign testi geçti; imza doğrulandı ve yalnızca PIN kayıtlı
  test ortamında prompt türü PIN olarak gözlendi.
- Yol B doğrudan CNG key creation/open/delete yolu `NTE_INVALID_PARAMETER (0x80090027)` ile
  desteklenmedi; probe bu sonucu artık `path_b_result=unsupported` olarak raporluyor.
- Yol A `handle-cycle 30` testi jestin process değil handle scope'unda olduğunu doğruladı; tüm
  örnekler 1372–3029 ms sürdü ve her yeni handle kullanıcı girdisi istedi.
- Aynı-handle `lock-probe` 2615.960 ms / 34.996 ms ölçtü; ikinci kullanımın cache'li ve ücretsiz
  olduğunu doğruladı.
- Taze-handle `lock-handle-probe`, kilit öncesi handle A'da 3386.454 ms ve kilit sonrası handle B'de
  3541.494 ms ölçtü; iki tarafta da jest gözlendi. Kilit durumu davranışı değiştirmiyor.
- Nihai model doğrulandı: jest yalnızca handle'a bağlıdır; yeni handle yeni jest, aynı handle cache'li
  kullanım üretir. Deney 1 **TAMAMLANDI** ve §22.1 kriter A karşılandı.
- `poc/cookie-probe/` altında yalnızca `tsc` ile derlenen MV3 Deney 2 uzantısı, sabit
  `localhost:43117` test sitesi ve tam sayfa rapor harness'i oluşturuldu. Manifest `key` alanıyla
  extension ID'si `dokhjkpkdknopgnjdmaogjhlelcaiigo` olarak sabitlendi; böylece **Q9 kapandı**.
- Deney 2 manuel Chrome ölçümü Windows 11 Pro build `10.0.26200`, Chrome `150.0.0.0`, tek normal
  profil `storeId=0` ortamında tamamlandı: **40/43 PASS**.
- Host-only, path/HttpOnly, Secure, dört SameSite değeri, session/expirationDate ilişkisi,
  `storeId=0`, URL üretimi ve `__Host-` / `__Secure-` prefix kuralları round-trip etti.
- `domain: "localhost"` verilen cookie doğrulanmış biçimde `hostOnly=true`, `domain="localhost"`
  olarak döndü. Bunun `localhost` özel-host davranışı olduğu olasıdır; gerçek eTLD+1 domain'lerde
  tekrarlanıp tekrarlanmadığı **doğrulanmadı**.
- CHIPS `partitionKey` yazımı cookie döndürmedi. Başarısız sonuç silinmedi; gerçek üçüncü-taraf
  bağlam gereksinimini araştırmak üzere **Q18** açıldı.
- `poc/session-probe/` altında Deney 3 başlangıç implementasyonu oluşturuldu: bellekte session store
  kullanan kontrollü localhost uygulaması, tek login üzerinde varsayılan 10 evict/restore döngüsü,
  korumalı endpoint kontrolleri ve sunucu tarafı logout invalidation kontrolü.
- Deney 3'ün ilk manuel çalışmasında extension-fetch login/protected kontrolleri geçti, ancak ilk
  snapshot `chrome.cookies.getAll` ile 0 cookie döndürdü; harness 0/10 döngüde kontrollü hata verdi.
  Tarayıcı veya profil çökmedi. Partitioning olasıdır fakat filtresiz metadata olmadığı için
  doğrulanmış kök neden değildir; başarısız deneme Deney 3 raporunda korunur.
- Harness düzeltildi: legacy extension-fetch için değer-redakte filtresiz metadata tanısı eklendi;
  asıl login ve oturum kontrolleri gerçek localhost sekmesinde first-party content script'e taşındı;
  store seçimi test web sekmesinin `tabId` değerine bağlandı.
- İkinci manuel çalışmada legacy filtresiz `getAll({url})` yine 0 döndürdü. Böylece extension
  context'inden çapraz-origin fetch ile oluşan cookie'nin ölçülen ortamda Cookies API'ye tamamen
  görünmediği doğrulandı. Bunun otomatik partitioned/izole storage'dan kaynaklanması olasıdır fakat
  iç mekanizma metadata ile doğrulanamadı; ürünün first-party kullanıcı login akışına genellenmez.
- Aynı çalışmanın first-party aşaması `/api/reset` için `Origin: http://localhost:43118` header'ının
  reddedilmesiyle durdu. Sunucu allowlist'i sabit extension origin'i ile sabit test origin'ini exact
  olarak kabul edecek biçimde düzeltildi; başka origin açılmadı.
- Üçüncü manuel çalışmada first-party login ve ayrı protected kontrolü geçtiği halde tamamen filtresiz
  `chrome.cookies.getAll({})` 0 döndürdü. Bu, görünmezliği yalnız extension çapraz-origin bağlamına
  bağlayan hipotezi desteklemez; Cookie header ve zamanlama kanıtı olmadan yeni kök neden atanmadı.
- Körlemesine yeni davranış değişikliği yapılmadan genişletilmiş tanı eklendi: first-party login'den
  hemen sonra filtresiz `chrome.cookies.getAll({})`, bütün cookie store `id/tabIds` kayıtları, test
  sekmesinin `url/windowId/incognito` alanları ve content page `document.cookie` adları değer-redakte
  ham blok olarak raporlanır. Harness bu bloktan otomatik kök neden sonucu çıkarmaz.
- Çelişkili `getAll({})=0` / authenticated bulgusunu ayırmak için tanı genişletildi: anlık filtresiz
  cookie okumasına ek olarak 250 ms gecikmeli ikinci `getAll({})` sonucu ve sunucunun son 10
  `/api/login`/`/api/protected` isteğinde Cookie header varlığı ile yalnız cookie adları raporlanır.
  Bu yalnız tanı değişikliğidir; henüz kalıcı davranış düzeltmesi veya kök neden kararı değildir.
- `localhost` özel-host hipotezini ayırmak için mevcut akış korunarak `http://127.0.0.1:43118`
  first-party karşılaştırması eklendi. Her iki origin login → anlık/250 ms filtresiz `getAll({})` →
  protected → sunucu Cookie-header kanıtı üretir; IP sonucu ayrı ham rapor bölümünde tutulur. Sunucu
  yalnız IPv6/IPv4 loopback adreslerinde dinler ve host izinleri bu iki sabit origin ile sınırlıdır.
- Dördüncü manuel ölçümde localhost ve 127.0.0.1 aynı sonucu verdi: sunucu protected isteğindeki
  session Cookie header'ını doğrularken anlık/250 ms filtresiz Cookies API okumaları 0 kaldı. Hostname
  hipotezi elendi. Her iki origin'e non-HttpOnly `FCP-docwrite-diagnostic` yaz/oku/API görünürlük
  tanısı ve redakte gerçek login Set-Cookie header şablonu eklendi; ölçüm gelmeden yeni neden atanmaz.
- Deney 2 kontrol uzantısının da dışarıdan `document.cookie` ile yazılmış cookie'yi görememesi,
  iki POC'un ortak manifest farkını ortaya çıkardı. Chromium'un `getAll()` izin filtresi cookie için
  portsuz scheme+domain URL'si üretirken POC manifestleri portlu host permission kullanıyordu.
- Host izinleri portsuz `http://localhost/*` / `http://127.0.0.1/*` kalıplarına çevrildi. Sabit key,
  statik izin ve özgün tam-sayfa harness korunarak yapılan nihai Deney 3 ölçümü **136/136 PASS** ve
  **10/10 evict/restore** verdi; restore başarı oranı **%100**, yanlış logout **%0**,
  `securityAlarmCount=0` ölçüldü.
- Logout invalidation kontrolü geçti: sunucu session'ı sildikten sonra stale snapshot'ın cookie olarak
  geri yazılması oturumu diriltmedi ve protected endpoint `invalid_session` döndürdü. Kullanıcı
  gözleminde yalnız beklenen localhost sekmesi açılıp kapandı; Chrome çökmesi veya kalıcı profil
  bozulması görülmedi. Deney 3 **TAMAMLANDI** ve §22.3 kriterlerinin tamamı karşılandı.
- Deney 4 duty-cycle harness'i aynı kontrollü session altyapısında gerçek `tabs.onRemoved`,
  `chrome.idle`, otomatik inject/evict, reconciliation ve cookie yeniden oluşma olaylarını ölçmek
  üzere tamamlandı. İlk manuel koşu kullanıcı aktif fazda uzaklaştığı için erken idle'a girdi ve
  geçersiz ölçüm olarak raporda ayrı tutuldu.
- İkinci ve geçerli Deney 4 koşusu tam 5 dakika (`300007 ms`) sürdü. Cookie `42033 ms` açık kaldı:
  `41998 ms` active exposure, yalnız `35 ms` unnecessary exposure ölçüldü. Sonuçlar
  `exposure_duty_cycle=%14,011` ve ana hedef
  `unnecessary_exposure / browser_open_time=%0,012` oldu.
- Son sekme kapanışı ve idle başlangıcı birer kez tetiklendi; `2/2` eviction ve `1/1` inject
  başarılı oldu. `failed_eviction_count=0`, `site_cookie_recreated_count=0` ölçüldü; kullanıcı
  çökme, profil bozulması veya beklenmedik davranış gözlemlemedi. Deney 4 **TAMAMLANDI** ve §22.4
  kriteri karşılandı.
- Faz 5 kullanıcı onayıyla resmi olarak başladı. Ürün kodu POC'lardan ayrılarak kökte
  `native-host/` ve ortak `protocol/v1/` çalışma alanlarında başlatıldı.
- Q16 **Aday A** ile kapatıldı. FCPV v1; AES-256-GCM, RSA-2048-OAEP-SHA256, 256-byte wrapped DEK,
  318-byte authenticated header ve grup dosyası içindeki tek wrapped-DEK doğruluk kaynağıyla
  donduruldu.
- Native host ilk diliminde AES-GCM seal/open, strict vault encode/decode, atomik write-through
  replace + geri-okuma/AEAD doğrulaması, TPM Platform KEK primitive'i ve Windows Hello capability
  signer/verifier modülleri oluşturuldu.
- İlk capability sözleşmesi; group ID, inject/evict operasyonu, expiry, monoton sequence ve 32-byte
  nonce'a canonical binary challenge ile bağlandı. Bu ilk kapsam daha sonra ADR-018 ile yalnız
  inject yönüne daraltıldı. Durable replay ledger tüketimi inject TPM unwrap'tan önce zorunludur;
  değiştirilen alan, expired payload ve ikinci kullanım reddedilir.
- Native Messaging v1 envelope/framing ve Faz 5 minimum mesaj DTO/JSON şemaları oluşturuldu.
  `cargo check` ve ilk dilim `cargo test` doğrulaması geçti.
- Capability ledger'ın durable `verify_and_consume` çıktısı doğrusal bir authorization token'ına
  dönüştürüldü. Yalnız bu token'ı tüketen inject vault-read transaction'ı cookie plaintext'ini
  açabilir; enrollment/eviction/reconciliation ise capability olmadan sessiz TPM transaction'ı
  kullanır. DEK `ZeroizeOnDrop` ile tek transaction sonunda temizlenir.
- `%LOCALAPPDATA%\FursoyCookieProtector` altında atomik lease/capability state, grup vault'u ve
  değer-redakte audit JSONL yerleşimi; startup reconciliation ve fail-closed lease dispatcher'ı
  tamamlandı.
- `fcp-host` stdin/stdout Native Messaging döngüsü 4-byte little-endian uzunluk çerçevesi, 1 MiB
  sınırı, ilk-mesaj handshake zorunluluğu, connection nonce ve iki yönlü monoton sequence kontrolüyle
  tamamlandı. Per-user Chrome native-host kayıt betiği eklendi.
- Kök `extension/`, yalnız `tsc` kullanan MV3 ürün bileşeni olarak oluşturuldu. Tek kontrollü grup
  için enrollment snapshot, host-backed inject, health check, son sekme/idle/lock eviction, startup
  reconciliation ve sealed halde cookie yeniden oluşumu tahliyesi bağlandı. Cookie plaintext'i
  extension storage veya log'a yazılmaz; `host_permissions` bağlayıcı olarak portsuzdur.
- `tests/controlled-session-app/` altında dummy credential, bellekte server-side session store,
  HttpOnly cookie, protected endpoint ve gerçek server-side logout invalidation içeren uygulama
  oluşturuldu. Faz 5 dikey dilimi manuel TPM/Hello kabul testine hazırlandı.
- Otomatik doğrulama: native host **16/16 Rust testi**, extension `npm run check` ve `npm run build`,
  kontrollü uygulama `node --check` geçmiştir. Etkileşimli TPM/Hello akışı otomatik testlerde
  çalıştırılmamış, manuel kabul adımına bırakılmıştır.
- İlk Faz 5 manuel kabul girişimi login/enrollment adımında durduruldu: tek login sonucunda art arda
  Hello istemleri görüldü. Windows Application Error kayıtları her istem sonrasında `fcp-host.exe`
  için aynı `0xc0000005` APPCRASH'i; redakte audit 22 ayrı handshake'i; capability ledger ise 15
  tüketilmiş sequence ve lease'in hâlâ `uninitialized` olduğunu doğruladı. `HelloAuthorizer` içindeki
  WinRT apartment guard'ı, COM-backed `KeyCredential` alanından önce drop edildiği için host başarılı
  capability tüketiminden hemen sonra çöküyor, extension reconnect edip yeniden enrollment istiyordu.
- Alan sırası credential önce, apartment guard son düşecek biçimde düzeltildi. Başlangıçta kalmış
  tüketilmemiş reservation, sequence geri kullanılmadan iptal edilen crash recovery eklendi. Extension
  `storage.session` state'ine enrollment/inject ve eviction single-flight kilitleri yazıyor; kesin yanıt
  gelmeden veya worker/host reconnect olsa bile ikinci istek gönderilmiyor. İlk kabul girişimi başarısız
  koşu olarak korunur; düzeltilmiş binary ile manuel test yeniden başlatılacaktır.
- Düzeltilmiş ikinci kabul girişiminde enrollment → seal ve `logged_out` kontrolü geçti. Aynı sekmedeki
  F5 sırasında startup reconciliation henüz sürerken `tabs.onUpdated` inject'i erteledi; reconciliation
  tamamlandığında ikinci navigation olayı gelmediği için inject edge'i kayboldu. Handshake anında açık
  ilgili sekme için `injectAfterReconciliation` niyeti latch edilip reconciliation başarılı biter bitmez,
  aynı sıralı native portta `evict.result` sonrasına `lease.request(inject)` yazılacak şekilde düzeltildi.
  İlk enrollment sonundaki zorunlu `logged_out` kontrolü bu latch'i kurmadığından korunur.
- Sonraki kabul adımında inject capability authorize edildiği halde host `leased` durumda kaldı ve
  `inject.result` almadı; sunucu diagnostics'inde extension health isteği yoktu. Extension reload'u,
  önceden açık test sekmesine yeni content script'i kendiliğinden enjekte etmediğinden
  `tabs.sendMessage` alıcısız hata veriyor ve inject handler exception ile yarıda kalıyordu. Health
  check port-suz izinli sekmenin first-party bağlamında `chrome.scripting.executeScript` ile çalışacak
  hale getirildi. Cookie set sonrası Cookies API geri-okuması eklendi; set/health'in herhangi bir
  exception'ında cookie fail-closed kaldırılır ve host'a başarısız `inject.result` mutlaka gönderilir.
- Bir sonraki ölçüm `inject.result=failed` ve server diagnostics'te tüm protected istekleri için
  `cookie_header_present=false` gösterdi; set sonrası Cookies API geri-okuması geçtiği halde varsayılan
  `executeScript` ISOLATED world fetch'i site cookie'sini taşımadı. Health fetch açıkça `world: MAIN`
  kullanarak gerçek first-party sayfa bağlamına alındı. Ayrıca manifest content script'inin klasik
  script olarak yüklenmesine rağmen derlenmiş dosyadaki `export {}` token'ı syntax hatası üretiyordu;
  gereksiz TypeScript modül işareti kaldırıldı ve klasik `dist/content.js` çıktısı doğrulandı.
- Chrome yalnız Reload sonrasında eski `content.js` kopyasını çalıştırmayı sürdürdü: hata satırı 15'te
  `export {}` bildirirken canonical disk artifact'inin aynı satırı `}` ve SHA-256 değeri
  `C40448CEFD4237CCF25735491938D4D3C16DEC7CC9D465879CACBCDF0941E565` idi. Bu koşu kod
  regresyonu değil stale unpacked yükleme/cache bulgusudur. Extension sürümü `0.1.1` yapıldı; sonraki
  kabul koşusu Remove → kök `extension/` klasöründen Load unpacked ile temiz kurulacaktır.
- Temiz `0.1.1` koşusunda enrollment/seal ve F5 startup reconciliation başarıyla audit edildi, fakat
  inject authorization hiç başlamadı. Handshake'in `relevantTabIds()` sorgusu portlu
  `http://localhost:43119/*` URL match pattern'ı kullandığı için açık sekmeyi bulamıyor ve
  `injectAfterReconciliation=false` kalıyordu. Chrome tab URL sorgusu bağlayıcı portsuz
  `http://localhost/*` ile yapılacak, dönen adaylar kod içinde exact origin
  (`http://localhost:43119`) karşılaştırmasıyla daraltılacaktır. Düzeltilmiş extension sürümü
  temiz artifact ayrımı için `0.1.2` olarak işaretlendi.
- `0.1.2` kabulünde tek Hello'nun inject olduğu doğrulandı: audit `inject authorized` kaydından yalnız
  25 ms sonra `inject failed`, lease ise `degraded` oldu. Tek seferlik anlık health 401'i cookie'yi
  hemen fail-closed siliyordu; Deney 3'teki yaklaşım taşınarak 0/100/200/400/800 ms sınırlı backoff ve
  her adımda cookie API presence kontrolü eklendi. Native audit başarısızlığın bounded/redakte health
  nedenini artık ayrı detail code ile kaydeder.
- Site-data temizliği extension `storage.session` state'ini etkilemediği için host/extension degraded
  kaldı; yeni login cookie'si mevcut listener tarafından yok sayılıp korunmasız kaldı. Degraded halde
  yeni cookie artık Hello-authorized snapshot/eviction recovery başlatır. Degraded halde F5 ve cookie
  yoksa extension tek-uçuş korumalı native reconnect ile startup reconciliation ister. Bu recovery
  değişikliklerini taşıyan extension sürümü `0.1.3` olarak işaretlendi.
- §13.2 incelemesi, ilk MVP enrollment implementasyonunun plana aykırı olduğunu ortaya çıkardı:
  enrollment snapshot'ı yanlışlıkla gerçek eviction ile aynı `remove` yoluna sokuluyor, aktif sekmedeki
  yeni login cookie'si hemen siliniyor ve F5 yapay unlock tetikleyicisine dönüşüyordu. Bu bilinçli test
  kısayolu değildir; implementasyon eksiğidir. Protokol `evict.confirmed.cookie_disposition` alanıyla
  ayrıştırıldı: enrollment `retain_leased` döndürür, cookie sayısı korunarak host `LEASED` finalize eder;
  yalnız gerçek eviction/reconciliation `remove` döndürüp sıfır cookie ile `SEALED` finalize eder.
- F5 yalnız sealed grubun yeniden kullanımı veya startup reconciliation sırasında unlock niyeti olabilir;
  leased aktif oturumda tahliye/inject nedeni değildir. Reconciliation sürerken somut `tabs.onUpdated`
  olayı artık `injectAfterReconciliation` latch'ini doğrudan kurar; handshake-time tab sorgusu yarışına
  bağımlı kalmaz. Lease grant expiry'si `chrome.alarms` ile zorlanır; last-tab, idle, lock ve host-disconnect
  tetikleyicileri korunur. Düzeltilmiş extension sürümü `0.1.4` olarak işaretlendi.
- `0.1.4` manuel turunda enrollment/F5 doğru çalıştı; son sekme kapanışında Hello çıkmadı ve yeniden
  açılıştaki inject `inject_execution_failed` ile fail-closed sonuçlandı. Audit gerçek `eviction`
  yerine handshake'in önceden başlattığı sessiz `reconciliation`ı, ardından inject authorization/failure'ı
  doğruladı. Sunucu diagnostics'i session map'te bir aktif oturum kaldığını, inject anında hiçbir
  `/api/protected` health isteği ulaşmadığını ve yalnız sonraki manuel kontrolün cookiesiz 401 aldığını
  gösterdi; hata server session invalidation değil, health script çalıştırılmadan önceki extension
  execution yolundaydı. Kök neden iki yarıştı: cold-worker handshake browser olayından önce host state'ini
  `EVICTING` yaparak `last_tab_closed` isteğini gölgeliyor; inject health ise storage'daki kapanmış ilk
  `relevantTabs` kimliğinde `executeScript` deneyebiliyordu. `0.1.5` ile handshake salt durable-state
  bildirimi oldu; güncel tab/cookie gözleminden sonra last-tab veya reconciliation seçiliyor, eski
  connection single-flight bayrakları temizleniyor ve inject health canlı tab sorgusunu işlem anında
  yeniliyor. Güvenli audit kodları set/round-trip/tab-query/health-execution aşamalarını ayrı raporlar.
- `0.1.5` manuel turunda F5 geçti, fakat ikinci hassas operasyon olan last-tab eviction Hello sırasında
  native host sonlandı. Windows Application Error/WER kayıtları 22:13:37 için `APPCRASH`, exception
  `0xc0000005`, faulting module `fcp-host.exe`, offset `0x5a26c` gösterdi; aynı binary 21:58:07'de de
  aynı offsette çökmüştü. Audit enrollment success'ten sonra hiçbir `eviction authorized` kaydı
  oluşmadığını doğruladı. Önceki field drop-order düzeltmesi credential'ı apartment'tan önce düşürse
  de `VaultTransactions::authorize` her capability'de yeni `HelloAuthorizer` kurup WinRT apartment'ı
  tekrar tekrar initialize/uninitialize ediyordu; deterministik çöküş ikinci teardown'da oluştu.
  `0.1.6` ile authorizer lazy oluşturulup native bağlantı ömrü boyunca `VaultTransactions` içinde
  tutulur; bütün enrollment/inject/evict capability'leri aynı yaşayan apartment/credential'ı kullanır,
  teardown yalnız bağlantı/process sonunda doğru field sırasıyla yapılır.
- `0.1.6` yeniden açılış inject'inde Hello tamamlandı, ancak `chrome.cookies.set` callback'i başarısız
  olup genel `cookie_set_failed` kodu nedeniyle gerçek neden güvenli biçimde ayrılamadı. Ham Chrome
  hata metni URL/cookie metadata sızdırabileceğinden loglanmaz. `0.1.7` callback içinde metni hemen
  sabit redakte sözlüğe çevirir: `permission`, `domain`, `samesite`, `secure`, `path`, `partition_key`,
  `store`, `url`, `invalid_cookie`, `unknown`; `lastError` olmadan boş sonuç ayrıca `no_result` olur.
  Yalnız `inject:cookie_set_<kategori>` extension state/console ve native audit'e taşınır; ham mesaj
  callback dışına çıkmaz. Kök attribute manual tekrar ölçümündeki bu kategoriyle belirlenecektir.
- `0.1.7` temiz yüklemesinde kullanıcı yeni bir jest vermeden `cookie_set_failed` görüldü. Audit bunun
  eski kuyruk mesajı olmadığını doğruladı: yeni `inject authorized` kaydı ve yeni tüketilmiş capability
  sequence vardı. `0.1.6` crash düzeltmesi apartment ile birlikte `KeyCredential` handle'ını da bağlantı
  boyunca tuttuğundan, Deney 1'de ölçülmüş same-handle jest cache sonraki capability'leri sessiz
  yetkilendirmişti. Ayrıca genel `cookie_set_failed` callback'ten önceydi: Rust `Option` alanı vault'tan
  JSON'a `partition_key:null` olarak çıkıyor, TypeScript'in yalnız `!== undefined` kontrolü null nesnenin
  `top_level_site` alanına erişerek senkron TypeError üretiyordu. `0.1.8` apartment'ı bağlantı boyunca
  tutar fakat her capability için fresh `KeyCredential` handle açar; böylece her inject/evict tekrar
  Hello jesti ister. Cookie wire tipi açıkça null kabul eder, yalnız string `top_level_site` varsa
  `partitionKey` SetDetails'e eklenir ve senkron Chrome API schema exception'ları da aynı redakte
  kategorizer'a alınır. Null partition/expiration ve geçerli partition fixture regresyonu 3/3 geçmiştir.
- §7, §8.3 ve §13 tasarım denetimi, Hello gereksiniminin risk artıran inject yönünden güvenliği artıran
  eviction yönüne yanlışlıkla genellendiğini ortaya çıkardı. Bu kasıtlı bir MVP kısayolu veya ürün
  kararı değildir: özellikle idle/lock tetikleyicisinde kullanıcı yokken prompt beklemek tahliyeyi
  fail-open yapıyordu. `0.1.9` ile capability `Evict` varyantı kaldırıldı; yalnız inject tek kullanımlık
  sequence/nonce tüketip fresh Hello jesti ister. Enrollment, eviction ve reconciliation ledger
  capability'si üretmeden, transaction-sınırlı TPM unwrap ile sessiz çalışır; non-interactive enrollment
  `lease.grant.capability_sequence=null` taşır ve audit'te `started` olarak ayrıştırılır.

### Faz 5 nihai manuel kabul sonucu (`0.1.9`)

- **PASS — enrollment:** login sonrası sessiz enrollment tamamlandı; Hello çıkmadı, cookie aktif
  sekmede kaldı ve protected endpoint `authenticated` döndürdü.
- **PASS — aktif lease/F5:** sayfa yenilemesi tahliye veya yeni Hello üretmedi; oturum
  `authenticated` kaldı.
- **PASS — last-tab eviction:** son ilgili sekme kapanınca Hello göstermeden doğrulanmış vault yazımı
  ve fiziksel cookie tahliyesi tamamlandı.
- **PASS — reopen/inject:** site yeniden açılınca tam bir adet inject Hello gösterildi; onay sonrası
  cookie restore ve protected health check `authenticated` oldu.
- **PASS — idle:** `30 s` test eşiğinde Hello göstermeyen idle eviction tamamlandı; sonraki reopen
  yalnız bir inject Hello istedi. `30 s` üretim policy'si değildi; konu o tarihte Q19'a taşındı ve
  Faz 6'nın `1/5/15 dk` policy ölçümüyle kapandı.
- **PASS — gereksiz unlock yok:** server-side logout sonrası F5, geçersiz oturumu açmaya çalışmadı ve
  gereksiz Hello üretmedi.
- **PASS — ret/iptal:** inject Hello reddi veya iptali cookie vermeden `logged_out` ile fail-closed
  sonuçlandı.

### Faz 5 sırasında bulunan sorunlar — tamamı çözüldü

1. **Çözüldü — `NativeClient` TDZ:** service worker, sınıf initialize edilmeden `connect()` çağırdığı
   için başlangıçta çöküyordu; tanım/başlatma sırası düzeltildi.
2. **Çözüldü — WinRT/COM çökmesi I:** `KeyCredential`, apartment guard'dan sonra drop edildiği için
   `0xc0000005` oluşuyordu; field/drop sırası güvenli hale getirildi.
3. **Çözüldü — F5/reconciliation yarışı:** navigation edge'i reconciliation sırasında kayboluyordu;
   `injectAfterReconciliation` niyeti latch edilip sıralı portta reconciliation sonrasına taşındı.
4. **Çözüldü — klasik content script syntax'ı:** `dist/content.js` içindeki `export {}` klasik script
   bağlamında syntax hatası veriyordu; modül işareti kaldırıldı ve temiz yükleme/artifact kontrolü eklendi.
5. **Çözüldü — health check JS world:** ISOLATED world fetch'i first-party cookie'yi taşımıyordu;
   protected health kontrolü açıkça `MAIN` world'de çalıştırıldı.
6. **Çözüldü — `tabs.query` port kalıbı:** portlu URL match açık test sekmesini bulamıyordu; portsuz
   Chrome match pattern + kod içinde exact origin kontrolüne geçildi.
7. **Çözüldü — enrollment sonrası hemen tahliye:** enrollment yanlışlıkla gerçek eviction remove
   yolunu kullanıyordu; `retain_leased` ve `remove` disposition'ları ayrıldı.
8. **Çözüldü — cold-worker/relevant-tab yarışı:** handshake reconciliation'ı gerçek last-tab olayını
   gölgeliyor ve stale tab ID inject health'i bozuyordu; handshake salt state bildirimi oldu, canlı tab
   sorgusu işlem anına taşındı.
9. **Çözüldü — WinRT/COM çökmesi II:** her capability'de apartment initialize/uninitialize edilmesi
   ikinci teardown'da `0xc0000005` üretiyordu; apartment native bağlantı ömrüne alındı.
10. **Çözüldü — `cookie_set_failed` teşhis eksikliği:** hassas ham Chrome mesajını yazmadan permission,
    domain, SameSite, secure, path, partition, store, URL ve diğer nedenleri ayıran redakte kodlar eklendi.
11. **Çözüldü — `partitionKey: null` senkron hatası:** nullable wire alanı nesne sanılıp
    `top_level_site` erişiminde TypeError üretiyordu; yalnız geçerli string metadata SetDetails'e ekleniyor.
12. **Çözüldü — aynı Hello handle'ında jestin atlanması:** Deney 1'de ölçülen handle-scope cache nedeniyle
    sonraki unlock sessiz yetkileniyordu; apartment korunurken her inject için fresh `KeyCredential`
    handle açılıyor.
13. **Çözüldü — eviction'ın Hello istemesi:** idle/lock anında kullanıcı yokken prompt beklemek
    fail-open tasarım hatasıydı; ADR-018 ile capability yalnız inject'e daraltıldı, enrollment/eviction/
    reconciliation sessiz TPM transaction'ı oldu.
14. **Çözüldü — external logout sonrası stale vault:** Wikipedia'nın kendi logout işlemi auth
    cookie'lerini extension'ın remove akışı dışında sildiğinde `removed` olayları yok sayılıyor, encrypted
    vault'taki geçersiz session yeniden inject edilip Hello/restore döngüsüne giriyordu. `0.1.11` ile dış
    silme, extension'ın suppression kayıtlarından ayrıldı; zorunlu selector kaybı `session.invalidate`
    mesajını üretir, host vault'u silip lease'i durable `UNINITIALIZED` yapar ve `session.invalidated` ile
    doğrular. Restore health sonucu `logged_out`/`invalid_session` olduğunda da aynı tek-seferlik fail-closed
    invalidation çalışır. Nihai Wikipedia logout + F5 + reopen kontrolü gereksiz Hello ve stale restore
    oluşmadığını doğruladı.

### Deney 5 gerçek-site manuel kabul sonucu (`0.1.11`)

- **PASS — selector kümesi:** `local_session`, `local_user_id`, `local_user_name`, `central_session` ve
  `central_user` zorunlu selector'ları tek login ile, cookie değerleri loglanmadan yakalandı.
- **PASS — enrollment/F5:** enrollment sessiz tamamlandı; F5 Hello üretmedi ve oturum açık kaldı.
- **PASS — last-tab/idle:** son ilgili sekme ve `30 s` test idle eşiği sessiz eviction üretti.
- **PASS — inject:** her yeniden açılış/dönüş yalnız bir inject Hello istedi; onay sonrası çoklu-cookie
  oturumu geri yüklendi.
- **PASS — external logout:** Wikipedia'nın gerçek “Çıkış yap” işlemi Hello göstermeden vault'u sildi,
  lease'i `UNINITIALIZED` yaptı; sonraki F5 ve reopen logged-out kaldı ve tekrar döngüsü üretmedi.
- **Çözüldü — Deney 5 UX borcu:** inject ve native health başarılı olsa da mevcut Wikipedia sayfası görünür auth
  durumunu kendiliğinden yenilemedi; bir F5 gerekti. Bu borç Faz 5.1 `0.1.12` ile kapatıldı.

### Windows Hello isteminin tarayıcının arkasında açılması (2026-08-07)

**Ölçülen davranış:** Hello istemi ekranda normal boyutta açılıyor fakat Chrome penceresinin
**arkasında** kalıyor; kullanıcı görev çubuğundan öne getirmek zorunda. Chrome kapalıyken veya
küçültülmüşken istem doğru şekilde önde açılıyor, yani sorun z-sırası/ön plan hakkıdır.

**Kök neden — mimari, hata değil.** Microsoft'un desktop uygulamalar için önerdiği çözüm, WinRT
nesnesine sahip pencere tanıtıcısı (HWND) vermektir; bu yalnızca `UserConsentVerifier` için
mevcuttur (`IUserConsentVerifierInterop::RequestVerificationForWindowAsync`). ADR-014 gereği
kullandığımız **`KeyCredentialManager` için böyle bir interop arayüzü yoktur.** Sahip penceresi
verilemediği için istem sahipsiz bir üst-seviye pencere olur ve tarayıcının altında kalır.
Chrome'un kendi şifre yöneticisi öne gelebiliyor çünkü HWND'sini veriyor. `UserConsentVerifier`'a
geçmek çözüm değildir: yalnız kullanıcı varlığını doğrular, capability için gereken **imzayı
üretmez**; ikisini birlikte kullanmak arka arkaya iki istem demek olurdu.

**İstem penceresinin ölçülen kimliği (2026-08-07, Windows 11):** sınıf
`Windows.UI.Core.CoreWindow`, başlık "Windows Giriş Deneyimi", barındıran process `TextInputHost`.
İlk denemede sınıf adının `credential` içerdiği varsayılmıştı; **ölçüm bunu yanlışladı** ve azaltım
hiçbir şey bulamadı. Başlık yerelleştirilmiş olduğu için eşleşme ölçütü olamaz; sınıf ise dokunmatik
klavye, emoji paneli ve IME aday penceresiyle ortaktır, tek başına eşleşmek yanlış pencereyi
yakalayabilir.

**Uygulanan azaltım (`crypto/prompt_raiser.rs`) — tavsiye niteliğinde, bağlayıcı değil.** İmzalama
çağrısı sürerken ayrı bir thread çalışır. Başlangıçta aday pencerelerin bir taban listesi alınır ve
yalnızca **sonradan beliren** bir aday istem kabul edilir; böylece ekranda zaten duran aynı sınıftan
pencereler elenir. Bulunan pencere `SetWindowPos(HWND_TOPMOST)` + `BringWindowToTop` ile üste
alınır — bu iki çağrı **ön plan hakkı gerektirmez**, bu yüzden tarayıcının başlattığı bir
process'ten çalışır. Ardından `SetForegroundWindow` da denenir; reddedilirse sorun değildir.

**Bilinçli olarak reddedilen alternatifler:**

- **`AttachThreadInput`** ile tarayıcının UI thread'ine bağlanıp ön plan hakkı kazanmak: iki
  process'in girdi kuyruğu birbirine bağlanır ve biri bloke olursa diğeri kilitlenebilir. Bir
  güvenlik ürününde tarayıcıyı kilitleme riski kabul edilemez.
- **Sentetik `ALT` tuşu enjeksiyonu** (PowerToys'un kullandığı yöntem): asılma riski yoktur fakat
  sisteme sahte girdi göndermek, kullanıcı yazarken yan etki üretebilir. Aşama 1 yetersiz kalırsa
  yeniden değerlendirilecektir.
- **`chrome.debugger`/CDP ile sayfayı dondurmak** (ilgili bir kullanıcı fikri): reddedildi. CDP,
  §2.1'de infostealer'ların App-Bound Encryption'ı aştığı yöntem olarak tanımlanır ve Faz 7 izleme
  katmanı bunu **Yüksek severity** alarm sayar; kendi ürünümüzün sayfalara debugger bağlaması bu
  duruşla çelişir. Ayrıca güvenliğe katkısı yoktur, yalnız sayfa durumunu korurdu.

**Bozulma davranışı — bağlayıcı gereksinim.** Azaltımın tamamı isteğe bağlıdır: imzalama yolunu
bloke etmez, hata döndürmez ve tek bir sabit pencere sınıfına bağlı değildir. İleride bir Windows
güncellemesi pencereyi değiştirirse eşleşme olmaz ve istem bugünkü gibi arkada açılır — **hata
üretmez, kimlik doğrulama ve koruma çalışmaya devam eder.** `FCP_DISABLE_PROMPT_RAISE=1` ortam
değişkeni ile yeniden derlemeye gerek kalmadan tamamen kapatılabilir.

**Doğrulanmadı:** Üste alma işleminin klavye odağını da getirip getirmediği (PIN girişi için önemli)
manuel olarak ölçülmemiştir. Getirmiyorsa kullanıcı bir kez tıklamak zorunda kalır; bu, aşama 2
kararının girdisidir.

### Q21 revizyonu — inject artık navigasyonla başlar (2026-08-07)

**Karar:** Korunan ve `sealed` bir siteye gidildiğinde ara sayfa açılır ve **Windows Hello
kendiliğinden başlar**; kullanıcının ayrıca "Cookie ile giriş yap" düğmesine basması gerekmez.
Aynı davranış, host yeniden bağlandıktan sonraki uzlaştırma restore'u için de geçerlidir. Ara
sayfa kaldırılmamıştır: hedef adresi taşır, iptal/hata durumunda dönülecek yeri sağlar ve tekrar
deneme düğmesini barındırır.

**Gerekçe:** Kullanıcının korunan siteye gitmesi zaten "bu oturumu aç" talebidir; araya ikinci bir
tıklama koymak koruma sağlamayan bir sürtünmeydi. Manuel kullanımda rahatsız edici bulundu.

**Güvenlik etkisi — yoktur.** Hello onayı hâlâ zorunludur ve her inject için tazedir; capability
grup kimliği, operasyon, expiry, monoton sequence ve nonce'a bağlı tek kullanımlıktır
([ADR-017](#adr-017--inject-hello-capability-beş-alana-bağlı-ve-tek-kullanımlık-olacaktır),
[ADR-018](#adr-018--hello-yalnız-injectunlock-yönünde-zorunludur)). Değişen tek şey **isteği kimin
başlattığıdır**, onayın kendisi değil.

**Kaybedilen özellik, açıkça kaydedilir:** Faz 5.1'in "Hello beklenmedik anda çıkmasın" UX ilkesi
artık geçerli değildir. Kullanıcı korunan bir siteye gittiğinde prompt kendiliğinden gelir.
İzlenmesi gereken risk: aynı anda birden çok korunan sekme açılırsa (ör. tarayıcı açılışında oturum
geri yüklenirken) arka arkaya birden çok prompt oluşabilir. Bu ölçülmemiştir.

### Faz 5.1 navigasyon-öncesi unlock gate sonucu (`0.1.12`)

- **PASS — sealed + yeni navigasyon:** `webNavigation.onBeforeNavigate` gerçek sayfa yerine tek
  “Cookie ile giriş yap” düğmeli ara sayfayı açtı; Hello otomatik başlamadı.
- **PASS — kullanıcı jesti ve tam URL:** düğme tıklaması tek inject Hello başlattı; onay ve cookie
  round-trip sonrasında saklanan path/query adresine otomatik dönüldü ve sayfa ilk yüklemede giriş yapılmış
  halde açıldı. F5 gerekmedi.
- **PASS — leased ayrımı:** aktif lease sırasında F5 ve site içi linkler gate veya Hello üretmeden normal
  çalıştı.
- **PASS — ret/tekrar:** Hello iptalinde ara sayfa korundu, hata gösterildi ve aynı düğmeyle ikinci deneme
  başarıyla tamamlandı.
- **PASS — idle:** sessiz idle eviction sonrasındaki yeni navigasyon/F5 gate'i yeniden açtı; düğme
  tıklanmadan Hello çıkmadı ve onay sonrası hedef F5'siz doğru yüklendi.
- **Kabul edilen prototip sınırı:** `webNavigation` blocking bir API değildir. `onBeforeNavigate` en erken
  bildirimde `tabs.update` ile ara sayfaya yönlendirir, ancak “ilk ağ isteği kesinlikle hiç çıkmadı” garantisi
  vermez. Daha sert bir garanti gerekirse blocking/declarativeNetRequest katmanı ayrıca tasarlanacaktır.
  Mevcut kapsamda bu güvenlik açığı değil, kabul edilmiş bir UX/timing inceliğidir.

### Faz 6 nihai kabul sonucu (`0.2.0`)

- Sürüm kontrollü `account-groups.json` içinde Wikipedia (`balanced`) ve Controlled Session App
  (`critical`) grupları tanımlandı. Config grup/selector sınırlarını ve belirsiz sahipliği reddeder.
- Native Messaging v2 handshake, aynı config byte'larının SHA-256 digest'ini ve bütün grupların durable
  state/lease özetini taşır; digest uyuşmazlığı lease verilmeden global fail-closed olur.
- Host tek dispatcher state'i yerine UUID-keyed `GroupRuntime` haritası kullanır. Vault, lease metadata,
  capability ledger, pending operation, jest cache ve reconciliation state'i grup bazında ayrıdır.
- Faz 5 Wikipedia vault UUID'si korundu; eski `mvp-group.json` ve `capability-ledger.json` aynı grubun yeni
  UUID yollarına tek-seferlik taşınır.
- Extension cookie/tab/navigation/alarm/external-logout olaylarını config sahipliğine göre gruba yönlendirir.
  Grup kilitleri bağımsız, browser cookie mutation kuyruğu çakışmayı önlemek için global ve sıralıdır.
- Policy süreleri host-authoritative hale geldi: Kritik `5 dk lease / 1 dk idle / anında last-tab`, Dengeli
  `10 dk / 5 dk / 2 dk grace`, Kullanışlı `30 dk / 15 dk / 5 dk grace`; İzleme cookie mutasyonu yapmaz.
- Jest cache DEK cache'i değildir. Her inject yeni sequence/nonce capability tüketir; yalnız aynı grup için
  policy-süreli Hello handle'ı tekrar kullanılabilir. Tek process-lifetime WinRT apartment korunur, cached
  handle'lar apartment'tan önce drop edilir ve lock'ta grup bazında temizlenir.
- Reconciliation bariyeri grup bazındadır: bir grubun business-operation hatası yalnız o grubu `degraded`
  yapar. Config/framing/nonce ihlali bağlantı-geneli fail-closed kalır.
- Audit şeması cookie adı/değeri kabul etmez; Q12 isim hash'i yerine veri-minimizasyonu ile kapatıldı.
- Otomatik Rust testleri iki grup handshake/config, state ayrımı ve bir grubun invalidation'ının diğer vault'u
  değiştirmediğini kapsar.
- **Manuel kabul 12/12 PASS:** migrate edilmiş Wikipedia vault'u gate ile açıldı; controlled app sessiz
  enroll oldu; Kritik last-tab anında, Dengeli last-tab 2 dk grace ile; Kritik idle ~70 sn'de, Dengeli
  idle 5+ dk'da tahliye oldu. Ret/retry, group-doğru gate, external logout izolasyonu ve extension reload
  reconciliation beklenen grup sınırlarında çalıştı.
- **Hello cache gözlemi:** Wikipedia'nın ikinci inject'i ilkinden yaklaşık 8 dk 23 sn sonra audit'te
  `hello_cached` kaydedildi; yani last-tab eviction cache'i temizlemedi ve aynı handle yolu seçildi. Windows
  buna rağmen yeniden Hello UI gösterdi. Uygulamadaki 10 dk değer OS'nin promptsuz kalma garantisi değil,
  handle yeniden kullanım üst sınırıdır. Güvenlik açısından fail-safe, UX açısından Q22 ile izlenecek bir
  açık ölçümdür; Faz 6'yı bloklamaz.

### Faz 7 manuel test oturumu (`0.3.1` — 2026-08-06)

**Sonuç: kısmi. Çekirdek döngü doğrulandı, kabul matrisi tamamlanmadı, bir açık bug kaldı (Q23).**

Ortam: Windows 11 Pro `10.0.26200`, Chrome, host ve extension `0.3.1`, gruplar Wikipedia (Dengeli)
ve kontrollü uygulama (Kritik).

**Kök neden — oturumun ilk yarısını tüketen sorun.** Oturum boyunca gözlenen "Hello hiç çıkmıyor",
"grup `sealed`'a düşmüyor", "kendiliğinden `external_logout` oluyor" belirtilerinin tamamı tek bir
operasyonel hatadan kaynaklandı: tanı amacıyla `fcp-host.exe` elle çalıştırılırken Chrome'un kendi
host process'i de bağlıydı. İki process aynı audit HMAC zincirine yazınca zincir bozuldu
(`vault format error: audit sequence regression or gap detected`) ve host **her açılışta fail-closed
çıktı**. Extension'ın 1 sn'lik reconnect döngüsü bunu `Unchecked runtime.lastError: Error when
communicating with the native messaging host` seli olarak gösterdi. Bozuk audit dizini kenara alınıp
(`audit.corrupted-<zaman damgası>`; silinmedi) host temiz zincirle başlatıldıktan sonra sistem
düzgün çalıştı. Bu kalıcı bir kod düzeltmesi **değildir**; sertleştirme önerisi §23.1'de,
test kuralı §29.1'de yazılıdır. Kaynak kodda hiçbir değişiklik yapılmadı.

**Doğrulanan davranışlar (tekrarlı, temiz zincirle):**

- **Çekirdek döngü uçtan uca PASS.** Wikipedia'da gerçek oturum → sessiz enrollment (Hello yok) →
  sekme kapanışı → `last_tab` alarmı Dengeli policy'nin 120 sn grace'i ile kuruldu → alarm ateşlendi →
  gerçek tahliye (audit `eviction success`, 6 cookie fiilen silindi) → `sealed` → yeni navigasyon →
  `unlock.html` ara sayfası açıldı → Hello → inject → sayfa authenticated yüklendi. Döngü art arda
  birkaç kez tekrarlandı.
- **Hello cache yolu** aynı oturumda hem `hello_fresh` hem `hello_cached` olarak gözlendi; ikisi de
  çalıştı (Q22 davranışıyla tutarlı).
- **Matris #1 (baseline) PASS** — boşta hiçbir uyarı/bildirim üretilmedi, yalnız 30 sn'lik
  `fcp-monitor-poll` alarmları.
- **Matris #2 (remote-debugging tespiti) PASS** — `%TEMP%` altında ayrı profil ve
  `--remote-debugging-port=0` ile açılan ikinci Chrome, audit'e `monitor/high/remote_debugging_port`
  olarak düştü ve **görünür Windows bildirimi oluştu**. Bildirim, SVG→PNG ikon düzeltmesinden sonra
  çalışmaktadır (aşağıya bkz.).
- **Matris #4 (aktif lease sırasında host sonlandırma) PASS** — `Stop-Process` sonrası audit'e
  `monitor/high/host_disconnect_active_lease` düştü, host ~38 sn içinde kendiliğinden geri geldi ve
  `reconciliation success` ile kapandı.
- **İzleme katmanı cookie sistemini bozmuyor.** Remote-debugging testi aktif bir Wikipedia lease'i
  varken tekrarlandı; ilgili grupta hiçbir `session_invalidation` üretilmedi.

**Koşulmayan maddeler:** kabul matrisi #3 (rate-limit ikinci tur), #5 (sealed grupta dış cookie),
#6 (selector değişimi), #7 (`FCP_MONITOR_RECONCILIATION_FIXTURE`), #8 (outbox/reconnect),
#9 (audit yeniden açılış doğrulaması). Faz 7 bu nedenle **kabul edilmiş sayılmaz**.

**Açık bug — Q23.** Kontrollü uygulama grubu bir kez `restore_rejected` ile invalidate olduktan
sonra bir daha kendiliğinden enroll olmadı: kullanıcı siteye taze login yaptı, 10+ dakika beklendi,
audit'e hiçbir `enrollment` kaydı düşmedi ve grup `uninitialized` kaldı. Extension reload edilince
düzeliyor. Bu **sessiz bir koruma kaybıdır** — kullanıcıya hiçbir uyarı çıkmaz. ADR-020 bu kod
yolunu zaten kaldıracak olsa da kök neden yazılı olarak doğrulanmalıdır.

**Önceki oturumdan devralınan, bu oturumda doğrulanan düzeltme:** monitor bildirimi ikonu SVG idi;
`chrome.notifications.create` `iconUrl` için SVG kabul etmediğinden bildirim sessizce hiç
gösterilmiyordu. PNG ikona geçildi (`extension/monitor-icon.png`) ve bu oturumda gerçek bildirimin
göründüğü doğrulandı.

**Bilinen, düşük öncelikli:** WMI process gözlemcisinin dedup anahtarı `(process_id, signal)`
olduğundan tek bir Chrome başlatması ~9 tekrarlı uyarı üretiyor (renderer/GPU/utility alt process'leri
komut satırını taşıyor). Kullanıcı tarafından bilinçli olarak ertelendi.

### Faz 7 kabul turu — kalan maddeler koşuldu (2026-08-08)

ADR-021 (Hello backend göçü) ve büyük commit hijyeni sonrası, koşulmayan 5 maddenin gerçekten
hâlâ geçerli olup olmadığı test edildi.

- **"Known broken" kök nedeni bulundu ve düzeltildi.** 2026-08-05 WIP commit notundaki çökme,
  kod hatası değil, operasyonel bir hataydı (bkz. 2026-08-06 oturumu, "Kök neden" paragrafı):
  `fcp-host.exe` elle çalıştırılırken Chrome'un kendi örneği de bağlıydı, iki süreç aynı audit
  HMAC zincirine yazınca zincir bozuluyordu. `instance_lock.rs` bunu önlemek için yazıldı ve
  `host_loop.rs`'e (stdin okumadan önce) bağlandı. **Canlı doğrulandı:** Chrome'un gerçek örneği
  çalışırken elle ikinci bir örnek başlatıldı, üç saniyeden kısa sürede temiz bir hatayla
  reddedildi (`another native host instance already owns the data directory`); Chrome'un örneği
  etkilenmedi.
- **#2 (remote-debugging tespiti) tekrar doğrulandı**, **#3 (rate-limit ikinci tur) PASS** —
  10 dakikalık pencere içinde ikinci tetiklemede yeni bir toast çıkmadı (beklenen).
- **#4 (aktif lease sırasında host sonlandırma) + #8 (outbox/reconnect) PASS** — `Stop-Process
  fcp-host` sonrası audit'e `host_disconnect_active_lease` → `reconnect_success` →
  `reconciliation success` sırayla düştü.
- **#7 (`FCP_MONITOR_RECONCILIATION_FIXTURE`)** debug build'de test edildi: host env var
  ayarlanmış halde çöküp çökmediği doğrulandı — çökmedi, temiz açılıp kapandı.
- **#9 (audit yeniden açılış)** extension reload sonrası audit `sequence` alanının kesintisiz
  devam ettiği, hiçbir `audit.corrupted-*` dizini oluşmadığı doğrulandı.
- **#5 (sealed grupta dış cookie / boş-kavanoz) — kanıt zayıf.** Kullanıcı bunu denediğini
  bildirdi, ancak o pencerede audit log'da hiçbir `scope_empty` kaydı gözlenmedi; koşulan
  tahliyelerin tamamı normal `eviction success` yoluyla gitti (kasada gerçekten çerez vardı).
  Bu maddenin gerçekten tetiklenmesi için hedef grubun eviction anında **sıfır** çerez taşıması
  gerekir (örn. hiç giriş yapılmamış yeni eklenmiş bir site). Fonksiyonel olarak çalıştığı
  düşünülüyor (kod yolu ADR-020'de zaten tasarlandı) ama doğrudan audit kanıtı yok.
- **Regresyon tespit edildi ve düzeltildi:** `remote_debugging_port`/`pipe` sinyali
  `account_group_id` taşımadığından (süreç geneli, tek bir gruba bağlı değil), olay günlüğündeki
  "Site" sütunu ve bildirim metni hep boştu. İki site gerçekten leased haldeyken tetiklenen canlı
  testte doğrulandı. Düzeltme: `handleMonitorAlert` o anda leased olan grupları hesaplayıp hem
  kayıtlı log girdisine (`StoredAlert.affectedScopes`) hem bildirim metnine geçiriyor; popup ve
  options sayfası da bunu gösteriyor.
- **Yeni gözlem — Windows toast bildirimi bu oturumda hiç çıkmadı.** 2026-08-06'da SVG→PNG ikon
  düzeltmesiyle çalıştığı doğrulanmıştı; bu oturumda badge/log/audit doğru çalışsa da OS toast'ı
  hiç görünmedi ve service worker konsolunda hata yoktu. Kullanıcının kendi Windows bildirim
  ayarları/Focus Assist şüpheli görüldü, kod tarafında bir hata bulunamadı; kesin kök neden
  **doğrulanmadı**, düşük öncelikli açık nokta olarak bırakıldı.

**Sonuç: Faz 7 fiilen tamamlandı sayılır.** Tek gerçek boşluk #5'in doğrudan audit kanıtıdır;
blocker değildir.

### ADR-020 dilim 1 uygulaması ve doğrulaması (2026-08-06)

Aynı gün, ADR-020 kararının birinci dilimi kodlandı ve manuel doğrulandı. Kapsam: config şeması v2
(`cookie_selectors[]`/`health_check`/`domains[]`/`navigation_patterns[]` yerine tek `scope` alanı),
kapsam bazlı tüm-çerez okuma/yazma/silme, login-tespit yollarının silinmesi
(`waitForStableEnrollmentCookies`, `hasRequiredEnrollmentCookies`, site-özel `healthCheck`), inject
başarısının doğrulanmış round-trip'e indirgenmesi, §13.2.2 sealed kuralı, boş-kavanoz kuralı
(`scope_empty` invalidation sebebi) ve uzantı ikonuyla jest tabanlı enrollment.

Kullanılmayan `scripting` izni manifest'ten kaldırıldı; cookie host izinleri tüm-çerez modeli
şemadan bağımsız çerez taşıdığı için `*://` kalıplarına genişletildi (ADR-015 portsuzluk kuralı
korunarak).

**Doğrulama:** kilitli grupta `chrome.cookies.getAll({domain:"wikipedia.org"})` **0**, unlock
sonrası **14** çerez döndürdü (eski selector modeli 7 selector taşıyordu). Sealed → gate → Hello →
inject → leased döngüsü yeni modelde çalıştı.

**Doğrulanmayanlar:** `localhost`/kontrollü uygulama grubu bu modelde test edilmedi; idle, lock,
expiry tetikleyicileri ve boş-kavanoz (`scope_empty`) yolu manuel olarak koşulmadı. Faz 7 kabul
matrisinin kalan maddeleri de bu model üzerinde yeniden koşulmalıdır.

### ADR-020 kabul matrisi — yeni model üzerinde koşum (2026-08-06/07)

Model ve config sahipliği değiştiği için Faz 7 matrisi yeni kod üzerinde baştan koşuldu.

**Geçenler:**

- **Çekirdek döngü (A):** kontrollü uygulamada giriş → son sekme kapanışında sessiz yakalama →
  `sealed` → ara sayfa → Hello → restore. Kilitli durumda kapsamda **0** çerez.
- **Kullanıcı tanımlı koruma (B):** `x.com` popup'tan eklendi (Chrome izin diyaloğu), yakalandı,
  gate + Hello ile geri yüklendi, popup'tan kaldırıldı. İzin kaybı senaryosu (uzantı silinip
  yeniden kurulunca) tespit edilip popup'tan onarıldı.
- **İzleme (C1, C3):** remote-debugging tespiti bildirim üretti; aktif lease sırasında host
  sonlandırıldığında fail-closed temizlik, `host_disconnect_active_lease`, otomatik reconnect ve
  `reconciliation success` zinciri audit'te doğrulandı, ardından inject başarıyla tamamlandı.

**Bu koşumda bulunan ve düzeltilen iki sorun:**

1. **Süresi dolmuş çerez bütün restore'u düşürüyordu** — bkz. ADR-020 kabul edilen sınırlar.
2. **Uyarı rozeti hiç temizlenmiyordu.** Yüksek/orta severity bir olayda rozet kırmızıya dönüyor
   fakat hiçbir yerde geri alınmıyordu; kullanıcı rozeti görüp popup'ı açtığında da **ne olduğunu
   okuyamıyordu**. Kalıcı ve okunamayan bir uyarı, kullanıcıya uyarıyı yok saymayı öğretir. Son
   olay artık popup'ta okunabilir metin olarak (sinyal + ilgili site + saat) gösteriliyor ve
   popup'ın açılması onay sayılarak rozet temizleniyor.

**Koşulmayanlar / ölçülenler:**

- **C2 (rate-limit) bir tasarım eksiği ortaya çıkardı.** Ölçümde, ilk uyarı onaylandıktan
  ~20 saniye sonra tetiklenen ikinci gerçek olay kullanıcıya **hiçbir iz bırakmadı**: rate-limit
  yalnız bildirimi değil, rozeti ve kaydı da bastırıyordu. Rate-limit'in amacı bildirim spamını
  önlemektir, olayı gizlemek değil. İki kanal ayrıldı: **bildirim (toast)** rahatsız edici kanal
  olduğu için 10 dk sınırlı kalır; **rozet ve popup kaydı** her olayda güncellenir ve tekrar sayısı
  gösterilir. Böylece onaylanmış bir uyarının hemen ardından gelen olay sessizce kaybolmaz.
  Chrome bildiriminin bildirim merkezinde kalıcı olması ayrı bir OS davranışıdır; ölçümü buna göre
  yorumlamak gerekir.
- **C4 (outbox/reconnect) koşulamadı:** host öldürüldüğünde Chrome onu neredeyse anında yeniden
  başlattığı için olayların biriktiği bir pencere yakalanamadı. Ayrı bir yöntem gerekiyor.
- **Dedup hatası ölçüldü ve sürüyor:** tek bir Chrome açılışı için **200 ms içinde 9 adet**
  `remote_debugging_port` kaydı düştü (audit `sequence 86–93`). Sebep, WMI process gözlemcisinin
  dedup anahtarının `(process_id, signal)` olması ve bir Chrome başlatmasının komut satırını
  taşıyan çok sayıda alt process doğurması. Kullanıcı tarafından bilinçli olarak ertelendi;
  güvenlik etkisi yok, gürültü sorunudur.

### Değişen varsayımlar (revizyon 3 — 2026-08-06)

| Önceki varsayım | Ölçülmüş / güncel durum |
|---|---|
| Site profilleri elle küratörlük + ampirik türetme ile üretilir (§17.1) | Terk edildi. Ölçeklenmiyor ve kullanıcı kendi sitesini ekleyemiyor. ADR-020: kapsam kullanıcı jestiyle eklenir, tüm çerezler kasalanır. |
| Enrollment'ı "login oldu mu" sezgisi tetikler | Terk edildi. 2026-08-06'da bu yolun kırılgan olduğu ölçüldü (Q23) ve CentralAuth cookie rotasyonunun gerçek logout'tan ayrılması güvenilir değil. Yeni modelde enrollment açık kullanıcı jestidir. |
| Site-özel `health_check` restore doğrulamasının temelidir | Yeni modelde kaldırılıyor. Ölü oturum kendiliğinden düzelir: bayat çerezler geri konur, kullanıcı logged-out görür, tekrar giriş yapar, sonraki kapanışta yeni çerezler kasalanır. Bedel bir boşa Hello jestidir; güvenlik açığı değildir. |
| `SEALED` durumunda oluşan cookie şüphelidir ve uyarı üretmelidir | Tüm-çerez modelinde sürdürülemez (analytics/prefetch sürekli tetikler). Yeni kural §13.2.2: ilgili sekme yoksa sessizce sil, varsa unlock akışına gir. |
| Audit zinciri tek yazıcı varsayımıyla güvenlidir | Yanlış. İki eşzamanlı host process'i zinciri kalıcı olarak bozuyor ve sistemi elle müdahale gerektirecek şekilde kilitliyor (§23.1). |

### Değişen varsayımlar (revizyon 2 — 2026-08-03)

| Önceki varsayım | Ölçülmüş / güncel durum |
|---|---|
| `domain` verilmesi her test hostunda `hostOnly=false` üretir | `localhost` üzerinde yanlış: Chrome 150 cookie'yi host-only olarak geri döndürdü. Bu sonuç gerçek eTLD+1 domain'lere genellenmez; onlar doğrulanmadı. |
| CHIPS `partitionKey` güncel Chrome'da doğrudan round-trip eder | Bu test bağlamında doğrulanmadı: `chrome.cookies.set` cookie döndürmedi. Bağlam gereksinimi Q18 olarak açık. |
| Deney 2, Q8'i tümüyle kapatır | Yalnızca tek normal profil `storeId=0` ölçüldü; çoklu profil ve incognito kısmı açık kaldı. |
| Unpacked extension ID'si kararsız kalabilir | Manifest `key` alanı ile sabit ID doğrulandı; Q9 kapandı. |
| Extension sayfasından çapraz-origin login ile oluşan cookie görünmezliği partitioning'i kanıtlar | Yanlış. First-party ve `document.cookie` cookie'leri de aynı sonucu verdi; nihai neden portlu host permission'ın `getAll()` portsuz izin filtresiyle eşleşmemesiydi. Ürün deneyi yine gerçek kullanıcı akışını temsil etmek için first-party site sekmesinde login olmalıdır. |
| Cookie host permission belirli uygulama portuna daraltılabilir | Yanlış. Cookie port taşımaz ve `getAll()` izin kontrolü portsuz URL üretir. Gerçek ürün cookie host permission kalıpları bağlayıcı olarak portsuz olacaktır. |

### Değişen varsayımlar (revizyon 1 — 2026-08-02)

İlk taslakta fazla iyimser olan beş varsayım düzeltildi:

| Eski varsayım | Düzeltilmiş durum |
|---------------|-------------------|
| Host, extension'dan bağımsız kalıcı bir lease enforcer olabilir | **Yanlış.** Standart NMH host'u `connectNative` portuna bağlıdır (§9.2.1). Birincil zorlayıcı extension'dır. |
| Host başlangıçta browser store'u okuyup cookie silerek reconciliation yapar | **Yanlış.** Host store'a erişemez (§9.2.2). Reconciliation ortak host+extension işlemidir ve handshake ile tetiklenir (§15.2). Extension yoksa yapılamaz (§15.3). |
| Windows lock'ta anında tahliye garanti edilir | **Yanlış.** Best-effort + stale lease modeli (§13.2.1). |
| `UNLOCKING` hatasında `EVICTING`'e geçilir | **Yanlış.** Henüz enjeksiyon olmadığı için doğru geçiş `SEALED`'dir (§13.1). |
| Vault formatı yazılmaya hazır | **Erken.** Format `provisional` işaretlendi; wrapped DEK yerleşimi Deney 1'e bırakıldı ([Q16](#24-açık-teknik-sorular)). |

Bu düzeltmelerin sonucu olarak üç yeni açık soru eklendi: **Q15** (kalıcı Windows user agent),
**Q16** (wrapped DEK yerleşimi), **Q17** (extension yokken kullanıcı bildirimi) ve
**ADR-013** açık karar olarak kaydedildi.

### Doğrulanmış ortam bilgileri

| Öğe | Değer | Doğrulama |
|-----|-------|-----------|
| OS | Windows 11 Pro, build 10.0.26200 | Ölçüldü |
| Rust | rustc 1.96.0 / cargo 1.96.0 | Ölçüldü |
| Git deposu | `main`, 1 commit (`a87957a Initial commit`) | Ölçüldü |
| Çalışma alanı | **Temiz değil** — Deney 4 doküman/kod değişiklikleri yerel; commit veya push yok | 2026-08-03 `git status --short` |
| Takip edilen kapsam | PLAN, Deney 1–3 raporları ve Deney 1–3 POC çalışma alanları; Deney 4 raporu/kod ekleri henüz yerel | 2026-08-03 `git ls-files` |
| TPM durumu | **TPM 2.0 doğrulandı** — Platform Crypto Provider hardware-only; PCP TPM version `0x00020000` | Deney 1 `status` |
| Windows Hello kayıt durumu | **Kayıtlı, yalnızca PIN** — Yol C prompt türü PIN; biyometrik donanım yok | Q13 / Deney 1 |
| Chrome | `150.0.0.0` | Deney 2 manuel raporu |
| Cookie probe extension ID | `dokhjkpkdknopgnjdmaogjhlelcaiigo` | Manifest `key` / Deney 2 manuel yükleme |
| Cookie store | `storeId=0`, tek normal profil, incognito değil | Deney 2 manuel raporu |

### Kod durumu

- Deney 1 probe kodu yazıldı: provider doğrulama, kalıcı RSA anahtar oluşturma/inceleme/silme,
  RSA-OAEP-SHA256 DEK wrap/unwrap, süre ölçümü ve secret buffer zeroize.
- `windows 0.62.2` ve `zeroize 1.9.0` bağımlılıkları gerekçeleriyle eklendi; `Cargo.lock` tutuluyor.
- Platform ve Passport provider yolları ayrıldı; hardware/software ayrımı provider seviyesinde
  doğrulanıyor. Platform hardware-only olmalıdır; Passport dual-capability (`0x3`) bildirebilir.
- Deney 2 extension'ı yalnızca `tsc` ile derleniyor; bundler yok. Probe yalnızca sentetik
  `FCP-probe-*`, `__Host-FCP-probe` ve `__Secure-FCP-probe` cookie'leri kullanıyor.
- Deney 3 extension'ı ve kontrollü session test uygulaması `poc/session-probe/` altındadır; extension
  yalnızca `tsc` ile derlenir ve session kimliği rapor/log çıktısına yazılmaz.
- Deney 4 aynı `poc/session-probe/` çalışma alanındaki gerçek sekme/idle olay motoru ve tam sayfa
  duty-cycle harness'iyle ölçüldü; sentetik cookie snapshot değeri rapora veya olay loguna yazılmaz.
- **Commit atılmadı, push yapılmadı, branch oluşturulmadı.**

### Bilinen regresyonlar

Yok.

### Güvenlik etkileri

Yol A için software KSP fallback reddi, TPM-backed/non-exportable anahtar, wrap/unwrap ve handle başına
jest doğrulandı. Ancak CNG parola kutusu keylogger'a açık yeni bir sır ve kabul edilmesi zor UX
oluşturur. Ürün jesti Yol C Hello capability'den alacak; CNG UI policy kaldırılıp anahtar yalnızca
fiili unwrap için kullanılacak. Taze-handle kilit ölçümü kilidin davranışı değiştirmediğini kanıtladı;
Go/No-Go kriteri A karşılandı. Deney 2 gerçek hesap veya gerçek oturum cookie'si kullanmadı; bütün
probe cleanup kontrolleri geçti. Deney 3 kontrollü session restore'unu, Deney 4 ise kullanılmayan
cookie'nin `%0,012` browser-open oranına indirildiğini doğruladı. Buna rağmen CHIPS restore
uyumluluğu kanıtlanmış kabul edilmez ve Q18 kapanmadan partitioned cookie desteği varmış gibi
gösterilmez. Deney 5, aynı zinciri düşük-riskli gerçek Wikipedia hesabında yerel + CentralAuth
çoklu-cookie grubuyla doğruladı; external logout artık stale vault'u sessizce geçersiz kılar. Bu sonuç
daha yüksek riskli sitelere veya farklı auth/storage modellerine kendiliğinden genellenmez.

### 2026-08-07/08 oturumu — ADR-021 (Hello backend göçü) ve commit hijyeni

**Bağlam:** Bu oturuma girerken ADR-020'nin her iki dilimi de (yukarıya bkz.) ve Faz 7 izleme
katmanı **kodda tamamlanmış ama hiç commit edilmemiş** durumdaydı (son commit 2026-08-05,
`1f39c9a`, "Faz 7 WIP ... known broken"). Bu, kendi başına bir bulgu: günler süren gerçek iş
güvencesiz duruyordu.

- Windows Hello onay penceresinin tarayıcının arkasında açılması sorunu araştırıldı. Doğrudan
  pencere manipülasyonu (`SetWindowPos`/`BringWindowToTop`/`SetForegroundWindow`, hatta
  `HWND_BOTTOM` ile z-sırasını değiştirme) `KeyCredentialManager`'ın onay penceresine karşı
  `ERROR_ACCESS_DENIED` ile tutarlı biçimde reddedildi — hem doğrudan hedef olarak hem anchor
  referansı olarak. Bitwarden'ın kendi mühendisliği (GitHub issue #5287) aynı platform
  sınırlamasını bağımsız olarak doğruladı: "Windows' API currently lacks the ability to set a
  parent window for these kinds of requests." Bu, kod hatası değil, dokümante edilmemiş bir
  Windows platform boşluğu olarak kayda geçti.
- `webauthn.dll` (`WebAuthNAuthenticatorMakeCredential`/`GetAssertion`) gerçek `hWnd` sahipliğini
  destekleyen alternatif olarak spike'landı (`poc/webauthn-probe/`) ve sentetik RP id/origin ile
  çalıştığı, pencere sahipliğinin gerçekten çalıştığı doğrulandı. Karşılığında ölçülen kayıp:
  `WebAuthNAuthenticatorGetAssertion` durum tutmuyor (stateless); `KeyCredentialManager`'ın
  handle-tabanlı sessiz tekrar-onay penceresi karşılığı yok, yani `hello_cache_ms` artık fiilen
  etkisiz (bkz. [ADR-021](#adr-021--windows-hello-imzalama-arka-ucu-webauthndlle-taşınmıştır)).
- Göç uygulandı: `native-host/src/crypto/hello.rs` `webauthn.dll` üzerinden imzalama/doğrulamaya
  yeniden yazıldı, `prompt_raiser.rs` (harici pencere-yükseltme iş-arounduı) tamamen kaldırıldı.
  Ölçülen ve düzeltilen iki gerçek hata: allow-list yanlış/eski struct alanına yazılıyordu
  (`AllowCredentialCount: 0` — WebAuthN operational event log ile doğrulandı) ve `dwVersion`
  eski sürümde bırakıldığı için yeni alanlar sessizce yok sayılıyordu.
- "Kutu geç açılıyor" şikâyeti Windows'un kendi `Microsoft-Windows-WebAuthN/Operational` event
  log'u (milisaniye hassasiyetinde) ile teşhis edildi: CTAP başlangıcı ile NGC/PIN kutusunun
  başlaması arasında ~1 saniyelik açıklanamayan bir boşluk var, ama bu **Chrome'un kendi
  webauthn.dll çağrısında da birebir aynı** (ölçülen: 1.066s bize karşı 1.064s Chrome'a) —
  evrensel Windows maliyeti, koddan kaynaklanmıyor. Kod imzalama (self-signed sertifika ile
  test edildi) bu boşluğu etkilemedi; teori çürütüldü ve temizlendi.
- Ayrı, alakasız bir hata bulundu ve düzeltildi: `cookie_roundtrip_failed` sağlık kontrolü
  youtube.com'da tekrarlanan bir başarısızlık üretiyordu. Kök neden: sitenin kendi sayfa script'i
  restore penceresinde kendi çerezini (`GPS`) ekliyordu ve "tam eşitlik" kontrolü bunu hata
  sanıyordu. Düzeltme: alt-küme kontrolüne (yalnızca kasalanan çerezlerin gerçekten geldiğini
  doğrula) geçildi, artı geçici okuma gecikmesi için sınırlı retry eklendi.
- **Commit hijyeni:** Faz 7 + ADR-020 birikimi ve ADR-021 göçü iki ayrı, kendi başına derlenen
  commit'e bölündü (`c9116f7`, `92845bf`); `poc/webauthn-probe/target/` yanlışlıkla stage'e giren
  derleme çıktısı ayıklanıp `.gitignore`'a eklendi.

---

## 31. Sonraki Kesin Adım

**Düzeltme (2026-08-08):** Bu bölüm önceki sürümde "Faz 8 tasarımı henüz yapılmadı" diyordu; bu
yanlıştı — ADR-020'nin uygulama sözleşmesi zaten yazılmış, kodlanmış ve her iki dilimiyle
(tüm-çerez kasalama + kullanıcının kendi sitesini eklemesi) 2026-08-06'da manuel doğrulanmıştı.
Belgenin özet/yol haritası bölümleri bunu yansıtmıyordu; §30 ve ADR-020'nin kendisiyle
tutarsızdı. Bu güncellemeyle düzeltildi (bkz. üstteki [Durum](#fursoy-cookie-protector--proje-planı-ve-teknik-karar-kaydı)
özeti, [§25](#25-yol-haritası) Faz 8 satırı, ADR-020).

**Gerçek durum:** Faz 8 (ADR-020) tasarım ve uygulama açısından tamamlanmıştır; kalan iş yalnızca
**tam kabul matrisinin koşulmasıdır**, yeni bir tasarım kararı değil. Ayrıca 2026-08-07/08
oturumunda ADR-021 ile Hello imzalama arka ucu değiştirildi (webauthn.dll) — bu da ayrı bir
kabul/regresyon turu gerektirir.

**Güncelleme (2026-08-08, aynı gün):** Madde 1 fiilen kapandı — bkz. "Faz 7 kabul turu — kalan
maddeler koşuldu" (§30). "Known broken" kök nedeni bulunup (`instance_lock.rs`) canlı doğrulandı;
kabul matrisinin #1–4 ve #7–9 maddeleri geçti, #6 anlamsızlaştı, yalnızca #5'in doğrudan audit
kanıtı eksik kaldı.

**Açık kalan, gerçekten "sıradaki iş" olan maddeler** (öncelik sırası kullanıcı tarafından
belirlenecektir):

1. **Faz 7 matris #5** (sealed grupta dış cookie / boş-kavanoz) — kullanıcı denedi ama audit'te
   `scope_empty` kaydı gözlenmedi; hedef grubun eviction anında gerçekten sıfır çerez taşıması
   sağlanarak tekrar denenmeli.
2. **ADR-020'nin tam kabul matrisi** — dilim 1+2 birlikte, birden fazla site ve policy seviyesinde
   uçtan uca koşulmadı (yalnızca Wikipedia + x.com nokta testleri yapıldı).
3. **ADR-021 kabulü** — webauthn.dll göçünün gerçek kullanımda (birden fazla site, birden fazla
   gün) regresyon üretmediğinin doğrulanması; özellikle `hello_cache_ms`'in artık etkisiz olmasının
   kabul edilebilir olup olmadığına dair kullanıcı geri bildirimi.
4. **Windows toast bildirimi** 2026-08-08 oturumunda hiç çıkmadı (badge/log/audit doğru
   çalışırken) — kök nedeni doğrulanmadı, kullanıcının kendi Windows bildirim ayarları şüpheli
   görüldü ama kesinleşmedi.
5. **Q23** (başarısız restore sonrası re-enroll olmama) kök nedeni, ilgili eski kod yolu
   kaldırılmış olsa bile yazılı olarak doğrulanmalı; aynı hata sınıfının yeni modelde
   tekrarlanmadığı gösterilmelidir.

**Kullanıcının açık onayı olmadan yüksek riskli site testi veya gerçek ana hesap testi
başlatılmayacaktır.** §29.1 test sırası geçerliliğini korur.

Q20 medya/pasif görünür kullanım, Q18 partitioned cookie, Q8 çoklu profil/incognito ve Q15/Q17
kalıcı agent/bildirim açık kalır. Q22 blocker değildir. Faz 5.1'in blocking olmayan `webNavigation`
sınırı kabul edilmiştir; ağ seviyesinde kesin pre-request engelleme gerekli görülürse blocking/DNR
tasarımı ayrı bir iş olacaktır.

---

## 32. Karar Günlüğü

> Yalnızca mimariyi, güvenlik modelini veya uzun vadeli bakımı etkileyen kararlar için ADR yazılır.
> Her küçük değişiklik için ADR oluşturulmaz.

---

### ADR-001 — Native host dili Rust

**Durum:** Kabul edildi

**Karar:** Native host Rust ile geliştirilecek. İlk TPM probe da aynı dilde yazılacak.

**Gerekçe:**
- Anahtar ve plaintext buffer yaşam döngüsü üzerinde kontrol
- CNG/NCrypt FFI ve WinRT erişimi (`windows` crate ile ikisi birden)
- `zeroize` ile kontrollü silme, sabit boyutlu buffer
- Managed runtime kopyalarından kaçınma (C#'ta `string` immutable, GC buffer'ları taşır,
  serializer geçici kopya üretir; `CryptographicOperations.ZeroMemory` yalnızca verilen
  span'i temizler, önceden oluşmuş kopyaları temizleyemez)

**Alternatifler:** C#, C++

**Sonuç:** Probe ile gerçek host aynı dilde olacağı için ölçülen davranış birebir geçerli kalır.

---

### ADR-002 — Cookie erişimi `chrome.cookies` API üzerinden, DB deşifresi yapılmayacak

**Durum:** Kabul edildi

**Karar:** Cookie okuma/yazma yalnızca extension'ın `chrome.cookies` API'si ile yapılacak.
Cookie SQLite veritabanı doğrudan okunmayacak veya deşifre edilmeyecek.

**Gerekçe:**
- HttpOnly cookie'ler bu API ile okunabilir ve yazılabilir
- App-Bound Encryption ile mücadele etmeye gerek kalmaz
- DPAPI / ABE davranışındaki tarayıcı sürümü değişikliklerine dayanıklı
- Chromium tabanlı tarayıcılar arasında taşınabilir

**Alternatifler:** DB + `Local State` deşifresi; CDP üzerinden erişim (`--remote-debugging-port`
açmak saldırı yüzeyi ekler — reddedildi)

**Sonuç:** Extension zorunlu bileşen haline gelir; cookie plaintext'i JS heap'inden geçer.

---

### ADR-003 — Grup başına DEK, transaction-scope anahtar ömrü

**Durum:** Kabul edildi

**Karar:** TPM-backed KEK altında her hesap grubu için ayrı DEK kullanılacak. DEK yalnızca
tek bir vault transaction süresince bellekte tutulacak; cache edilmeyecek.

**Gerekçe:**
- Tek global master key bellekte beklerse, process belleğini okuyan malware **tüm kasayı** açar
- Grup ayrımı bir grubun açılmasını diğerlerinden izole eder
- TPM unwrap maliyeti (~50–200 ms beklenen) lease'ler dakikalar arayla olduğu için ihmal edilebilir
- Rastgele bellek snapshot saldırısının başarı penceresi transaction süresine iner

**Alternatifler:** Tek master key + süreli cache (8 saat / 30 dk) — reddedildi

**Sonuç:** Policy seviyeleri **jest cache süresi** üzerinden tanımlanır, anahtar cache'i üzerinden değil.

**Deney 1 doğrulaması (2026-08-03):** `handle-cycle 30` içinde her yeni CNG key handle 1372–3029 ms
sürdü ve yeniden jest istedi. Aynı handle kilit sınırı boyunca yeniden kullanıldığında ikinci unwrap
34.996 ms ile ücretsizdi; kilit öncesi ve sonrası farklı handle'lar kullanıldığında ise sırasıyla
3386.454 ms ve 3541.494 ms ölçüldü ve ikisi de jest istedi. Bu nedenle üretim invariant'ı şudur:
her vault transaction'ında yeni handle aç, unwrap işlemini yap ve handle'ı kapat; handle'ı
transaction'lar arasında cache'leme veya yeniden kullanma.

---

### ADR-004 — Recovery / escrow anahtarı yok

**Durum:** Kabul edildi

**Karar:** Kasa için recovery kodu, yedek anahtar veya escrow mekanizması **uygulanmayacak**.

**Gerekçe:**
- Cookie **yeniden üretilebilir** bir kimlik bilgisidir; kayıp = tekrar giriş yap
- Escrow anahtarı, kasanın en zayıf halkası ve en cazip hedefi olurdu
- Büyük bir tasarım ve bakım yükünü ortadan kaldırır
- Anahtar kaybının **kullanılabilirlik** sorunu olması, **güvenlik** sorunu olmaması ilkesine uyar

**Alternatifler:** Recovery kodu, ikinci cihaz escrow'u, parola tabanlı yedek KEK

**Sonuç:** TPM clear / anakart değişimi / Windows yeniden kurulumu tüm korunan oturumları
sıfırlar. Bu ürün metninde açıkça belirtilir.

---

### ADR-005 — Fail-to-logout varsayılan hata davranışı

**Durum:** Kabul edildi

**Karar:** Belirsizlik veya hata durumunda sistem cookie **vermez**; tercih edilen sonuç logout'tur.

**Gerekçe:**
- Yanlış yönde hata yapmanın maliyeti düşüktür (kullanıcı tekrar giriş yapar)
- ADR-004 sayesinde "kalıcı veri kaybı" riski yoktur
- Bu, sistemin her yerde agresif davranmasına izin verir

**Sonuç:** Health check başarısızlığı, vault doğrulama hatası, reconciliation hatası ve
protokol tutarsızlığı hep aynı güvenli sonuca yönlenir.

---

### ADR-006 — Obscurity güvenlik sınırı değildir; minimum host sertleştirme seti

**Durum:** Kabul edildi

**Karar:** Rastgele binary/registry adları, parent process doğrulama ve diskte statik shared
secret **uygulanmayacak** veya güvenlik sınırı olarak sunulmayacak. Yalnızca protokol hijyeni
niteliğindeki düşük maliyetli önlemler uygulanacak ([§16.4](#164-minimum-host-sertleştirme)).

**Gerekçe:**
- PPID sahtelenebilir; manifest ve registry enumerate edilebilir; diskteki sır okunabilir
- Bu önlemler haftalar alır ve gerçek sınırı değiştirmez
- Aynı kullanıcı yetkisindeki saldırgana karşı tek gerçek sınır TPM/Hello jesti ve kısa
  maruziyet süresidir

**Sonuç:** Connection nonce ve sequence number **replay/protokol hijyeni** olarak etiketlenir,
kimlik doğrulama olarak değil.

---

### ADR-007 — Kasalama birimi account group

**Durum:** Kabul edildi

**Karar:** Kasalama birimi tek origin değil, birden çok domain'i kapsayan hesap grubudur.

**Gerekçe:**
- Bir hesap birden çok domain kullanır; auth domain'i uygulama domain'inden farklı olabilir
- OAuth redirect'leri ve iframe'ler grup sınırını genişletir
- Partitioned cookie'ler top-level site bağlamına bağlıdır
- Kısmi tahliye hem korumayı hem oturumu bozar

**Sonuç:** Veri modeli `account_group` etrafında kurulur; v1 yalnızca cookie destekler ancak
şema `storage_artifacts` için yer bırakır.

---

### ADR-008 — Jest cache süresi, DEK ömrü ve cookie lease süresi ayrı katmanlardır

**Durum:** Kabul edildi

**Karar:** Üç yaşam süresi bağımsız yönetilecek: kullanıcı jesti (cache'lenebilir),
DEK unwrap (cache'lenmez), cookie lease (dakikalar).

**Gerekçe:**
- Bunları birleştirmek ya UX'i (her 10 dk Hello) ya güvenliği (gün boyu açık cookie) yok eder
- Ayırmak, günde 1–3 Hello ile düşük duty cycle'ı aynı anda mümkün kılar
- Takas tek bir yerde (jest cache) toplanır ve policy ile yönetilebilir hale gelir

**Sonuç:** Policy seviyeleri jest cache süresine göre tanımlanır. "Chrome açılınca bir Hello,
kapanana kadar açık" modeli **reddedilmiştir** — maruziyet penceresini maksimize eder.

---

### ADR-009 — v1 kapsamı: Chrome, tek profil, yalnızca cookie

**Durum:** Kabul edildi

**Karar:** v1 yalnızca Google Chrome'u, tek profili ve yalnızca cookie artefaktını destekler.
Firefox kapsam dışıdır.

**Gerekçe:**
- Firefox'un kendi cookie partitioning modeli (dFPI) ayrı bir uyumluluk çalışması gerektirir
- Çoklu profil `storeId` semantiği doğrulanmadı ([Q8](#24-açık-teknik-sorular))
- `localStorage` / `IndexedDB` artefaktları farklı bir çıkarma/geri yükleme mekanizması ister

**Sonuç:** Veri modeli genişlemeye açık tutulur; Edge/Brave Faz 8'de değerlendirilir.

---

### ADR-010 — TPM/Hello davranışı ölçülecek, varsayılmayacak

**Durum:** Kabul edildi

**Karar:** `NCRYPT_UI_POLICY`'nin her kullanımda kullanıcı jesti üreteceği **varsayılmayacak**.
Üç yol (CNG UI policy, Passport KSP, `KeyCredentialManager` capability) ölçülerek karşılaştırılacak.

**Gerekçe:**
- Jest davranışı anahtar özelliklerine, KSP'ye, process ömrüne, Windows credential cache'ine
  ve cihaz politikasına bağlı olabilir
- Ürünün en güçlü iddiası buna dayanıyor; dokümana bakarak kabul edilemez

**Sonuç:** Deney 1 projenin ilk işidir ve ürün iddiasının kapısıdır ([§22](#22-go--no-go-kriterleri)).
Sonuç olumsuzsa proje iptal edilmez; iddia küçültülür.

---

### ADR-011 — Bellek hijyeni kademelendirilir

**Durum:** Kabul edildi

**Karar:** DEK hijyeni katı, cookie plaintext hijyeni best-effort olarak uygulanacak.

**Gerekçe:**
- Cookie plaintext'inin son durağı Chrome'un JS heap'idir; `chrome.cookies.set` için değer bir
  JS string'inde bulunmak zorundadır ve JS string'leri immutable ve GC'lidir
- Host tarafında aşırı hijyen yatırımı bu tavanı yükseltmez
- DEK sızıntısının etkisi (tüm grup) cookie plaintext sızıntısından (tek oturum, lease penceresi)
  büyüktür

**Sonuç:** Efor DEK ve vault transaction'ına odaklanır; extension tarafında makul özen yeterlidir.

---

### ADR-012 — Watcher ve kernel minifilter v1'de yok

**Durum:** Kabul edildi

**Karar:** İzleme/tespit katmanı ve kernel minifilter driver v1 kapsamı dışındadır.

**Gerekçe:**
- Minifilter EV sertifika + Microsoft attestation imzalama gerektirir; maliyet ve uyumluluk riski yüksek
- İzleme katmanı faydalıdır ancak mimarinin ana belirsizliğini (Deney 1) çözmez
- Önce koruma mekanizmasının çalıştığı doğrulanmalı

**Sonuç:** Watcher Faz 7'ye ertelendi; kernel minifilter yol haritasında yok.

---

### ADR-013 — Kalıcı Windows user agent (açık karar)

**Durum:** 🔶 **AÇIK — karar verilmedi.** Deney 1 ve Deney 4 sonuçlarından sonra karara bağlanacak.

**Bağlam:** Standart Chrome Native Messaging modelinde host, extension'ın `connectNative()`
çağrısıyla başlatılır ve port kapandığında yaşam döngüsü sona erebilir (§9.2.1). Ayrıca host
browser cookie store'una erişemez (§9.2.2). Bu iki sınır birlikte şu boşlukları yaratır:

- Chrome kapalıyken lease expiry takibi yapılamaz
- Windows lock anında tahliye garanti edilemez (§13.2.1)
- Extension devre dışıysa reconciliation yapılamaz ve **kullanıcı bilgilendirilemez** ([Q17](#24-açık-teknik-sorular))
- Koruma durumu için kalıcı bir UI/bildirim kanalı yoktur

**Seçenek 1 — Kalıcı user agent eklenmez (mevcut varsayılan)**

- Artı: saldırı yüzeyi küçük, kurulum basit, autostart yok, güncelleme yolu tek
- Eksi: yukarıdaki boşluklar kalır; güvenlik tamamen extension + başlangıç reconciliation'ına dayanır
- Bu seçenekte sistem, boşlukları **gizlemez**: `degraded` durumu ve `unnecessary_exposure`
  metriği ile görünür kılar

**Seçenek 2 — Kalıcı Windows user agent eklenir**

Oturum açılışında başlayan, Chrome'dan bağımsız yaşayan bir kullanıcı process'i (tray/agent).

- Artı: Chrome kapalıyken lease takibi; lock bildirimini güvenilir alma; extension yokken
  kullanıcıyı uyarabilecek bir UI kanalı; reconciliation'ı tetikleyebilme
- Eksi: kalıcı bir saldırı yüzeyi; autostart ve kurulum karmaşıklığı; ayrı güncelleme yolu;
  öldürülebilir olduğu için **yine kesin garanti vermez**
- **Önemli:** Kalıcı agent cookie store'a yine erişemez. Cookie kaldırma her koşulda
  extension'a bağımlı kalır. Yani agent, "Chrome kapalıyken tahliye" sorununu **çözmez**;
  yalnızca takip, bildirim ve tetikleme boşluklarını kapatır.

**Karar girdisi:** Deney 4'ün `unnecessary_exposure / browser_open_time` sonucu. Boşluğun
gerçek maruziyete katkısı ölçüldükten sonra, ek saldırı yüzeyinin değip değmediği
değerlendirilecek.

**Şimdilik geçerli varsayım:** Seçenek 1. v1 tasarımı kalıcı agent olmadan çalışacak şekilde
kurgulanır; Seçenek 2 seçilirse ek bir bileşen olarak gelir, mimariyi yeniden yazmayı gerektirmez.

---

### ADR-014 — Jest kaynağı Windows Hello, DEK unwrap TPM-backed CNG anahtarı

**Durum:** Kabul edildi

**Bağlam:** Deney 1'de Yol A'nın Platform Crypto Provider strong-key protection diyaloğu her yeni
handle için parola/PIN metni girip Enter basılmasını gerektirdi. Yol C'nin `KeyCredentialManager`
akışı ise Windows Hello'nun tek adımlı kullanıcı deneyimini verdi. İki yol da handle-scoped per-use
jest üretebildi; ancak Yol C yalnızca challenge imzalar ve doğrudan DEK unwrap yapamaz.

**Karar:**

- Gerçek kullanıcı jesti ve yetkilendirme kaynağı Yol C (`KeyCredentialManager` / Windows Hello)
  olacaktır.
- Hello imzası, hesap grubu ve işlem bağlamına bağlı kısa ömürlü, tek kullanımlık bir capability
  üretecektir.
- DEK'in fiili unwrap işlemini Yol A'daki Platform Crypto Provider TPM-backed CNG anahtarı yapacaktır.
- CNG anahtarının `NCRYPT_UI_POLICY` ayarı kaldırılacak ve unwrap sessiz çalışacaktır; ikinci bir CNG
  parola istemi gösterilmeyecektir.
- ADR-003 ile uyumlu olarak her vault transaction'ı yeni CNG key handle açacak, unwrap yapacak ve
  handle'ı kapatacaktır.

**Gerekçe:** Yol A'nın parola + Enter akışı Yol C'nin Hello akışından belirgin biçimde daha yavaş ve
sürtünmelidir; ayrıca yazılabilir bir parola keylogger riskini ve yönetilecek ek bir sırrı doğurur.
Yol C daha iyi jest UX'i sağlarken Yol A TPM'e bağlı, dışa aktarılamayan anahtarla gerekli unwrap
primitive'ini sağlar. İki mekanizma alternatif değil, birbirini tamamlayan katmanlardır.

**Ölçüm dayanağı:** `handle-cycle 30` her yeni handle'da jest gösterdi. Aynı-handle lock testi ikinci
kullanımın cache'li olduğunu; taze-handle lock testi ise kilit öncesi ve sonrasında yeni handle'ların
ikisinin de yeniden jest istediğini gösterdi. Jest süre, process veya kilit durumuna değil handle'a
bağlıdır.

**Alternatifler:** Yalnız Yol A strong-key UI — UX ve keylogger riski nedeniyle reddedildi. Yalnız
Yol C — DEK unwrap yapamadığı için reddedildi. Passport KSP üzerinden doğrudan CNG — bu makinede
`NTE_INVALID_PARAMETER (0x80090027)` ile desteklenmedi.

**Sonuç:** Yetkilendirme Hello capability ile, kriptografik unwrap TPM-backed CNG anahtarıyla
gerçekleşir. CNG handle'ları transaction kapsamının dışına taşmaz.

---

### ADR-015 — Cookie host permission kalıpları portsuz olacaktır

**Durum:** Kabul edildi

**Bağlam:** Deney 3'ün ilk dört manuel çalışmasında server ve `document.cookie` tarafından yazıldığı
kanıtlanan cookie'ler `chrome.cookies.getAll()` sonucunda görünmedi. Deney 2'nin kendi
`chrome.cookies.set()` çağrısıyla yazdığı cookie'leri okuyabilmesi; partitioning, store, profil,
popup, izin türü, manifest key ve unpacked/Web Store hipotezlerine yol açtı. Chromium uygulaması
incelendiğinde `getAll()` sonucundaki her cookie'nin izninin cookie scheme+domain alanlarından
üretilen portsuz URL ile denetlendiği görüldü. POC manifestlerindeki
`http://localhost:43118/*` gibi portlu kalıplar bu URL ile eşleşmiyor ve cookie'leri sessizce
eliyordu. `set()` ve URL tabanlı `get()` verilen portlu URL'yi doğrudan kullandığından aynı kalıpla
çalışabiliyordu.

**Karar:** Cookie erişimi yetkilendiren bütün `host_permissions` ve `optional_host_permissions`
kalıpları portsuz yazılacaktır. Uygulama URL'leri ile `content_scripts.matches` gerektiğinde belirli
porta bağlı kalabilir; ancak Cookies API host izni `https://example.com/*`,
`http://localhost/*` veya `http://127.0.0.1/*` biçiminde olacaktır. CI/manifest doğrulaması ileride
cookie host izinlerinde açık port bulunmasını hata saymalıdır.

**Gerekçe:** Cookie kapsamı porttan bağımsızdır. Portlu izin `set()`/`get()` ve `getAll()` arasında
yanıltıcı, sessiz bir davranış farkı üretir; snapshot/eviction/reconciliation güvenilirliğini
bozar. Portu kaldırmak gereksiz bir cookie yetki genişlemesi değildir; tarayıcının cookie güvenlik
modeliyle uyumlu doğru yetkilendirmedir.

**Ölçüm dayanağı:** İzinler portsuz kalıplara çevrildikten sonra aynı unpacked ve sabit-key Deney 3
extension'ı 136/136 kontrolü geçti; aynı session üzerinde 10/10 restore, %0 yanlış logout ve 0
güvenlik alarmı ölçüldü. Server-side logout sonrası stale restore `invalid_session` verdi.

**Alternatifler:** Portlu host permission'ı koruyup yalnız URL filtreli `get()` kullanmak — snapshot
ve domain genelindeki cookie keşfini eksik bırakacağı için reddedildi. `<all_urls>` — gereksiz geniş
yetki olduğu için reddedildi. Web Store dağıtımına güvenmek — kök neden dağıtım kaynağı olmadığı için
reddedildi.

**Sonuç:** Deney 3 tamamlandı ve §22.3 kriterleri karşılandı. Bu kural ürün manifesti için
bağlayıcıdır; ihlali cookie'lerin sessizce snapshot dışında kalmasına yol açabilir.

---

### ADR-016 — Wrapped DEK grup dosyasında saklanacaktır

**Durum:** Kabul edildi — Q16 kapandı

**Bağlam:** Deney 1, üretim KEK yolunu TPM-backed/non-exportable RSA-2048 ve
RSA-OAEP-SHA256 olarak doğruladı. 32-byte DEK'in wrapped çıktısı 256 byte'tır. Wrapped DEK'in
manifest'te tutulması, encrypted grup dosyasıyla iki dosyalı transaction ve çapraz tutarlılık
gerektirir.

**Karar:** Aday A seçildi. Wrapped DEK yalnız `<group_id>.fcpv` authenticated başlığında tutulur;
manifest'te ikinci kopyası bulunmaz. FCPV v1 offset/algoritma/uzunlukları §12.1'deki biçimde
dondurulmuştur. KEK rotasyonu grup dosyalarını tek tek atomik olarak yeniden yazar.

**Gerekçe:** Grup dosyasının kendi kendine yeterli olması ve tek dosyalı atomik write/verify/replace
zinciri, manifest merkezli toplu rotasyon kolaylığından daha önemlidir. Faz 5 tek grupla başlar;
çoklu grupta bozulma ve rotasyon etki alanı yine grup başına sınırlı kalır.

**Alternatif:** Aday B — wrapped DEK manifest'te. İki dosyalı crash tutarlılığı ve hangi kopyanın
yetkili olduğu riskini getirdiği için reddedildi.

---

### ADR-017 — Inject Hello capability beş alana bağlı ve tek kullanımlık olacaktır

**Durum:** Kabul edildi — kapsam ADR-018 ile inject-only olarak daraltıldı

**Bağlam:** Deney 1 Yol C, KeyCredentialManager challenge imzasının mümkün olduğunu doğruladı;
fakat alan binding ve replay kontrolü deney kapsamı dışındaydı. Jest cache'i varken bağlamsız veya
yeniden kullanılabilir bir imza farklı grup/operasyon için kötüye kullanılabilir.

**Karar:** Windows Hello'nun imzaladığı canonical challenge; `account_group_id`, `operation`
(`inject`), `expiry`, `monotonic_sequence` ve 32-byte `nonce` alanlarının tamamını içerir.
Sequence/nonce host tarafından ayrılır ve kalıcı ledger'a yazılır. İmza ile bekleyen state-machine
geçişi tam eşleştirilir; capability TPM unwrap'tan önce durable olarak tüketilir. Expired,
alanı değiştirilmiş, yanlış operasyona/gruba ait veya daha önce kullanılmış capability reddedilir.

**Gerekçe:** Hello yalnız kullanıcı varlığını kanıtlar; yetkinin hangi hassas transaction'a ait
olduğunu ve tek kullanımlılığını uygulama katmanı sağlamalıdır. Canonical binary encoding JSON
normalizasyon belirsizliğini kaldırır; durable consume-before-unwrap sırası crash/replay penceresini
kapatır.

**Sonuç:** `protocol/messages.rs` alan/canonical sözleşmesini, `crypto/hello.rs` Hello
sign/verify işlemini ve `lease/state_machine.rs` reserve/consume/replay reddini uygular. Capability
ledger persistence başarısız olursa TPM unwrap yapılmaz.

---

### ADR-018 — Hello yalnız inject/unlock yönünde zorunludur

**Durum:** Kabul edildi

**Bağlam:** İlk Faz 5 implementasyonu capability `operation` alanını `inject|evict` olarak modelledi
ve enrollment, last-tab, idle, lock, expiry ile reconciliation vault yazımlarının tümünü Hello'ya
bağladı. Bu, §7 fail-to-logout ilkesiyle çelişir. Özellikle idle/lock kullanıcı yokken tetiklendiği
için prompt onayı beklemek tahliyeyi pratikte tamamlanamaz ve fail-open hale getirir.

**Karar:** Windows Hello capability yalnız cookie plaintext'ini `SEALED` durumdan browser store'a
çıkaran inject/unlock transaction'ını yetkilendirir. Capability `operation` alanı explicit binding
olarak kalır fakat yalnız `inject` değerini kabul eder; `evict` varyantı protokol ve Rust enumundan
kaldırılır. Enrollment, eviction ve reconciliation capability üretmez veya ledger sequence tüketmez.
Gerekli TPM-backed DEK unwrap sessiz CNG yoluyla, tek transaction kapsamında yürütülür.

**Gerekçe:** Inject gizlilik açısından risk artıran yöndür ve kullanıcı varlığı gerektirir. Eviction
maruziyeti azaltır; ek insan onayı güvenlik sağlamaz, yalnız last-tab/idle/lock/expiry güvenilirliğini
düşürür. Aynı kullanıcıdaki saldırgan vault'u zaten silebildiğinden evict capability kullanılabilirlik
DoS'una anlamlı yeni bir sınır eklemiyordu. Atomik write/read-back doğrulaması ve cookie'nin yalnız
`evict.confirmed` sonrasında silinmesi veri kaybı koruması olarak kalır.

**Alternatif:** `operation=evict` için otomatik/sessiz capability üretmek. İnsan jesti taşımayan bir
capability gerçek bir yetkilendirme sınırı olmadığı halde öyle görünür ve ledger/protokolü gereksiz
karmaşıklaştırır; reddedildi.

**Sonuç:** Enrollment ve eviction audit outcome'u `started` olur; `authorized` yalnız inject için
kullanılır. Idle/lock tahliyesi kullanıcı yokken tamamlanabilir. Capability replay/nonce/sequence
koruması yalnız inject unwrap öncesinde bağlayıcıdır.

---

### ADR-019 — Account group config-authoritative ve çalışma zamanı grup-izole olacaktır

**Durum:** Kabul edildi ve doğrulandı — Faz 6 `0.2.0` manuel kabul 12/12 PASS

**Bağlam:** Faz 5 dispatcher, extension state, cookie selector ve policy değerleri tek Wikipedia UUID'sine
hardcoded idi. Yalnız sabiti listeye çevirmek; config drift, selector çakışması, bir grubun pending/crash
durumunun diğerini engellemesi ve iki ayrı WinRT apartment yaşam döngüsü risklerini çözmez.

**Karar:** Sürüm kontrollü `account-groups.json` domain/selector/policy/health sözleşmesinin kaynağıdır.
Host ve extension aynı byte'ların SHA-256 digest'ini Native Messaging v2 handshake'te bağlar. Host UUID-keyed
`GroupRuntime`, extension UUID-keyed runtime state kullanır; vault, lease, capability ledger, pending operation,
alarm ve reconciliation bariyeri grup bazındadır. Cookie mutation ve Hello prompt işleme tek bağlantıda
sıralı kalır. Tek process-lifetime WinRT apartment içinde Hello handle cache'i grup UUID'sine göre ayrılır.

**Gerekçe:** Config yönetim UI'ı olmadan Faz 6 kapsamını dar tutar; digest deploy edilen iki bileşenin farklı
selector/policy ile çalışmasını fail-closed engeller. Grup-bazlı state bir business-operation hatasının diğer
grupları bozmasını önler. Global sıralama ise ortak browser cookie store ve Windows Hello UI'sında yarışları
engeller.

**Sınırlar:** Dinamik kullanıcı grubu/optional host permission UI'ı, incognito/çoklu profil, kalıcı agent ve
medya-aware idle sonraki kararlardır. Config/framing/nonce ihlali bağlantı-geneli fail-closed kalır; sıradan
grup hatası yalnız ilgili grubu `degraded` yapar.

> **ADR-020 notu:** Bu ADR'nin "dinamik kullanıcı grubu sonraki karardır" sınırı ADR-020 ile
> karara bağlanmıştır. Config-authoritative ilkesi ve grup-izolasyonu geçerliliğini korur; değişen
> şey config'in kaynağıdır (elle yazılan dosya → kullanıcı jestiyle büyüyen kayıt). Digest/fail-closed
> sözleşmesinin çalışma zamanında nasıl korunacağı sorusu **Q24** idi; ADR-020'nin "host config'in
> tek sahibidir" çözümüyle 2026-08-06'da kapandı (bkz. ADR-020 "Q24 çözümü").

---

### ADR-020 — Korunan site kullanıcı tarafından eklenir ve tüm çerezler kasalanır

**Durum:** Kabul edildi (2026-08-06). **Her iki dilim de uygulandı ve doğrulandı** (bkz. aşağıdaki
tablo); tam kabul matrisi koşulmadı.

**Uygulama durumu:**

| Dilim | Kapsam | Durum |
|---|---|---|
| 1 | Config şeması v2 (`scope`), tüm-çerez kasalama, login tespitinin kaldırılması, sealed/boş-kavanoz kuralları, jest ile enrollment | ✅ Uygulandı, 2026-08-06 manuel doğrulandı |
| 2 | Kullanıcının kendi sitesini eklemesi: çalışma zamanı config (Q24), `group.add`/`group.remove`, `optional_host_permissions`, popup UI | ✅ Uygulandı; `x.com` üzerinde ekleme → yakalama → gate → Hello → restore turu 2026-08-06'da çalıştı. Tam kabul matrisi koşulmadı |

**Q24 çözümü — host config'in tek sahibidir.** Extension kendi config dosyasını taşımaz; config'i
handshake'te host'tan alır, doğrular ve yalnız host erişilemezken fail-closed tahliye yapabilmek
için `chrome.storage.local`'a cache'ler. Digest artık "iki taraf bağımsızca aynı dosyaya mı sahip"
değil, "gönderilen config bozulmadan ulaştı mı" anlamındadır; geçersiz config extension'ı durdurur.
Kullanıcı eklemeleri `group.add`/`group.remove` ile hosta gider, host UUID atar, **açılışta
kullandığı aynı validator'dan geçirir** (böylece çalışma zamanında kendi reddedeceği bir config
üretemez) ve vault ile aynı write/read-back/replace disipliniyle atomik yazar.

**Ölçümle bulunan sınır — izin ile config ayrışabilir (2026-08-06).** Optional host izinleri
extension **kurulumuna** bağlıdır; extension silinip yeniden yüklendiğinde kaybolurlar. Korunan
site listesi ise artık host'ta kullanıcı verisi olarak kalıcıdır. Bu ikisi ayrışınca korunan bir
site için `chrome.cookies.getAll` sıfır çerez döndürür ve her restore `cookie_set_permission` ile
başarısız olup tekrar denenir. Manuel testte tam olarak bu gözlendi (`chrome.permissions.getAll`
çıktısında eklenen sitenin origin'i yoktu). **Düzeltme:** lease, tahliye ve navigasyon-gate
yollarının üçü de işleme başlamadan önce `chrome.permissions.contains` ile kapsam iznini denetler;
izin yoksa hiçbir cookie çağrısı yapılmaz, grup popup'ta "izin yok — koruma çalışmıyor" olarak
**görünür kılınır** ve tek düğmeyle izin yeniden verilebilir. Sessiz başarısızlık yerine görünür
degraded durum, §29.2 ile uyumludur.

**Dilim 1 ölçüm dayanağı (2026-08-06):** `tr.wikipedia.org` grubunda kilitli durumda
`chrome.cookies.getAll({domain:"wikipedia.org"})` **0** çerez döndürdü; unlock sonrası aynı çağrı
**14** çerez döndürdü. Eski selector modelinde grup 7 selector taşıyordu ve tahliye yalnız onları
kaldırıyordu. Bu ölçüm, kapsamdaki bütün çerezlerin — analytics/tercih çerezleri dahil — gerçekten
kasalandığını ve geri yüklendiğini doğrular. Otomatik kontroller: Rust 43/43, `clippy -D warnings`,
`cargo fmt`, extension `tsc` ve monitor testleri PASS.

**Bağlam:** Faz 5–7 boyunca korunan gruplar, elle araştırılmış cookie selector listeleriyle
(`trwikiSession`, `centralauth_Session`, …) tanımlandı ve enrollment'ı "zorunlu selector'lar belirdi
mi" sezgisi tetikledi. Bu modelin iki yapısal sorunu ölçüldü:

1. **Ölçeklenmiyor.** Her yeni site için cookie isimlerinin elle araştırılması gerekir; kullanıcı
   kendi sitesini ekleyemez. Ürünün gerçek kullanım senaryosu bunu gerektirir.
2. **Login tespiti kırılgan.** 2026-08-06 manuel oturumunda CentralAuth'un arka planda yaptığı cookie
   rotasyonu ile gerçek logout'un ayırt edilmesi güvenilir olmadı; ayrıca başarısız bir restore
   sonrası grup bir daha hiç enroll olmadı (Q23) ve bu **sessiz** bir koruma kaybı üretti.
   `waitForStableEnrollmentCookies` gibi zamanlama sezgileri (3 sn stabilite, 20 sn timeout) yarış
   koşullarına açık.

**Karar:**

- Korunacak site **kullanıcı tarafından açık bir jestle eklenir** (ayarlar ekranı veya sayfa
  üzerinden "bu siteyi korumaya al"). Ekleme anı serbesttir; kullanıcının sitenin içinde olması
  gerekmez.
- Ekleme anında **oturum durumu sorgulanmaz.** "Giriş yapılmış mı", "bu bir login mi", "bu logout
  muydu" gibi hiçbir değerlendirme yapılmaz. Sistem çerezlerin anlamını yorumlamaz.
- Grubun kapsamı, eklenen adresin **kayıtlı domaini (eTLD+1)** olarak türetilir.
- Tahliye anında kapsamdaki **tüm çerezler** kasalanır; unlock anında aynı küme geri yazılır.
  Kasa artık bir "oturum" değil, **çerez kavanozu**dur.
- `cookie_selectors[]`, `required_for_enrollment` ve site-özel `health_check` tanımları kaldırılır.
  `session.invalidate` / `restore_rejected` yolu da kaldırılır.
- Tahliye anında kapsamda hiç çerez yoksa grup `SEALED`'a geçmez ve gate açılmaz; böylece kullanıcı
  zaten çıkışlıyken gereksiz Hello istenmez.
- `SEALED` durumunda ortaya çıkan çerez için §13.2.2 kuralı uygulanır: ilgili sekme yoksa sessizce
  silinir, varsa unlock akışına girilir.

**Gerekçe:**

- Login tespitini kaldırmak, 2026-08-06 oturumunda gözlenen hata sınıfının tamamını ortadan
  kaldırır: makine artık tahmin etmez, kullanıcı söyler.
- **Ölü oturum kendiliğinden düzelir.** Kasada bayat çerezler varsa geri konur, kullanıcı
  logged-out görür, tekrar giriş yapar, sonraki kapanışta yeni çerezler kasalanır ve eskisinin
  üzerine yazılır. Bedeli boşa harcanmış bir Hello jestidir — güvenlik açığı değil, UX maliyeti.
  Faz 5'te bu durumu tespit etmek için yazılan `session.invalidate` protokolü gereksizleşir.
- Tehdit modeliyle **daha uyumlu**: infostealer profildeki tek bir session token'ını değil, bulduğu
  her şeyi alır. Tüm çerezleri kasalamak T1–T5 için gerçek kapsamı genişletir.
- Reklam/analitik çerezlerinin de kasalanması ek maliyet değildir; kasa şifrelidir ve boyut sınırı
  FCPV v1'de 4 MiB'dır.

**Alternatifler:**

- **Selector modelini sürdürmek** — reddedildi: ölçeklenmiyor, kullanıcı sitesi ekleyemiyor.
- **Otomatik "her yerdeki tüm çerezleri topla"** — reddedildi: hangi sitenin korunduğu kullanıcı
  kararı olmaktan çıkar, kapsam kontrolsüz büyür.
- **Genel bir restore doğrulama sezgisi** (restore sonrası site kendi çerezlerimizi `expired_overwrite`
  ile silerse oturumu ölü say) — teknik olarak çalışabilir ve 2026-08-06 loglarında bu imza net
  gözlendi; ancak kullanıcı bilinçli olarak sinyal tabanlı karmaşıklık istemedi ve kendiliğinden
  düzelme davranışı bunu gereksiz kılıyor. Reddedildi, kayıt olarak burada tutulur.

**Kabul edilen sınırlar:**

- **Süresi dolmuş çerezler geri yüklenmez.** Tüm-çerez modelinde kapsama kısa ömürlü çerezler de
  girer (bot-yönetimi çerezleri gibi, dakikalar mertebesinde). Kasalanma ile restore arasında
  süresi geçen bir çerez için `chrome.cookies.set` çerezi kabul edip anında düşürür ve **hata
  vermeden hiçbir şey döndürmez** (`cookie_set_no_result`); tek bir ölü çerez bütün restore'u
  düşürüyordu. 2026-08-06 manuel testinde `x.com` üzerinde gözlendi. Restore artık süresi geçmiş
  kayıtları atlar ve round-trip doğrulamasını kalan kümeye uygular. Veri kaybı değildir: tarayıcı
  o çerezi zaten atmış olurdu ve site kendisi yeniden üretir.
- **Farklı eTLD+1'deki SSO çerezleri kapsam dışıdır.** `auth.wikimedia.org` örneğinde olduğu gibi
  oturum başka bir kayıtlı domaine bağlı olabilir. İşlev bozulmaz (o çerezler tarayıcıda kalır),
  fakat koruma eksik kalır. Kullanıcı o domaini ayrıca ekleyebilir. Ürün metninde gizlenmez.
- Tercih/consent/dil çerezleri de tahliye edilir. Unlock'ta geri geldikleri için kalıcı kayıp
  yoktur; kilitliyken siteye girilirse bir kereye mahsus consent bandı görülebilir.

**Sonuç:** §17.1–17.3 tarihsel kayıt olarak korunur, yeni model §17.4'tedir. Q4 yürürlükten
kalkmıştır. Uygulama Faz 8'dir; Q24 (çalışma zamanı config digest) kapanmıştır ve her iki dilim de
uygulanmıştır (bkz. yukarıdaki uygulama durumu tablosu).

---

### ADR-021 — Windows Hello imzalama arka ucu webauthn.dll'e taşınmıştır

**Durum:** Kabul edildi ve uygulandı (2026-08-08).

**Bağlam:** Faz 5'ten beri inject capability'sini imzalamak için WinRT `KeyCredentialManager`
kullanılıyordu. Bu API'nin onay penceresi (`Credential Dialog Xaml Host`) hiçbir owner pencereyle
ilişkilendirilmeden oluşturuluyor ve bu yüzden çoğunlukla tetikleyen Chrome penceresinin
**arkasında** açılıyordu — kullanıcı PIN'i girmesi gereken pencereyi görmüyor, işlemin
donduğunu düşünüyordu.

**Araştırılan ve reddedilen düzeltmeler:**

1. **Harici pencere manipülasyonu** (`SetWindowPos`/`BringWindowToTop`/`SetForegroundWindow`,
   `HWND_TOPMOST`, `HWND_BOTTOM` ile tetikleyen pencereyi geriye itme). Ölçüldü: bu pencereye
   yönelik her çağrı — doğrudan hedef olarak da, z-sırası anchor referansı olarak da —
   `ERROR_ACCESS_DENIED` (`0x80070005`) ile tutarlı biçimde reddedildi. Bu, kimlik doğrulama
   arayüzlerini dışarıdan manipülasyona (spoofing/overlay saldırılarına) karşı koruyan kasıtlı bir
   Windows sertleştirmesi olarak değerlendirildi.
2. **Bitwarden'ın bağımsız doğrulaması** (GitHub `bitwarden/clients` issue #5287): Bitwarden
   mühendisliği aynı sorunu kendi masaüstü uygulamasında yaşadığını ve aynı duvara çarptığını
   resmen doğruladı: *"Windows' API currently lacks the ability to set a parent window for these
   kinds of requests."* Bitwarden da yalnızca "öne çekmeyi dene" tipi garantisiz bir mitigasyon
   kullanıyor. Bu, sorunun bu projeye özgü bir kod hatası değil, dokümante edilmemiş, genel bir
   Windows platform boşluğu olduğunu doğruladı.
3. **`KeyCredentialManagerShowUIOperation`** (Win32, `keycredmgr.h`, gerçek `hWndOwner` alıyor) —
   incelendi ve reddedildi: yalnızca `Provisioning`/`PinChange`/`PinReset` işlemlerini destekliyor,
   imzalama (`RequestSignAsync`) kapsamı dışında.

**Karar:** İmzalama arka ucu `windows::Win32::Networking::WindowsWebServices` (`webauthn.dll`,
`WebAuthNAuthenticatorMakeCredential`/`GetAssertion`) API'sine taşındı. Bu API gerçek bir `hWnd`
parametresi alıyor ve OS bunu onaylıyor — tarayıcıların WebAuthn/passkey akışlarında kullandığı
aynı, düzgün pencere-sahipliği destekleyen yol. Spike ile doğrulandı (`poc/webauthn-probe/`):
sentetik (gerçek olmayan) RP id/origin (`fursoy-cookie-protector.local`) sorunsuz kabul edildi ve
pencere gerçekten sahipli/önde açıldı — hiçbir harici z-sırası hack'i gerekmedi.

**Uygulama detayları:**

- Tek, sabit bir platform authenticator credential'ı (`fursoy-cookie-protector.local` RP id'si
  altında) oluşturulup credential ID + COSE EC2 (ES256/P-256) public key koordinatları
  `%LOCALAPPDATA%\FursoyCookieProtector\hello-credential.json`'a (hex-encoded, secret değil)
  kaydediliyor.
- İmzalama `WebAuthNAuthenticatorGetAssertion` ile; imzalanan mesaj WebAuthn spesifikasyonunun
  kendisi (`authenticatorData || SHA-256(clientDataJSON)`), tek başına `canonical_bytes()` değil.
  Bu yüzden `SignedCapability` artık `authenticator_data` alanı da taşıyor (yalnızca bellekte,
  capability ledger'a hiç yazılmıyor — bkz. [§13](#13-lease-modeli)/[§10.2](#102-lease-kaydı-diskte-plaintext-metadata)).
  Doğrulama, DER-kodlu ECDSA imzasını ham (r‖s) forma çevirip Windows CNG/BCrypt
  (`BCryptVerifySignature`, ECDSA P-256) ile kontrol ediyor.
- `clientDataJSON`'daki `challenge` alanı, imzalanan `CapabilityPayload`'ın canonical byte'larının
  hex'i; bu, `sign`/`verify_signature` arasında ek bir alan saklamadan deterministik olarak
  yeniden üretilebiliyor.
- COSE/`authenticatorData` ve DER-ECDSA-imza çözücüleri (`webauthn_codec.rs`) genel amaçlı değil;
  yalnızca Windows'un bu credential için ürettiği sabit şekli kabul ediyor, başka her şeyi
  reddediyor.
- `prompt_raiser.rs` (harici pencere-yükseltme iş-around'u) tamamen kaldırıldı — artık gerekmiyor.

**Ölçümle bulunan ve düzeltilen iki hata (göç sırasında):**

- Allow-list `WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS`'ın yanlış/eski alanına
  (`pAllowCredentialList` yerine gömülü `CredentialList`) yazılıyordu; `Microsoft-Windows-WebAuthN/Operational`
  event log'unda `AllowCredentialCount: 0` ile doğrulandı. Ayrıca `dwVersion` `VERSION_1`de
  bırakıldığı için OS yeni alanları zaten görmüyordu. İkisi de düzeltildi, log artık
  `AllowCredentialCount: 1` gösteriyor.
- `cookie_roundtrip_failed` sağlık kontrolü (ADR-020'nin bir parçası, bu ADR'nin konusu değil ama
  aynı oturumda bulundu): "tam eşitlik" kontrolü, sitenin restore penceresinde kendi eklediği
  çerezleri (örn. youtube.com'un `GPS`'i) hata sanıyordu. Alt-küme kontrolüne çevrildi.

**Kabul edilen sınırlar:**

- **`hello_cache_ms` fiilen etkisiz.** Eski `KeyCredentialManager` handle'ı kısa süre içinde
  tekrar kullanıldığında Windows'un sessizce tekrar sormaması — dokümante edilmemiş ama Deney 1'de
  ölçülmüş bir davranıştı (bkz. [§30](#30-son-durum) Deney 1 `lock-probe` ölçümleri). `webauthn.dll`'in
  `GetAssertion`'ı durum tutmuyor (stateless); art arda çağrılar arasında ölçülebilir bir fark
  gözlenmedi (~3.1s → ~3.9s, aynı büyüklük mertebesinde). Bunun sonucu: Dengeli/Kullanışlı
  politika seviyeleri artık **fiilen Kritik gibi davranıyor** — her yeniden girişte PIN isteniyor.
  `hello_cache_ms` alanı [§14](#14-policy-seviyeleri)'te ve `config.rs`'te durmaktadır (gelecekte
  uygulama-seviyesi bir önbellekleme katmanı düşünülürse); şu an hiçbir kod yolu bunu okuyup
  davranışı değiştirmiyor. Kullanıcı bu ödünleşimi bilerek kabul etti (2026-08-08).
- **~1 saniyelik NGC/PIN-kutusu açılış gecikmesi kalıcı ve düzeltilemez olarak kabul edildi.**
  Windows' `Microsoft-Windows-WebAuthN/Operational` event log'unda milisaniye hassasiyetinde
  ölçüldü ve Chrome'un GitHub/Google Şifreler üzerindeki **kendi** `webauthn.dll` çağrısıyla
  birebir karşılaştırıldı: ikisi de ~1.06 saniye. Bu, çağıran uygulamadan bağımsız, evrensel bir
  Windows NGC-etkinleştirme maliyetidir. Kod imzalama (self-signed sertifikayla test edildi) bu
  gecikmeyi etkilemedi; teori kuruldu, test edildi, çürütüldü ve ilgili geçici tanı kodu/sertifika
  temizlendi.
- Tek, sabit RP id/user kullanılıyor (`fursoy-cookie-protector.local` / gerçek bir web origin'i
  değil) — bu, bir tarayıcı/gerçek Relying Party ile hiçbir zaman etkileşmediği için zararsızdır;
  yalnızca bu makinedeki tek yerel kullanıcıyı temsil eder.

**Alternatifler:**

- **Mevcut durumu korumak** (harici pencere-yükseltme iş-around'u ile devam) — reddedildi:
  ölçülen yan etkisi var (`HWND_BOTTOM` tetikleyen pencereyi mutlak dibe atıyor, arkadaki alakasız
  başka bir uygulama birkaç saniyeliğine öne çıkabiliyor) ve OS korumasının gelecekte
  sıkılaştırılması riskine açık.
- **Kod imzalama sertifikası satın almak** — spekülatif olarak denendi (self-signed, güven
  deposuna eklenmeden), gecikmeyi etkilemediği ölçüldü, reddedildi.
- **Uygulama-seviyesi Hello-sonucu önbellekleme** (imza her seferinde tazelense de, kullanıcıya
  görünen jest sıklığını app tarafında sınırlamak) — bu ADR kapsamında **yapılmadı**; olası bir
  gelecek işi olarak `hello_cache_ms` alanı bilerek dormant bırakıldı (bkz. yukarıdaki "Kabul
  edilen sınırlar").
