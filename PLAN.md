# FURSOY Cookie Protector — Proje Planı ve Teknik Karar Kaydı

> Bu belge projenin **ana hafızasıdır**. Projeyi hiç görmemiş bir geliştirici bu belgeyi
> okuyarak doğru bağlamla devam edebilmelidir. Konuşma geçmişine bağımlılık kabul edilmez.
>
> Belge **yaşayan bir dokümandır**. Her önemli çalışma sonunda [Son Durum](#30-son-durum),
> [Sonraki Kesin Adım](#31-sonraki-kesin-adım) ve [Karar Günlüğü](#32-karar-günlüğü)
> bölümleri gözden geçirilir. Günlük tarzı uzun kayıt tutulmaz; belge güncel ve okunabilir kalır.

**Son güncelleme:** 2026-08-03
**Durum:** Tasarım tamamlandı. **Faz 1 / Deney 1 TAMAMLANDI; Go/No-Go kriteri A karşılandı.**
Faz 2 / Deney 2 kullanıcı onayını bekliyor.

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

### 8.3 Üç katmanlı yaşam süresi ayrımı (merkezi tasarım kararı)

Bu projenin en önemli tasarım kararı, birbirine karıştırılan üç şeyin **ayrılmasıdır**:

| Katman | Ne | Ömrü |
|--------|-----|------|
| 1 | **Kullanıcı jesti** (Windows Hello onayı) | Policy'ye göre cache'lenebilir |
| 2 | **Grup DEK'inin TPM ile unwrap edilmesi** | **Cache'lenmez** — tek vault transaction |
| 3 | **Cookie'nin browser store'da bulunması** | Lease ile sınırlı (dakikalar) |

Sonuç: kullanıcı günde 1–3 Hello görür, ancak cookie günün küçük bir yüzdesinde açıktadır ve
host belleğinde bekleyen uzun ömürlü bir anahtar yoktur.

Güvenlik/UX takası **1. katmanda** yaşanır; 2. ve 3. katmanda taviz verilmez.

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
Host: DEK unwrap → yeni nonce ile şifrele → atomik yaz → doğrula → DEK zeroize
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
- Login ve session durumu değişikliklerini gözlemlemek
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

> **Wrapped DEK'in yeri bilinçli olarak yukarıda belirtilmemiştir.** Manifest ile grup dosyası
> arasında **çift doğruluk kaynağı oluşturulması yasaktır**; kesin yerleşim Deney 1 sonucunda
> belirlenir (bkz. [§12.0](#120-tek-doğruluk-kaynağı-ilkesi-bağlayıcı)).

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

> **⚠ PROVISIONAL — VALIDATION PENDING.**
> Bu bölüm bir **taslaktır** ve henüz doğrulanmamıştır. Kesin yerleşim, alan boyutları ve
> özellikle **wrapped DEK'in nerede saklanacağı**, [Deney 1](#19-tpm--hello-deney-planı-deney-1)
> sonucuna bağlıdır: KEK'in RSA mı ECC mi olacağı, wrap çıktısının boyutu ve wrap/unwrap
> semantiği ([Q2](#24-açık-teknik-sorular)) belirlenmeden format dondurulmaz.
> Implementasyona bu bölüm doğrulanmadan başlanmaz.

Her hesap grubu ayrı dosyada tutulur. Ayrı dosya seçimi atomik yazmayı kolaylaştırır ve
bozulmanın etki alanını tek grupla sınırlar.

### 12.0 Tek doğruluk kaynağı ilkesi (bağlayıcı)

**Wrapped DEK yalnızca TEK bir yerde saklanır.** Aynı sarmalanmış anahtarın hem
`manifest.json` içinde hem grup dosyasında bulunması yasaktır: iki kopya birbirinden ayrışır,
rotasyon ve kurtarma mantığını belirsizleştirir ve hangisinin geçerli olduğu sorusunu doğurur.

İki aday yerleşim vardır ve **seçim Deney 1'e bırakılmıştır**:

| Aday | Wrapped DEK nerede | Artı | Eksi |
|------|--------------------|------|------|
| **A — grup dosyası içinde** | `<group_id>.fcpv` başlığında | Grup dosyası kendi kendine yeter; atomik yazma tek dosyada biter | Manifest yalnızca indeks olur; KEK rotasyonu tüm grup dosyalarını yeniden yazmayı gerektirir |
| **B — manifest içinde** | `manifest.json` | KEK rotasyonu tek dosyada biter | Manifest ile grup dosyası arasında çapraz tutarlılık ve iki-dosyalı atomiklik gerekir |

Hangi aday seçilirse seçilsin, diğerinde wrapped DEK **bulunmaz**. Aşağıdaki §12.1 şeması
**Aday A** varsayımıyla yazılmıştır ve karar değiştiğinde güncellenecektir.

### 12.1 Kayıt düzeni (taslak — v0 şeması, Aday A varsayımı)

```text
offset  alan                 boyut     not
------  -------------------  --------  ----------------------------------------
0       magic                4         "FCPV"
4       format_version       2         u16, little-endian
6       group_id             16        UUID
22      alg_id               2         u16 (1 = AES-256-GCM)
24      kek_key_id           16        KEK tanımlayıcı (rotasyon için)
40      nonce                12        benzersiz, asla tekrar kullanılmaz
52      wrapped_dek_len      2         u16      ← yalnızca Aday A'da bulunur
54      wrapped_dek          değişken  TPM KEK ile sarmalanmış 32-byte DEK  ← Aday A
..      ciphertext_len       4         u32
..      ciphertext           değişken  AEAD çıktısı
..      tag                  16        GCM authentication tag
```

**AAD (Additional Authenticated Data)** = `magic || format_version || group_id || alg_id ||
kek_key_id || nonce || wrapped_dek`.
Böylece başlık alanları da kimlik doğrulamasına dahil olur; header üzerinde oynama tespit edilir.

> Aday B seçilirse `wrapped_dek_len` ve `wrapped_dek` alanları kayıttan çıkarılır ve AAD'ye
> `kek_key_id` ile birlikte manifest kayıt sürümü dahil edilir.

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

### 13.2 Tahliye tetikleyicileri

| Tetikleyici | Açıklama |
|-------------|----------|
| `last_tab_closed` | Hesap grubuna ait son sekme kapandı |
| `idle` | Kullanıcı etkileşimi policy eşiğini aştı |
| `lock` | Windows oturumu kilitlendi → **best-effort** anında tahliye (bkz. §13.2.1) |
| `expiry` | Lease süresi doldu |
| `manual` | Kullanıcı talebi ("şimdi kilitle") |
| `host_disconnect` | Extension host bağlantısını kaybetti → extension kendi başına tahliye eder |

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
reconciliation'ın tetikleyicisi "host başlangıcı" değil, **host ile extension arasındaki
bağlantının kurulması**dır (`handshake`).

```text
 1. Host başlatılır (extension connectNative ile bağlanır)
 2. Handshake tamamlanır
 3. Host: vault manifest'i ve lease kayıtlarını okur
       → açık/stale lease var mı?
 4. Host → Extension: reconcile.request(beklenen açık cookie referansları)
 5. Extension: chrome.cookies ile browser store snapshot'ı alır
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
- Reconciliation, host başlangıcında **ve** her extension yeniden bağlantısında çalışır.
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

### 16.3 Mesaj tipleri (taslak)

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
| `evict.confirmed` | H→E | Vault yazımı doğrulandı, silmeye izin |
| `evict.result` | E→H | Silme sonucu |
| `reconcile.request` | H→E | Reconciliation başlat; beklenen açık cookie referansları |
| `reconcile.report` | H→E | Reconciliation sonucu ve grup durumları |
| `heartbeat` | E→H | Canlılık + SW ömrü |
| `audit.event` | çift yön | Redakte olay kaydı |

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

### 17.1 Yaklaşım

Grupları **yalnızca statik domain listesiyle** tanımlamak yetmez. Kimlik sağlayıcı
yönlendirmeleri, iframe'ler ve partitioned cookie'ler top-level site bağlamına göre değişir.

Bu nedenle profiller iki kaynaktan üretilir:

1. **Elle küratörlük** — ilk 15–20 yüksek değerli hedef için manuel doğrulama
2. **Ampirik türetme** — login/logout sırasında hangi cookie'lerin gerçekten değiştiğini
   gözlemleyip grubu buradan çıkarma

### 17.2 Profil yaşam döngüsü

- Her profilin `compatibility_version` alanı vardır.
- Profil doğrulanmadan **kritik** seviyeye alınamaz.
- Doğrulanmamış profiller varsayılan olarak **izleme** seviyesindedir.
- Health check başarısız olan profil otomatik olarak izleme seviyesine düşürülür.

### 17.3 Öncelikli hedef listesi (doğrulanmayı bekliyor)

Kendi kontrolümüzdeki test uygulaması → düşük riskli test hesabı → sonra gerçek hedefler.
Google, Steam, banka ve ana e-posta **erken testlerde kullanılmaz**.

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
| `domain` | var | opsiyonel | **`hostOnly` ile birlikte ele alınır** |
| `hostOnly` | var | **yok** | Türetilir: `domain` verilirse `false`, verilmezse `true` |
| `path` | var | var | — |
| `secure` | var | var | — |
| `httpOnly` | var | var | Uzantı izinliyse okunur ve yazılır |
| `sameSite` | `no_restriction`/`lax`/`strict`/`unspecified` | aynı | `no_restriction` için `secure=true` zorunlu |
| `session` | var | **yok** | `expirationDate` verilmezse session cookie olur |
| `expirationDate` | var (session değilse) | opsiyonel | — |
| `partitionKey` | var (CHIPS) | var | Güncel Chrome'da round-trip eder; doğrulanacak |
| `storeId` | var | var | Çoklu profil / incognito ayrımı |
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
operation          (inject | evict)
expiry
monotonic_sequence
nonce
```

Bu modelde Hello doğrudan DEK çözmez; jest **kısa ömürlü bir yetkilendirme** üretir, DEK unwrap
işlemini ayrı bir TPM-backed CNG anahtarı yapar. Capability'nin yukarıdaki alanlara bağlanması
zorunludur; aksi halde jest cache'i açıkken malware herhangi bir operasyonu tetikleyebilir.

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

### Deney 3 — Disposable profile uçtan uca

- Ayrı Chrome profili
- Test hesabı
- Önce **kendi kontrolümüzdeki test uygulaması**, sonra düşük riskli site
- Adımlar: cookie snapshot → eviction → oturumun kapandığını doğrula → restore →
  oturumun geri geldiğini doğrula
- Rotation ve background request gözlemi
- **Aynı oturum üzerinde** tekrar eden evict/restore döngüleri

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

### 22.3 Deney 3 (uçtan uca) — devam kriteri

- Restore başarı oranı ≥ %99
- Yanlış logout ≤ %0,1
- Kalıcı profil bozulması = 0
- Restore sonrası hesap güvenlik alarmı oluşmaması

Karşılanmazsa: ilgili site profili **izleme** seviyesine düşürülür, mimari değişmez.
Birden çok hedefte sistematik başarısızlık varsa cookie-only yaklaşımı yeniden değerlendirilir.

### 22.4 Deney 4 (duty cycle) — devam kriteri

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
| **Q4** | Hesap grubu domain kümesi statik mi tanımlanacak, ampirik mi türetilecek? İkisinin karışımı nasıl yönetilir? | Profil modeli | Deney 3 |
| **Q5** | MV3 service worker'ı boşta sonlandırılıyor. Lease zamanlayıcısını ne zorlayacak? `chrome.alarms` granülaritesi yeterli mi? Açık native messaging port'u SW ömrünü ne kadar uzatıyor? | **Tahliye hassasiyeti — kritik** | Küçük extension deneyi (Deney 2'ye eklenebilir) |
| **Q6** | Host, Chrome tarafından başlatılan bir process olarak Windows lock bildirimini nasıl alacak? (`WTSRegisterSessionNotification` pencere handle'ı ister; gizli mesaj penceresi mi kurulacak?) Host kalıcı olmadığı için bu bildirim yalnızca port açıkken anlamlıdır (§9.2.1). | Lock tahliyesinin best-effort yolu | Deney 1'e ek |
| **Q7** | Cookie tahliye edildikten sonra hâlâ çalışan bir service worker veya background fetch cookie'yi yeniden oluşturuyor mu? | Duty cycle doğruluğu | Deney 3/4 metriği |
| **Q8** | Çoklu Chrome profili ve incognito `storeId` davranışı nasıl ele alınacak? | Kapsam | Deney 2 |
| **Q9** | Extension ID pinlenmesi: unpacked geliştirme uzantısı rastgele ID alır. Manifest `key` alanı ile sabitlenecek mi? | Kurulum / native host manifest | Deney 2 öncesi karar |
| **Q10** | ✅ Kapandı — Rust binary için `Cargo.lock` repoda tutulacak. | Repo hijyeni | §8.2.1 |
| **Q11** | Mevcut lisans GPL-3.0 (repoda hazır). Bu bilinçli bir tercih mi, teyit edilmeli. | Dağıtım | Kullanıcı teyidi |
| **Q12** | Audit log'da cookie **isimleri** bile hash'lenecekse, hash tuzu nerede saklanacak? (Tuz kasada olursa log kasasız okunamaz) | Log tasarımı | Vault implementasyonu öncesi |
| **Q13** | ✅ Kapandı — Windows Hello kayıtlıdır; Yol A bu mekanizmayı kullanmıyor ve parola tabanlı CNG strong-key protection diyaloğu gösteriyor. Yol C prompt türü yalnızca PIN kayıtlı test ortamında PIN olarak ölçüldü; biyometrik cihaz test edilmedi. | Policy | Deney 1 ikinci tur + Yol C |
| **Q14** | ✅ Kapandı — Platform Crypto Provider hardware-only ve TPM sürümü `2.0` bildirdi. `Get-Tpm` yönetici istediği için doğrulama doğrudan CNG provider özellikleriyle yapıldı. | Deney 1'in ön koşulu | Deney 1 `status` ölçümü |
| **Q15** | **Kalıcı bir Windows user agent eklenecek mi?** Standart NMH host'u kalıcı değildir (§9.2.1); Chrome kapalıyken lease expiry takibi, lock tahliyesi ve reconciliation tetikleme yapılamaz. Kalıcı agent bunu çözer ancak yeni saldırı yüzeyi, autostart ve güncelleme yükü getirir. | **Mimari — lease zorlama modeli** | [ADR-013](#adr-013--kalıcı-windows-user-agent-açık-karar); Deney 4 duty cycle sonuçları karar girdisi olacak |
| **Q16** | Wrapped DEK grup dosyasında mı (Aday A) manifest'te mi (Aday B) saklanacak? | Vault formatının dondurulması | Deney 1 — KEK tipi ve wrap çıktısı boyutu belirlendiğinde ([§12.0](#120-tek-doğruluk-kaynağı-ilkesi-bağlayıcı)) |
| **Q17** | Extension kaldırıldığında veya devre dışı bırakıldığında kullanıcı `degraded` durumdan nasıl haberdar edilecek? Host kalıcı değil ve UI'ı yok; extension da yoksa bildirim kanalı kalmıyor. | Kullanıcının yanlış güven hissine kapılmaması | Q15 kararına bağlı; kalıcı agent varsa çözülür |

---

## 25. Yol Haritası

| Faz | İçerik | Çıktı | Kapı |
|-----|--------|-------|------|
| **Faz 0** | Plan ve karar kaydı | `PLAN.md` | ✅ Tamamlandı |
| **Faz 1** | Deney 1 — TPM/Hello probe (Rust) | `poc/tpm-probe/`, `docs/experiments/exp-01-*.md` | ✅ Tamamlandı — §22.1 kriter A karşılandı |
| **Faz 2** | Deney 2 — Cookie attribute probe (extension) + Q5, Q8, Q9 | `poc/cookie-probe/`, exp-02 raporu | ⏳ Kullanıcı onayı bekleniyor; round-trip uyumluluğu |
| **Faz 3** | Deney 3 — Disposable profile uçtan uca | exp-03 raporu | §22.3 kriterleri |
| **Faz 4** | Deney 4 — Duty cycle ölçümü | exp-04 raporu | §22.4 kriteri |
| **Faz 5** | Tek grup, uçtan uca MVP (vault + host + extension) | Çalışan dikey dilim | Manuel kabul |
| **Faz 6** | Çoklu grup, policy seviyeleri, reconciliation sertleştirme | v0.1 | — |
| **Faz 7** | Watcher / monitoring katmanı | v0.2 | — |
| **Faz 8** | Edge / Brave desteği | v0.3 | — |

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
- Google, Steam, banka ve ana e-posta hesapları erken testlerde kullanılmaz.
- Anti-abuse tetikleyecek yoğun login/logout döngülerinden kaçınılır.
- Testler **aynı session üzerinde evict/restore** şeklinde yapılır.
- Gerçek cookie değerleri test raporlarına yazılmaz.
- Test sonuçları tekrarlanabilir olmalıdır (ortam bilgisi raporda).

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
- Cookie isimleri dahi gerektiğinde hash veya redaction ile tutulur ([Q12](#24-açık-teknik-sorular)).
- Debug çıktıları production build'de kapalıdır.

---

## 30. Son Durum

**Tarih:** 2026-08-03

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
| Çalışma alanı | **Temiz değil** — `PLAN.md` untracked (`?? PLAN.md`) | Ölçüldü |
| Takip edilen dosyalar | `.gitattributes`, `LICENSE` (GPL-3.0) — `PLAN.md` **dahil değil** | Ölçüldü |
| TPM durumu | **TPM 2.0 doğrulandı** — Platform Crypto Provider hardware-only; PCP TPM version `0x00020000` | Deney 1 `status` |
| Windows Hello kayıt durumu | **Kayıtlı, yalnızca PIN** — Yol C prompt türü PIN; biyometrik donanım yok | Q13 / Deney 1 |

### Kod durumu

- Deney 1 probe kodu yazıldı: provider doğrulama, kalıcı RSA anahtar oluşturma/inceleme/silme,
  RSA-OAEP-SHA256 DEK wrap/unwrap, süre ölçümü ve secret buffer zeroize.
- `windows 0.62.2` ve `zeroize 1.9.0` bağımlılıkları gerekçeleriyle eklendi; `Cargo.lock` tutuluyor.
- Platform ve Passport provider yolları ayrıldı; hardware/software ayrımı provider seviyesinde
  doğrulanıyor. Platform hardware-only olmalıdır; Passport dual-capability (`0x3`) bildirebilir.
- **Commit atılmadı, push yapılmadı, branch oluşturulmadı.**

### Bilinen regresyonlar

Yok.

### Güvenlik etkileri

Yol A için software KSP fallback reddi, TPM-backed/non-exportable anahtar, wrap/unwrap ve handle başına
jest doğrulandı. Ancak CNG parola kutusu keylogger'a açık yeni bir sır ve kabul edilmesi zor UX
oluşturur. Ürün jesti Yol C Hello capability'den alacak; CNG UI policy kaldırılıp anahtar yalnızca
fiili unwrap için kullanılacak. Taze-handle kilit ölçümü kilidin davranışı değiştirmediğini kanıtladı;
Go/No-Go kriteri A karşılandı.

---

## 31. Sonraki Kesin Adım

**Faz 2 / Deney 2 — Cookie attribute probe.**

Deney 1 tamamlandı ve §22.1 Go/No-Go kriteri A karşılandı. Sıradaki adım, gerçek cookie silinmeden
önce aynı attribute'larla probe cookie yazıp geri okuyacak extension deneyidir.

- Çalışma alanı: `poc/cookie-probe/`
- Rapor: `docs/experiments/exp-02-cookie-attributes.md`
- Kapsam: cookie attribute round-trip uyumluluğu ile Q5, Q8 ve Q9 ölçümleri

**Deney 2 başlatılmadı. Kullanıcının ayrıca açık onayı olmadan başlanmayacaktır.**

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
