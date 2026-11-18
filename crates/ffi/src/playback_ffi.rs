use std::path::PathBuf;
use std::sync::Arc;

use cinma_core::CoreError;

use crate::{
    AudioDeviceInfo, ChapterInfo, CinmaError, MediaTrackInfo, NextEpisodePrefetchInfo,
    PlayerStatsInfo, RawFrameFfi, RenderObserver, StreamInfo, StreamStatsInfo, ThumbnailImageInfo,
    VideoFileIdentityInfo,
};

#[derive(uniffi::Object)]
pub struct PlaybackSession {
    pub(crate) inner: Arc<cinma_core::Playback>,
}

#[uniffi::export]
impl PlaybackSession {
    pub async fn play(&self, stream: StreamInfo) -> Result<(), CinmaError> {
        let stream: cinma_stremio::Stream =
            serde_json::from_str(&stream.raw).map_err(|e| CinmaError::Invalid {
                message: e.to_string(),
            })?;
        Ok(self.inner.play(stream).await?)
    }

    /// Plays a local file or URL directly (debug / render tests).
    pub fn play_url(&self, url: String) -> Result<(), CinmaError> {
        Ok(self.inner.play_url(&url)?)
    }

    /// Streams an active, paused, or completed download directly via `Blob::source()`.
    pub fn play_download(&self, download_id: i64) -> Result<(), CinmaError> {
        Ok(self.inner.play_download(download_id)?)
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_paused(paused)
            .map_err(CoreError::from)?)
    }

    pub fn toggle_pause(&self) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .toggle_pause()
            .map_err(CoreError::from)?)
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_absolute(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn seek_absolute_exact(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_absolute_exact(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_relative(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn seek_relative_mode(&self, seconds: f64, exact: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .seek_relative_mode(seconds, exact)
            .map_err(CoreError::from)?)
    }

    pub fn tracks(&self) -> Result<Vec<MediaTrackInfo>, CinmaError> {
        Ok(self
            .inner
            .player()
            .tracks()
            .map_err(CoreError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn chapters(&self) -> Result<Vec<ChapterInfo>, CinmaError> {
        Ok(self
            .inner
            .player()
            .chapters()
            .map_err(CoreError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn stats(&self) -> PlayerStatsInfo {
        self.inner.player().stats().into()
    }

    pub fn stream_stats(&self) -> StreamStatsInfo {
        self.inner.stream_stats().into()
    }

    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_track(id)
            .map_err(CoreError::from)?)
    }

    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_track(id)
            .map_err(CoreError::from)?)
    }

    pub fn set_secondary_subtitle_track(&self, id: Option<i64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_secondary_subtitle_track(id)
            .map_err(CoreError::from)?)
    }

    pub fn add_subtitle(
        &self,
        path: String,
        title: Option<String>,
        lang: Option<String>,
        select: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .add_subtitle(&path, title.as_deref(), lang.as_deref(), select)
            .map_err(CoreError::from)?)
    }

    pub fn set_subtitle_delay(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_delay(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_audio_delay(&self, seconds: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_delay(seconds)
            .map_err(CoreError::from)?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_subtitle_style(
        &self,
        font: Option<String>,
        font_size: Option<u32>,
        color_hex: Option<String>,
        border_size: Option<u32>,
        border_color_hex: Option<String>,
        back_color_hex: Option<String>,
        bold: bool,
        pos: Option<u32>,
        ass_override: bool,
        opacity_pct: u32,
        box_opacity_pct: u32,
        align_x: String,
        secondary_pos: u32,
        scale_pct: u32,
        ass_scale_with_window: bool,
        hdr_peak_nits: u32,
        use_margins: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_style(
                font.as_deref(),
                font_size,
                color_hex.as_deref(),
                border_size,
                border_color_hex.as_deref(),
                back_color_hex.as_deref(),
                bold,
                pos,
                ass_override,
                opacity_pct,
                box_opacity_pct,
                &align_x,
                secondary_pos,
                scale_pct,
                ass_scale_with_window,
                hdr_peak_nits,
                use_margins,
            )
            .map_err(CoreError::from)?)
    }

    pub fn set_video_eq(
        &self,
        brightness: i32,
        contrast: i32,
        gamma: i32,
        saturation: i32,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_video_eq(brightness, contrast, gamma, saturation)
            .map_err(CoreError::from)?)
    }

    pub fn frame_step(&self, forward: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .frame_step(forward)
            .map_err(CoreError::from)?)
    }

    pub fn sub_seek(&self, offset: i32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .sub_seek(offset)
            .map_err(CoreError::from)?)
    }

    pub fn audio_devices(&self) -> Result<Vec<AudioDeviceInfo>, CinmaError> {
        Ok(self
            .inner
            .player()
            .audio_devices()
            .map_err(CoreError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn current_audio_device(&self) -> Result<String, CinmaError> {
        Ok(self
            .inner
            .player()
            .current_audio_device()
            .map_err(CoreError::from)?)
    }

    pub fn set_audio_device(&self, name: String) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_device(&name)
            .map_err(CoreError::from)?)
    }

    pub fn capture_raw_frame(&self) -> Result<RawFrameFfi, CinmaError> {
        let f = self
            .inner
            .player()
            .capture_raw_frame()
            .map_err(CoreError::from)?;
        Ok(RawFrameFfi {
            width: f.width,
            height: f.height,
            stride: f.stride,
            bgr0: f.bgr0,
        })
    }

    pub fn set_subtitle_pos(&self, pos: u32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_subtitle_pos(pos)
            .map_err(CoreError::from)?)
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_speed(speed)
            .map_err(CoreError::from)?)
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_volume(volume)
            .map_err(CoreError::from)?)
    }

    pub fn set_volume_max(&self, max_pct: u32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_volume_max(max_pct)
            .map_err(CoreError::from)?)
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_muted(muted)
            .map_err(CoreError::from)?)
    }

    pub fn set_crop_mode(&self, mode: String) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_crop_mode(&mode)
            .map_err(CoreError::from)?)
    }

    pub fn set_audio_processing(
        &self,
        normalize: bool,
        downmix_stereo: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_audio_processing(normalize, downmix_stereo)
            .map_err(CoreError::from)?)
    }

    pub fn set_ab_loop_a(&self, seconds: Option<f64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_ab_loop_a(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_ab_loop_b(&self, seconds: Option<f64>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_ab_loop_b(seconds)
            .map_err(CoreError::from)?)
    }

    pub fn set_hdr_target_peak(&self, nits: Option<u32>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_hdr_target_peak(nits)
            .map_err(CoreError::from)?)
    }

    pub fn set_tone_mapping(&self, mode: String) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_tone_mapping(&mode)
            .map_err(CoreError::from)?)
    }

    pub fn set_glsl_shaders(&self, paths: Vec<String>) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .set_glsl_shaders(&paths)
            .map_err(CoreError::from)?)
    }

    pub fn screenshot_to_file(&self, path: String) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .screenshot_to_file(&PathBuf::from(path))
            .map_err(CoreError::from)?)
    }

    pub fn screenshot_to_file_mode(
        &self,
        path: String,
        include_subtitles: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .screenshot_to_file_mode(&PathBuf::from(path), include_subtitles)
            .map_err(CoreError::from)?)
    }

    pub fn video_identity(&self) -> Option<VideoFileIdentityInfo> {
        self.inner.video_identity().map(|id| VideoFileIdentityInfo {
            hash: id.hash,
            size: id.size,
            filename: id.filename,
        })
    }

    pub fn thumbnail_at(&self, seconds: f64) -> Option<ThumbnailImageInfo> {
        self.inner
            .thumbnail_at(seconds)
            .map(|t| ThumbnailImageInfo {
                width: t.width,
                height: t.height,
                rgba: t.rgba,
            })
    }

    pub fn start_next_episode_prefetch(
        &self,
        meta_id: String,
        current_season: u32,
        current_episode: u32,
        binge_group: Option<String>,
    ) {
        self.inner.start_next_episode_prefetch(
            meta_id,
            current_season,
            current_episode,
            binge_group,
        );
    }

    pub fn prefetched_next_episode(&self) -> Option<NextEpisodePrefetchInfo> {
        self.inner
            .prefetched_next_episode()
            .map(|p| NextEpisodePrefetchInfo {
                video_id: p.video_id,
                season: p.season,
                episode: p.episode,
                title: p.title,
                still: p.still,
                best_stream: p.best_stream.map(Into::into),
            })
    }

    pub fn cancel_prefetch(&self) {
        self.inner.cancel_prefetch();
    }

    /// Creates the OpenGL render context. Call with the video layer's CGL
    /// context current (e.g. inside `CAOpenGLLayer.draw`).
    pub fn render_init(&self, observer: Arc<dyn RenderObserver>) -> Result<(), CinmaError> {
        let callback: cinma_core::UpdateCallback = Arc::new(move || observer.needs_display());
        Ok(self
            .inner
            .player()
            .render_init(callback)
            .map_err(CoreError::from)?)
    }

    pub fn render_should_draw(&self) -> bool {
        self.inner.player().render_should_draw()
    }

    /// Draws the current frame into `fbo`. CGL context must be current.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .render(fbo, width, height)
            .map_err(CoreError::from)?)
    }

    pub fn render_frame(
        &self,
        fbo: i32,
        width: i32,
        height: i32,
        block_for_target_time: bool,
    ) -> Result<(), CinmaError> {
        Ok(self
            .inner
            .player()
            .render_frame(fbo, width, height, block_for_target_time)
            .map_err(CoreError::from)?)
    }

    /// Frees the render context. CGL context must be current. Call before `stop`.
    pub fn render_free(&self) {
        self.inner.player().render_free();
    }

    /// Destroys mpv and stops torrent traffic. Call `render_free` first.
    pub async fn stop(&self) -> Result<(), CinmaError> {
        Ok(self.inner.stop().await?)
    }
}
