#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use haw_core::manifest::{Manifest, ManifestError};
use haw_core::resolver::{self, ResolveError};

const EXAMPLE: &str = r#"
[remote.internal]
url = "git@gitlab.company.com:firmware"

[remote.github]
url = "git@github.com:acme"

[repo.kernel]
remote = "internal"
repo = "kernel.git"
rev = "v6.1.2"
groups = ["firmware"]

[repo.hal]
remote = "internal"
repo = "hal.git"
rev = "main"
groups = ["firmware"]

[repo.app-mqtt]
remote = "github"
repo = "app-mqtt.git"
rev = "release/2.x"
path = "apps/mqtt"

[stack.gateway]
repos = ["kernel", "hal", "app-mqtt"]

[stack.sensor-node]
repos = ["kernel", "hal"]

[overlay.dev.repo.kernel]
rev = "main"
"#;

#[test]
fn parses_the_reference_example() {
    let manifest: Manifest = EXAMPLE.parse().unwrap();
    assert_eq!(manifest.remotes.len(), 2);
    assert_eq!(manifest.repos.len(), 3);
    assert_eq!(manifest.stacks.len(), 2);
    assert_eq!(manifest.overlays.len(), 1);

    let kernel = &manifest.repos["kernel"];
    assert_eq!(kernel.rev, "v6.1.2");
    assert_eq!(kernel.groups, ["firmware"]);
    assert_eq!(
        kernel.clone_url(&manifest.remotes).unwrap(),
        "git@gitlab.company.com:firmware/kernel.git"
    );
    assert_eq!(kernel.checkout_path("kernel"), PathBuf::from("kernel"));

    let mqtt = &manifest.repos["app-mqtt"];
    assert_eq!(mqtt.checkout_path("app-mqtt"), PathBuf::from("apps/mqtt"));
}

#[test]
fn round_trips_through_toml() {
    let manifest: Manifest = EXAMPLE.parse().unwrap();
    let serialized = toml::to_string(&manifest).unwrap();
    let reparsed: Manifest = serialized.parse().unwrap();
    assert_eq!(manifest, reparsed);
}

#[test]
fn rejects_unknown_remote() {
    let err = r#"
[repo.a]
remote = "nope"
repo = "a.git"
rev = "main"
"#
    .parse::<Manifest>()
    .unwrap_err();
    assert!(matches!(err, ManifestError::UnknownRemote { .. }));
}

#[test]
fn rejects_repo_without_source() {
    let err = r#"
[repo.a]
rev = "main"
"#
    .parse::<Manifest>()
    .unwrap_err();
    assert!(matches!(err, ManifestError::MissingSource(name) if name == "a"));
}

#[test]
fn rejects_ambiguous_source() {
    let err = r#"
[remote.r]
url = "git@example.com:x"

[repo.a]
url = "git@example.com:x/a.git"
remote = "r"
repo = "a.git"
rev = "main"
"#
    .parse::<Manifest>()
    .unwrap_err();
    assert!(matches!(err, ManifestError::AmbiguousSource(name) if name == "a"));
}

#[test]
fn rejects_unknown_repo_in_stack() {
    let err = r#"
[stack.p]
repos = ["ghost"]
"#
    .parse::<Manifest>()
    .unwrap_err();
    assert!(matches!(err, ManifestError::UnknownRepoInStack { .. }));
}

#[test]
fn rejects_unknown_repo_in_overlay() {
    let err = r#"
[overlay.dev.repo.ghost]
rev = "main"
"#
    .parse::<Manifest>()
    .unwrap_err();
    assert!(matches!(err, ManifestError::UnknownRepoInOverlay { .. }));
}

#[test]
fn rejects_unknown_top_level_key() {
    assert!("[repos.a]\nrev = \"main\"\n".parse::<Manifest>().is_err());
}

#[test]
fn resolves_a_stack() {
    let manifest: Manifest = EXAMPLE.parse().unwrap();
    let resolution = resolver::resolve(&manifest, "gateway", &[]).unwrap();
    assert_eq!(resolution.stack, "gateway");
    assert_eq!(resolution.repos.len(), 3);

    let kernel = &resolution.repos[0];
    assert_eq!(kernel.name, "kernel");
    assert_eq!(kernel.rev, "v6.1.2");
    assert_eq!(kernel.url, "git@gitlab.company.com:firmware/kernel.git");
    assert_eq!(kernel.path, PathBuf::from("kernel"));

    let mqtt = &resolution.repos[2];
    assert_eq!(mqtt.path, PathBuf::from("apps/mqtt"));
    assert_eq!(mqtt.url, "git@github.com:acme/app-mqtt.git");
}

#[test]
fn overlay_overrides_rev() {
    let manifest: Manifest = EXAMPLE.parse().unwrap();
    let resolution = resolver::resolve(&manifest, "sensor-node", &["dev".into()]).unwrap();
    assert_eq!(resolution.repos[0].rev, "main");
    assert_eq!(resolution.repos[1].rev, "main");
}

#[test]
fn unknown_stack_and_overlay_error() {
    let manifest: Manifest = EXAMPLE.parse().unwrap();
    assert!(matches!(
        resolver::resolve(&manifest, "ghost", &[]),
        Err(ResolveError::UnknownStack(_))
    ));
    assert!(matches!(
        resolver::resolve(&manifest, "gateway", &["ghost".into()]),
        Err(ResolveError::UnknownOverlay(_))
    ));
}

#[test]
fn parses_repo_stack_lexicon_and_serializes_canonically() {
    let manifest: Manifest = r#"
[remote.r]
url = "git@example.com:org"

[repo.kernel]
remote = "r"
repo = "kernel.git"
rev = "main"

[stack.gateway]
repos = ["kernel"]

[overlay.dev.repo.kernel]
rev = "next"
"#
    .parse()
    .unwrap();
    assert_eq!(manifest.repos.len(), 1);
    assert_eq!(manifest.stacks["gateway"].repos, ["kernel"]);
    assert_eq!(
        manifest.overlays["dev"].repos["kernel"].rev.as_deref(),
        Some("next")
    );

    let out = toml::to_string(&manifest).unwrap();
    assert!(out.contains("[repo.kernel]"), "canonical spelling is repo");
    assert!(
        out.contains("[stack.gateway]"),
        "canonical spelling is stack"
    );
    let reparsed: Manifest = out.parse().unwrap();
    assert_eq!(manifest, reparsed);
}
