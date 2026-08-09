// Message table for the "tr" locale. Onboarding is the first surface migrated onto i18n
// (2026-08-09); the rest of the UI (popup/options/unlock) is still hardcoded Turkish and will be
// migrated incrementally as those pages are redesigned, not as a separate mechanical pass.
export const tr: Record<string, string> = {
  "onboarding.welcome.title": "FURSOY Vault'a hoş geldin",
  "onboarding.welcome.body":
    "Önemli hesaplarının çerezlerini, kullanmadığın zaman TPM korumalı bir kasada saklar. Bir " +
    "siteyi açtığında Windows Hello ile onaylarsın, çerez geri gelir; sekmeyi kapattığında ya da " +
    "bilgisayarını kilitlediğinde otomatik olarak kasaya geri konur.",
  "onboarding.welcome.next": "Devam et",

  "onboarding.install.title": "Companion uygulamayı kur",
  "onboarding.install.body":
    "FURSOY Vault'un çalışması için bilgisayarına küçük bir yardımcı program kurman gerekiyor. " +
    "Aşağıdaki düğmeyle indir, indirilen dosyayı aç ve install.bat'a çift tıkla.",
  "onboarding.install.downloadButton": "İndir",
  "onboarding.install.checkButton": "Bağlantıyı kontrol et",
  "onboarding.install.waiting": "Companion uygulama bekleniyor…",
  "onboarding.install.connected": "Bağlandı.",
  "onboarding.install.notConnected":
    "Henüz bağlanamadı. install.bat'ı çalıştırdığından emin ol, sonra bu sekmeyi yenileyip tekrar dene.",
  "onboarding.install.skip": "Şimdilik atla",

  "onboarding.addsite.title": "İlk siteni koru",
  "onboarding.addsite.body": "Hangi sitenin oturumunu korumak istersin? (E-posta, banka, sosyal medya gibi.)",
  "onboarding.addsite.scopeLabel": "Alan adı",
  "onboarding.addsite.scopePlaceholder": "ornek.com",
  "onboarding.addsite.policyLabel": "Koruma düzeyi",
  "onboarding.addsite.submit": "Korumaya al",
  "onboarding.addsite.skip": "Şimdilik atla",
  "onboarding.addsite.error.empty": "Alan adı boş olamaz.",
  "onboarding.addsite.error.overlap": "Bu alan adı zaten korunan bir siteyle çakışıyor.",
  "onboarding.addsite.error.permission": "Chrome izni verilmedi; site korumaya alınmadı.",
  "onboarding.addsite.error.generic": "Koruma eklenemedi.",

  "onboarding.done.title": "Hazırsın",
  "onboarding.done.body":
    "FURSOY Vault artık çalışıyor. Araç çubuğundaki simgeden istediğin zaman yeni site " +
    "ekleyebilir ya da ayarları değiştirebilirsin.",
  "onboarding.done.finish": "Bitir",

  "policy.critical": "Kritik — 5 dk kira · 1 dk boşta · anında tahliye",
  "policy.balanced": "Dengeli — 10 dk kira · 5 dk boşta · 2 dk bekleme",
  "policy.convenient": "Kullanışlı — 30 dk kira · 15 dk boşta · 5 dk bekleme",
};
