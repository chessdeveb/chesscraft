//! Telechargement de la megabase : base de reference prete a l'emploi
//! (~10 M de parties, construite a partir de Lumbra's GigaBase), proposee au
//! premier lancement et depuis le menu Fichier.
//!
//! Le fichier publie est le .sqlite compresse en zstd (~16 Go au lieu de
//! 46,5 Go). Son emplacement, sa taille et son empreinte SHA-256 sont lus
//! dans un petit manifeste JSON (MANIFEST_URL) : remplacer la base publiee ne
//! demande donc qu'un commit de ce manifeste, pas une nouvelle version de
//! l'appli.
//!
//! Etapes : telechargement (reprenable : requete Range sur le fichier
//! partiel, y compris apres une fermeture de l'appli grace a PENDING_FILE),
//! verification de l'empreinte, decompression, puis remplacement atomique
//! (rename) vers megabase.sqlite. L'archive est supprimee a la fin.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MANIFEST_URL: &str = "https://raw.githubusercontent.com/chessdeveb/chesscraft/main/megabase.json";
pub const FILE_NAME: &str = "megabase.sqlite";
const ARCHIVE_PART: &str = "megabase.sqlite.zst.part";
const OUTPUT_PART: &str = "megabase.sqlite.part";
const PENDING_FILE: &str = "megabase-download.json";
const USER_AGENT: &str = "chesscraft (+https://github.com/chessdeveb/chesscraft)";
/// Echecs reseau consecutifs (sans aucun octet recu entre deux) toleres
/// avant d'abandonner : couvre une coupure Wi-Fi ou une mise en veille.
const MAX_CONSECUTIVE_FAILURES: u32 = 6;
const RETRY_DELAY: Duration = Duration::from_secs(5);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Date de la partie la plus recente, ex. "2026-11-03".
    pub version: String,
    /// Lien de telechargement direct (Dropbox : dl=1).
    pub url: String,
    pub compressed_size: u64,
    pub size: u64,
    /// SHA-256 (hexadecimal) du fichier compresse.
    pub sha256: String,
    pub games: u64,
    /// track.php du site : comptage anonyme des installations terminees.
    #[serde(default)]
    pub count_url: Option<String>,
}

impl Manifest {
    /// Meme archive (meme contenu) : seuls l'empreinte et la taille
    /// comptent. Un nouveau lien de telechargement ou une nouvelle adresse de
    /// comptage (changement de domaine du site) ne doit pas faire jeter un
    /// telechargement deja bien avance.
    pub fn same_archive(&self, other: &Manifest) -> bool {
        self.sha256.eq_ignore_ascii_case(&other.sha256) && self.compressed_size == other.compressed_size
    }

    fn validate(&self) -> Result<(), MegabaseError> {
        let hex_ok = self.sha256.len() == 64 && self.sha256.chars().all(|c| c.is_ascii_hexdigit());
        if !self.url.starts_with("https://") || self.compressed_size == 0 || self.size == 0 || !hex_ok {
            return Err(MegabaseError::new("manifestInvalid"));
        }
        Ok(())
    }
}

/// Telechargement commence mais pas termine, memorise dans le dossier de
/// donnees de l'appli pour etre repris au prochain lancement.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub dir: String,
    pub manifest: Manifest,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingStatus {
    pub dir: String,
    pub downloaded: u64,
    pub manifest: Manifest,
}

/// Erreur traduite cote interface a partir de `code` (cles megabase.error.*).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MegabaseError {
    pub code: &'static str,
    pub detail: Option<String>,
    pub needed: Option<u64>,
    pub available: Option<u64>,
}

impl MegabaseError {
    pub fn new(code: &'static str) -> Self {
        MegabaseError { code, detail: None, needed: None, available: None }
    }
    pub fn with(code: &'static str, detail: impl ToString) -> Self {
        MegabaseError { detail: Some(detail.to_string()), ..Self::new(code) }
    }
    fn io(e: io::Error) -> Self {
        Self::with("io", e)
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// "download" | "verify" | "decompress"
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
    /// Debit moyen depuis le debut de l'etape (octets/s), 0 si inconnu.
    pub bytes_per_sec: f64,
}

pub enum Outcome {
    Done { path: PathBuf, manifest: Manifest },
    Stopped,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(60))
        .user_agent(USER_AGENT)
        .build()
}

pub fn fetch_manifest() -> Result<Manifest, MegabaseError> {
    let text = agent()
        .get(MANIFEST_URL)
        .call()
        .map_err(|e| MegabaseError::with("manifest", e))?
        .into_string()
        .map_err(|e| MegabaseError::with("manifest", e))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|_| MegabaseError::new("manifestInvalid"))?;
    manifest.validate()?;
    Ok(manifest)
}

fn pending_file(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PENDING_FILE)
}

pub fn load_pending(app_data_dir: &Path) -> Option<Pending> {
    let text = fs::read_to_string(pending_file(app_data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_pending(app_data_dir: &Path, pending: &Pending) -> Result<(), MegabaseError> {
    let text = serde_json::to_string_pretty(pending).map_err(|e| MegabaseError::with("io", e))?;
    fs::write(pending_file(app_data_dir), text).map_err(MegabaseError::io)
}

pub fn pending_status(app_data_dir: &Path) -> Option<PendingStatus> {
    let pending = load_pending(app_data_dir)?;
    let downloaded = fs::metadata(Path::new(&pending.dir).join(ARCHIVE_PART)).map(|m| m.len()).unwrap_or(0);
    Some(PendingStatus { dir: pending.dir, downloaded, manifest: pending.manifest })
}

/// Supprime le telechargement en cours (fichiers partiels et memo).
pub fn discard(app_data_dir: &Path) {
    if let Some(pending) = load_pending(app_data_dir) {
        let dir = Path::new(&pending.dir);
        let _ = fs::remove_file(dir.join(ARCHIVE_PART));
        let _ = fs::remove_file(dir.join(OUTPUT_PART));
    }
    let _ = fs::remove_file(pending_file(app_data_dir));
}

/// Prepare un nouveau telechargement dans `dir` : refuse d'ecraser une
/// megabase existante et verifie l'espace disque avant de commencer.
pub fn prepare(app_data_dir: &Path, dir: &Path, manifest: &Manifest) -> Result<(), MegabaseError> {
    manifest.validate()?;
    fs::create_dir_all(dir).map_err(MegabaseError::io)?;
    if dir.join(FILE_NAME).exists() {
        return Err(MegabaseError::with("exists", dir.join(FILE_NAME).display()));
    }
    // un telechargement precedent vers un autre dossier, ou d'une autre
    // archive, est abandonne
    if let Some(previous) = load_pending(app_data_dir) {
        if Path::new(&previous.dir) != dir || !previous.manifest.same_archive(manifest) {
            discard(app_data_dir);
        }
    }
    check_space(dir, manifest)?;
    save_pending(app_data_dir, &Pending { dir: dir.to_string_lossy().into_owned(), manifest: manifest.clone() })
}

/// Espace necessaire pendant l'operation : le reste de l'archive a
/// telecharger, plus la base decompressee (l'archive n'est supprimee
/// qu'ensuite).
pub fn check_space(dir: &Path, manifest: &Manifest) -> Result<(), MegabaseError> {
    let downloaded = fs::metadata(dir.join(ARCHIVE_PART)).map(|m| m.len()).unwrap_or(0);
    let needed = manifest.compressed_size.saturating_sub(downloaded) + manifest.size;
    if let Some(available) = available_space(dir) {
        if available < needed {
            return Err(MegabaseError { needed: Some(needed), available: Some(available), ..MegabaseError::new("space") });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn available_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(path: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn available_space(_dir: &Path) -> Option<u64> {
    None
}

/// Suit le debit et limite la frequence des evenements de progression.
struct Meter {
    stage: &'static str,
    total: u64,
    start: Instant,
    start_done: u64,
    last_emit: Option<Instant>,
}

impl Meter {
    fn new(stage: &'static str, total: u64, start_done: u64) -> Self {
        Meter { stage, total, start: Instant::now(), start_done, last_emit: None }
    }

    fn report(&mut self, done: u64, force: bool, on_progress: &mut dyn FnMut(Progress)) {
        let now = Instant::now();
        if !force && self.last_emit.is_some_and(|t| now.duration_since(t) < PROGRESS_INTERVAL) {
            return;
        }
        self.last_emit = Some(now);
        let elapsed = now.duration_since(self.start).as_secs_f64();
        let bytes_per_sec = if elapsed > 1.0 { done.saturating_sub(self.start_done) as f64 / elapsed } else { 0.0 };
        on_progress(Progress { stage: self.stage, done, total: self.total, bytes_per_sec });
    }
}

/// Attend `delay` en verifiant regulierement la demande d'arret.
fn wait(delay: Duration, should_stop: &dyn Fn() -> bool) -> bool {
    let end = Instant::now() + delay;
    while Instant::now() < end {
        if should_stop() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    true
}

enum Attempt {
    Complete,
    Stopped,
    /// Interrompu ; `progressed` indique si des octets ont ete recus.
    Failed { progressed: bool, error: String },
}

fn download_attempt(
    manifest: &Manifest,
    part: &Path,
    meter: &mut Meter,
    on_progress: &mut dyn FnMut(Progress),
    should_stop: &dyn Fn() -> bool,
) -> Result<Attempt, MegabaseError> {
    let mut have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if have > manifest.compressed_size {
        fs::remove_file(part).map_err(MegabaseError::io)?;
        have = 0;
    }
    if have == manifest.compressed_size {
        return Ok(Attempt::Complete);
    }
    let mut request = agent().get(&manifest.url);
    if have > 0 {
        request = request.set("Range", &format!("bytes={have}-"));
    }
    let response = match request.call() {
        Ok(response) => response,
        // plage refusee : le fichier distant a change de taille, on repart de zero
        Err(ureq::Error::Status(416, _)) => {
            fs::remove_file(part).map_err(MegabaseError::io)?;
            return Ok(Attempt::Failed { progressed: true, error: "416".into() });
        }
        Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) && code != 429 => {
            return Err(MegabaseError::with("network", format!("HTTP {code}")));
        }
        Err(e) => return Ok(Attempt::Failed { progressed: false, error: e.to_string() }),
    };
    // 200 au lieu de 206 : le serveur ignore la reprise, on reecrit depuis le debut
    let resumed = have > 0 && response.status() == 206;
    let mut file = if resumed {
        OpenOptions::new().append(true).open(part).map_err(MegabaseError::io)?
    } else {
        have = 0;
        File::create(part).map_err(MegabaseError::io)?
    };
    meter.report(have, true, on_progress);
    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut received = 0u64;
    loop {
        if should_stop() {
            file.flush().map_err(MegabaseError::io)?;
            return Ok(Attempt::Stopped);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Ok(Attempt::Failed { progressed: received > 0, error: e.to_string() }),
        };
        let n = n.min((manifest.compressed_size - have) as usize);
        file.write_all(&buf[..n]).map_err(MegabaseError::io)?;
        have += n as u64;
        received += n as u64;
        meter.report(have, false, on_progress);
        if have == manifest.compressed_size {
            break;
        }
    }
    file.flush().map_err(MegabaseError::io)?;
    meter.report(have, true, on_progress);
    if have == manifest.compressed_size {
        Ok(Attempt::Complete)
    } else {
        Ok(Attempt::Failed { progressed: received > 0, error: "connexion interrompue".into() })
    }
}

fn download(
    manifest: &Manifest,
    part: &Path,
    on_progress: &mut dyn FnMut(Progress),
    should_stop: &dyn Fn() -> bool,
) -> Result<bool, MegabaseError> {
    let start = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let mut meter = Meter::new("download", manifest.compressed_size, start);
    let mut failures = 0;
    loop {
        match download_attempt(manifest, part, &mut meter, on_progress, should_stop)? {
            Attempt::Complete => return Ok(true),
            Attempt::Stopped => return Ok(false),
            Attempt::Failed { progressed, error } => {
                failures = if progressed { 1 } else { failures + 1 };
                if failures >= MAX_CONSECUTIVE_FAILURES {
                    return Err(MegabaseError::with("network", error));
                }
                if !wait(RETRY_DELAY, should_stop) {
                    return Ok(false);
                }
            }
        }
    }
}

fn verify(
    manifest: &Manifest,
    part: &Path,
    on_progress: &mut dyn FnMut(Progress),
    should_stop: &dyn Fn() -> bool,
) -> Result<bool, MegabaseError> {
    let mut file = File::open(part).map_err(MegabaseError::io)?;
    let mut hasher = Sha256::new();
    let mut meter = Meter::new("verify", manifest.compressed_size, 0);
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        if should_stop() {
            return Ok(false);
        }
        let n = file.read(&mut buf).map_err(MegabaseError::io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        meter.report(done, false, on_progress);
    }
    meter.report(done, true, on_progress);
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !digest.eq_ignore_ascii_case(&manifest.sha256) {
        // archive corrompue : la supprimer pour que la prochaine tentative reparte de zero
        let _ = fs::remove_file(part);
        return Err(MegabaseError::new("checksum"));
    }
    Ok(true)
}

fn decompress(
    manifest: &Manifest,
    archive: &Path,
    output: &Path,
    on_progress: &mut dyn FnMut(Progress),
    should_stop: &dyn Fn() -> bool,
) -> Result<bool, MegabaseError> {
    let input = File::open(archive).map_err(MegabaseError::io)?;
    let mut decoder = zstd::stream::read::Decoder::new(input).map_err(|e| MegabaseError::with("decompress", e))?;
    // autorise les archives produites avec --long (fenetre jusqu'a 2 Gio)
    decoder.window_log_max(31).map_err(|e| MegabaseError::with("decompress", e))?;
    let mut out = File::create(output).map_err(MegabaseError::io)?;
    let mut meter = Meter::new("decompress", manifest.size, 0);
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        if should_stop() {
            drop(out);
            let _ = fs::remove_file(output);
            return Ok(false);
        }
        let n = match decoder.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                drop(out);
                let _ = fs::remove_file(output);
                let _ = fs::remove_file(archive);
                return Err(MegabaseError::with("decompress", e));
            }
        };
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(MegabaseError::io)?;
        done += n as u64;
        meter.report(done, false, on_progress);
    }
    meter.report(done, true, on_progress);
    out.sync_all().map_err(MegabaseError::io)?;
    if done != manifest.size {
        drop(out);
        let _ = fs::remove_file(output);
        return Err(MegabaseError::with("decompress", format!("{done} / {} octets", manifest.size)));
    }
    Ok(true)
}

/// Execute (ou reprend) le telechargement memorise dans PENDING_FILE
/// jusqu'a la base installee, ou jusqu'a une demande d'arret.
pub fn run(
    app_data_dir: &Path,
    on_progress: &mut dyn FnMut(Progress),
    should_stop: &dyn Fn() -> bool,
) -> Result<Outcome, MegabaseError> {
    let mut pending = load_pending(app_data_dir).ok_or_else(|| MegabaseError::new("noPending"))?;
    // manifeste republie depuis le debut du telechargement : nouvelle base
    // -> l'archive partielle ne correspond plus, on repart sur la nouvelle ;
    // meme base -> on garde l'archive et on adopte simplement les nouvelles
    // adresses (lien de telechargement, comptage).
    if let Ok(latest) = fetch_manifest() {
        if latest != pending.manifest {
            if !latest.same_archive(&pending.manifest) {
                let _ = fs::remove_file(Path::new(&pending.dir).join(ARCHIVE_PART));
            }
            pending.manifest = latest;
            save_pending(app_data_dir, &pending)?;
        }
    }
    let dir = PathBuf::from(&pending.dir);
    let manifest = &pending.manifest;
    let archive = dir.join(ARCHIVE_PART);
    let output = dir.join(OUTPUT_PART);
    let target = dir.join(FILE_NAME);
    if target.exists() {
        return Err(MegabaseError::with("exists", target.display()));
    }
    check_space(&dir, manifest)?;

    if !download(manifest, &archive, on_progress, should_stop)? {
        return Ok(Outcome::Stopped);
    }
    if !verify(manifest, &archive, on_progress, should_stop)? {
        return Ok(Outcome::Stopped);
    }
    if !decompress(manifest, &archive, &output, on_progress, should_stop)? {
        return Ok(Outcome::Stopped);
    }
    // Restes d'une megabase precedente supprimee a la main (megabase.sqlite
    // efface, mais pas son -wal/-shm) : SQLite rejouerait ce journal etranger
    // sur la nouvelle base des sa premiere ouverture et la corromprait.
    for suffix in ["-wal", "-shm"] {
        let mut stale = target.clone().into_os_string();
        stale.push(suffix);
        let _ = fs::remove_file(PathBuf::from(stale));
    }
    fs::rename(&output, &target).map_err(MegabaseError::io)?;
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_file(pending_file(app_data_dir));
    Ok(Outcome::Done { path: target, manifest: manifest.clone() })
}

/// Comptage anonyme d'une installation terminee (meme compteur que les clics
/// du site, source "application"). Les erreurs sont ignorees.
pub fn count_install(manifest: &Manifest) {
    if let Some(url) = manifest.count_url.as_deref().filter(|u| u.starts_with("https://")) {
        let _ = agent().post(url).send_form(&[("version", "megabase"), ("source", "application")]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("chesscraft-megabase-test-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest_for(data: &[u8], compressed: &[u8]) -> Manifest {
        let sha: String = Sha256::digest(compressed).iter().map(|b| format!("{b:02x}")).collect();
        Manifest {
            version: "2026-11-03".into(),
            url: "https://example.invalid/megabase.sqlite.zst".into(),
            compressed_size: compressed.len() as u64,
            size: data.len() as u64,
            sha256: sha,
            games: 3,
            count_url: None,
        }
    }

    #[test]
    fn manifest_parses_and_validates() {
        let json = r#"{"version":"2026-11-03","url":"https://www.dropbox.com/x?dl=1","compressedSize":10,
            "size":20,"sha256":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","games":5}"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert!(m.validate().is_ok());
        assert_eq!(m.count_url, None);
        let bad = Manifest { url: "http://insecure".into(), ..m.clone() };
        assert_eq!(bad.validate().unwrap_err().code, "manifestInvalid");
        let bad = Manifest { sha256: "zz".into(), ..m };
        assert_eq!(bad.validate().unwrap_err().code, "manifestInvalid");
    }

    #[test]
    fn verify_then_decompress_restores_the_original() {
        let dir = temp_dir();
        let data: Vec<u8> = (0..300_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let compressed = zstd::encode_all(&data[..], 3).unwrap();
        let manifest = manifest_for(&data, &compressed);
        let archive = dir.join(ARCHIVE_PART);
        let output = dir.join(OUTPUT_PART);
        fs::write(&archive, &compressed).unwrap();
        let mut stages = Vec::new();
        let mut on_progress = |p: Progress| stages.push(p.stage);
        let never = || false;
        assert!(verify(&manifest, &archive, &mut on_progress, &never).unwrap());
        assert!(decompress(&manifest, &archive, &output, &mut on_progress, &never).unwrap());
        assert_eq!(fs::read(&output).unwrap(), data);
        assert!(stages.contains(&"verify") && stages.contains(&"decompress"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_archive_is_rejected_and_removed() {
        let dir = temp_dir();
        let data = b"chesscraft".repeat(1000);
        let compressed = zstd::encode_all(&data[..], 3).unwrap();
        let manifest = manifest_for(&data, &compressed);
        let archive = dir.join(ARCHIVE_PART);
        let mut corrupted = compressed.clone();
        corrupted[10] ^= 0xff;
        fs::write(&archive, &corrupted).unwrap();
        let err = verify(&manifest, &archive, &mut |_| {}, &|| false).unwrap_err();
        assert_eq!(err.code, "checksum");
        assert!(!archive.exists());
        fs::remove_dir_all(&dir).ok();
    }

    /// Mini serveur HTTP local : repond une fois, en honorant l'en-tete
    /// Range si `honor_range`, et renvoie l'en-tete Range recu.
    fn serve_once(body: Vec<u8>, honor_range: bool) -> (String, std::thread::JoinHandle<Option<String>>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/megabase.sqlite.zst", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut range = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    range = Some(v.trim().trim_end_matches('-').to_string());
                }
            }
            let mut stream = stream;
            let start: usize = if honor_range { range.as_deref().and_then(|r| r.parse().ok()).unwrap_or(0) } else { 0 };
            let status = if honor_range && start > 0 { "206 Partial Content" } else { "200 OK" };
            let part = &body[start..];
            write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len()).unwrap();
            stream.write_all(part).unwrap();
            range
        });
        (url, handle)
    }

    #[test]
    fn download_resumes_from_the_partial_file() {
        let dir = temp_dir();
        let body: Vec<u8> = (0..50_000u32).map(|i| (i % 253) as u8).collect();
        let part = dir.join(ARCHIVE_PART);
        fs::write(&part, &body[..20_000]).unwrap();
        let (url, server) = serve_once(body.clone(), true);
        let manifest = Manifest { url, ..manifest_for(&body, &body) };
        assert!(download(&manifest, &part, &mut |_| {}, &|| false).unwrap());
        assert_eq!(server.join().unwrap().as_deref(), Some("20000"));
        assert_eq!(fs::read(&part).unwrap(), body);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn download_restarts_when_the_server_ignores_the_range() {
        let dir = temp_dir();
        let body: Vec<u8> = (0..50_000u32).map(|i| (i % 241) as u8).collect();
        let part = dir.join(ARCHIVE_PART);
        fs::write(&part, vec![0u8; 20_000]).unwrap();
        let (url, server) = serve_once(body.clone(), false);
        let manifest = Manifest { url, ..manifest_for(&body, &body) };
        assert!(download(&manifest, &part, &mut |_| {}, &|| false).unwrap());
        server.join().unwrap();
        assert_eq!(fs::read(&part).unwrap(), body);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prepare_refuses_to_overwrite_an_existing_megabase() {
        let data_dir = temp_dir();
        let target_dir = temp_dir();
        fs::write(target_dir.join(FILE_NAME), b"deja la").unwrap();
        let manifest = manifest_for(b"x", b"y");
        let err = prepare(&data_dir, &target_dir, &manifest).unwrap_err();
        assert_eq!(err.code, "exists");
        assert!(load_pending(&data_dir).is_none());
        fs::remove_dir_all(&data_dir).ok();
        fs::remove_dir_all(&target_dir).ok();
    }

    #[test]
    fn new_addresses_keep_the_partial_download() {
        let data_dir = temp_dir();
        let target_dir = temp_dir();
        let manifest = manifest_for(b"x", b"yy");
        prepare(&data_dir, &target_dir, &manifest).unwrap();
        fs::write(target_dir.join(ARCHIVE_PART), b"y").unwrap();
        // nouveau domaine du site et nouveau lien Dropbox, meme archive
        let moved = Manifest {
            url: "https://dl.example.org/megabase.sqlite.zst".into(),
            count_url: Some("https://nouveau-domaine.example/track.php".into()),
            ..manifest.clone()
        };
        assert!(moved.same_archive(&manifest));
        prepare(&data_dir, &target_dir, &moved).unwrap();
        assert_eq!(pending_status(&data_dir).unwrap().downloaded, 1);
        // nouvelle base publiee : l'archive partielle est jetee
        let newer = Manifest { sha256: "ab".repeat(32), ..manifest };
        assert!(!newer.same_archive(&moved));
        prepare(&data_dir, &target_dir, &newer).unwrap();
        assert_eq!(pending_status(&data_dir).unwrap().downloaded, 0);
        fs::remove_dir_all(&data_dir).ok();
        fs::remove_dir_all(&target_dir).ok();
    }

    #[test]
    fn prepare_remembers_the_download_and_discard_forgets_it() {
        let data_dir = temp_dir();
        let target_dir = temp_dir();
        let manifest = manifest_for(b"x", b"y");
        prepare(&data_dir, &target_dir, &manifest).unwrap();
        fs::write(target_dir.join(ARCHIVE_PART), b"y").unwrap();
        let status = pending_status(&data_dir).unwrap();
        assert_eq!(status.downloaded, 1);
        assert_eq!(status.manifest, manifest);
        discard(&data_dir);
        assert!(pending_status(&data_dir).is_none());
        assert!(!target_dir.join(ARCHIVE_PART).exists());
        fs::remove_dir_all(&data_dir).ok();
        fs::remove_dir_all(&target_dir).ok();
    }
}
