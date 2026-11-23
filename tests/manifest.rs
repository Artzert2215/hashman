//! Integration tests for the hashing / manifest engine.
//!
//! These exercise only the public API (`hashman::manifest`), so they need no
//! display server or GUI libraries and run with `cargo test --no-default-features`.
//!

use hashman::manifest::{
    generate, generate_with, list_files, validate, validate_with, JobResult, MANIFEST,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

// --- helpers -----------------------------------------------------------------

fn noop(_: u64) {}

fn write(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}

fn read_manifest(root: &Path) -> String {
    fs::read_to_string(root.join(MANIFEST)).unwrap()
}

fn sha_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Run `validate` and unpack the result, panicking on the wrong variant.
struct Report {
    checked: usize,
    bad: Vec<String>,
    missing: Vec<String>,
    extra: Vec<String>,
    unreadable: Vec<String>,
	#[cfg_attr(not(unix), allow(dead_code))] // atm only used by unix tests
    symlinks: usize,
}

fn run_validate(root: &Path) -> Report {
    match validate(root, 2, &noop).expect("validate returned an error") {
        JobResult::Validate {
            checked,
            bad,
            missing,
            extra,
            unreadable,
            symlinks,
        } => Report {
            checked,
            bad,
            missing,
            extra,
            unreadable,
            symlinks,
        },
        other => panic!("expected Validate result, got {other:?}"),
    }
}

fn assert_clean(r: &Report) {
    assert!(
        r.bad.is_empty() && r.missing.is_empty() && r.extra.is_empty() && r.unreadable.is_empty(),
        "expected clean report, got bad={:?} missing={:?} extra={:?} unreadable={:?}",
        r.bad,
        r.missing,
        r.extra,
        r.unreadable
    );
}

// --- list_files --------------------------------------------------------------

#[test]
fn list_files_is_sorted_recursive_and_uses_forward_slashes() {
    let d = TempDir::new().unwrap();
    write(d.path(), "b.txt", b"1");
    write(d.path(), "a/z.txt", b"22");
    write(d.path(), "a/b/c.txt", b"333");
    fs::create_dir_all(d.path().join("empty_dir")).unwrap();

    let files = list_files(d.path());
    assert_eq!(
        files,
        vec![
            ("a/b/c.txt".to_string(), 3),
            ("a/z.txt".to_string(), 2),
            ("b.txt".to_string(), 1),
        ]
    );
}

#[test]
fn list_files_excludes_only_the_root_manifest() {
    let d = TempDir::new().unwrap();
    write(d.path(), MANIFEST, b"x");
    write(d.path(), "sub/MANIFEST.sha256", b"y"); // nested manifests are ordinary files
    let names: Vec<_> = list_files(d.path()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["sub/MANIFEST.sha256"]);
}

// --- generate ----------------------------------------------------------------

#[test]
fn generate_writes_sha256sum_compatible_lines() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    write(d.path(), "empty.bin", b"");

    generate(d.path(), 2, &noop).unwrap();

    let m = read_manifest(d.path());
    assert_eq!(
        m,
        format!("{ABC_SHA256}  a.txt\n{EMPTY_SHA256}  empty.bin\n"),
        "format must be `<hash>  <path>\\n`, sorted, so `sha256sum -c` accepts it"
    );
}

#[test]
fn generate_on_empty_folder_creates_empty_manifest_and_validates() {
    let d = TempDir::new().unwrap();
    match generate(d.path(), 1, &noop).unwrap() {
        JobResult::Generate {
            files, unreadable, ..
        } => {
            assert_eq!(files, 0);
            assert!(unreadable.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(read_manifest(d.path()), "");
    let r = run_validate(d.path());
    assert_eq!(r.checked, 0);
    assert_clean(&r);
}

#[test]
fn generate_is_idempotent_and_never_hashes_its_own_manifest() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    write(d.path(), "dir/b.txt", b"def");

    generate(d.path(), 2, &noop).unwrap();
    let first = read_manifest(d.path());
    generate(d.path(), 2, &noop).unwrap();
    let second = read_manifest(d.path());

    assert_eq!(first, second);
    assert!(!second.contains(MANIFEST));
}

#[test]
fn generate_overwrites_a_stale_manifest() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(d.path().join(MANIFEST), "garbage that must disappear\n").unwrap();

    generate(d.path(), 1, &noop).unwrap();

    assert_eq!(read_manifest(d.path()), format!("{ABC_SHA256}  a.txt\n"));
}

#[test]
fn generate_handles_odd_but_valid_filenames() {
    let d = TempDir::new().unwrap();
    write(d.path(), "with space.txt", b"abc");
    write(d.path(), "ünïcödé/日本語.txt", b"abc");
    write(d.path(), "dir with  two spaces/f.txt", b"abc");

    generate(d.path(), 2, &noop).unwrap();
    assert_clean(&run_validate(d.path()));
}

#[test]
fn generate_is_the_same_for_any_worker_count() {
    let d = TempDir::new().unwrap();
    for i in 0..40 {
        write(
            d.path(),
            &format!("d{}/f{i}.bin", i % 5),
            &vec![i as u8; 1000 + i],
        );
    }
    generate(d.path(), 1, &noop).unwrap();
    let one = read_manifest(d.path());
    generate(d.path(), 8, &noop).unwrap();
    let eight = read_manifest(d.path());
    // 0 workers is clamped to 1 rather than panicking
    generate(d.path(), 0, &noop).unwrap();
    let zero = read_manifest(d.path());
    assert_eq!(one, eight);
    assert_eq!(one, zero);
}

// --- hashing correctness at chunk boundaries -----------------------------------

#[test]
fn hashes_match_reference_around_the_4mib_chunk_boundary() {
    const CHUNK: usize = 4 * 1024 * 1024;
    let d = TempDir::new().unwrap();
    let sizes = [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK + 17];
    let mut expected = Vec::new();
    for (i, n) in sizes.iter().enumerate() {
        // non-constant content so an off-by-one in the read loop changes the hash
        let data: Vec<u8> = (0..*n).map(|j| (j.wrapping_mul(31) ^ i) as u8).collect();
        write(d.path(), &format!("f{i}.bin"), &data);
        expected.push(format!("{}  f{i}.bin", sha_hex(&data)));
    }
    generate(d.path(), 3, &noop).unwrap();
    let got: Vec<String> = read_manifest(d.path()).lines().map(String::from).collect();
    assert_eq!(got, expected);
}

#[test]
fn progress_callback_reports_exactly_the_bytes_read() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a", &vec![1u8; 5_000_000]);
    write(d.path(), "b/c", &[2u8; 123]);
    write(d.path(), "empty", b"");

    let seen = AtomicU64::new(0);
    let cb = |n: u64| {
        seen.fetch_add(n, Ordering::Relaxed);
    };
    generate(d.path(), 2, &cb).unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 5_000_123);

    seen.store(0, Ordering::Relaxed);
    run_validate_with(d.path(), &cb);
    assert_eq!(seen.load(Ordering::Relaxed), 5_000_123);
}

fn run_validate_with(root: &Path, cb: &(dyn Fn(u64) + Sync)) {
    validate(root, 2, cb).unwrap();
}

// --- validate ----------------------------------------------------------------

#[test]
fn validate_reports_bad_missing_and_extra_independently() {
    let d = TempDir::new().unwrap();
    write(d.path(), "ok.txt", b"ok");
    write(d.path(), "changed.txt", b"before");
    write(d.path(), "gone.txt", b"bye");
    generate(d.path(), 2, &noop).unwrap();

    write(d.path(), "changed.txt", b"after");
    fs::remove_file(d.path().join("gone.txt")).unwrap();
    write(d.path(), "sub/new.txt", b"n");

    let r = run_validate(d.path());
    assert_eq!(r.checked, 2, "ok.txt + changed.txt are present and checked");
    assert_eq!(r.bad, vec!["changed.txt"]);
    assert_eq!(r.missing, vec!["gone.txt"]);
    assert_eq!(r.extra, vec!["sub/new.txt"]);
    assert!(r.unreadable.is_empty());
}

#[test]
fn validate_detects_single_bit_flip_in_a_large_file() {
    let d = TempDir::new().unwrap();
    let mut data = vec![0u8; 9_000_000];
    write(d.path(), "big.bin", &data);
    generate(d.path(), 2, &noop).unwrap();
    data[8_999_999] ^= 1;
    write(d.path(), "big.bin", &data);
    assert_eq!(run_validate(d.path()).bad, vec!["big.bin"]);
}

#[test]
fn validate_without_manifest_is_a_clear_error() {
    let d = TempDir::new().unwrap();
    let err = validate(d.path(), 1, &noop).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(err.to_string().contains(MANIFEST));
}

#[test]
fn validate_accepts_foreign_formatting() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    write(d.path(), "b.txt", b"abc");
    write(d.path(), "c.txt", b"abc");
    let upper = ABC_SHA256.to_uppercase();
    // CRLF line endings, comment, blank line, UPPERCASE hex, and binary-mode (` *`) entry
    let manifest = format!(
        "# made on windows\r\n\r\n{upper}  a.txt\r\n{ABC_SHA256} *b.txt\r\n{ABC_SHA256}  c.txt\r\n"
    );
    fs::write(d.path().join(MANIFEST), manifest).unwrap();

    let r = run_validate(d.path());
    assert_eq!(r.checked, 3);
    assert_clean(&r);
}

#[test]
fn validate_never_reads_outside_the_root() {
    let outer = TempDir::new().unwrap();
    let root = outer.path().join("root");
    fs::create_dir_all(&root).unwrap();
    write(outer.path(), "secret.txt", b"abc");
    write(&root, "a.txt", b"abc");
    fs::write(
        root.join(MANIFEST),
        format!("{ABC_SHA256}  a.txt\n{ABC_SHA256}  ../secret.txt\n"),
    )
    .unwrap();

    let r = run_validate(&root);
    assert_eq!(r.checked, 1, "the traversal entry must not be hashed");
    assert_eq!(r.missing, vec!["../secret.txt"]);
}

#[test]
fn validate_ignores_nested_manifest_names_only_at_root() {
    let d = TempDir::new().unwrap();
    write(d.path(), "sub/MANIFEST.sha256", b"inner");
    generate(d.path(), 1, &noop).unwrap();
    assert!(read_manifest(d.path()).contains("  sub/MANIFEST.sha256"));
    assert_clean(&run_validate(d.path()));
}

// --- platform specific -----------------------------------------------------------

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_and_are_counted() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    write(d.path(), "real.txt", b"abc");
    symlink(d.path().join("real.txt"), d.path().join("link.txt")).unwrap();
    symlink(d.path(), d.path().join("loop")).unwrap(); // must not recurse forever

    generate(d.path(), 1, &noop).unwrap();
    // Symlinks are never followed or recorded...
    assert_eq!(read_manifest(d.path()), format!("{ABC_SHA256}  real.txt\n"));
    // ...but they are counted so the UI can say they were skipped.
    match generate(d.path(), 1, &noop).unwrap() {
        JobResult::Generate { symlinks, .. } => assert_eq!(symlinks, 2),
        other => panic!("{other:?}"),
    }
    assert_eq!(run_validate(d.path()).symlinks, 2);
}

#[test]
fn malformed_manifest_lines_are_reported() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(
        d.path().join(MANIFEST),
        format!("{ABC_SHA256}  a.txt\nthis line is not valid\nnothex  b.txt\n"),
    )
    .unwrap();
    assert!(
        validate(d.path(), 1, &noop).is_err(),
        "validate should fail (or report) on malformed manifest lines"
    );
}

/// chmod 000 a directory; returns `None` (test should be skipped) when the current user can
/// still read it anyway, i.e. when running as root.
#[cfg(unix)]
fn lock_dir(dir: &Path) -> Option<impl FnOnce()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(dir).is_ok() {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: running as a user that ignores directory permissions (root?)");
        return None;
    }
    let dir = dir.to_path_buf();
    Some(move || fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap())
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_reported_not_silently_skipped() {
	use hashman::manifest::scan;
    let d = TempDir::new().unwrap();
    write(d.path(), "ok.txt", b"abc");
    write(d.path(), "locked/secret.txt", b"abc");
    let Some(unlock) = lock_dir(&d.path().join("locked")) else {
        return;
    };

    let result = generate(d.path(), 1, &noop);
    let s = scan(d.path());
    unlock();

    match result.unwrap() {
        JobResult::Generate {
            files, unreadable, ..
        } => {
            assert_eq!(files, 1);
            assert_eq!(unreadable, vec!["locked"]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.unreadable, vec!["locked"]);
}

#[cfg(unix)]
#[test]
fn files_under_an_unreadable_directory_are_unreadable_not_missing() {
    let d = TempDir::new().unwrap();
    write(d.path(), "ok.txt", b"abc");
    write(d.path(), "locked/secret.txt", b"abc");
    generate(d.path(), 1, &noop).unwrap();
    let Some(unlock) = lock_dir(&d.path().join("locked")) else {
        return;
    };

    let r = run_validate(d.path());
    unlock();

    assert!(r.missing.is_empty(), "missing: {:?}", r.missing);
    assert_eq!(r.unreadable, vec!["locked"]);
    assert_eq!(r.checked, 1);
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn file_names_that_are_not_utf8_are_reported_not_mangled() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let d = TempDir::new().unwrap();
    write(d.path(), "ok.txt", b"abc");
    let bad_name = OsStr::from_bytes(b"bad-\xff-name.txt");
    fs::write(d.path().join(bad_name), b"abc").unwrap();

    match generate(d.path(), 1, &noop).unwrap() {
        JobResult::Generate {
            files, unreadable, ..
        } => {
            assert_eq!(files, 1);
            assert_eq!(unreadable.len(), 1);
            assert!(unreadable[0].starts_with("bad-"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(read_manifest(d.path()), format!("{ABC_SHA256}  ok.txt\n"));
}

// --- strict manifest parsing -------------------------------------------------------

#[test]
fn malformed_manifest_error_names_the_line() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(
        d.path().join(MANIFEST),
        format!("{ABC_SHA256}  a.txt\nfine? no\n"),
    )
    .unwrap();
    let err = validate(d.path(), 1, &noop).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("line 2"), "{err}");
}

#[test]
fn malformed_variants_are_all_rejected() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    let short = &ABC_SHA256[..63];
    let long = format!("{ABC_SHA256}0");
    let nonhex = format!("{}g", &ABC_SHA256[..63]);
    for bad in [
        format!("{short}  a.txt"),     // hash too short
        format!("{long}  a.txt"),      // hash too long
        format!("{nonhex}  a.txt"),    // not hex
        format!("{ABC_SHA256} a.txt"), // single space, no `*`
        format!("{ABC_SHA256}  "),     // no path
        ABC_SHA256.to_string(),        // no separator
    ] {
        fs::write(d.path().join(MANIFEST), format!("{bad}\n")).unwrap();
        assert!(validate(d.path(), 1, &noop).is_err(), "accepted: {bad:?}");
    }
}

#[test]
fn non_utf8_manifest_is_an_error_not_a_crash() {
    let d = TempDir::new().unwrap();
    fs::write(d.path().join(MANIFEST), b"\xff\xfe\xfd\n").unwrap();
    assert!(validate(d.path(), 1, &noop).is_err());
}

#[test]
fn identical_duplicate_entries_are_harmless() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(
        d.path().join(MANIFEST),
        format!("{ABC_SHA256}  a.txt\n{ABC_SHA256}  a.txt\n"),
    )
    .unwrap();
    assert_clean(&run_validate(d.path()));
}

#[test]
fn conflicting_duplicate_entries_are_an_error() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(
        d.path().join(MANIFEST),
        format!("{EMPTY_SHA256}  a.txt\n{ABC_SHA256}  a.txt\n"),
    )
    .unwrap();
    let err = validate(d.path(), 1, &noop).unwrap_err();
    assert!(err.to_string().contains("a.txt"), "{err}");
}

#[test]
fn binary_mode_entry_with_double_space_in_path() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a  b.txt", b"abc");
    fs::write(d.path().join(MANIFEST), format!("{ABC_SHA256} *a  b.txt\n")).unwrap();
    let r = run_validate(d.path());
    assert_eq!(r.checked, 1);
    assert_clean(&r);
}

#[test]
fn text_mode_path_starting_with_star() {
    let d = TempDir::new().unwrap();
    write(d.path(), "*star.txt", b"abc");
    fs::write(
        d.path().join(MANIFEST),
        format!("{ABC_SHA256}  *star.txt\n"),
    )
    .unwrap();
    assert_clean(&run_validate(d.path()));
}

#[test]
fn manifest_with_utf8_bom_is_accepted() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(
        d.path().join(MANIFEST),
        format!("\u{feff}{ABC_SHA256}  a.txt\n"),
    )
    .unwrap();
    assert_clean(&run_validate(d.path()));
}

// --- progress totals, atomic writes ------------------------------------------------

#[test]
fn generate_reports_total_before_hashing_and_matches_bytes_read() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a", &vec![1u8; 70_000]);
    write(d.path(), "b/c", &vec![2u8; 30_000]);

    let total = AtomicU64::new(0);
    let done = AtomicU64::new(0);
    let saw_total_first = std::sync::atomic::AtomicBool::new(true);
    generate_with(d.path(), 2, &|n| total.store(n, Ordering::SeqCst), &|n| {
        if total.load(Ordering::SeqCst) == 0 {
            saw_total_first.store(false, Ordering::SeqCst);
        }
        done.fetch_add(n, Ordering::SeqCst);
    })
    .unwrap();
    assert!(saw_total_first.load(Ordering::SeqCst));
    assert_eq!(total.load(Ordering::SeqCst), 100_000);
    assert_eq!(done.load(Ordering::SeqCst), 100_000);
}

#[test]
fn validate_total_counts_only_files_that_are_checked() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a", &vec![1u8; 1000]);
    generate(d.path(), 1, &noop).unwrap();
    write(d.path(), "extra_big", &vec![9u8; 500_000]); // not in manifest: must not be counted

    let total = AtomicU64::new(0);
    let done = AtomicU64::new(0);
    let r = validate_with(d.path(), 1, &|n| total.store(n, Ordering::SeqCst), &|n| {
        done.fetch_add(n, Ordering::SeqCst);
    })
    .unwrap();
    assert!(matches!(r, JobResult::Validate { .. }));
    assert_eq!(total.load(Ordering::SeqCst), 1000);
    assert_eq!(
        done.load(Ordering::SeqCst),
        total.load(Ordering::SeqCst),
        "progress must be able to reach 100%"
    );
}

#[test]
fn generate_leaves_no_temp_file_and_ignores_a_stale_one() {
    let d = TempDir::new().unwrap();
    write(d.path(), "a.txt", b"abc");
    fs::write(d.path().join("MANIFEST.sha256.tmp"), "stale").unwrap();

    generate(d.path(), 1, &noop).unwrap();

    assert!(!d.path().join("MANIFEST.sha256.tmp").exists());
    assert_eq!(read_manifest(d.path()), format!("{ABC_SHA256}  a.txt\n"));
}
