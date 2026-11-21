use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use walkdir::WalkDir;

pub const MANIFEST: &str = "MANIFEST.sha256";
const CHUNK: usize = 4 * 1024 * 1024;

pub type OnBytes<'a> = &'a (dyn Fn(u64) + Sync);

#[derive(Serialize, Debug)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum JobResult {
    Generate {
        files: usize,
        unreadable: Vec<String>,
    },
    Validate {
        checked: usize,
        bad: Vec<String>,
        missing: Vec<String>,
        extra: Vec<String>,
        unreadable: Vec<String>,
    },
}

/// All files under `root` as (relative path with '/' separators, size), sorted, excluding the manifest itself.
pub fn list_files(root: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for e in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !e.file_type().is_file() {
            continue;
        }
        let rel = match e.path().strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if rel == MANIFEST {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push((rel, size));
    }
    out.sort();
    out
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

fn hash_file(path: &Path, on_bytes: OnBytes) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
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
            .map(|r| (r.clone(), hash_file(&root.join(r), on_bytes).ok()))
            .collect()
    })
}

pub fn generate(root: &Path, workers: usize, on_bytes: OnBytes) -> io::Result<JobResult> {
    let rels: Vec<String> = list_files(root).into_iter().map(|(r, _)| r).collect();
    let hashes = hash_all(root, &rels, workers, on_bytes);

    let mut w = BufWriter::new(File::create(root.join(MANIFEST))?);
    let mut unreadable = Vec::new();
    for r in &rels {
        match &hashes[r] {
            Some(h) => writeln!(w, "{h}  {r}")?, // same format as `sha256sum`
            None => unreadable.push(r.clone()),
        }
    }
    w.flush()?;
    Ok(JobResult::Generate {
        files: rels.len(),
        unreadable,
    })
}

pub fn validate(root: &Path, workers: usize, on_bytes: OnBytes) -> io::Result<JobResult> {
    let f = File::open(root.join(MANIFEST)).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("No {MANIFEST} found in that folder."),
        )
    })?;

    let mut expected: HashMap<String, String> = HashMap::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // "hash  path" (text mode) or "hash *path" (binary mode)
        if let Some((h, p)) = line.split_once("  ").or_else(|| line.split_once(" *")) {
            expected.insert(p.to_string(), h.to_lowercase());
        }
    }

    let present: HashMap<String, u64> = list_files(root).into_iter().collect();
    let mut missing: Vec<String> = expected
        .keys()
        .filter(|r| !present.contains_key(*r))
        .cloned()
        .collect();
    let mut extra: Vec<String> = present
        .keys()
        .filter(|r| !expected.contains_key(*r))
        .cloned()
        .collect();
    let mut to_check: Vec<String> = expected
        .keys()
        .filter(|r| present.contains_key(*r))
        .cloned()
        .collect();
    missing.sort();
    extra.sort();
    to_check.sort();

    let hashes = hash_all(root, &to_check, workers, on_bytes);
    let mut bad = Vec::new();
    let mut unreadable = Vec::new();
    for r in &to_check {
        match &hashes[r] {
            Some(h) if *h == expected[r] => {}
            Some(_) => bad.push(r.clone()),
            None => unreadable.push(r.clone()),
        }
    }
    Ok(JobResult::Validate {
        checked: to_check.len(),
        bad,
        missing,
        extra,
        unreadable,
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
        assert!(m.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.txt"));
        assert!(m.contains("  sub dir/b.bin"));
        assert!(!m.contains(MANIFEST));

        match validate(&d, 2, &noop).unwrap() {
            JobResult::Validate { checked, bad, missing, extra, .. } => {
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
            JobResult::Validate { bad, missing, extra, .. } => {
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
