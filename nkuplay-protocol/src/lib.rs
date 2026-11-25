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

    /// 详情页剧集的分组。
    #[value]
    pub enum DetailGroup {
        #[label(tr! { "en" => "Episodes", "zh-Hans" => "正片" })]
        Main,
        #[label(tr! { "en" => "Specials", "zh-Hans" => "特别篇" })]
        Special,
        #[label(tr! { "en" => "OP / ED", "zh-Hans" => "OP / ED" })]
        Extra,
    }

    /// 继续播放里的一部番（最近看的那一集），或没认出的一个视频。
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
        /// 卡片上的画面：上次离开时的，全黑时换成这一集的剧照；`has_shot` 为假时没有意义。
        pub shot: MediaSource,
        pub has_shot: bool,
        /// 看了百分之几。
        pub percent: i64,
        /// 卡片上的进度条画多长，0 到 1；刚开始看的也露出一点。
        pub bar: f64,
        /// 剧集名里的“第N话”；没有时为空。
        #[content]
        pub part: String,
        /// 作品类别，例如“TV动画”；不知道时为空。
        #[content]
        pub kind: String,
        /// 弹弹play 的作品编号，有详情页；没认出时为 0。
        pub anime_id: i64,
        /// 首页大图；`has_backdrop` 为假时没有意义。
        pub backdrop: MediaSource,
        pub has_backdrop: bool,
        /// 大图是剧照或离开时的画面（清晰显示）；否则是海报（虚化显示）。
        pub sharp: bool,
    }

    /// 首页大图下面的一个翻页点。
    #[value]
    pub struct HeroDot {
        pub id: i64,
        /// 正在显示这一张。
        pub on: bool,
    }

    /// 首页上一个看不见的记号，用来知道滚到了哪里。
    #[value]
    pub struct HomeMark {
        /// 从 100 起。滚动位置按各项的编号报告，所以和首页别的列表（大图、翻页点是 1 起，
        /// 星期是 201 起）错开。
        pub id: i64,
        /// 到下一个记号有多高。
        pub height: f64,
    }

    /// 本季新番上面的一个星期按钮。
    #[value]
    pub struct DayChip {
        pub id: i64,
        pub day: Weekday,
        /// 正在看这一天。
        pub on: bool,
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
        /// 播出时间（北京时间，例如“22:30”）；不知道时为空。
        #[content]
        pub time: String,
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

    /// 媒体库海报墙上的一部番：同一作品在各个文件夹里的剧集合在一起。
    #[value]
    pub struct Series {
        pub id: i64,
        /// 弹弹play 认出的作品名；没认出时是从文件名里认出的。
        #[content]
        pub title: String,
        /// 海报；`has_poster` 为假时没有意义。
        pub poster: MediaSource,
        pub has_poster: bool,
        /// 有几集。
        pub episodes: i64,
        /// 看过（看完或看了一部分）的最后一集是第几话；都没看过时为 0。
        pub watched_up_to: i64,
        /// 每一集都看完了。
        pub finished: bool,
        /// 看完了几分之几的集，0 到 1：海报底边的进度。
        pub bar: f64,
        /// “继续”要播的那一集是第几话。
        pub resume_number: i64,
    }

    /// 详情页剧集列表里的一集：弹弹play 的一集（对上了本地文件或没对上），或本地多出来的文件。
    #[value]
    pub struct DetailEpisode {
        pub id: i64,
        /// 特别篇和 OP/ED 的短编号（`S1`、`C1`）；正片和本地文件为空。
        #[content]
        pub code: String,
        /// 第几集；特别篇和认不出集数的文件为 0。
        pub number: i64,
        /// 剧集标题（不含“第N话”），本地多出来的文件是文件名。
        #[content]
        pub title: String,
        /// 本地有这一集，点了能播放。
        pub local: bool,
        /// 本地文件；`local` 为假时没有意义。
        pub source: MediaSource,
        /// 看了百分之几；没看过为 0。
        pub percent: i64,
        /// 看完了（播到 90% 以上）。
        pub finished: bool,
        /// “继续”指向这一集。
        pub current: bool,
        /// 缩略图（这一集的第一张剧照）；`has_thumb` 为假时没有意义。
        pub thumb: MediaSource,
        pub has_thumb: bool,
    }

    /// 作品的一个标签。
    #[value]
    pub struct DetailTag {
        pub id: i64,
        #[content]
        pub name: String,
    }

    /// 一条短评。
    #[value]
    pub struct DetailComment {
        pub id: i64,
        #[content]
        pub user: String,
        /// 1 到 10；没打分时为 0。
        pub score: i64,
        #[content]
        pub text: String,
    }

    /// 一个网站的评分，例如“Bangumi评分”和“6.4”。
    #[value]
    pub struct DetailScore {
        pub id: i64,
        #[content]
        pub site: String,
        #[content]
        pub score: String,
    }

    /// 制作信息的一条，例如“导演”和“小出卓史”。
    #[value]
    pub struct DetailFact {
        pub id: i64,
        #[content]
        pub key: String,
        #[content]
        pub value: String,
    }

    /// 详情页的背景图。换图时编号变，界面借此淡入淡出。
    #[value]
    pub struct Backdrop {
        pub id: i64,
        pub source: MediaSource,
        /// 是剧照（清晰显示）；否则是海报（虚化显示）。
        pub still: bool,
    }

    /// 界面跟随系统，或固定浅色、深色。
    #[value]
    pub enum Appearance {
        #[label(tr! { "en" => "System", "zh-Hans" => "跟随系统" })]
        System,
        #[label(tr! { "en" => "Light", "zh-Hans" => "浅色" })]
        Light,
        #[label(tr! { "en" => "Dark", "zh-Hans" => "深色" })]
        Dark,
    }

    /// 顶栏能放的页面。
    #[value]
    pub enum Page {
        #[label(tr! { "en" => "Home", "zh-Hans" => "首页" })]
        Home,
        #[label(tr! { "en" => "Library", "zh-Hans" => "媒体库" })]
        Library,
        #[label(tr! { "en" => "Schedule", "zh-Hans" => "新番时间表" })]
        Schedule,
        #[label(tr! { "en" => "History", "zh-Hans" => "观看历史" })]
        History,
    }

    /// 一集播完以后做什么。
    #[value]
    pub enum AfterEnd {
        #[label(tr! { "en" => "Next", "zh-Hans" => "下一话" })]
        Next,
        #[label(tr! { "en" => "Loop", "zh-Hans" => "循环" })]
        Loop,
        #[label(tr! { "en" => "Stop", "zh-Hans" => "停住" })]
        Stop,
        #[label(tr! { "en" => "Back", "zh-Hans" => "返回" })]
        Back,
    }

    /// 片头片尾（按 mkv 章节认）怎么处理。
    #[value]
    pub enum SkipMode {
        #[label(tr! { "en" => "Off", "zh-Hans" => "不提示" })]
        Off,
        #[label(tr! { "en" => "Button", "zh-Hans" => "显示按钮" })]
        Button,
        #[label(tr! { "en" => "Auto", "zh-Hans" => "自动跳过" })]
        Auto,
    }

    /// Anime4K 超分的档位。
    #[value]
    pub enum Upscale {
        #[label(tr! { "en" => "Off", "zh-Hans" => "关闭" })]
        Off,
        #[label(tr! { "en" => "Light", "zh-Hans" => "轻量" })]
        Light,
        #[label(tr! { "en" => "Standard", "zh-Hans" => "标准" })]
        Standard,
        #[label(tr! { "en" => "High", "zh-Hans" => "高质量" })]
        High,
    }

    /// 画面比例。
    #[value]
    pub enum Aspect {
        #[label(tr! { "en" => "Match Video", "zh-Hans" => "跟随视频" })]
        Auto,
        #[label(tr! { "en" => "16:9", "zh-Hans" => "16:9" })]
        Wide,
        #[label(tr! { "en" => "4:3", "zh-Hans" => "4:3" })]
        Classic,
        #[label(tr! { "en" => "21:9", "zh-Hans" => "21:9" })]
        Cinema,
        #[label(tr! { "en" => "Stretch", "zh-Hans" => "填满" })]
        Stretch,
    }

    /// 画面比例和窗口不一样时怎么摆。
    #[value]
    pub enum Fill {
        #[label(tr! { "en" => "Fit", "zh-Hans" => "完整显示" })]
        Fit,
        #[label(tr! { "en" => "Crop", "zh-Hans" => "裁掉黑边" })]
        Crop,
    }

    /// 弹幕的字体。
    #[value]
    pub enum DanmakuFont {
        #[label(tr! { "en" => "Microsoft YaHei", "zh-Hans" => "微软雅黑" })]
        YaHei,
        #[label(tr! { "en" => "SimHei", "zh-Hans" => "黑体" })]
        Hei,
        #[label(tr! { "en" => "DengXian", "zh-Hans" => "等线" })]
        Deng,
        #[label(tr! { "en" => "SimSun", "zh-Hans" => "宋体" })]
        Song,
    }

    /// 设置里的开关。
    #[value]
    pub enum Toggle {
        #[label(tr! { "en" => "Resume", "zh-Hans" => "从上次的位置继续" })]
        Resume,
        #[label(tr! { "en" => "Full screen on start", "zh-Hans" => "自动全屏" })]
        AutoFullscreen,
        #[label(tr! { "en" => "Keep pitch", "zh-Hans" => "倍速时保持音调" })]
        KeepPitch,
        #[label(tr! { "en" => "Chapters", "zh-Hans" => "章节标记" })]
        Chapters,
        #[label(tr! { "en" => "Danmaku heat", "zh-Hans" => "弹幕热度" })]
        Heat,
        #[label(tr! { "en" => "Hover preview", "zh-Hans" => "悬停预览图" })]
        Preview,
        #[label(tr! { "en" => "Hardware decoding", "zh-Hans" => "硬件解码" })]
        Hwdec,
        #[label(tr! { "en" => "Overlap", "zh-Hans" => "允许重叠" })]
        Overlap,
        #[label(tr! { "en" => "Scrolling", "zh-Hans" => "滚动" })]
        Scroll,
        #[label(tr! { "en" => "Top", "zh-Hans" => "顶部" })]
        Top,
        #[label(tr! { "en" => "Bottom", "zh-Hans" => "底部" })]
        Bottom,
        #[label(tr! { "en" => "Colors", "zh-Hans" => "彩色" })]
        Colored,
        #[label(tr! { "en" => "Merge repeats", "zh-Hans" => "合并重复" })]
        Merge,
    }

    /// 设置里的数值。
    #[value]
    pub enum Amount {
        #[label(tr! { "en" => "Countdown", "zh-Hans" => "下一话倒计时" })]
        Countdown,
        #[label(tr! { "en" => "Manual skip", "zh-Hans" => "手动跳过" })]
        SkipSeconds,
        #[label(tr! { "en" => "Seek step", "zh-Hans" => "快进快退" })]
        SeekStep,
        #[label(tr! { "en" => "Hold speed", "zh-Hans" => "长按倍速" })]
        HoldSpeed,
        #[label(tr! { "en" => "Bar stays", "zh-Hans" => "控制栏停留" })]
        BarStay,
        #[label(tr! { "en" => "Cache", "zh-Hans" => "预缓存" })]
        Cache,
        #[label(tr! { "en" => "Opacity", "zh-Hans" => "不透明度" })]
        Opacity,
        #[label(tr! { "en" => "Size", "zh-Hans" => "字号" })]
        Size,
        #[label(tr! { "en" => "Outline", "zh-Hans" => "描边" })]
        Outline,
        #[label(tr! { "en" => "Area", "zh-Hans" => "显示区域" })]
        Area,
        #[label(tr! { "en" => "Scroll time", "zh-Hans" => "滚动时间" })]
        ScrollTime,
        #[label(tr! { "en" => "Line gap", "zh-Hans" => "行距" })]
        Gap,
    }

    /// 能改按键的操作。
    #[value]
    pub enum KeyAction {
        #[label(tr! { "en" => "Play / Pause", "zh-Hans" => "播放 / 暂停" })]
        Play,
        #[label(tr! { "en" => "Back", "zh-Hans" => "快退" })]
        Rewind,
        #[label(tr! { "en" => "Forward", "zh-Hans" => "快进" })]
        Forward,
        #[label(tr! { "en" => "Previous", "zh-Hans" => "上一话" })]
        Previous,
        #[label(tr! { "en" => "Next", "zh-Hans" => "下一话" })]
        Next,
        #[label(tr! { "en" => "Skip", "zh-Hans" => "跳过" })]
        Skip,
        #[label(tr! { "en" => "Faster", "zh-Hans" => "加快 0.1" })]
        Faster,
        #[label(tr! { "en" => "Slower", "zh-Hans" => "减慢 0.1" })]
        Slower,
        #[label(tr! { "en" => "Normal speed", "zh-Hans" => "恢复 1 倍" })]
        NormalSpeed,
        #[label(tr! { "en" => "Hold to speed up", "zh-Hans" => "长按快进" })]
        Hold,
        #[label(tr! { "en" => "Louder", "zh-Hans" => "增大音量" })]
        Louder,
        #[label(tr! { "en" => "Quieter", "zh-Hans" => "减小音量" })]
        Quieter,
        #[label(tr! { "en" => "Mute", "zh-Hans" => "静音" })]
        Mute,
        #[label(tr! { "en" => "Danmaku", "zh-Hans" => "显示 / 隐藏弹幕" })]
        Danmaku,
        #[label(tr! { "en" => "Danmaku panel", "zh-Hans" => "弹幕面板" })]
        DanmakuPanel,
        #[label(tr! { "en" => "Subtitles", "zh-Hans" => "切换字幕" })]
        Subtitle,
        #[label(tr! { "en" => "Audio", "zh-Hans" => "切换音轨" })]
        Audio,
        #[label(tr! { "en" => "Full screen", "zh-Hans" => "全屏" })]
        Fullscreen,
        #[label(tr! { "en" => "Exit full screen", "zh-Hans" => "退出全屏" })]
        ExitFullscreen,
        #[label(tr! { "en" => "Episodes", "zh-Hans" => "选集" })]
        Episodes,
        #[label(tr! { "en" => "Playback info", "zh-Hans" => "播放信息" })]
        Info,
    }

    /// 快进快退一次几秒。
    #[value]
    pub enum SeekStep {
        #[label(tr! { "en" => "1 s", "zh-Hans" => "1 秒" })]
        One,
        #[label(tr! { "en" => "5 s", "zh-Hans" => "5 秒" })]
        Five,
        #[label(tr! { "en" => "10 s", "zh-Hans" => "10 秒" })]
        Ten,
        #[label(tr! { "en" => "15 s", "zh-Hans" => "15 秒" })]
        Fifteen,
        #[label(tr! { "en" => "30 s", "zh-Hans" => "30 秒" })]
        Thirty,
    }

    /// 长按快进的倍速。
    #[value]
    pub enum HoldRate {
        #[label(tr! { "en" => "1.5×", "zh-Hans" => "1.5×" })]
        OneAndHalf,
        #[label(tr! { "en" => "2.0×", "zh-Hans" => "2.0×" })]
        Two,
        #[label(tr! { "en" => "3.0×", "zh-Hans" => "3.0×" })]
        Three,
        #[label(tr! { "en" => "4.0×", "zh-Hans" => "4.0×" })]
        Four,
    }

    /// 控制栏停留几秒。
    #[value]
    pub enum BarStay {
        #[label(tr! { "en" => "1 s", "zh-Hans" => "1 秒" })]
        One,
        #[label(tr! { "en" => "2 s", "zh-Hans" => "2 秒" })]
        Two,
        #[label(tr! { "en" => "3 s", "zh-Hans" => "3 秒" })]
        Three,
        #[label(tr! { "en" => "5 s", "zh-Hans" => "5 秒" })]
        Five,
    }

    /// 媒体库按观看状态筛选。
    #[value]
    pub enum LibraryFilter {
        #[label(tr! { "en" => "All", "zh-Hans" => "全部" })]
        All,
        #[label(tr! { "en" => "Watching", "zh-Hans" => "在看" })]
        Watching,
        #[label(tr! { "en" => "Not started", "zh-Hans" => "没看" })]
        Unwatched,
        #[label(tr! { "en" => "Finished", "zh-Hans" => "看完" })]
        Finished,
    }

    /// 媒体库的排序。
    #[value]
    pub enum LibrarySort {
        #[label(tr! { "en" => "Recently added", "zh-Hans" => "最近加入" })]
        Added,
        #[label(tr! { "en" => "Name", "zh-Hans" => "名称" })]
        Name,
        #[label(tr! { "en" => "Recently watched", "zh-Hans" => "最近观看" })]
        Watched,
    }

    /// 设置页里能恢复默认的一类。
    #[value]
    pub enum Section {
        #[label(tr! { "en" => "Playback", "zh-Hans" => "播放" })]
        Play,
        #[label(tr! { "en" => "Picture", "zh-Hans" => "画面" })]
        Picture,
        #[label(tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" })]
        Danmaku,
        #[label(tr! { "en" => "Shortcuts", "zh-Hans" => "快捷键" })]
        Keys,
    }

    /// 关于页里的链接。
    #[value]
    pub enum Link {
        #[label(tr! { "en" => "GitHub", "zh-Hans" => "GitHub" })]
        Source,
        #[label(tr! { "en" => "dandanplay", "zh-Hans" => "弹弹play" })]
        Dandanplay,
        #[label(tr! { "en" => "TMDB", "zh-Hans" => "TMDB" })]
        Tmdb,
        #[label(tr! { "en" => "bangumi-data", "zh-Hans" => "bangumi-data" })]
        BangumiData,
    }

    /// 顶栏里的一页：开着就显示在顶栏上。
    #[value]
    pub struct NavItem {
        pub id: i64,
        pub page: Page,
        pub on: bool,
    }

    /// 一个屏蔽词；写成 `/…/` 的是正则表达式。
    #[value]
    pub struct Word {
        pub id: i64,
        #[content]
        pub text: String,
    }

    /// 每个操作的按键，按 Bern 的写法，例如 `shift+right`。
    #[value]
    pub struct Keys {
        pub play: String,
        pub rewind: String,
        pub forward: String,
        pub previous: String,
        pub next: String,
        pub skip: String,
        pub faster: String,
        pub slower: String,
        pub normal_speed: String,
        pub hold: String,
        /// 长按快进键帽上写的键，例如“→”。
        #[content]
        pub hold_label: String,
        pub louder: String,
        pub quieter: String,
        pub mute: String,
        pub danmaku: String,
        pub danmaku_panel: String,
        pub subtitle: String,
        pub audio: String,
        pub fullscreen: String,
        pub exit_fullscreen: String,
        pub episodes: String,
        pub info: String,
    }

    /// 播放页选集里的一集。
    #[value]
    pub struct PlayEpisode {
        pub id: i64,
        /// “第8话”。
        #[content]
        pub code: String,
        #[content]
        pub title: String,
        pub thumb: MediaSource,
        pub has_thumb: bool,
        /// “已看完”“看到 35%”“正在播放”“未找到”，没看过时为空。
        #[content]
        pub note: String,
        pub current: bool,
        /// 本地有这一集的文件。
        pub local: bool,
        /// 看到一半时缩略图下面的进度，0 到 1；没看或看完时为 0。
        pub bar: f64,
    }

    /// 播放信息的一行。
    #[value]
    pub struct InfoRow {
        pub id: i64,
        #[content]
        pub key: String,
        #[content]
        pub value: String,
    }

    /// 字幕页的一条字幕轨。
    #[value]
    pub struct SubtitleRow {
        /// mpv 的轨道编号。
        pub id: i64,
        /// “中文 (Subtitle track 0)”。
        #[content]
        pub name: String,
        /// “语言：中文”，导入的写“导入的文件”。
        #[content]
        pub note: String,
        pub on: bool,
    }

    /// 弹幕列表的一条。
    #[value]
    pub struct DanmakuLine {
        pub id: i64,
        /// 出现时间，`7:03`。
        #[content]
        pub time: String,
        #[content]
        pub text: String,
        /// 离现在的播放位置最近的那一条。
        pub now: bool,
    }

    /// 手动匹配时搜到的一部作品。
    #[value]
    pub struct MatchAnime {
        pub id: i64,
        #[content]
        pub title: String,
        /// “TV动画 · 12 话”。
        #[content]
        pub note: String,
        pub chosen: bool,
    }

    /// 手动匹配时选中作品的一集。
    #[value]
    pub struct MatchEpisode {
        pub id: i64,
        #[content]
        pub title: String,
        pub chosen: bool,
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
        /// 弹幕显示在画面上；换视频时保持不变。
        pub danmaku_visible: bool,

        /// 每打开一次视频加一。界面看到它变化，就让播放器载入 `source`。
        pub opened: i64,
        /// 每请求一次跳转加一；界面看到它变化，就让播放器跳到 `seek_ms`。
        /// 界面不能做算术，进度条给的比例由引擎换算成毫秒。
        pub seek_serial: i64,
        /// 要跳到的位置，毫秒。
        pub seek_ms: i64,
        /// 想要的音量，0 到 1。界面看到它变化就交给播放器；换视频时保持不变。
        pub volume: f64,

        /// 继续播放：每部番只留最近看的一集，最近的在前。
        pub recents: Vec<Recent>,
        /// `recents` 的个数。
        pub recent_count: i64,
        /// 首页大图正在显示的那一部：没有或一个。
        pub hero: Vec<Recent>,
        /// 大图的翻页点，和 `recents` 一一对应。
        pub hero_dots: Vec<HeroDot>,
        /// 首页大图旁的记号：到了 102，大图就滚出去了。
        pub home_marks: Vec<HomeMark>,

        /// 今天更新的新番，和今天是星期几。
        pub today: Vec<ShinAnime>,
        pub today_count: i64,
        pub today_day: Weekday,

        /// 本季新番列表的阶段。
        pub shin: ShinStatus,
        /// 缓存的新番列表是上一季或更早取的，应当刷新。
        pub shin_outdated: bool,
        /// 列表是哪一年（文字，免得显示成 2,026）、从几月开始的那一季；没有列表时年份为空。
        #[content]
        pub shin_year: String,
        pub shin_month: i64,
        /// 正在看哪一天更新的新番；默认是今天。
        pub shin_day: Weekday,
        /// 周一到周日的按钮。
        pub shin_days: Vec<DayChip>,
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

        /// 媒体库海报墙，按作品名排好。
        pub series: Vec<Series>,
        /// `series` 的个数。
        pub series_count: i64,
        /// 媒体库页显示的番：按筛选和排序，编号和 `series` 里的一样。
        pub library_shown: Vec<Series>,
        pub library_filter: LibraryFilter,
        pub library_sort: LibrarySort,
        /// 各个筛选下有几部。
        pub count_all: i64,
        pub count_watching: i64,
        pub count_unwatched: i64,
        pub count_finished: i64,
        /// 还有文件夹在扫描或识别。
        pub library_scanning: bool,

        /// 番剧详情页打开着：窗口不显示导航，只显示这一页。
        pub detail_open: bool,
        /// 作品名。
        #[content]
        pub detail_title: String,
        /// 日文标题；没有或和作品名一样时为空。
        #[content]
        pub detail_native_title: String,
        pub detail_poster: MediaSource,
        pub detail_has_poster: bool,

        /// 背景图：没有或一张。
        pub detail_backdrop: Vec<Backdrop>,
        /// 背景在轮换（不固定，而且有不止一张剧照）：界面定时换下一张。
        pub detail_rotates: bool,
        /// 背景固定一张；每部番各记各的，默认轮换。
        pub detail_fixed: bool,
        /// 有不止一张剧照可以切换。
        pub detail_can_switch: bool,
        /// 弹弹play 的资料已经有了（下面的评分、类别、简介等有意义）。
        pub detail_known: bool,
        /// 正在向弹弹play 取资料。
        pub detail_loading: bool,
        /// 取资料失败的原因；没有失败时为空。
        #[content]
        pub detail_error: String,
        /// 综合评分，0 到 10；没有评分时为 0。
        pub detail_rating: f64,
        /// 评分写成一位小数。
        #[content]
        pub detail_rating_text: String,
        /// 作品类别，例如“TV动画”。
        #[content]
        pub detail_kind: String,
        /// 开播的年份（文字，免得显示成 2,026）和月份；`detail_has_premiere` 为假时没有意义。
        #[content]
        pub detail_year: String,
        pub detail_month: i64,
        pub detail_has_premiere: bool,
        /// 还在连载。
        pub detail_on_air: bool,
        pub detail_tags: Vec<DetailTag>,
        #[content]
        pub detail_summary: String,

        /// 正在看哪一组剧集，以及这一组的剧集。
        pub detail_group: DetailGroup,
        pub detail_episodes: Vec<DetailEpisode>,
        /// 剧集条该滚到哪一集；`detail_strip_serial` 每变一次就滚一次。
        pub detail_strip_at: i64,
        pub detail_strip_serial: i64,
        /// 三组各有几集，弹弹play 列出的正片集数，和一集多少分钟（不知道时为 0）。
        pub detail_main_count: i64,
        pub detail_special_count: i64,
        pub detail_extra_count: i64,
        pub detail_episode_count: i64,
        pub detail_minutes: i64,
        /// 本地有可以播放的一集：“继续”按钮可用，播放 `detail_resume`。
        pub detail_can_resume: bool,
        pub detail_resume: MediaSource,
        /// 这部番看过（按钮写“继续”，否则写“播放”）。
        pub detail_started: bool,
        /// “继续”那一集看到总时长的几分之几，0 到 1。
        pub detail_resume_fraction: f64,

        /// 短评（第一页）、一共多少条、来自哪个网站。
        pub detail_comments: Vec<DetailComment>,
        /// 这一页的评论比显示的多，可以展开；展开着时为假。
        pub detail_comments_more: bool,
        /// 评论展开着（显示这一页全部）。
        pub detail_comments_expanded: bool,
        pub detail_comment_count: i64,
        #[content]
        pub detail_comment_source: String,
        pub detail_scores: Vec<DetailScore>,
        pub detail_facts: Vec<DetailFact>,
        // ——设置（存在硬盘上）——
        pub appearance: Appearance,
        pub start_page: Page,
        /// 顶栏的页面，按顺序；关着的也在里面。
        pub nav: Vec<NavItem>,
        /// 顶栏上显示的页面（开着的那些）。
        pub nav_shown: Vec<NavItem>,
        pub resume: bool,
        pub auto_fullscreen: bool,
        pub after_end: AfterEnd,
        /// 下一话倒计时几秒。
        pub countdown: f64,
        pub skip_mode: SkipMode,
        /// 按 S 往后跳几秒。
        pub skip_seconds: f64,
        /// 快进快退几秒。
        pub seek_step: i64,
        /// 长按快进的倍速，乘以 10（15、20、30、40）。
        pub hold_speed: i64,
        pub keep_pitch: bool,
        /// 控制栏停留几秒。
        pub bar_stay: i64,
        /// 上面三项的档位，给选择器用。
        pub seek_choice: SeekStep,
        pub hold_choice: HoldRate,
        pub stay_choice: BarStay,
        pub chapter_marks: bool,
        pub heat_curve: bool,
        pub hover_preview: bool,
        pub hwdec: bool,
        /// 预缓存多少 MB。
        pub cache_mb: f64,
        pub upscale: Upscale,
        pub aspect: Aspect,
        pub fill: Fill,
        /// 弹幕不透明度，10 到 100。
        pub d_opacity: f64,
        pub d_size: f64,
        pub d_outline: f64,
        pub d_font: DanmakuFont,
        /// 显示区域，25 到 100。
        pub d_area: f64,
        pub d_seconds: f64,
        /// 行距，0 到 50（字号的百分之几）。
        pub d_gap: f64,
        pub d_overlap: bool,
        pub d_scroll: bool,
        pub d_top: bool,
        pub d_bottom: bool,
        pub d_colored: bool,
        pub d_merge: bool,
        pub blocked: Vec<Word>,
        pub blocked_count: i64,
        pub keys: Keys,

        // ——播放页——
        /// 番名；没认出时是文件名。
        #[content]
        pub play_title: String,
        /// “第7话 星期六10点，在琵琶湖露台”；没认出时为空。
        #[content]
        pub play_subtitle: String,
        pub speed: f64,
        /// “1.0×”。
        #[content]
        pub speed_text: String,
        /// 正按住长按快进。
        pub fast: bool,
        pub has_previous: bool,
        pub has_next: bool,
        pub episodes: Vec<PlayEpisode>,
        pub episode_count: i64,
        /// 音量的百分数，0 到 100。
        pub volume_percent: i64,
        /// 每一章从哪里开始，0 到 1（第一章的 0 不算）。章节标记关着时为空。
        pub chapters: Vec<f64>,
        /// 弹幕热度，120 段，0 到 1。热度关着时为空。
        pub heat: Vec<f64>,
        /// 正在片头或片尾里，显示“跳过”按钮。
        pub skip_shown: bool,
        /// “到 2:09”。
        #[content]
        pub skip_label: String,
        /// 片头或片尾：按钮上写“跳过片头”还是“跳过片尾”。
        pub skip_outro: bool,
        /// 下一话倒计时卡片。
        pub next_shown: bool,
        pub next_left: i64,
        #[content]
        pub next_code: String,
        #[content]
        pub next_title: String,
        pub next_thumb: MediaSource,
        pub next_has_thumb: bool,
        pub info: Vec<InfoRow>,
        /// 字幕页：视频里的字幕轨和导入的字幕文件。
        pub subtitles: Vec<SubtitleRow>,
        pub subtitle_count: i64,
        /// 弹幕的时间偏移（秒），用户自己调的。
        pub danmaku_shift: f64,
        #[content]
        pub danmaku_shift_text: String,
        pub danmaku_lines: Vec<DanmakuLine>,
        /// 弹幕从哪里来：“弹弹play”或文件名。
        #[content]
        pub danmaku_source: String,
        pub danmaku_from_file: bool,
        /// 悬停预览图：图片和它是第几秒的画面。没有时 `preview_ready` 为假。
        pub preview: MediaSource,
        pub preview_ready: bool,
        // ——手动匹配——
        pub match_open: bool,
        #[content]
        pub match_file: String,
        pub match_busy: bool,
        pub match_animes: Vec<MatchAnime>,
        pub match_episodes: Vec<MatchEpisode>,
        pub match_folder: bool,
        pub match_ready: bool,
        /// 刚打开的视频要自动全屏，还没进入。
        pub fullscreen_pending: bool,
    }

    #[commands]
    pub trait Commands {
        /// 打开一个视频文件并播放，同时识别它并获取弹幕。上次看到一半的，从那里接着播。
        fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError>;

        /// 停止播放，回到首页。
        #[idempotent]
        fn close(&mut self) -> Result<(), CommandError>;

        /// 跳到总时长的 `fraction` 处（0 到 1）。`duration_ms` 是播放器报的总时长，
        /// 不知道时为空，此时不跳。
        fn seek(&mut self, fraction: f64, duration_ms: Option<i64>) -> Result<(), CommandError>;

        /// 把音量设为 `volume`（0 到 1）。
        #[idempotent]
        fn set_volume(&mut self, volume: f64) -> Result<(), CommandError>;

        /// 音量加 `step`（可以是负数），结果限制在 0 到 1。
        fn nudge_volume(&mut self, step: f64) -> Result<(), CommandError>;

        /// 从 `position_ms`（播放器报的当前位置）往后跳 `seconds` 秒，负数往前跳，
        /// 结果限制在 0 到总时长。连按时在上一次要跳到的位置上接着加减。
        fn skip(
            &mut self,
            seconds: f64,
            position_ms: i64,
            duration_ms: Option<i64>,
        ) -> Result<(), CommandError>;

        /// 显示或隐藏画面上的弹幕。
        fn toggle_danmaku(&mut self) -> Result<(), CommandError>;

        /// 重新整理继续观看：去掉文件已经不在了的视频。
        fn refresh_recents(&mut self) -> Result<(), CommandError>;

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

        /// 重新扫描所有文件夹：归出番剧，对还没认出的每部请弹弹play 认一集。
        fn refresh_library(&mut self) -> Result<(), CommandError>;

        /// 打开海报墙上编号为 `id` 的番的详情页。
        #[idempotent]
        fn open_series(&mut self, id: i64) -> Result<(), CommandError>;

        /// 打开弹弹play 作品 `anime_id`（例如本季新番里的一部）的详情页。
        #[idempotent]
        fn open_anime(&mut self, anime_id: i64) -> Result<(), CommandError>;

        /// 关掉详情页，回到原来那一页。
        #[idempotent]
        fn close_details(&mut self) -> Result<(), CommandError>;

        /// 不管缓存，向弹弹play 重新取详情页的资料。
        fn refresh_details(&mut self) -> Result<(), CommandError>;

        /// 剧集条只显示 `group` 这一组。
        #[idempotent]
        fn show_group(&mut self, group: DetailGroup) -> Result<(), CommandError>;

        /// 剧集条从现在最前面的那一集（编号 `at`）往后翻 `pages` 屏，负数往前翻。
        fn page_episodes(&mut self, at: i64, pages: i64) -> Result<(), CommandError>;

        /// 把“继续”指向的那一集记成看完了。
        fn mark_watched(&mut self) -> Result<(), CommandError>;

        /// 背景固定一张（`fixed` 为真），或轮换。每部番各记各的。
        #[idempotent]
        fn set_backdrop_fixed(&mut self, fixed: bool) -> Result<(), CommandError>;

        /// 首页出现：为继续播放里的番准备大图。
        #[idempotent]
        fn load_home(&mut self) -> Result<(), CommandError>;

        /// 首页大图换成继续播放里编号为 `id` 的那一部。
        #[idempotent]
        fn show_hero(&mut self, id: i64) -> Result<(), CommandError>;

        /// 首页大图往后换 `step` 部，负数往前，到头了从另一头接着来。
        fn step_hero(&mut self, step: i64) -> Result<(), CommandError>;

        /// 忘掉详情页这部番每一集看到哪里。
        fn clear_progress(&mut self) -> Result<(), CommandError>;

        /// 评论在只显示几条和显示这一页全部之间切换。
        fn toggle_comments(&mut self) -> Result<(), CommandError>;

        /// 背景换下一张剧照；固定着的就固定在换到的那一张上。
        fn next_backdrop(&mut self) -> Result<(), CommandError>;

        /// 记下正在看的视频看到了哪里：`position_ms` 和 `duration_ms` 是播放器报的位置和总时长。
        /// 刚打开视频的几秒里不记（播放器报的可能还是上一个视频的）。
        fn record_progress(
            &mut self,
            position_ms: i64,
            duration_ms: Option<i64>,
        ) -> Result<(), CommandError>;

        /// 把编号为 `id` 的文件夹移出媒体库；不删除任何文件。
        #[idempotent]
        fn remove_folder(&mut self, id: i64) -> Result<(), CommandError>;
        #[idempotent]
        fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError>;

        #[idempotent]
        fn set_start_page(&mut self, page: Page) -> Result<(), CommandError>;

        /// 顶栏显示或不显示这一页；至少留一页。
        #[idempotent]
        fn show_page(&mut self, page: Page, on: bool) -> Result<(), CommandError>;

        /// 把顶栏第 `from` 个页面挪到第 `to` 个（从 0 数）。
        fn move_page(&mut self, from: i64, to: i64) -> Result<(), CommandError>;

        #[idempotent]
        fn set_toggle(&mut self, toggle: Toggle, on: bool) -> Result<(), CommandError>;

        #[idempotent]
        fn set_amount(&mut self, amount: Amount, value: f64) -> Result<(), CommandError>;

        #[idempotent]
        fn set_seek_step(&mut self, step: SeekStep) -> Result<(), CommandError>;

        #[idempotent]
        fn set_hold_rate(&mut self, rate: HoldRate) -> Result<(), CommandError>;

        #[idempotent]
        fn set_bar_stay(&mut self, stay: BarStay) -> Result<(), CommandError>;

        #[idempotent]
        fn set_after_end(&mut self, after: AfterEnd) -> Result<(), CommandError>;

        #[idempotent]
        fn set_skip_mode(&mut self, mode: SkipMode) -> Result<(), CommandError>;

        #[idempotent]
        fn set_upscale(&mut self, upscale: Upscale) -> Result<(), CommandError>;

        #[idempotent]
        fn set_aspect(&mut self, aspect: Aspect) -> Result<(), CommandError>;

        #[idempotent]
        fn set_fill(&mut self, fill: Fill) -> Result<(), CommandError>;

        #[idempotent]
        fn set_danmaku_font(&mut self, font: DanmakuFont) -> Result<(), CommandError>;

        /// 加一个屏蔽词；空的、重复的不加。
        fn add_blocked(&mut self, text: String) -> Result<(), CommandError>;

        #[idempotent]
        fn remove_blocked(&mut self, id: i64) -> Result<(), CommandError>;

        /// 换一个操作的按键；空的表示不用按键。
        #[idempotent]
        fn set_key(&mut self, action: KeyAction, keys: String) -> Result<(), CommandError>;

        /// 这一类设置恢复默认。
        fn reset_section(&mut self, section: Section) -> Result<(), CommandError>;

        /// 用浏览器打开关于页里的链接。
        fn open_link(&mut self, link: Link) -> Result<(), CommandError>;

        /// 播放速度，0.25 到 4。
        #[idempotent]
        fn set_speed(&mut self, speed: f64) -> Result<(), CommandError>;

        /// 在当前速度上加减。
        fn nudge_speed(&mut self, step: f64) -> Result<(), CommandError>;

        /// 按住长按快进键（为真）或松开。
        #[idempotent]
        fn set_fast(&mut self, fast: bool) -> Result<(), CommandError>;

        /// 播放器每秒报一次位置：片头片尾按钮、自动跳过、播完以后都靠它。
        fn tick(&mut self, position_ms: i64, duration_ms: Option<i64>) -> Result<(), CommandError>;

        /// 跳过片头或片尾。
        fn skip_part(&mut self) -> Result<(), CommandError>;

        /// 往后跳“手动跳过”的秒数。
        fn skip_ahead(
            &mut self,
            position_ms: i64,
            duration_ms: Option<i64>,
        ) -> Result<(), CommandError>;

        fn play_previous(&mut self) -> Result<(), CommandError>;

        fn play_next(&mut self) -> Result<(), CommandError>;

        fn cancel_next(&mut self) -> Result<(), CommandError>;

        /// 播放选集里的一集。
        fn play_episode(&mut self, id: i64) -> Result<(), CommandError>;

        fn cycle_subtitle(&mut self) -> Result<(), CommandError>;

        fn cycle_audio(&mut self) -> Result<(), CommandError>;

        fn add_subtitle(&mut self, file: MediaSource) -> Result<(), CommandError>;

        /// 显示第 `id` 条字幕轨。
        fn select_subtitle(&mut self, id: i64) -> Result<(), CommandError>;

        /// 读一遍播放信息。
        fn load_info(&mut self) -> Result<(), CommandError>;

        /// 弹幕列表：`position_ms` 附近的弹幕，有 `query` 时是含有它的全部。
        fn list_danmaku(&mut self, position_ms: i64, query: String) -> Result<(), CommandError>;

        /// 屏蔽列表里的这一条（加成屏蔽词）。
        fn block_line(&mut self, id: i64) -> Result<(), CommandError>;

        /// 在弹幕时间偏移上加减（秒）。
        fn nudge_shift(&mut self, step: f64) -> Result<(), CommandError>;

        /// 重新获取这一集的弹幕。
        fn refetch_danmaku(&mut self) -> Result<(), CommandError>;

        /// 加载一个弹幕文件（弹弹play 的 xml 或 json）。
        fn load_danmaku_file(&mut self, file: MediaSource) -> Result<(), CommandError>;

        /// 悬停预览：鼠标在进度条的第几分之几处；负数表示移开了。
        fn peek(&mut self, fraction: f64, duration_ms: Option<i64>) -> Result<(), CommandError>;

        /// 打开手动匹配。
        fn open_match(&mut self) -> Result<(), CommandError>;

        #[idempotent]
        fn close_match(&mut self) -> Result<(), CommandError>;

        fn search_match(&mut self, query: String) -> Result<(), CommandError>;

        #[idempotent]
        fn choose_anime(&mut self, id: i64) -> Result<(), CommandError>;

        #[idempotent]
        fn choose_episode(&mut self, id: i64) -> Result<(), CommandError>;

        #[idempotent]
        fn set_match_folder(&mut self, on: bool) -> Result<(), CommandError>;

        /// 按选好的那一集取弹幕，记住这个文件（勾选时连同文件夹里的其他集）是这部番。
        fn confirm_match(&mut self) -> Result<(), CommandError>;

        /// 这个文件不加载弹幕。
        fn skip_danmaku(&mut self) -> Result<(), CommandError>;

        /// 往前（负数）或往后跳几个“快进快退”的步长。
        fn seek_steps(
            &mut self,
            steps: i64,
            position_ms: i64,
            duration_ms: Option<i64>,
        ) -> Result<(), CommandError>;

        /// 自动全屏已经进入全屏。
        #[idempotent]
        fn fullscreen_done(&mut self) -> Result<(), CommandError>;

        #[idempotent]
        fn set_library_filter(&mut self, filter: LibraryFilter) -> Result<(), CommandError>;

        #[idempotent]
        fn set_library_sort(&mut self, sort: LibrarySort) -> Result<(), CommandError>;

        /// 接着看这部番：看到一半的那一集，或看完的下一集。
        fn resume_series(&mut self, id: i64) -> Result<(), CommandError>;

        /// 把这部番的每一集都记成看完。
        fn finish_series(&mut self, id: i64) -> Result<(), CommandError>;

        /// 从媒体库隐藏这部番（文件不动）。
        fn hide_series(&mut self, id: i64) -> Result<(), CommandError>;

        /// 忘掉这部番是哪部作品，重新问弹弹play。
        fn reidentify_series(&mut self, id: i64) -> Result<(), CommandError>;

        /// 在资源管理器里打开这部番所在的文件夹。
        fn reveal_series(&mut self, id: i64) -> Result<(), CommandError>;
    }
}
