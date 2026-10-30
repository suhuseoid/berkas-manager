# Panduan Instalasi & Pemakaian "Dokumen"

1. [Mengunduh](#1-mengunduh)
2. [Memasang di macOS](#2-memasang-di-macos)
3. [Memasang di Windows](#3-memasang-di-windows)
4. [Memaksimalkan kamuflase](#4-memaksimalkan-kamuflase)
5. [Pemakaian](#5-pemakaian)
6. [Cadangan & pindah komputer](#6-cadangan--pindah-komputer)
7. [Menghapus aplikasi](#7-menghapus-aplikasi)
8. [Membangun dari kode sumber](#8-membangun-dari-kode-sumber)
9. [Mengatasi masalah](#9-mengatasi-masalah)
10. [Batasan yang perlu diketahui](#10-batasan-yang-perlu-diketahui)

---

## 1. Mengunduh

Buka halaman **[Releases](../../releases)**:

| Sistem | File |
|---|---|
| macOS (Apple Silicon & Intel) | `Dokumen_x.y.z_universal.dmg` |
| Windows 10/11 | `Dokumen_x.y.z_x64-setup.exe` (disarankan) atau `.msi` |

> Belum ada di Releases? Buka tab **Actions > Build Dokumen**, klik proses terakhir yang berhasil,
> lalu unduh dari bagian **Artifacts** (berupa `.zip`, ekstrak dulu).

---

## 2. Memasang di macOS

1. Buka file `.dmg`, seret **Dokumen** ke folder **Applications**.
2. Pertama kali: **klik kanan Dokumen > Open > Open** (aplikasi belum bersertifikat Apple berbayar).
3. Jika muncul **"Dokumen is damaged"**, jalankan di Terminal:
   ```bash
   xattr -dr com.apple.quarantine /Applications/Dokumen.app
   ```

## 3. Memasang di Windows

1. Jalankan `Dokumen_x.y.z_x64-setup.exe`.
2. Jika muncul **"Windows protected your PC"**: klik **More info > Run anyway**.
3. Selesai. Dokumen ada di menu Start, dan bisa dibuat shortcut-nya.

---

## 4. Memaksimalkan kamuflase

**macOS**
- Seret **Dokumen** dari Applications ke Desktop sambil menahan **Option + Command** untuk membuat *alias*.
  Alias tampil sebagai folder biru bernama "Dokumen".
- Ganti nama alias sesuka hati (mis. "Arsip 2023"). Aplikasinya tetap sama.
- Agar tidak muncul di Dock saat dibuka, cukup tutup jendelanya setelah selesai (Cmd+Q).

**Windows**
- Klik kanan file `Dokumen.exe` (di `C:\Users\<nama>\AppData\Local\Dokumen\`) > **Send to > Desktop (create shortcut)**.
- Ganti nama shortcut (mis. "Arsip 2023") dan hapus tulisan "- Shortcut".
- Shortcut tampil dengan ikon folder kuning.
- Untuk menghapus panah kecil shortcut tidak disarankan karena butuh mengubah registry.

> Ikon folder menyamarkan dari pandangan sekilas. Isi file tetap aman karena enkripsi,
> bukan karena penyamaran.

---

## 5. Pemakaian

**Pertama kali:** buat password minimal 10 karakter. **Catat di kertas.** Tidak ada cara memulihkannya.

**Membuka:** layar awal hanya berupa kolom password polos. Ketik password lalu tekan **Enter**.

**Menambah file**
- Klik **+ File** atau **+ Folder**, atau **seret file/folder** ke jendela.
- Menu **⋯ > Hapus file asli setelah diamankan**: file asli dihapus setelah berhasil dienkripsi.
  > Penghapusan biasa di SSD tidak menjamin data lama tidak bisa dipulihkan dengan alat khusus.
  > Untuk file yang sangat sensitif, enkripsikan file sejak awal sebelum disimpan di tempat lain.

**Melihat**
- **Klik dua kali** atau tekan **Enter** pada file.
- Gambar: tampil dalam galeri, pindah dengan panah **← →**, tutup dengan **Esc**.
- Teks (`.txt`, `.md`, `.csv`, …): tampil langsung.
- Video & lainnya: dibuka dengan aplikasi bawaan lewat salinan sementara yang dihapus saat **Kunci** atau aplikasi ditutup.

**Mengelola:** pilih file, lalu gunakan **Buka**, **Keluarkan…**, **Ganti nama**, atau **Hapus**.

**Mengunci:** klik **Kunci**, atau otomatis setelah 5 menit tidak ada aktivitas.

**Ganti password:** menu **⋯ > Ganti password**.

---

## 6. Cadangan & pindah komputer

Semua isi brankas ada di satu folder, dan sudah terenkripsi:

| Sistem | Lokasi |
|---|---|
| macOS | `~/Library/Application Support/id.suhu.lemari/lemari/` |
| Windows | `%APPDATA%\id.suhu.lemari\lemari\` |

Buka cepat lewat menu **⋯ > Tampilkan folder brankas**.

**Cadangan:** tutup aplikasi, lalu salin **seluruh folder `lemari`** ke SSD/flashdisk.

**Pindah komputer (termasuk Mac ↔ Windows):**
1. Pasang Dokumen di komputer baru, buka sekali, lalu tutup tanpa membuat brankas.
2. Salin folder `lemari` dari cadangan ke lokasi di tabel di atas.
3. Buka Dokumen dan masukkan password yang sama.

**Berhenti memakai:** menu **⋯ > Keluarkan semua file…** untuk mengembalikan semua file ke folder biasa.

---

## 7. Menghapus aplikasi

Menghapus aplikasi **tidak** menghapus isi brankas.

- **macOS:** seret `Dokumen.app` ke Trash. Isi brankas: hapus `~/Library/Application Support/id.suhu.lemari`.
- **Windows:** Settings > Apps > Dokumen > Uninstall. Isi brankas: hapus `%APPDATA%\id.suhu.lemari`.

Keluarkan file terlebih dahulu jika masih membutuhkannya.

---

## 8. Membangun dari kode sumber

Butuh **Rust** dan **Node.js 18+**.

**macOS**
```bash
xcode-select --install
brew install rust node
git clone https://github.com/suhuseoid/berkas-manager.git
cd berkas-manager && npm install && npx tauri build
```

**Windows** — pasang [Rust](https://rustup.rs), [Node.js LTS](https://nodejs.org), dan
Visual Studio Build Tools (workload "Desktop development with C++"), lalu di PowerShell:
```powershell
git clone https://github.com/suhuseoid/berkas-manager.git
cd berkas-manager; npm install; npx tauri build
```

---

## 9. Mengatasi masalah

| Masalah | Solusi |
|---|---|
| Lupa password | Tidak bisa dipulihkan. Ini disengaja agar tidak ada "pintu belakang". |
| "Password salah" padahal yakin benar | Cek Caps Lock & bahasa keyboard. Jika file indeks rusak: tutup aplikasi, ganti nama `index.vault`, lalu salin `index.vault.bak` menjadi `index.vault`. |
| "File rusak atau telah diubah" | Isi file di folder `blobs` berubah atau tidak lengkap. Pulihkan folder `lemari` dari cadangan. |
| Gambar tidak tampil di pratinjau (mis. HEIC di Windows) | Pilih file lalu klik **Buka** untuk membuka dengan aplikasi bawaan. |
| Video tidak bisa dibuka | Pastikan ada pemutar yang mendukung formatnya (mis. VLC). |
| macOS: "Dokumen is damaged" | Lihat [bagian 2](#2-memasang-di-macos). |
| Windows: SmartScreen memblokir | **More info > Run anyway**. |

---

## 10. Batasan yang perlu diketahui

- **Saat dibuka**, file yang Anda buka dengan aplikasi lain sempat ada sebagai salinan sementara
  yang tidak terenkripsi, sampai brankas dikunci atau ditutup.
- **Pratinjau** gambar & teks hanya ada di memori aplikasi, tidak ditulis ke disk.
- Melindungi dari orang yang mengintip atau meminjam komputer, **bukan** dari malware yang
  sudah berjalan di komputer saat brankas sedang terbuka.
