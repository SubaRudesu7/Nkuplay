//! Nkuplay 的引擎：业务逻辑，纯 Rust。它不知道窗口的存在，所以不需要窗口就能测试。

use bern::CommandError;
use bern::media::MediaSource;
use nkuplay_protocol::nkuplay::{self, State};

pub struct NkuplayEngine {
    state: State,
}

/// 没有打开任何视频时 `source` 里放的值；界面不会显示它。
fn no_source() -> MediaSource {
    MediaSource::file("")
}

impl Default for NkuplayEngine {
    fn default() -> Self {
        // 有 libmpv 时视频由它播放，没有时由平台自带的播放器接手。测试两者都不需要。
        #[cfg(not(test))]
        nkuplay_mpv::install();
        Self {
            state: State {
                has_video: false,
                source: no_source(),
            },
        }
    }
}

impl nkuplay::Commands for NkuplayEngine {
    fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError> {
        self.state.source = file;
        self.state.has_video = true;
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.source = no_source();
        self.state.has_video = false;
        Ok(())
    }
}

impl nkuplay::Engine for NkuplayEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nkuplay::Commands;

    #[test]
    fn opening_shows_the_video_and_closing_returns_to_the_empty_frame() {
        let mut engine = NkuplayEngine::default();
        assert!(!engine.state.has_video);

        let file = MediaSource::file("C:/videos/sample.mkv");
        engine.open_file(file.clone()).unwrap();
        assert!(engine.state.has_video);
        assert_eq!(engine.state.source, file);

        engine.close().unwrap();
        assert!(!engine.state.has_video);
    }
}
