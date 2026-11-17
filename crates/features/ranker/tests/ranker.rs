#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_ranker::{
    AudioCodec, FilterReason, MachineCapabilities, RankerPreferences, Resolution, SafetyLevel,
    SourceQuality, StreamTier, VideoCodec, parse_hardware_av1, parse_stream, rank_streams,
};
use cinma_stremio::{BehaviorHints, Stream};

fn torrent_stream(name: &str, title: &str, filename: Option<&str>, size: Option<u64>) -> Stream {
    Stream {
        name: Some(name.to_owned()),
        title: Some(title.to_owned()),
        info_hash: Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
        file_idx: Some(0),
        behavior_hints: Some(BehaviorHints {
            filename: filename.map(str::to_owned),
            video_size: size,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn detects_hardware_av1_only_on_m3_and_newer() {
    assert!(!parse_hardware_av1("Apple M1", "MacBookPro17,1"));
    assert!(!parse_hardware_av1("Apple M2 Max", "Mac14,6"));
    assert!(parse_hardware_av1("Apple M3", "Mac15,3"));
    assert!(parse_hardware_av1("Apple M4 Pro", "Mac16,8"));
    assert!(parse_hardware_av1("Apple M5", "Mac17,1"));
    assert!(!parse_hardware_av1(
        "Intel(R) Core(TM) i9",
        "MacBookPro16,1"
    ));
}

#[test]
fn parses_torrentio_remux_dv_hdr10_atmos() {
    let s = torrent_stream(
        "Torrentio\n4k DV | HDR",
        "Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR10+.HEVC.TrueHD.7.1.Atmos-FraMeSToR.mkv\n👤 184 💾 62.4 GB ⚙️ 1337x\n🇬🇧 / 🇹🇷",
        Some(
            "Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR10+.HEVC.TrueHD.7.1.Atmos-FraMeSToR.mkv",
        ),
        Some(67_001_489_817),
    );
    let p = parse_stream(&s);
    assert_eq!(p.resolution, Resolution::Uhd2160);
    assert_eq!(p.source, SourceQuality::Remux);
    assert!(p.hdr.dolby_vision);
    assert!(p.hdr.hdr10_plus);
    assert!(p.hdr.hdr10);
    assert_eq!(p.codec, VideoCodec::Hevc);
    assert_eq!(p.audio, AudioCodec::Atmos);
    assert_eq!(p.seeders, Some(184));
    assert_eq!(p.size_bytes, Some(67_001_489_817));
    assert_eq!(p.release_group.as_deref(), Some("FraMeSToR"));
    assert!(p.languages.contains(&"en".to_owned()));
    assert!(p.languages.contains(&"tr".to_owned()));
    assert!(!p.is_cam_or_ts);
    assert!(!p.is_sample_or_fake);
}

#[test]
fn parses_webdl_ddp_size_from_description_and_turkish_tags() {
    let s = Stream {
        name: Some("MediaFusion | 1080p".into()),
        description: Some(
            "Inception.2010.1080p.WEB-DL.DDP5.1.H.264.DUAL.TUR-FLUX\n👥 42 💾 4.5 GB".into(),
        ),
        info_hash: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        ..Default::default()
    };
    let p = parse_stream(&s);
    assert_eq!(p.resolution, Resolution::Fhd1080);
    assert_eq!(p.source, SourceQuality::WebDl);
    assert_eq!(p.codec, VideoCodec::H264);
    assert_eq!(p.audio, AudioCodec::Eac3);
    assert_eq!(p.seeders, Some(42));
    assert_eq!(p.release_group.as_deref(), Some("FLUX"));
    assert!(p.languages.contains(&"tr".to_owned()));
    assert!(p.languages.contains(&"dual".to_owned()));
    let expected_size = (4.5 * 1024.0 * 1024.0 * 1024.0) as u64;
    assert_eq!(p.size_bytes, Some(expected_size));
}

#[test]
fn filters_cam_ts_fakes_and_respects_safety_levels() {
    let cam = torrent_stream(
        "Torrentio\n1080p",
        "Movie.2025.1080p.HDCAM.x264\n👤 900 💾 1.8 GB",
        None,
        None,
    );
    let exe = torrent_stream(
        "Torrentio\n1080p",
        "Movie.2025.1080p.BluRay.exe\n👤 500 💾 12 MB",
        Some("Movie.2025.1080p.BluRay.exe"),
        Some(12 * 1024 * 1024),
    );
    let zero_seed = torrent_stream(
        "Torrentio\n1080p",
        "Movie.2025.1080p.WEB-DL.x265-NTb\n👤 0 💾 2.1 GB",
        None,
        None,
    );

    let standard = rank_streams(
        vec![
            ("A".into(), cam.clone()),
            ("A".into(), exe.clone()),
            ("A".into(), zero_seed.clone()),
        ],
        &RankerPreferences::default(),
        &MachineCapabilities { hardware_av1: true },
    );
    assert_eq!(standard.ranked.len(), 1);
    assert_eq!(standard.filtered.len(), 2);
    assert_eq!(standard.filtered[0].reason, FilterReason::CamOrTelesync);
    assert_eq!(standard.filtered[1].reason, FilterReason::FakeOrMalicious);

    let strict = rank_streams(
        vec![("A".into(), cam.clone()), ("A".into(), zero_seed)],
        &RankerPreferences {
            safety_level: SafetyLevel::Strict,
            ..Default::default()
        },
        &MachineCapabilities { hardware_av1: true },
    );
    assert!(strict.ranked.is_empty());
    assert_eq!(strict.filtered[1].reason, FilterReason::BelowMinSeeders);

    let permissive = rank_streams(
        vec![("A".into(), cam), ("A".into(), exe)],
        &RankerPreferences {
            safety_level: SafetyLevel::Permissive,
            ..Default::default()
        },
        &MachineCapabilities { hardware_av1: true },
    );
    assert_eq!(permissive.ranked.len(), 1);
    assert_eq!(permissive.filtered.len(), 1);
    assert_eq!(permissive.filtered[0].reason, FilterReason::FakeOrMalicious);
}

#[test]
fn av1_scores_higher_on_m3_plus_than_on_m1() {
    let av1 = torrent_stream(
        "Torrentio\n1080p",
        "Show.S01E01.1080p.WEB-DL.AV1.EAC3-HONE\n👤 60 💾 900 MB",
        None,
        None,
    );
    let x264 = torrent_stream(
        "Torrentio\n1080p",
        "Show.S01E01.1080p.WEB-DL.x264.AAC\n👤 60 💾 1.2 GB",
        None,
        None,
    );

    let m1 = rank_streams(
        vec![("T".into(), av1.clone()), ("T".into(), x264.clone())],
        &RankerPreferences::default(),
        &MachineCapabilities {
            hardware_av1: false,
        },
    );
    // On M1/M2 without HW AV1, x264 outscores software AV1.
    assert_eq!(m1.ranked[0].parsed.codec, VideoCodec::H264);
    assert!(
        m1.ranked[1]
            .reasons
            .iter()
            .any(|r| r.code == "codec.av1_sw" && r.points < 0)
    );

    let m4 = rank_streams(
        vec![("T".into(), av1), ("T".into(), x264)],
        &RankerPreferences::default(),
        &MachineCapabilities { hardware_av1: true },
    );
    // On M3+, HW AV1 outscores x264.
    assert_eq!(m4.ranked[0].parsed.codec, VideoCodec::Av1);
}

#[test]
fn ranks_and_explains_tiers_and_debrid_direct_streams() {
    let best_debrid = Stream {
        name: Some("Torrentio [RD+]\n4k HDR".into()),
        title: Some(
            "Film.2024.2160p.BluRay.REMUX.DV.HDR10.HEVC.Atmos-FraMeSToR.mkv\n💾 50 GB\n🇹🇷".into(),
        ),
        url: Some("https://rd.example/file.mkv".into()),
        ..Default::default()
    };
    let good_torrent = torrent_stream(
        "Torrentio\n1080p",
        "Film.2024.1080p.WEB-DL.x265.DDP5.1-NTb\n👤 35 💾 3.2 GB",
        None,
        None,
    );
    let weak_720 = torrent_stream(
        "Torrentio\n720p",
        "Film.2024.720p.HDTV.x264\n👤 2 💾 800 MB",
        None,
        None,
    );

    let prefs = RankerPreferences {
        prefer_languages: vec!["tr".into(), "en".into()],
        ..Default::default()
    };
    let res = rank_streams(
        vec![
            ("T".into(), weak_720),
            ("T".into(), good_torrent),
            ("T".into(), best_debrid),
        ],
        &prefs,
        &MachineCapabilities { hardware_av1: true },
    );

    assert_eq!(res.ranked.len(), 3);
    assert_eq!(res.ranked[0].tier, StreamTier::Best);
    assert!(res.ranked[0].parsed.cached_debrid);
    assert!(
        res.ranked[0]
            .reasons
            .iter()
            .any(|r| r.code == "language" && r.detail.as_deref() == Some("tr"))
    );
    assert!(
        res.ranked[0]
            .reasons
            .iter()
            .any(|r| r.code == "release_group")
    );
    assert_eq!(res.ranked[1].tier, StreamTier::Good);
    assert_eq!(res.ranked[2].tier, StreamTier::Fallback);
}
