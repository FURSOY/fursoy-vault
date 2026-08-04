# Faz 6 Kabul Testi — Çoklu Grup, Policy ve Reconciliation İzolasyonu

**Başlangıç tarihi:** 2026-08-04  
**Tamamlanma tarihi:** 2026-08-04  
**Sürüm:** `0.2.0`  
**Durum:** ✅ MANUEL KABUL TAMAMLANDI — 12/12 kontrol PASS

## Amaç

Faz 5/5.1'de tek Wikipedia grubuyla doğrulanan zinciri iki bağımsız account group'a genişletmek;
policy sürelerinin, unlock/eviction işlemlerinin ve reconciliation hata alanlarının gerçekten grup
sınırında kaldığını ölçmek.

## Gruplar

| Grup | Policy | Selector kapsamı | Test türü |
|---|---|---|---|
| Wikipedia | Dengeli | 5 zorunlu yerel + CentralAuth selector | Düşük-riskli gerçek hesap; migrate edilmiş sealed vault |
| Controlled Session App | Kritik | `FCP-mvp-session` | Dummy hesap; izolasyon/ret/logout kontrolleri |

## Bağlayıcı policy değerleri

| Policy | Hello handle yeniden kullanım penceresi | Lease | Idle | Last-tab |
|---|---:|---:|---:|---:|
| Kritik | Yok | 5 dk | 1 dk | Anında |
| Dengeli | 10 dk | 10 dk | 5 dk | 2 dk grace |
| Kullanışlı | Lock'a kadar, 30 dk üst sınır | 30 dk | 15 dk | 5 dk grace |
| İzleme | Yok | Yok | Yok | Yalnız audit |

DEK hiçbir policy'de cache'lenmez. Aynı Hello handle'ı kullanılsa bile her inject yeni, tek
kullanımlık sequence/nonce capability tüketir. Windows lock grup jest handle'larını temizler.

## Otomatik doğrulamalar

- Config şeması, selector/grup sınırları ve policy değerleri
- Native Messaging v2 nonce/sequence ve config digest doğrulaması
- İki grup için ayrı runtime/vault/lease/capability dosyaları
- Bir grubun invalidation işleminin diğer vault/state'i değiştirmemesi
- Capability replay ve consume-before-unwrap kuralları
- Audit şemasında cookie adı/değeri bulunmaması
- Config mismatch'in lease vermeden global fail-closed olması
- Bir grubun pending/hata durumunun diğer grubun runtime'ını değiştirmemesi

## Manuel kabul matrisi

| # | Kontrol | Beklenen | Gerçekleşen | Sonuç |
|---|---|---|---|---|
| 1 | Wikipedia vault migration + unlock | Eski sealed vault doğru UUID yoluna taşınır; gate ve tek Hello sonrası ilk yükleme authenticated | Migrate edilmiş vault gate ile açıldı; onay sonrası ilk yükleme authenticated | PASS |
| 2 | Controlled app enrollment | Dummy login sessiz enrollment üretir | Hello çıkmadan enrollment tamamlandı | PASS |
| 3 | Eşzamanlı iki grup | İki grup bağımsız `LEASED` ve authenticated kalır | Wikipedia ve controlled app aynı anda authenticated | PASS |
| 4 | Kritik last-tab | Controlled app son sekmede anında/sessiz sealed olur; Wikipedia etkilenmez | Anında tahliye oldu, Wikipedia açık kaldı | PASS |
| 5 | Kritik yeniden açılış | Yalnız controlled app gate'lenir; taze Hello sonrası ilk yükleme authenticated | Gate + düğme + taze Hello ile F5'siz authenticated | PASS |
| 6 | Hello ret izolasyonu ve retry | Hedef grup sealed kalır; düğme tekrar kullanılabilir; diğer grup etkilenmez | Ret gate'te kaldı, Wikipedia etkilenmedi, ikinci deneme başarılı | PASS |
| 7 | Dengeli last-tab grace | 2 dk içinde yeniden açılış gate üretmez | Grace içinde gate/Hello çıkmadı | PASS |
| 8 | Dengeli grace sonrası eviction | 2 dk sonunda Wikipedia sealed olur ve sonraki açılış gate'lenir | 2 dk sonrasında gate çalıştı | PASS |
| 9 | Dengeli Hello handle yolu | 10 dk içinde host cached-handle yolunu seçebilir; OS'nin promptsuz davranışı garanti değildir | Audit `hello_cached` kaydetti; Windows yine taze Hello UI gösterdi | PASS — gözlem aşağıda |
| 10 | Policy idle ayrımı | ~1 dk Kritik tahliye; Dengeli 5 dk dolana kadar leased, sonra tahliye | ~70 sn'de yalnız controlled app; 5+ dk'da Wikipedia sessiz tahliye oldu | PASS |
| 11 | External logout izolasyonu | Yalnız controlled vault `UNINITIALIZED`; gereksiz Hello yok | Server-side logout yalnız controlled grubu invalid etti; Wikipedia etkilenmedi | PASS |
| 12 | Extension reload reconciliation | Leased Wikipedia ve sealed controlled state bağımsız korunur; gereksiz Hello yok | Reload sonrası Wikipedia açık, controlled sealed kaldı; sonraki gate yalnız controlled için çalıştı | PASS |

## Faz D — Hello handle cache gözlemi

Bu gözlem last-tab eviction'ın cache'i temizlemesinden kaynaklanmıyor. Extension yalnız Windows
`locked` durumunda `auth.cache.clear` gönderiyor; host da eviction sırasında yalnız
`reason=locked` için handle'ı temizliyor. Last-tab yolu bu alanlara dokunmuyor.

Redakte audit kaydı ayrımı kesinleştirdi:

- İlk Wikipedia inject: `timestamp_unix_ms=1785871074817`, `detail_code=hello_fresh`
- Last-tab eviction: `timestamp_unix_ms=1785871499360`–`1785871499670`, başarılı
- Sonraki inject: `timestamp_unix_ms=1785871577911`, `detail_code=hello_cached`

İkinci inject ilk yetkilendirmeden yaklaşık 8 dk 23 sn sonra, uygulamanın 10 dakikalık penceresi
içinde gerçekleşti ve host gerçekten `sign_cached`/aynı `KeyCredential` handle yolunu seçti. Buna
rağmen Windows Hello UI yeniden gösterildi. Dolayısıyla implementasyonun handle seçimi doğrudur;
fakat uygulama süresi Windows'un kendi credential/UI cache süresini zorlamaz. Deney 1 yalnız kısa
aralıklı aynı-handle tekrarının promptsuz olabildiğini ölçmüştü, 10 dakikalık OS davranışını
ölçmemişti. Bu bir grup izolasyonu veya güvenlik hatası değildir; daha fazla prompt üreten fail-safe
bir UX sınırıdır. Gerçek promptsuz pencere ayrıca ölçülmelidir.

## Açık sınırlar

- Yalnız normal Chrome profili ve ölçülmüş `storeId=0` desteklenir; Q8 kapanmaz.
- Extension kaldırılmışken bildirim kanalı yoktur; Q17/Q15 açık kalır.
- Medya oynatma sistem idle sinyalini geçersiz kılmaz; Q20 açık kalır.
- 10 dakikalık Dengeli değer uygulamanın cached-handle yeniden kullanım üst sınırıdır; Windows Hello
  UI'sının bu süre boyunca bastırılacağı garantisi değildir.
- `webNavigation` gate blocking değildir; ağ seviyesinde pre-request garanti verilmez.

## Sonuç

Faz 6 manuel kabulü **12/12 PASS** ile tamamlandı. İki account group aynı anda bağımsız çalıştı;
Kritik ve Dengeli last-tab/idle davranışları ayrıştı; ret, external logout ve extension reload
reconciliation işlemleri yalnız ilgili grubu etkiledi. Q4, Q12 ve Q19 kapandı. Hello UI cache
süresinin Windows tarafından belirlenmesi blocker değildir ve ayrı bir UX ölçümü olarak açık kalır.
