//! Nkuplay 的协议：界面显示的状态，以及改变它的命令。
//! 界面、命令行和 MCP 服务器都通过这些命令操作。

#[bern::protocol(namespace = "nkuplay", version = 1)]
pub mod nkuplay {
    use bern::prelude::*;

    /// 当前视频的弹幕处于什么阶段。
    #[value]
    pub enum DanmakuStatus {
        /// 没有打开视频。
        #[label(tr! { "en" => "Off", "zh-Hans" => "未开启" })]
        Off,
        /// 这个构建没有弹弹play 的凭证，弹幕功能不可用。
        #[label(tr! { "en" => "Not configured", "zh-Hans" => "未配置" })]
        Unavailable,
        /// 正在识别文件并获取弹幕。
        #[label(tr! { "en" => "Looking up…", "zh-Hans" => "正在识别…" })]
        Matching,
        /// 已识别出是哪一集，弹幕已取回。
        #[label(tr! { "en" => "Loaded", "zh-Hans" => "已加载" })]
        Loaded,
        /// 弹弹play 认不出这个文件。
        #[label(tr! { "en" => "No match", "zh-Hans" => "未识别" })]
        NoMatch,
        /// 识别或获取弹幕时出错，原因在 `danmaku_error` 里。
        #[label(tr! { "en" => "Failed", "zh-Hans" => "获取失败" })]
        Failed,
    }

    /// 一周中的一天，用来按更新日筛选新番。
    #[value]
    pub enum Weekday {
        #[label(tr! { "en" => "Mon", "zh-Hans" => "周一" })]
        Monday,
        #[label(tr! { "en" => "Tue", "zh-Hans" => "周二" })]
        Tuesday,
        #[label(tr! { "en" => "Wed", "zh-Hans" => "周三" })]
        Wednesday,
        #[label(tr! { "en" => "Thu", "zh-Hans" => "周四" })]
        Thursday,
        #[label(tr! { "en" => "Fri", "zh-Hans" => "周五" })]
        Friday,
        #[label(tr! { "en" => "Sat", "zh-Hans" => "周六" })]
        Saturday,
        #[label(tr! { "en" => "Sun", "zh-Hans" => "周日" })]
        Sunday,
    }

    /// 本季新番列表处于什么阶段。列表来自弹弹play，存在硬盘上：
    /// 第一次打开首页时取一次，之后只读缓存，用户点“刷新”才重新取。
    #[value]
    pub enum ShinStatus {
        /// 首页还没打开过，没有读缓存也没有取。
        #[label(tr! { "en" => "Not loaded", "zh-Hans" => "未获取" })]
        NotLoaded,
        /// 这个构建没有弹弹play 的凭证，又没有缓存。
        #[label(tr! { "en" => "Not configured", "zh-Hans" => "未配置" })]
        Unavailable,
        #[label(tr! { "en" => "Loading…", "zh-Hans" => "正在获取…" })]
        Loading,
        #[label(tr! { "en" => "Loaded", "zh-Hans" => "已获取" })]
        Loaded,
        /// 获取失败，原因在 `shin_error` 里。
        #[label(tr! { "en" => "Failed", "zh-Hans" => "获取失败" })]
        Failed,
    }

    /// 最近看过的一个视频。
    #[value]
    pub struct Recent {
        pub id: i64,
        /// 作品名；弹弹play 认出之前是文件名。
        #[content]
        pub title: String,
        /// 剧集名；没认出时为空。
        #[content]
        pub episode: String,
        pub source: MediaSource,
        /// 作品的海报（弹弹play 认出后才有）；`has_poster` 为假时没有意义。
        pub poster: MediaSource,
        pub has_poster: bool,
    }

    /// 本季新番里的一部作品。
    #[value]
    pub struct ShinAnime {
        /// 弹弹play 的作品编号。
        pub id: i64,
        #[content]
        pub title: String,
        /// 海报地址；`has_poster` 为假时没有意义。
        pub poster: MediaSource,
        pub has_poster: bool,
    }

    /// 媒体库里的一个文件夹。
    #[value]
    pub struct Folder {
        pub id: i64,
        #[content]
        pub name: String,
        /// 里面（含子文件夹）的视频文件数。
        pub videos: i64,
        /// 还在扫描这个文件夹。
        pub scanning: bool,
    }

    #[state]
    pub struct State {
        /// 已经打开了一个视频；在此之前窗口显示首页。
        pub has_video: bool,
        /// 视频视图播放的内容。只在 `has_video` 为真时有意义。
        pub source: MediaSource,
        /// 当前视频的弹幕阶段。
        pub danmaku: DanmakuStatus,
        /// 弹幕已加载：下面的作品、剧集和条数有意义。
        pub danmaku_matched: bool,
        /// 识别出的作品标题。
        #[content]
        pub danmaku_anime: String,
        /// 识别出的剧集标题。
        #[content]
        pub danmaku_episode: String,
        /// 取回的弹幕条数。
        pub danmaku_count: i64,
        /// 弹幕获取失败的原因；没有失败时为空。
        #[content]
        pub danmaku_error: String,

        /// 继续观看：最近打开过的视频，最近的在前。
        pub recents: Vec<Recent>,
        /// `recents` 的个数。
        pub recent_count: i64,

        /// 本季新番列表的阶段。
        pub shin: ShinStatus,
        /// 缓存的新番列表是上一季或更早取的，应当刷新。
        pub shin_outdated: bool,
        /// 正在看哪一天更新的新番；默认是今天。
        pub shin_day: Weekday,
        /// `shin_day` 那天更新的新番。
        pub shin_shown: Vec<ShinAnime>,
        /// `shin_shown` 的个数。
        pub shin_count: i64,
        /// 获取新番失败的原因；没有失败时为空。
        #[content]
        pub shin_error: String,

        /// 媒体库里的文件夹，按添加顺序。
        pub folders: Vec<Folder>,
        /// `folders` 的个数。
        pub folder_count: i64,
    }

    #[commands]
    pub trait Commands {
        /// 打开一个视频文件并播放，同时识别它并获取弹幕。
        fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError>;

        /// 停止播放，回到首页。
        #[idempotent]
        fn close(&mut self) -> Result<(), CommandError>;

        /// 准备本季新番列表：有缓存就读缓存，从没取过才向弹弹play 取一次。
        /// 已经准备过则什么也不做。
        #[idempotent]
        fn load_shin(&mut self) -> Result<(), CommandError>;

        /// 向弹弹play 重新取本季新番列表，替换缓存。
        fn refresh_shin(&mut self) -> Result<(), CommandError>;

        /// 只显示 `day` 那天更新的新番。
        #[idempotent]
        fn show_day(&mut self, day: Weekday) -> Result<(), CommandError>;

        /// 把一个文件夹加入媒体库，并在后台数出里面的视频。
        fn add_folder(&mut self, folder: MediaSource) -> Result<(), CommandError>;

        /// 把编号为 `id` 的文件夹移出媒体库；不删除任何文件。
        #[idempotent]
        fn remove_folder(&mut self, id: i64) -> Result<(), CommandError>;
    }
}
