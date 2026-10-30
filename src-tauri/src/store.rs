//! Inti Lemari: brankas file terenkripsi.
//!
//! Struktur folder brankas:
//!   index.vault        indeks terenkripsi (nama file, ukuran, kunci tiap file)
//!   index.vault.bak    versi indeks sebelum perubahan terakhir
//!   blobs/<id>.bin     isi file terenkripsi (nama acak)
//!
//! Master password -> Argon2id -> kunci indeks (XChaCha20-Poly1305).
//! Setiap file punya kunci acak sendiri dan dienkripsi bertahap per 1 MiB
//! (konstruksi STREAM), dengan ID file sebagai associated data, sehingga
//! potongan yang diubah, dipotong, ditukar, atau dipindah ke file lain akan terdeteksi.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::{
    aead::{stream, Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const FORMAT: &str = "lemari-vault";
pub const VERSION: u32 = 1;
pub const MIN_MASTER_LEN: usize = 10;
pub const CHUNK: usize = 1024 * 1024;
const TAG: usize = 16;

#[derive(Debug)]
pub enum Error { WrongPassword, Corrupt(String), Io(String), Invalid(String) }

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::WrongPassword => write!(f, "Password salah."),
            Error::Corrupt(m) => write!(f, "Data rusak atau telah diubah: {m}"),
            Error::Io(m) => write!(f, "Gagal membaca/menulis: {m}"),
            Error::Invalid(m) => write!(f, "{m}"),
        }
    }
}
impl From<std::io::Error> for Error { fn from(e: std::io::Error) -> Self { Error::Io(e.to_string()) } }
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy)]
pub struct KdfCost { pub m_kib: u32, pub t: u32, pub p: u32 }
impl Default for KdfCost { fn default() -> Self { KdfCost { m_kib: 64 * 1024, t: 3, p: 1 } } }

#[derive(Serialize, Deserialize, Clone)]
struct KdfParams { alg: String, m_kib: u32, t: u32, p: u32, salt: String }

#[derive(Serialize, Deserialize)]
struct IndexFile { format: String, version: u32, kdf: KdfParams, nonce: String, ciphertext: String }

#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop)]
pub struct FileMeta {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub kind: String,
    pub added: i64,
    key: String,
    nonce: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Index { pub files: Vec<FileMeta> }

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn kind_of(name: &str) -> &'static str {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "bmp" | "tif" | "tiff" | "svg" => "image",
        "mp4" | "mov" | "m4v" | "mkv" | "avi" | "webm" | "wmv" | "3gp" => "video",
        "txt" | "md" | "csv" | "json" | "log" | "xml" | "ini" | "conf" => "text",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "key" | "odt" => "doc",
        "mp3" | "m4a" | "wav" | "aac" | "flac" | "ogg" => "audio",
        "zip" | "rar" | "7z" | "tar" | "gz" => "archive",
        _ => "other",
    }
}

fn aad(kdf: &KdfParams) -> Vec<u8> {
    format!("{FORMAT}|{VERSION}|{}|{}|{}|{}|{}", kdf.alg, kdf.m_kib, kdf.t, kdf.p, kdf.salt).into_bytes()
}

fn derive_key(master: &str, kdf: &KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    if kdf.alg != "argon2id" || kdf.m_kib < 8 || kdf.m_kib > 4 * 1024 * 1024 || kdf.t == 0 || kdf.t > 100 || kdf.p == 0 || kdf.p > 16 {
        return Err(Error::Corrupt("parameter kunci tidak valid".into()));
    }
    let salt = B64.decode(&kdf.salt).map_err(|_| Error::Corrupt("salt".into()))?;
    let params = Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|e| Error::Corrupt(e.to_string()))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(master.as_bytes(), &salt, key.as_mut())
        .map_err(|e| Error::Corrupt(e.to_string()))?;
    Ok(key)
}

fn new_kdf(c: KdfCost) -> KdfParams {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    KdfParams { alg: "argon2id".into(), m_kib: c.m_kib, t: c.t, p: c.p, salt: B64.encode(salt) }
}

fn check_master(m: &str) -> Result<()> {
    if m.chars().count() < MIN_MASTER_LEN { return Err(Error::Invalid(format!("Password minimal {MIN_MASTER_LEN} karakter."))); }
    Ok(())
}

/// Baca sampai buffer penuh atau EOF.
fn read_full(r: &mut impl Read, max: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; max];
    let mut n = 0;
    while n < max {
        let k = r.read(&mut buf[n..])?;
        if k == 0 { break; }
        n += k;
    }
    buf.truncate(n);
    Ok(buf)
}

pub struct Store { dir: PathBuf, key: Zeroizing<[u8; 32]>, kdf: KdfParams, pub index: Index }

impl Store {
    pub fn index_path(dir: &Path) -> PathBuf { dir.join("index.vault") }
    pub fn exists(dir: &Path) -> bool { Self::index_path(dir).exists() }
    pub fn blobs_dir(&self) -> PathBuf { self.dir.join("blobs") }
    pub fn dir(&self) -> &Path { &self.dir }

    pub fn create(dir: &Path, master: &str, cost: KdfCost) -> Result<Self> {
        if Self::exists(dir) { return Err(Error::Invalid("Brankas sudah ada.".into())); }
        check_master(master)?;
        fs::create_dir_all(dir.join("blobs"))?;
        let kdf = new_kdf(cost);
        let s = Store { dir: dir.to_path_buf(), key: derive_key(master, &kdf)?, kdf, index: Index::default() };
        s.save()?;
        Ok(s)
    }

    pub fn open(dir: &Path, master: &str) -> Result<Self> {
        let f: IndexFile = serde_json::from_str(&fs::read_to_string(Self::index_path(dir))?)
            .map_err(|e| Error::Corrupt(e.to_string()))?;
        if f.format != FORMAT || f.version != VERSION { return Err(Error::Corrupt("format tidak dikenal".into())); }
        let key = derive_key(master, &f.kdf)?;
        let nonce = B64.decode(&f.nonce).map_err(|_| Error::Corrupt("nonce".into()))?;
        if nonce.len() != 24 { return Err(Error::Corrupt("nonce".into())); }
        let ct = B64.decode(&f.ciphertext).map_err(|_| Error::Corrupt("ciphertext".into()))?;
        let plain = Zeroizing::new(
            XChaCha20Poly1305::new(key.as_ref().into())
                .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ct, aad: &aad(&f.kdf) })
                .map_err(|_| Error::WrongPassword)?,
        );
        let index: Index = serde_json::from_slice(&plain).map_err(|e| Error::Corrupt(e.to_string()))?;
        fs::create_dir_all(dir.join("blobs"))?;
        Ok(Store { dir: dir.to_path_buf(), key, kdf: f.kdf, index })
    }

    pub fn save(&self) -> Result<()> {
        let plain = Zeroizing::new(serde_json::to_vec(&self.index).map_err(|e| Error::Io(e.to_string()))?);
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let ct = XChaCha20Poly1305::new(self.key.as_ref().into())
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: &plain, aad: &aad(&self.kdf) })
            .map_err(|_| Error::Io("enkripsi gagal".into()))?;
        let f = IndexFile { format: FORMAT.into(), version: VERSION, kdf: self.kdf.clone(), nonce: B64.encode(nonce), ciphertext: B64.encode(ct) };
        let path = Self::index_path(&self.dir);
        let tmp = path.with_extension("vault.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&f).unwrap())?;
        if path.exists() { fs::copy(&path, path.with_extension("vault.bak"))?; }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn change_master(&mut self, new_master: &str, cost: KdfCost) -> Result<()> {
        check_master(new_master)?;
        let kdf = new_kdf(cost);
        self.key = derive_key(new_master, &kdf)?;
        self.kdf = kdf;
        self.save() // kunci tiap file tidak berubah, jadi file besar tidak perlu dienkripsi ulang
    }

    pub fn get(&self, id: &str) -> Option<&FileMeta> { self.index.files.iter().find(|f| f.id == id) }

    pub fn insert(&mut self, meta: FileMeta) -> Result<()> { self.index.files.push(meta); self.save() }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\']) { return Err(Error::Invalid("Nama tidak valid.".into())); }
        let f = self.index.files.iter_mut().find(|f| f.id == id).ok_or(Error::Invalid("File tidak ditemukan.".into()))?;
        f.name = name.to_string();
        f.kind = kind_of(name).to_string();
        self.save()
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let before = self.index.files.len();
        self.index.files.retain(|f| f.id != id);
        if before == self.index.files.len() { return Err(Error::Invalid("File tidak ditemukan.".into())); }
        self.save()?;
        let _ = fs::remove_file(self.blobs_dir().join(format!("{id}.bin")));
        Ok(())
    }
}

/// Enkripsi satu file ke folder blobs. Tidak butuh master password, karena
/// setiap file memakai kunci acaknya sendiri yang nanti disimpan di indeks.
pub fn encrypt_file(blobs: &Path, src: &Path, mut progress: impl FnMut(u64, u64)) -> Result<FileMeta> {
    let md = fs::metadata(src)?;
    if !md.is_file() { return Err(Error::Invalid(format!("{} bukan file.", src.display()))); }
    let total = md.len();
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(key.as_mut());
    let mut nonce = [0u8; 19];
    OsRng.fill_bytes(&mut nonce);

    fs::create_dir_all(blobs)?;
    let dest = blobs.join(format!("{id}.bin"));
    let tmp = blobs.join(format!("{id}.tmp"));
    let result = (|| -> Result<()> {
        let mut r = BufReader::new(File::open(src)?);
        let mut w = BufWriter::new(File::create(&tmp)?);
        let mut enc = stream::EncryptorBE32::from_aead(XChaCha20Poly1305::new(key.as_ref().into()), nonce.as_ref().into());
        let mut cur = Zeroizing::new(read_full(&mut r, CHUNK)?);
        let mut done = 0u64;
        loop {
            let next = Zeroizing::new(read_full(&mut r, CHUNK)?);
            done += cur.len() as u64;
            if next.is_empty() {
                let ct = enc.encrypt_last(Payload { msg: &cur, aad: id.as_bytes() }).map_err(|_| Error::Io("enkripsi gagal".into()))?;
                w.write_all(&ct)?;
                break;
            }
            let ct = enc.encrypt_next(Payload { msg: &cur, aad: id.as_bytes() }).map_err(|_| Error::Io("enkripsi gagal".into()))?;
            w.write_all(&ct)?;
            progress(done, total);
            cur = next;
        }
        w.flush()?;
        w.get_ref().sync_all()?;
        progress(total, total);
        Ok(())
    })();
    if let Err(e) = result { let _ = fs::remove_file(&tmp); return Err(e); }
    fs::rename(&tmp, &dest)?;
    Ok(FileMeta { id, kind: kind_of(&name).into(), name, size: total, added: now(), key: B64.encode(key.as_ref()), nonce: B64.encode(nonce) })
}

/// Dekripsi file ke writer. Setiap potongan diverifikasi sebelum ditulis.
pub fn decrypt_to(blobs: &Path, meta: &FileMeta, w: &mut impl Write, mut progress: impl FnMut(u64, u64)) -> Result<()> {
    let key = Zeroizing::new(B64.decode(&meta.key).map_err(|_| Error::Corrupt("kunci file".into()))?);
    let nonce = B64.decode(&meta.nonce).map_err(|_| Error::Corrupt("nonce file".into()))?;
    if key.len() != 32 || nonce.len() != 19 { return Err(Error::Corrupt("kunci file".into())); }
    let mut r = BufReader::new(File::open(blobs.join(format!("{}.bin", meta.id)))
        .map_err(|_| Error::Corrupt(format!("isi file '{}' hilang", meta.name)))?);
    let mut dec = stream::DecryptorBE32::from_aead(XChaCha20Poly1305::new(key.as_slice().into()), nonce.as_slice().into());
    let bad = || Error::Corrupt(format!("file '{}' rusak atau telah diubah", meta.name));
    let mut cur = read_full(&mut r, CHUNK + TAG)?;
    let mut done = 0u64;
    loop {
        let next = read_full(&mut r, CHUNK + TAG)?;
        if next.is_empty() {
            let pt = Zeroizing::new(dec.decrypt_last(Payload { msg: &cur, aad: meta.id.as_bytes() }).map_err(|_| bad())?);
            w.write_all(&pt)?;
            done += pt.len() as u64;
            break;
        }
        let pt = Zeroizing::new(dec.decrypt_next(Payload { msg: &cur, aad: meta.id.as_bytes() }).map_err(|_| bad())?);
        w.write_all(&pt)?;
        done += pt.len() as u64;
        progress(done, meta.size);
        cur = next;
    }
    if done != meta.size { return Err(bad()); }
    w.flush()?;
    progress(meta.size, meta.size);
    Ok(())
}

/// Dekripsi ke file tujuan secara aman (via file sementara).
pub fn decrypt_to_path(blobs: &Path, meta: &FileMeta, dest: &Path, progress: impl FnMut(u64, u64)) -> Result<()> {
    let tmp = dest.with_extension("lemari-part");
    let res = (|| {
        let mut w = BufWriter::new(File::create(&tmp)?);
        decrypt_to(blobs, meta, &mut w, progress)
    })();
    if let Err(e) = res { let _ = fs::remove_file(&tmp); return Err(e); }
    fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const FAST: KdfCost = KdfCost { m_kib: 1024, t: 1, p: 1 };
    const PW: &str = "password-master-kuat";

    fn tmpdir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lemari-test-{n}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn data(n: usize) -> Vec<u8> { (0..n).map(|i| (i * 31 % 251) as u8).collect() }

    fn roundtrip(size: usize) {
        let d = tmpdir("rt");
        let mut s = Store::create(&d.join("v"), PW, FAST).unwrap();
        let src = d.join("foto liburan.jpg");
        fs::write(&src, data(size)).unwrap();
        let meta = encrypt_file(&s.blobs_dir(), &src, |_, _| {}).unwrap();
        s.insert(meta).unwrap();
        let s2 = Store::open(&d.join("v"), PW).unwrap();
        let m = &s2.index.files[0];
        assert_eq!((m.name.as_str(), m.size, m.kind.as_str()), ("foto liburan.jpg", size as u64, "image"));
        let mut out = Vec::new();
        decrypt_to(&s2.blobs_dir(), m, &mut out, |_, _| {}).unwrap();
        assert_eq!(out, data(size));
    }

    #[test] fn empty_file() { roundtrip(0); }
    #[test] fn small_file() { roundtrip(10); }
    #[test] fn exact_chunk() { roundtrip(CHUNK); }
    #[test] fn multi_chunk() { roundtrip(CHUNK * 3 + 777); }

    #[test]
    fn nothing_readable_on_disk() {
        let d = tmpdir("plain");
        let mut s = Store::create(&d.join("v"), PW, FAST).unwrap();
        let src = d.join("rahasia-pribadi.txt");
        fs::write(&src, b"ISI SANGAT RAHASIA").unwrap();
        s.insert(encrypt_file(&s.blobs_dir(), &src, |_, _| {}).unwrap()).unwrap();
        for e in walk(&d.join("v")) {
            let name = e.file_name().unwrap().to_string_lossy().to_string();
            assert!(!name.contains("rahasia"), "nama file asli terlihat: {name}");
            let bytes = fs::read(&e).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("RAHASIA") && !text.contains("rahasia-pribadi"));
        }
    }
    fn walk(p: &Path) -> Vec<PathBuf> {
        let mut v = vec![];
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { v.extend(walk(&p)) } else { v.push(p) }
        }
        v
    }

    #[test]
    fn wrong_password() {
        let d = tmpdir("wp");
        Store::create(&d, PW, FAST).unwrap();
        assert!(matches!(Store::open(&d, "password-yang-salah"), Err(Error::WrongPassword)));
    }

    #[test]
    fn tampered_blob_detected() {
        let d = tmpdir("tb");
        let mut s = Store::create(&d.join("v"), PW, FAST).unwrap();
        let src = d.join("a.mp4");
        fs::write(&src, data(CHUNK + 500)).unwrap();
        let m = encrypt_file(&s.blobs_dir(), &src, |_, _| {}).unwrap();
        let blob = s.blobs_dir().join(format!("{}.bin", m.id));
        s.insert(m.clone()).unwrap();
        let mut b = fs::read(&blob).unwrap();
        b[CHUNK + 100] ^= 1;
        fs::write(&blob, &b).unwrap();
        assert!(decrypt_to(&s.blobs_dir(), &m, &mut Vec::new(), |_, _| {}).is_err());
    }

    #[test]
    fn truncated_blob_detected() {
        let d = tmpdir("tr");
        let s = Store::create(&d.join("v"), PW, FAST).unwrap();
        let src = d.join("a.mp4");
        fs::write(&src, data(CHUNK * 2 + 5)).unwrap();
        let m = encrypt_file(&s.blobs_dir(), &src, |_, _| {}).unwrap();
        let blob = s.blobs_dir().join(format!("{}.bin", m.id));
        let b = fs::read(&blob).unwrap();
        fs::write(&blob, &b[..(CHUNK + TAG) * 2]).unwrap(); // buang potongan terakhir
        assert!(decrypt_to(&s.blobs_dir(), &m, &mut Vec::new(), |_, _| {}).is_err());
    }

    #[test]
    fn swapped_blobs_detected() {
        let d = tmpdir("sw");
        let s = Store::create(&d.join("v"), PW, FAST).unwrap();
        fs::write(d.join("a.txt"), b"aaaa").unwrap();
        fs::write(d.join("b.txt"), b"bbbb").unwrap();
        let a = encrypt_file(&s.blobs_dir(), &d.join("a.txt"), |_, _| {}).unwrap();
        let b = encrypt_file(&s.blobs_dir(), &d.join("b.txt"), |_, _| {}).unwrap();
        fs::copy(s.blobs_dir().join(format!("{}.bin", b.id)), s.blobs_dir().join(format!("{}.bin", a.id))).unwrap();
        assert!(decrypt_to(&s.blobs_dir(), &a, &mut Vec::new(), |_, _| {}).is_err());
    }

    #[test]
    fn change_master_keeps_files() {
        let d = tmpdir("cm");
        let mut s = Store::create(&d.join("v"), PW, FAST).unwrap();
        fs::write(d.join("x.txt"), b"halo").unwrap();
        s.insert(encrypt_file(&s.blobs_dir(), &d.join("x.txt"), |_, _| {}).unwrap()).unwrap();
        s.change_master("password-baru-sekali", FAST).unwrap();
        assert!(Store::open(&d.join("v"), PW).is_err());
        let s2 = Store::open(&d.join("v"), "password-baru-sekali").unwrap();
        let mut out = Vec::new();
        decrypt_to(&s2.blobs_dir(), &s2.index.files[0], &mut out, |_, _| {}).unwrap();
        assert_eq!(out, b"halo");
    }

    #[test]
    fn delete_removes_blob_and_rename_works() {
        let d = tmpdir("dl");
        let mut s = Store::create(&d.join("v"), PW, FAST).unwrap();
        fs::write(d.join("x.txt"), b"halo").unwrap();
        let m = encrypt_file(&s.blobs_dir(), &d.join("x.txt"), |_, _| {}).unwrap();
        let id = m.id.clone();
        s.insert(m).unwrap();
        s.rename(&id, "catatan.md").unwrap();
        assert_eq!(s.get(&id).unwrap().name, "catatan.md");
        assert!(s.rename(&id, "../keluar").is_err());
        s.delete(&id).unwrap();
        assert!(!s.blobs_dir().join(format!("{id}.bin")).exists());
        assert!(Store::open(&d.join("v"), PW).unwrap().index.files.is_empty());
    }

    #[test]
    fn export_to_path() {
        let d = tmpdir("ex");
        let s = Store::create(&d.join("v"), PW, FAST).unwrap();
        fs::write(d.join("v.mov"), data(CHUNK + 3)).unwrap();
        let m = encrypt_file(&s.blobs_dir(), &d.join("v.mov"), |_, _| {}).unwrap();
        decrypt_to_path(&s.blobs_dir(), &m, &d.join("hasil.mov"), |_, _| {}).unwrap();
        assert_eq!(fs::read(d.join("hasil.mov")).unwrap(), data(CHUNK + 3));
    }

    #[test]
    fn short_master_rejected() { assert!(Store::create(&tmpdir("sm"), "pendek", FAST).is_err()); }
}
