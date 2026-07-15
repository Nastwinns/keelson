#![allow(clippy::unwrap_used)]

use haw_core::manifest::import::{DEFAULT_STACK, from_repo_xml_str, from_west_str};

const WEST: &str = r#"
manifest:
  defaults:
    remote: upstream
    revision: main
  remotes:
    - name: upstream
      url-base: https://github.com/zephyrproject-rtos
  projects:
    - name: zephyr
      revision: v3.6.0
      path: zephyr
    - name: hal_stm32
      path: modules/hal/stm32
      groups: [hal]
    - name: extern
      url: https://example.com/extern.git
"#;

const REPO_XML: &str = r#"
<manifest>
  <remote name="aosp" fetch="https://android.googlesource.com/" />
  <default revision="main" remote="aosp" />
  <project name="platform/build" path="build" groups="pdk,tools" />
  <project name="kernel/common" revision="android-mainline" />
</manifest>
"#;

#[test]
fn west_projects_become_repos_with_defaults_applied() {
    let manifest = from_west_str(WEST).unwrap();
    assert_eq!(
        manifest.remotes["upstream"].url.as_str(),
        "https://github.com/zephyrproject-rtos"
    );

    let zephyr = &manifest.repos["zephyr"];
    assert_eq!(zephyr.rev, "v3.6.0");
    assert_eq!(zephyr.remote.as_deref(), Some("upstream"));

    let hal = &manifest.repos["hal_stm32"];
    assert_eq!(hal.rev, "main", "defaults.revision applies");
    assert_eq!(hal.groups, vec!["hal"]);

    let ext = &manifest.repos["extern"];
    assert_eq!(ext.url.as_deref(), Some("https://example.com/extern.git"));

    assert_eq!(manifest.stacks[DEFAULT_STACK].repos.len(), 3);
}

#[test]
fn repo_xml_projects_become_repos() {
    let manifest = from_repo_xml_str(REPO_XML).unwrap();
    assert_eq!(
        manifest.remotes["aosp"].url.as_str(),
        "https://android.googlesource.com",
        "trailing slash trimmed"
    );

    let build = &manifest.repos["build"];
    assert_eq!(build.repo.as_deref(), Some("platform/build"));
    assert_eq!(build.rev, "main");
    assert_eq!(build.groups, vec!["pdk", "tools"]);

    let kernel = &manifest.repos["common"];
    assert_eq!(kernel.rev, "android-mainline");
    assert_eq!(
        kernel
            .path
            .as_deref()
            .map(|p| p.to_string_lossy().into_owned()),
        Some("kernel/common".to_string()),
        "path defaults to the full project name"
    );
}

#[test]
fn imported_manifest_serializes_deterministically() {
    let a = toml::to_string_pretty(&from_west_str(WEST).unwrap()).unwrap();
    let b = toml::to_string_pretty(&from_west_str(WEST).unwrap()).unwrap();
    assert_eq!(a, b, "same input -> byte-identical manifest");
    assert!(a.parse::<haw_core::manifest::Manifest>().is_ok());
}
