//! Nkuplay 的协议：界面显示的状态，以及改变它的命令。
//! 界面、命令行和 MCP 服务器都通过这些命令操作。

#[bern::protocol(namespace = "nkuplay", version = 1)]
pub mod nkuplay {
    use bern::prelude::*;

    #[state]
    pub struct State {
        /// 已经打开了一个视频；在此之前窗口显示空画框。
        pub has_video: bool,
        /// 视频视图播放的内容。只在 `has_video` 为真时有意义。
        pub source: MediaSource,
    }

    #[commands]
    pub trait Commands {
        /// 打开一个视频文件并播放。
        fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError>;

        /// 停止播放，回到空画框。
        #[idempotent]
        fn close(&mut self) -> Result<(), CommandError>;
    }
}
