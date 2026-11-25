//! `packaging/release.sh cut`, run from a copy of the repo's packaging/
//! against tests/release/stand-in for gh, git and the build's programs: a
//! poll GitHub's API fails is tried again, and a cut that ends early cancels
//! its run before it takes its tag away.
use std::path::PathBuf;
use std::process::Command;

/// A cut of v1.2.3 whose `gh run view` answers `views` in turn, with `env`:
/// its exit code, what it printed (stdout, then stderr), and the argv each stand-in was run with.
fn cut(name: &str, views: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let root = std::env::temp_dir()
        .join(format!("stablehand-release-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    let (repo, bin, state) = (root.join("repo"), root.join("bin"), root.join("state"));
    for d in [repo.join("packaging"), bin.clone(), state.clone()] {
        std::fs::create_dir_all(d).unwrap();
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = repo.join("packaging/release.sh");
    std::fs::copy(here.join("packaging/release.sh"), &script).unwrap();
    for tool in ["gh", "git", "rustup", "cargo", "lipo", "ruby", "sleep"] {
        std::os::unix::fs::symlink(here.join("tests/release/stand-in"), bin.join(tool)).unwrap();
    }
    let lines: String = views.iter().map(|v| format!("{v}\n")).collect();
    std::fs::write(state.join("views"), lines).unwrap();
    let log = root.join("argv.log");
    let out = Command::new(&script)
        .args(["cut", "v1.2.3"])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &root)
        .env("FAKE_STATE", &state)
        .env("FAKE_ARGV_LOG", &log)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let argv = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !state.join("tag").exists(),
        "the tag is left on origin:\n{argv}"
    );
    let said = String::from_utf8([out.stdout, out.stderr].concat()).unwrap();
    (out.status.code().unwrap_or(-1), said, argv)
}

#[test]
fn a_run_view_failing_twice_with_http_500_is_polled_again_until_the_run_succeeds() {
    let (code, said, argv) = cut("http-500", &["500", "500", "completed success"], &[]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.contains("release: stablehand v1.2.3 is published"),
        "{said}"
    );
    assert_eq!(argv.matches("gh run view 42").count(), 3, "{argv}");
    assert!(!argv.contains("gh run cancel"), "{argv}");
}

#[test]
fn a_run_still_queued_at_the_limit_is_cancelled_before_the_tag_is_taken_away() {
    let (code, said, argv) = cut("queued", &["queued "], &[("RELEASE_WAIT_LIMIT", "0")]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("still 'queued '"), "{said}");
    let cancel = argv.find("gh run cancel 42 -R sunfmin/homebrew-tap");
    let untag = argv.find("git push --quiet origin :refs/tags/v1.2.3");
    assert!(cancel.is_some() && cancel < untag, "{argv}");
}
