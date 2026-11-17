#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_core::{Core, CoreError, DiscordPresenceInput, Feature};

#[test]
fn phase9_features_off_by_default_and_zero_cost() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    for feat in [
        Feature::SkipSegments,
        Feature::Debrid,
        Feature::Iptv,
        Feature::Discord,
    ] {
        assert!(!core.feature_enabled(feat).unwrap());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Calling disabled features returns empty or FeatureDisabled without touching runtime/network.
    assert!(
        rt.block_on(core.skip_segments("series".into(), "tt0903747".into(), Some(1), Some(1)))
            .unwrap()
            .is_empty()
    );
    assert!(
        rt.block_on(
            core.check_debrid_cache_hashes(vec!["0123456789abcdef0123456789abcdef01234567".into()])
        )
        .unwrap()
        .is_empty()
    );
    assert!(core.list_iptv_sources().unwrap().is_empty());
    assert!(core.list_iptv_channels(None, None).unwrap().is_empty());
    assert!(matches!(
        core.begin_tracking_oauth("mal".into(), "cid".into(), "".into()),
        Err(CoreError::FeatureDisabled("tracking"))
    ));
    assert!(matches!(
        rt.block_on(core.add_m3u_source("Test".into(), "http://127.0.0.1:9/a.m3u".into(), None)),
        Err(CoreError::FeatureDisabled("iptv"))
    ));
    rt.block_on(core.update_discord_presence(DiscordPresenceInput {
        title: "Test".into(),
        subtitle: None,
        duration_s: Some(120),
        position_s: 10,
        paused: false,
    }))
    .unwrap();
}

#[test]
fn vpn_binding_persists_in_disk_policy() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(core.set_vpn_torrent_binding(Some("utun4".into()), true))
        .unwrap();
    let settings = core.disk_policy_settings().unwrap();
    assert_eq!(settings.bind_device_name.as_deref(), Some("utun4"));
    assert!(settings.vpn_kill_switch);
    let tp = settings.torrent_policy(false);
    assert_eq!(tp.bind_device_name.as_deref(), Some("utun4"));
    assert!(tp.vpn_kill_switch);
}

#[test]
fn tracking_oauth_pkce_state_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(core.set_feature_enabled(Feature::Tracking, true))
        .unwrap();
    let started = core
        .begin_tracking_oauth("mal".into(), "test-client".into(), String::new())
        .unwrap();
    assert!(
        started
            .authorize_url
            .contains("myanimelist.net/v1/oauth2/authorize")
    );
    assert!(started.authorize_url.contains("code_challenge="));
    assert_eq!(started.provider, "mal");
}
