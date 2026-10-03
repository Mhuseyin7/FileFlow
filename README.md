# FileFlow

[![CI](https://github.com/Mhuseyin7/FileFlow/actions/workflows/ci.yml/badge.svg)](https://github.com/Mhuseyin7/FileFlow/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-24C8DB.svg)](https://tauri.app/)

**Explainable file organization for your desktop.**
Açık kaynak kodlu bir desktop utility — geliştiren: **[muhammedkoca.com.tr](https://muhammedkoca.com.tr)**.

FileFlow, seçtiğiniz klasörleri izleyen ve dosyaları **açık, explainable rule'larla** organize eden bir local-first desktop application'dır. Piyasadaki "smart" file organizer'lar çoğu zaman biraz korkutucudur: predictable olmayan bir şekilde sınıflandırırlar, silently çalışırlar ve diskinizi kendi playground'ları gibi görürler. FileFlow tam tersini yapar — her action önce **preview** edilir, **dry-run** edilebilir, journal'a yazılır ve **undo** ile geri alınabilir.

> FileFlow, user file'larınızı **asla** sessizce silmez.

---

## Bu proje kimin?

FileFlow **açık kaynak** (Apache-2.0). Design, architecture ve implementation:
[**muhammedkoca.com.tr**](https://muhammedkoca.com.tr). Issue, PR ve contribution'lara açık — detay için [CONTRIBUTING.md](CONTRIBUTING.md).

---

## FileFlow nedir

- Cross-platform desktop application (Tauri + Rust + React + TypeScript).
- Okuyabileceğiniz bir **rule engine**:
  `WHEN extension in [pdf] AND age > 30 days THEN move → ~/Documents/PDF`.
- Bir safety net: **preview → dry-run → run once → enable automation**.
- Safety'si **verified** olan bir action journal + undo — destination'daki dosya değiştiyse undo reddedilir.
- Real-time file watcher (`notify` v6) + Daily/Weekly scheduler + native OS trash entegrasyonu.
- İsteğe bağlı **EXIF** ve **PDF page-count** condition'ları — dosya *content'i* asla dışarı çıkmaz.
- **Turkish/English** UI, otomatik `navigator.language` detection ile.

## FileFlow ne değildir

- Bir "random AI file organizer" değildir.
- Bir destructive cleaner değildir.
- Silently her şeyi taşıyan bir background app değildir.
- Bir cloud file manager değildir.
- Basit bir extension-only sorter değildir (rule'lar age, size, category, regex, EXIF/PDF metadata ve folder membership'i kombine eder).

---

## Primary flow

1. Bir folder'a FileFlow'a access verirsiniz (mesela `~/Downloads`).
2. Bir rule yaratırsınız. FileFlow live preview verir:

   ```
   Preview
   12 files match · 4.8 GB total
   [View Files]  [Dry Run]  [Run Once]  [Enable Automation]
   ```

3. **Run once** ile bir sefer çalıştırırsınız, **Activity** log'una bakarsınız ve bir şey ters gittiyse **Undo**'ya basarsınız. Undo, destination file execute anındaki hash'iyle uyuşmuyorsa aborted olur.
4. Sonuçtan memnunsanız rule'u enable edersiniz — FileFlow ondan sonra real-time watcher üzerinden apply eder.

---

## Getting started (development)

```bash
# 1) Rust core: rule engine, planner, safety, journal, undo, watcher, scheduler.
cargo test -p fileflow-core        # 55 test, Tauri dependency'sine gerek yok

# 2) Frontend + dev shell (mock IPC ile browser preview).
npm install
npm run dev                         # http://localhost:1420

# 3) Full desktop app (Tauri v2 prerequisite'leri gerekir).
npm run tauri dev

# 4) Native installer'lar (Windows / macOS / Linux — bkz. RELEASING.md).
npm run tauri build
```

`fileflow-core` crate'i Tauri'den bağımsızdır — rule engine ve test'ler WebView2 / GTK olmadan çalışır, iteration hızlıdır.

---

## Architecture

```
File Events / on-demand scan
        ↓
    Normalizer         (crates/fileflow-core/src/model.rs)
        ↓
   Rule Engine         (crates/fileflow-core/src/rule.rs)
        ↓
  Action Planner       (crates/fileflow-core/src/planner.rs)
        ↓
 Safety Validator      (crates/fileflow-core/src/safety.rs)
        ↓
   Preview / Execute   (crates/fileflow-core/src/fs_ops.rs)
        ↓
     Journal           (crates/fileflow-core/src/journal.rs)
        ↓
      Undo             (journal.rs :: Journal::undo)
```

FileFlow raw filesystem event'ten hiçbir zaman direkt execute etmez. Detay için [ARCHITECTURE.md](ARCHITECTURE.md).

### Modüller

| Module | Sorumluluk |
|--------|-----------|
| `model` | `FileMeta`, `Rule`, `Condition`, `Action`, `PlannedOp` — normalized data model |
| `rule` | Deterministic condition evaluation + reason trace |
| `planner` | Rule → PlannedOp, priority ordering, overlap detection |
| `safety` | Sandbox, forbidden roots, symlink escape, invalid filename check |
| `fs_ops` | Atomic rename, cross-fs copy + SHA-256 verify + delete, OS trash |
| `journal` | SQLite journal + state machine + verified undo |
| `watcher` | `notify` v6 + debouncer + stability gate + partial-download filter |
| `scheduler` | Daily / Weekly trigger runner |
| `metadata` | Optional EXIF (kamadak-exif) + PDF (lopdf) reader — local only |
| `rename` | Deterministic template engine + cross-platform filename sanitize |
| `category` | Extension → category (Documents / Images / Video / Audio / Archives / Installers / Code / Data / Other) |

---

## Safety guarantees

- **Permanent delete default olarak yoktur.** `Trash` action'ı ya native OS trash'e ya da source root'unun yanındaki local `.fileflow-trash/` klasörüne gönderir.
- **Silent overwrite yoktur.** Default collision policy `Rename`. `Ask` ve `Replace` açıkça configure edilmelidir.
- **Cross-filesystem move'lar verify edilir.** SHA-256 stream'lenirken hesaplanır; destination yazıldıktan sonra tekrar hash'lenir; ancak match ederse source silinir.
- **Undo verified.** Restore öncesi FileFlow destination'ı re-hash'ler ve size check yapar. Uyuşmazsa undo abort edilir ve `undo_status = unsafe` yazılır.
- **Full-computer scan yoktur.** Sadece açıkça grant ettiğiniz folder'lar.
- **Half-downloaded file'lara dokunmaz.** Browser partial-download suffix'leri (`.crdownload`, `.part`, `.partial`, `.download`, `.!ut`, `.!qb`, `.opdownload`) ve bir stability window (N saniye unchanged) gate olarak çalışır.
- **Zero AI / zero network / zero telemetry** — default state. Bkz. [PRIVACY.md](PRIVACY.md).

---

## Test coverage

`cargo test -p fileflow-core` — **55 test** geçiyor:

| Suite | Test count |
|-------|-----------|
| rule_eval | 9 |
| planner | 7 |
| rename | 6 |
| category | 4 |
| stability | 4 |
| fs_ops (tempfile-based, incl. real 10 MB copy+verify) | 11 |
| safety | 5 |
| journal + undo (refusal on drift) | (fs_ops içinde) |
| scheduler | 5 |
| watcher (real notify) | 2 |
| metadata (real PDF built in-test with lopdf) | 2 |

Clippy strict mode (`-D warnings`) temiz, TypeScript strict + `exactOptionalPropertyTypes` temiz, Vite production build temiz.

---

## Roadmap

MVP feature-complete. Planlanan next step'ler:

- Real-time watcher event'lerinin planner'a otomatik dispatch'i (şu an manual `preview` + `run` cycle).
- `Ask` collision policy için native OS dialog wiring.
- Native OS trash'ten restore (Windows Recycle Bin + Linux `gio trash` restore path'leri).
- Rule import/export tam workflow'u (export'ta absolute personal path'lerin `~` ile normalize edilmesi).
- Signed installer distribution — bkz. [RELEASING.md](RELEASING.md).

---

## License

[Apache-2.0](LICENSE). Copyright © 2026 [muhammedkoca.com.tr](https://muhammedkoca.com.tr) ve contributor'lar.
