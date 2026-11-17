//! Subtitle & player profile settings, per-video timing offsets, Stremio +
//! OpenSubtitles aggregation, and UTF-8 / Turkish encoding normalization.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cinma_stremio::SubtitleQuery;
use cinma_subtitles::{
    OpenSubtitlesClient, PreparedSubtitle, SubtitleCandidate, SubtitleQueryParams,
    candidate_from_stremio, prepare_subtitle_file, rank_candidates,
};
use serde::{Deserialize, Serialize};

use crate::keychain::{
    get_opensubtitles_api_key, get_opensubtitles_token, set_opensubtitles_api_key,
    set_opensubtitles_token,
};
use crate::settings::Feature;
use crate::{Core, CoreError};

const KEY_SUBTITLE_SETTINGS: &str = "player.subtitles";
const KEY_PLAYER_PREFS: &str = "player.prefs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SubtitleSettings {
    pub preferred_langs: Vec<String>,
    pub audio_langs: Vec<String>,
    pub dual_secondary_lang: Option<String>,
    pub srt_font: Option<String>,
    pub srt_font_size: u32,
    pub srt_color: Option<String>,
    pub srt_pos: u32,
    pub ass_override: bool,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            preferred_langs: vec!["tur".to_owned(), "eng".to_owned()],
            audio_langs: vec!["eng".to_owned(), "jpn".to_owned(), "tur".to_owned()],
            dual_secondary_lang: Some("eng".to_owned()),
            srt_font: None,
            srt_font_size: 46,
            srt_color: None,
            srt_pos: 95,
            ass_override: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlayerPreferences {
    pub exact_seek: bool,
    pub hdr_target_peak: Option<u32>,
    /// `"none"` | `"sharpen"` | custom `.glsl` file path.
    pub shader_preset: String,
    pub autoplay_next: bool,
    pub autoplay_countdown_s: u32,
    /// When `false`, SDR sources use an 8-bit BT.709 layer instead of 16-bit float EDR.
    pub force_hdr_surface: bool,
}

impl Default for PlayerPreferences {
    fn default() -> Self {
        Self {
            exact_seek: true,
            hdr_target_peak: None,
            shader_preset: "none".to_owned(),
            autoplay_next: true,
            autoplay_countdown_s: 10,
            force_hdr_surface: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoOffsets {
    pub sub_delay: f64,
    pub audio_delay: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleSearchRequest {
    pub media_type: String,
    pub video_id: String,
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub video_hash: Option<String>,
    pub video_size: Option<u64>,
    pub filename: Option<String>,
}

const CAS_SHARPEN_SHADER: &str = r#"//!HOOK MAIN
//!BIND HOOKED
//!DESC Cinma Adaptive Luma Sharpen (CAS)

vec4 hook() {
    vec2 px = HOOKED_pt;
    vec3 c = HOOKED_tex(HOOKED_pos).rgb;
    vec3 n = HOOKED_tex(HOOKED_pos + vec2(0.0, -px.y)).rgb;
    vec3 s = HOOKED_tex(HOOKED_pos + vec2(0.0,  px.y)).rgb;
    vec3 e = HOOKED_tex(HOOKED_pos + vec2( px.x, 0.0)).rgb;
    vec3 w = HOOKED_tex(HOOKED_pos + vec2(-px.x, 0.0)).rgb;
    vec3 blur = (n + s + e + w) * 0.25;
    vec3 sharp = clamp(c + (c - blur) * 0.35, 0.0, 1.0);
    return vec4(sharp, HOOKED_tex(HOOKED_pos).a);
}
"#;

impl Core {
    pub fn subtitle_settings(&self) -> Result<SubtitleSettings, CoreError> {
        self.load_profile_json(KEY_SUBTITLE_SETTINGS)
    }

    pub fn set_subtitle_settings(&self, settings: &SubtitleSettings) -> Result<(), CoreError> {
        self.save_profile_json(KEY_SUBTITLE_SETTINGS, settings)
    }

    pub fn player_preferences(&self) -> Result<PlayerPreferences, CoreError> {
        self.load_profile_json(KEY_PLAYER_PREFS)
    }

    pub fn set_player_preferences(&self, prefs: &PlayerPreferences) -> Result<(), CoreError> {
        self.save_profile_json(KEY_PLAYER_PREFS, prefs)
    }

    pub fn video_offsets(&self, video_id: &str) -> Result<VideoOffsets, CoreError> {
        self.load_profile_json(&format!("offset.{video_id}"))
    }

    pub fn set_video_offsets(
        &self,
        video_id: &str,
        offsets: &VideoOffsets,
    ) -> Result<(), CoreError> {
        self.save_profile_json(&format!("offset.{video_id}"), offsets)
    }

    fn load_profile_json<T: serde::de::DeserializeOwned + Default>(
        &self,
        key: &str,
    ) -> Result<T, CoreError> {
        let profile = self.active_profile()?;
        let raw = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .get_profile_setting(profile, key)?;
        match raw {
            Some(s) => serde_json::from_str(&s).map_err(|e| CoreError::Invalid(e.to_string())),
            None => Ok(T::default()),
        }
    }

    fn save_profile_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), CoreError> {
        let json = serde_json::to_string(value).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let profile = self.active_profile()?;
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_profile_setting(profile, key, &json)?;
        Ok(())
    }

    pub fn has_opensubtitles_key(&self) -> Result<bool, CoreError> {
        Ok(get_opensubtitles_api_key()?.is_some())
    }

    pub fn set_opensubtitles_credentials(
        &self,
        api_key: Option<&str>,
        token: Option<&str>,
    ) -> Result<(), CoreError> {
        set_opensubtitles_api_key(api_key)?;
        if token.is_some() || api_key.is_none() {
            set_opensubtitles_token(token)?;
        }
        Ok(())
    }

    /// Resolves GLSL shader file paths for a preset (`"none"` | `"sharpen"` | path).
    pub fn resolve_shader_paths(&self, preset: &str) -> Result<Vec<String>, CoreError> {
        match preset.trim() {
            "" | "none" => Ok(Vec::new()),
            "sharpen" => {
                let dir = self.data_dir.join("shaders");
                std::fs::create_dir_all(&dir)?;
                let path = dir.join("cas.glsl");
                if !path.exists() {
                    std::fs::write(&path, CAS_SHARPEN_SHADER)?;
                }
                Ok(vec![path.to_string_lossy().into_owned()])
            }
            custom => {
                let p = Path::new(custom);
                if p.exists() {
                    Ok(vec![p.to_string_lossy().into_owned()])
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    /// Queries enabled Stremio `subtitles` addons + optional direct OpenSubtitles
    /// API (when configured in Keychain), ranked by preferred languages & hash match.
    pub async fn fetch_subtitles(
        self: &Arc<Self>,
        req: SubtitleSearchRequest,
    ) -> Result<Vec<SubtitleCandidate>, CoreError> {
        let settings = self.subtitle_settings()?;
        let had_hash = req.video_hash.as_deref().is_some_and(|h| !h.is_empty());
        let stremio_query = SubtitleQuery {
            video_hash: req.video_hash.clone(),
            video_size: req.video_size,
            filename: req.filename.clone(),
        };

        let mut handles = Vec::new();
        if self.feature_enabled(Feature::Addons)? {
            let http = self.http()?;
            let targets: Vec<_> = self
                .addons()?
                .into_iter()
                .filter(|a| {
                    a.enabled
                        && a.manifest
                            .supports("subtitles", &req.media_type, &req.video_id)
                })
                .collect();
            for addon in targets {
                let client = http.clone();
                let mt = req.media_type.clone();
                let id = req.video_id.clone();
                let q = stremio_query.clone();
                handles.push(self.runtime()?.spawn(async move {
                    let subs = client
                        .fetch_subtitles(&addon.transport_url, &mt, &id, &q)
                        .await
                        .unwrap_or_default();
                    subs.into_iter()
                        .map(|s| candidate_from_stremio(&addon.manifest.name, s, had_hash))
                        .collect::<Vec<_>>()
                }));
            }
        }

        if let Some(os_key) = get_opensubtitles_api_key()? {
            let os_token = get_opensubtitles_token()?;
            let params = SubtitleQueryParams {
                imdb_id: req.imdb_id,
                season: req.season,
                episode: req.episode,
                video_hash: req.video_hash,
                video_size: req.video_size,
                filename: req.filename,
                languages: settings.preferred_langs.clone(),
            };
            handles.push(self.runtime()?.spawn(async move {
                let Ok(client) = OpenSubtitlesClient::new(os_key, os_token) else {
                    return Vec::new();
                };
                client.search(&params).await.unwrap_or_default()
            }));
        }

        let mut all = Vec::new();
        for handle in handles {
            if let Ok(mut batch) = handle.await {
                all.append(&mut batch);
            }
        }
        rank_candidates(&mut all, &settings.preferred_langs);
        Ok(all)
    }

    /// Downloads a remote subtitle candidate (Stremio URL or OpenSubtitles `file_id`),
    /// unpacks gzip if compressed, detects/converts Turkish legacy encodings to
    /// UTF-8 while keeping ASS styles intact, and caches under `data_dir/subtitles`.
    pub async fn download_and_prepare_subtitle(
        self: &Arc<Self>,
        candidate: SubtitleCandidate,
    ) -> Result<PreparedSubtitle, CoreError> {
        let cache_dir = self.subtitles_cache_dir();
        let bytes_and_ext: (Vec<u8>, Option<String>) =
            if let Some(file_id) = candidate.opensubtitles_file_id {
                let api_key = get_opensubtitles_api_key()?.ok_or_else(|| {
                    CoreError::Invalid("OpenSubtitles API key missing in Keychain".into())
                })?;
                let token = get_opensubtitles_token()?;
                self.spawn(async move {
                    let client = OpenSubtitlesClient::new(api_key, token)
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    let (bytes, ext) = client
                        .download_file(file_id)
                        .await
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    Ok((bytes, Some(ext)))
                })
                .await?
            } else {
                let url = candidate.download_url.clone();
                let ext = url
                    .split('?')
                    .next()
                    .and_then(|p| p.rsplit_once('.'))
                    .map(|(_, e)| e.to_owned())
                    .filter(|e| !e.eq_ignore_ascii_case("gz"));
                self.spawn(async move {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(20))
                        .build()
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    let resp = client
                        .get(&url)
                        .send()
                        .await
                        .map_err(|e| CoreError::Invalid(e.to_string()))?;
                    if !resp.status().is_success() {
                        return Err(CoreError::Invalid(format!(
                            "subtitle HTTP {}",
                            resp.status()
                        )));
                    }
                    let data = resp
                        .bytes()
                        .await
                        .map_err(|e| CoreError::Invalid(e.to_string()))?
                        .to_vec();
                    Ok((data, ext))
                })
                .await?
            };

        prepare_subtitle_file(
            &bytes_and_ext.0,
            &cache_dir,
            &format!("{}_{}", candidate.lang, candidate.id),
            bytes_and_ext.1.as_deref(),
            candidate.encoding_hint.as_deref(),
            Some(&candidate.lang),
        )
        .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    /// Normalizes a user-picked local subtitle file (converting CP1254/ISO-8859-9
    /// `.srt` to UTF-8 if needed while leaving `.ass` styling untouched).
    pub fn prepare_local_subtitle(&self, path: &Path) -> Result<PreparedSubtitle, CoreError> {
        let bytes = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("local");
        let ext = path.extension().and_then(|s| s.to_str());
        prepare_subtitle_file(&bytes, &self.subtitles_cache_dir(), stem, ext, None, None)
            .map_err(|e| CoreError::Invalid(e.to_string()))
    }

    fn subtitles_cache_dir(&self) -> PathBuf {
        self.data_dir.join("subtitles")
    }
}
