# FileFlow (Türkçe)

**Masaüstünüz için açıklanabilir dosya düzenleme.**

FileFlow, seçtiğiniz klasörleri izler ve dosyaları **açık, açıklanabilir
kurallara** göre düzenler. "Akıllı" dosya düzenleyiciler genellikle
korkutucudur: tahmin edilemez şekilde sınıflandırırlar, sessizce çalışırlar
ve diskinize kendi bahçeleriymiş gibi davranırlar. FileFlow bunun tam tersini
yapar: her işlem önizlenebilir, kuru çalıştırılabilir, kayıt altına alınır ve
geri alınabilir.

> FileFlow, kullanıcı dosyalarını **asla sessizce silmez.**

## FileFlow nedir

- Yerel öncelikli bir masaüstü uygulaması (Tauri + Rust + React).
- Okuyabileceğiniz bir kural motoru:
  `EĞER uzantı [pdf] AND yaş > 30 gün İSE, taşı -> ~/Documents/PDF`.
- Bir güvenlik ağı: önizleme → kuru çalıştırma → bir kez çalıştır → otomasyonu
  etkinleştir.
- Doğruluğu **doğrulanan** geri alma özellikli bir eylem günlüğü.

## Ne değildir

- "Rastgele bir AI dosya düzenleyicisi" değildir.
- Yıkıcı bir temizleyici değildir.
- Sessizce her şeyi taşıyan bir arka plan uygulaması değildir.
- Bir bulut dosya yöneticisi değildir.
- Basit bir uzantı sıralayıcı değildir.

## Kullanım (geliştirme)

```bash
cargo test -p fileflow-core       # Kural motoru testleri
npm install && npm run dev         # Tarayıcıda geliştirme önizlemesi
npm run tauri dev                  # Tam masaüstü uygulaması
```

Ayrıntı için İngilizce [README.md](README.md), [PRIVACY.md](PRIVACY.md),
[SECURITY.md](SECURITY.md), [ARCHITECTURE.md](ARCHITECTURE.md).
