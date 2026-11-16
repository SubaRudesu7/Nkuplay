//! Nkuplay 的界面：一个窗口，打开视频之前是空画框，之后是视频加平台自带的播放控件。
//! 每段文字都在显示它的地方写出所有语言的版本，每个按钮都执行协议里的一条命令。

use bern::prelude::*;
use nkuplay_protocol::nkuplay;

/// 主窗口的草稿状态。
#[bern::session]
pub struct MainDraft {
    /// 最近一次选择或拖入的视频。
    pub file: MediaSource,
    /// 是否有视频正悬停在窗口上方。
    pub over: bool,
}

#[component]
pub fn app(app: nkuplay::Client, windows: AppWindows) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main) {
            Window(
                id: main,
                title: tr! { "en" => "Nkuplay", "zh-Hans" => "Nkuplay" },
                initial: open,
                width: 960,
                height: 600,
                min_width: 480,
                min_height: 320
            ) {
                player_window(app: app)
            }
        }
    }
}

/// 整个窗口都接受拖入的视频；里面要么是空画框，要么是视频。
#[component]
pub fn player_window(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Drop(
            id: drop,
            label: tr! { "en" => "Drop a video here", "zh-Hans" => "把视频拖到这里" },
            value: draft.file,
            files: "video",
            over: draft.over
        ) {
            if app.has_video {
                Column(gap: 8, padding: 8) {
                    Video(
                        id: video,
                        source: app.source,
                        label: tr! { "en" => "Video", "zh-Hans" => "视频" }
                    )
                    Row(gap: 8) {
                        Button(
                            id: open_another,
                            label: tr! { "en" => "Open…", "zh-Hans" => "打开…" },
                            pick: draft.file,
                            files: "video"
                        ) => app.open_file(draft.file)
                        Spacer
                        Button(
                            id: close,
                            label: tr! { "en" => "Close", "zh-Hans" => "关闭" }
                        ) => app.close()
                    }
                    // 用 `bern new --module` 添加的模块显示在这里。
                    Slot(id: extras)
                }
            } else {
                Column(gap: 16, padding: 24, align: center) {
                    Spacer
                    Text(
                        content: tr! { "en" => "Drop a video here", "zh-Hans" => "把视频拖到这里" },
                        style: title
                    )
                    Button(
                        id: open,
                        label: tr! { "en" => "Open File", "zh-Hans" => "打开文件" },
                        prominence: prominent,
                        pick: draft.file,
                        files: "video"
                    ) => app.open_file(draft.file)
                    Spacer
                }
            }
        } => app.open_file(draft.file)
    }
}
