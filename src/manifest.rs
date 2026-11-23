//! Hashing / manifest engine. No GUI dependencies.
//!
//! The manifest format is the one `sha256sum` uses: `<64 hex chars>  <relative/path>`
//! (two spaces = text mode, space + `*` = binary mode), one entry per line.

use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use walkdir::WalkDir;

pub const MANIFEST: &str = "MANIFEST.sha256";
/// The manifest is written here first and renamed, so a crash never leaves a truncated manifest.
const MANIFEST_TMP: &str = "MANIFEST.sha256.tmp";
const CHUNK: usize = 4 * 1024 * 1024;

/// Called with the number of bytes just read.
pub type OnBytes<'a> = &'a (dyn Fn(u64) + Sync);
/// Called once, before hashing starts, with the total number of bytes that will be hashed.
pub type OnTotal<'a> = &'a (dyn Fn(u64) + Sync);

#[derive(Debug)]
pub enum JobResult {
    Generate {
        /// Files that were hashed and written to the manifest.
        files: usize,
        unreadable: Vec<String>,
        /// Symbolic links are never followed or recorded; this is how many were skipped.
        symlinks: usize,
    },
    Validate {
        checked: usize,
        bad: Vec<String>,
        missing: Vec<String>,
        extra: Vec<String>,
        unreadable: Vec<String>,
        symlinks: usize,
    },
}

/// A sensible default for the hashing thread pool: all cores, capped because hashing is
/// usually disk-bound and many threads thrash spinning disks.
pub fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().clamp(1, 8))
}

/// Result of walking a folder.
#[derive(Debug, Default)]
pub struct Scan {
    /// Regular files: (relative path with '/' separators, size), sorted, excluding the manifest.
    pub files: Vec<(String, u64)>,
    /// Paths that could not be listed or whose names cannot be stored in a UTF-8 manifest.
    /// An unreadable directory is listed by its own path.
    pub unreadable: Vec<String>,
    /// Number of symbolic links skipped.
    pub symlinks: usize,
}

fn rel_lossy(root: &Path, p: &Path) -> String {
    let rel = p.strip_prefix(root).unwrap_or(p);
    let s = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    if s.is_empty() {
        ".".to_string()
    } else {
        s
    }
}

/// Relative path with '/' separators, or `None` if any component is not valid UTF-8.
fn rel_utf8(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

/// Walk `root`, recording everything that could not be read instead of silently dropping it.
pub fn scan(root: &Path) -> Scan {
    let mut out = Scan::default();
    for entry in WalkDir::new(root).follow_links(false) {
        let e = match entry {
            Ok(e) => e,
            Err(err) => {
                if let Some(p) = err.path() {
                    out.unreadable.push(rel_lossy(root, p));
                }
                continue;
            }
        };
        let ft = e.file_type();
        if ft.is_symlink() {
            out.symlinks += 1;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Some(rel) = rel_utf8(root, e.path()) else {
            out.unreadable.push(rel_lossy(root, e.path()));
            continue;
        };
        if rel == MANIFEST || rel == MANIFEST_TMP {
            continue;
        }
        match e.metadata() {
            Ok(m) => out.files.push((rel, m.len())),
            Err(_) => out.unreadable.push(rel),
        }
    }
    out.files.sort();
    out.unreadable.sort();
    out.unreadable.dedup();
    out
}

/// All files under `root` as (relative path with '/' separators, size), sorted, excluding the
/// manifest itself. Use [`scan`] if you also need to know what could not be read.
pub fn list_files(root: &Path) -> Vec<(String, u64)> {
    scan(root).files
}

fn hex(bytes: &[u8]) -> String {
    const T: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(T[(b >> 4) as usize] as char);
        s.push(T[(b & 15) as usize] as char);
    }
    s
}

fn hash_file(path: &Path, buf: &mut [u8], on_bytes: OnBytes) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    loop {
        let n = f.read(buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        on_bytes(n as u64);
    }
    Ok(hex(&h.finalize()))
}

/// Hash many files on a pool of `workers` threads. `None` = could not be read.
fn hash_all(
    root: &Path,
    rels: &[String],
    workers: usize,
    on_bytes: OnBytes,
) -> HashMap<String, Option<String>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .expect("failed to build thread pool");
    pool.install(|| {
        rels.par_iter()
            // One read buffer per rayon work unit instead of one per file.
            .map_init(
                || vec![0u8; CHUNK],
                |buf, r| (r.clone(), hash_file(&root.join(r), buf, on_bytes).ok()),
            )
            .collect()
    })
}

pub fn generate(root: &Path, workers: usize, on_bytes: OnBytes) -> io::Result<JobResult> {
    generate_with(root, workers, &|_| {}, on_bytes)
}

/// Like [`generate`], but reports the total byte count up front (from the same walk that
/// drives the job, so the folder is only traversed once).
pub fn generate_with(
    root: &Path,
    workers: usize,
    on_total: OnTotal,
    on_bytes: OnBytes,
) -> io::Result<JobResult> {
    let scan = scan(root);
    on_total(scan.files.iter().map(|(_, s)| *s).sum());
    let rels: Vec<String> = scan.files.into_iter().map(|(r, _)| r).collect();
    let hashes = hash_all(root, &rels, workers, on_bytes);

    let tmp = root.join(MANIFEST_TMP);
    let mut unreadable = scan.unreadable;
    let mut written = 0usize;
    let write_result = (|| -> io::Result<()> {
        let mut w = BufWriter::new(File::create(&tmp)?);
        for r in &rels {
            match &hashes[r] {
                Some(h) => {
                    writeln!(w, "{h}  {r}")?; // same format as `sha256sum`
                    written += 1;
                }
                None => unreadable.push(r.clone()),
            }
        }
        w.flush()?;
        w.get_ref().sync_all()?;
        drop(w);
        fs::rename(&tmp, root.join(MANIFEST))
    })();
    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    unreadable.sort();
    unreadable.dedup();
    Ok(JobResult::Generate {
        files: written,
        unreadable,
        symlinks: scan.symlinks,
    })
}

fn bad_manifest(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// `<64 hex>  <path>` (text mode) or `<64 hex> *<path>` (binary mode).
/// The hash has a fixed width, so paths containing spaces are unambiguous.
fn parse_line(line: &str) -> Option<(String, &str)> {
    let hash = line.get(..64)?;
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rest = line.get(64..)?;
    let path = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))?;
    if path.is_empty() {
        return None;
    }
    Some((hash.to_ascii_lowercase(), path))
}

/// Parse a manifest strictly: malformed lines and conflicting duplicates are errors, because
/// silently ignoring them would make a damaged manifest look like "every file is extra".
fn parse_manifest(reader: impl BufRead) -> io::Result<HashMap<String, String>> {
    let mut expected: HashMap<String, String> = HashMap::new();
    let mut malformed: Vec<usize> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();

    for (i, raw) in reader.split(b'\n').enumerate() {
        let n = i + 1;
        let Ok(mut line) = String::from_utf8(raw?) else {
            malformed.push(n);
            continue;
        };
        if n == 1 {
            if let Some(stripped) = line.strip_prefix('\u{feff}') {
                line = stripped.to_string(); // UTF-8 BOM from some Windows editors
            }
        }
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line) {
            Some((hash, path)) => match expected.get(path) {
                Some(existing) if *existing != hash => conflicts.push(path.to_string()),
                Some(_) => {} // identical duplicate: harmless
                None => {
                    expected.insert(path.to_string(), hash);
                }
            },
            None => malformed.push(n),
        }
    }

    if !malformed.is_empty() {
        let shown: Vec<String> = malformed.iter().take(3).map(|n| n.to_string()).collect();
        return Err(bad_manifest(format!(
            "{MANIFEST} is not a valid SHA-256 manifest: {} malformed line{} (line {}{}).",
            malformed.len(),
            if malformed.len() == 1 { "" } else { "s" },
            shown.join(", "),
            if malformed.len() > 3 { ", ..." } else { "" },
        )));
    }
    if !conflicts.is_empty() {
        conflicts.sort();
        conflicts.dedup();
        return Err(bad_manifest(format!(
            "{MANIFEST} lists the same file with different hashes: {}",
            conflicts.join(", ")
        )));
    }
    Ok(expected)
}

/// Is `path` equal to, or inside, one of the unreadable entries?
fn covered_by(unreadable: &[String], path: &str) -> bool {
    unreadable.iter().any(|u| {
        u == "."
            || path == u
            || path
                .strip_prefix(u.as_str())
                .is_some_and(|r| r.starts_with('/'))
    })
}

pub fn validate(root: &Path, workers: usize, on_bytes: OnBytes) -> io::Result<JobResult> {
    validate_with(root, workers, &|_| {}, on_bytes)
}

/// Like [`validate`], but reports the total number of bytes that will be hashed up front.
/// Only files that are in the manifest and on disk are hashed, so extra files do not count.
pub fn validate_with(
    root: &Path,
    workers: usize,
    on_total: OnTotal,
    on_bytes: OnBytes,
) -> io::Result<JobResult> {
    let f = File::open(root.join(MANIFEST)).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("No {MANIFEST} found in that folder."),
        )
    })?;
    let expected = parse_manifest(BufReader::new(f))?;

    let scan = scan(root);
    let present: HashMap<&str, u64> = scan.files.iter().map(|(r, s)| (r.as_str(), *s)).collect();

    // Entries under a folder we could not read are "unreadable", not "missing".
    let mut missing: Vec<String> = expected
        .keys()
        .filter(|r| !present.contains_key(r.as_str()) && !covered_by(&scan.unreadable, r))
        .cloned()
        .collect();
    let mut extra: Vec<String> = present
        .keys()
        .filter(|r| !expected.contains_key(**r))
        .map(|r| r.to_string())
        .collect();
    let mut to_check: Vec<String> = expected
        .keys()
        .filter(|r| present.contains_key(r.as_str()))
        .cloned()
        .collect();
    missing.sort();
    extra.sort();
    to_check.sort();

    on_total(to_check.iter().map(|r| present[r.as_str()]).sum());
    let hashes = hash_all(root, &to_check, workers, on_bytes);

    let mut bad = Vec::new();
    let mut unreadable = scan.unreadable;
    for r in &to_check {
        match &hashes[r] {
            Some(h) if *h == expected[r] => {}
            Some(_) => bad.push(r.clone()),
            None => unreadable.push(r.clone()),
        }
    }
    unreadable.sort();
    unreadable.dedup();
    Ok(JobResult::Validate {
        checked: to_check.len(),
        bad,
        missing,
        extra,
        unreadable,
        symlinks: scan.symlinks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("hm-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("sub dir")).unwrap();
        p
    }

    #[test]
    fn known_hash_and_roundtrip() {
        let d = tmp("rt");
        fs::write(d.join("a.txt"), b"abc").unwrap();
        fs::write(d.join("sub dir/b.bin"), vec![7u8; 9_000_000]).unwrap();
        let noop = |_n: u64| {};

        generate(&d, 2, &noop).unwrap();
        let m = fs::read_to_string(d.join(MANIFEST)).unwrap();
        assert!(
            m.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.txt")
        );
        assert!(m.contains("  sub dir/b.bin"));
        assert!(!m.contains(MANIFEST));

        match validate(&d, 2, &noop).unwrap() {
            JobResult::Validate {
                checked,
                bad,
                missing,
                extra,
                ..
            } => {
                assert_eq!(checked, 2);
                assert!(bad.is_empty() && missing.is_empty() && extra.is_empty());
            }
            _ => panic!(),
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn detects_bad_missing_extra() {
        let d = tmp("bad");
        fs::write(d.join("a.txt"), b"abc").unwrap();
        fs::write(d.join("b.txt"), b"def").unwrap();
        let noop = |_n: u64| {};
        generate(&d, 1, &noop).unwrap();

        fs::write(d.join("a.txt"), b"abd").unwrap();
        fs::remove_file(d.join("b.txt")).unwrap();
        fs::write(d.join("new.txt"), b"x").unwrap();

        match validate(&d, 1, &noop).unwrap() {
            JobResult::Validate {
                bad,
                missing,
                extra,
                ..
            } => {
                assert_eq!(bad, vec!["a.txt"]);
                assert_eq!(missing, vec!["b.txt"]);
                assert_eq!(extra, vec!["new.txt"]);
            }
            _ => panic!(),
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_manifest_is_an_error() {
        let d = tmp("nom");
        assert!(validate(&d, 1, &|_| {}).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}
