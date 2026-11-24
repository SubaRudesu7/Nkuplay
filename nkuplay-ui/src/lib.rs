//! Nkuplay 的界面：一个窗口，顶上是导航（首页、媒体库、新番时间表、观看历史）和右侧的账号、设置；
//! 点开一部番换成它的详情页，打开视频后换成视频页。
//! 每段文字都在显示它的地方写出所有语言的版本，每个按钮都执行协议里的一条命令。

use bern::prelude::*;
use nkuplay_protocol::nkuplay::{
    self, Amount, DetailGroup, KeyAction, LibraryFilter, Link, Page, PlayEpisode, Section, ShinStatus, Toggle,
};

/// 播放页右下角的面板：关着，或开在哪一页。后三项是弹幕页的下一层。
#[bern::value]
pub enum PlayerTab {
    #[label(tr! { "en" => "Closed", "zh-Hans" => "关着" })]
    Closed,
    #[label(tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" })]
    Danmaku,
    #[label(tr! { "en" => "Subtitles", "zh-Hans" => "字幕" })]
    Subtitle,
    #[label(tr! { "en" => "Picture", "zh-Hans" => "画面" })]
    Picture,
    #[label(tr! { "en" => "Playback", "zh-Hans" => "播放" })]
    Play,
    #[label(tr! { "en" => "Episodes", "zh-Hans" => "选集" })]
    Episodes,
    #[label(tr! { "en" => "Info", "zh-Hans" => "信息" })]
    Info,
    #[label(tr! { "en" => "Danmaku list", "zh-Hans" => "弹幕列表" })]
    DanmakuList,
    #[label(tr! { "en" => "Source", "zh-Hans" => "来源" })]
    DanmakuSource,
    #[label(tr! { "en" => "Blocked words", "zh-Hans" => "屏蔽词" })]
    Blocked,
}

/// 设置页左边的分类。
#[bern::value]
pub enum SettingsTab {
    #[label(tr! { "en" => "Appearance", "zh-Hans" => "外观" })]
    Look,
    #[label(tr! { "en" => "Playback", "zh-Hans" => "播放" })]
    Play,
    #[label(tr! { "en" => "Picture", "zh-Hans" => "画面" })]
    Picture,
    #[label(tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" })]
    Danmaku,
    #[label(tr! { "en" => "Shortcuts", "zh-Hans" => "快捷键" })]
    Keys,
    #[label(tr! { "en" => "About", "zh-Hans" => "关于" })]
    About,
}

/// 从顶栏右侧打开、盖在页面上的那一页。
#[bern::value]
pub enum Panel {
    /// 没有打开。
    #[label(tr! { "en" => "None", "zh-Hans" => "无" })]
    None,
    #[label(tr! { "en" => "Account", "zh-Hans" => "账号" })]
    Account,
    #[label(tr! { "en" => "Settings", "zh-Hans" => "设置" })]
    Settings,
}

/// 应用的播放器。界面直接读它的状态（进度、时长、是否在播、音量、静音），
/// 播放、暂停、静音也直接发给它；要算一下的（跳转、滚轮调音量、悬停时间）先交给引擎。
#[bern::players]
pub struct Players {
    /// 视频页的画面。它不随视频页消失：回到首页时要让它暂停。
    pub video: Player,
}

/// 主窗口的草稿状态。
#[bern::session]
pub struct MainDraft {
    /// 最近一次选择或拖入的视频。
    pub file: MediaSource,
    /// 是否有视频正悬停在窗口上方。
    pub over: bool,
    /// 顶栏导航选中的页面。
    pub page: Page,
    /// 顶栏右侧打开的账号或设置页。
    pub panel: Panel,
    /// 详情页剧集条最前面的那一集。
    pub strip: i64,
    /// 首页滚到了哪一段（里面某一项的编号）：102 起是大图已经滚出去，
    /// 顶栏换成不透明的。
    pub home_at: i64,
    /// 最近一次选择的要加入媒体库的文件夹。
    pub folder: MediaSource,
    /// 设置页选中的分类。
    pub settings_tab: SettingsTab,
    /// 顶栏页面拖动排序时从第几个挪到第几个。
    pub move_from: i64,
    pub move_to: i64,
    /// 正在输入的屏蔽词。
    pub word: String,
    /// 启动后已经按“启动时打开”换过页。
    pub started: bool,
    /// 播放页右下角的面板。
    pub player_panel: PlayerTab,
    /// 正按住快进键（长按快进）。
    pub fast: bool,
    /// 最近一次选的字幕、音轨或弹幕文件。
    pub track_file: MediaSource,
    /// 弹幕列表里搜的字。
    pub dm_query: String,
    /// 手动匹配时搜的作品名。
    pub match_query: String,
}

#[component]
pub fn app(app: nkuplay::Client, windows: AppWindows, players: Players) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main, appearance: app.appearance) {
            Window(
                id: main,
                title: tr! { "en" => "Nkuplay", "zh-Hans" => "Nkuplay" },
                initial: open,
                width: 1440,
                height: 900,
                min_width: 800,
                min_height: 560,
                // 顶栏和标题栏合成一条。
                chrome: unified,
                // 视频页隐藏标题栏；鼠标停住后连指针也隐藏。
                immersive: app.has_video,
                // 1 秒没有操作，`window.idle` 变真，视频页的按钮随之隐藏。
                idle_after: 1
            ) {
                main_window(app: app, windows: windows, players: players)
            }
        }
    }
}

/// 整个窗口都接受拖入的视频。最底下是所选的页面；账号、设置和番剧详情页盖在上面，
/// 打开视频时视频页盖在最上面。
#[component]
pub fn main_window(
    app: nkuplay::Client,
    windows: AppWindows,
    players: Players,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        Drop(
            id: drop,
            label: tr! { "en" => "Drop a video here", "zh-Hans" => "把视频拖到这里" },
            value: draft.file,
            files: "video",
            over: draft.over
        ) {
            Stack {
                match draft.page {
                    Page::Home => { home(app: app) }
                    Page::Library => { library_page(app: app) }
                    Page::Schedule => { schedule_page(app: app) }
                    Page::History => { history_page(app: app) }
                }
                // 顶栏和标题栏合成一条（窗口的 `chrome: unified`）：左边导航，右边账号，设置在它右边。
                // 账号、设置、详情页和播放页盖满窗口，那时顶栏不挂：挂着的话标题栏那一条有 48 高，
                // 盖住播放页左上角的返回，看不见的导航按钮也还能点到。
                if !app.detail_open && !app.has_video && draft.panel == Panel::None {
                    Toolbar {
                        // 开头的文字就是窗口的名字，标题栏不再另外显示图标和名字。
                        Text(content: tr! { "en" => "Nkuplay", "zh-Hans" => "Nkuplay" }, font_size: 19, bold: true)
                        ForEach(items: app.nav_shown, key: id) |item| {
                            if draft.page == item.page {
                                Button(id: nav_on, label: item.page, variant: nav_on) => draft.page.set(item.page)
                            } else {
                                Button(id: nav_off, label: item.page, variant: nav, prominence: plain) => draft.page.set(item.page)
                            }
                        }
                        Spacer
                        Button(
                            id: open_account,
                            label: tr! { "en" => "Sign In", "zh-Hans" => "登录" },
                            icon: "person.crop.circle",
                            variant: nav_on
                        ) => draft.panel.set(Panel::Account)
                        IconButton(
                            id: open_settings,
                            icon: "gearshape",
                            label: tr! { "en" => "Settings", "zh-Hans" => "设置" },
                            height: 34,
                            variant: nav_round
                        ) => draft.panel.set(Panel::Settings)
                    }
                }
                match draft.panel {
                    Panel::None => {}
                    Panel::Account => { panel_page(panel: Panel::Account) }
                    Panel::Settings => { settings_page(app: app) }
                }
                // 启动后先打开“启动时打开”的那一页；之后不再跟着这项设置换页。
                if !draft.started {
                    Watch(value: app.start_page) => draft.page.set(app.start_page)
                    Timer(interval: 1) => draft.started.set(true)
                }
                // 点海报墙或新番里的一部番：详情页盖满窗口。
                if app.detail_open {
                    details_page(app: app)
                }
                if app.has_video {
                    player(app: app, windows: windows, players: players)
                } else {
                    // 播放器不随视频页消失，回来时让它暂停，并记下最后看到哪里。
                    // 打开过视频才有东西可停。
                    if app.opened > 0 {
                        Watch(value: app.opened) => players.video.pause()
                        Watch(value: app.has_video) => app.record_progress(players.video.position_ms, players.video.duration_ms)
                    }
                    // 从视频页回来时还在全屏，就退出全屏。
                    if window.fullscreen {
                        Watch(value: app.has_video) => windows.main.toggle_fullscreen()
                    }
                }
            }
        } => app.open_file(draft.file)
    }
}

/// 账号页和设置页（还没做）：盖满窗口，左上角返回。
#[component]
pub fn panel_page(panel: Panel, #[session] draft: MainDraft) -> impl View {
    ui! {
        Stack(under_bars: true, variant: panel_ground) {
            Column(gap: 12, padding_x: 56, padding_y: 48) {
                Row {
                    Button(
                        id: close_panel,
                        label: tr! { "en" => "Back", "zh-Hans" => "返回" },
                        icon: "arrow.left"
                    ) => draft.panel.set(Panel::None)
                    Spacer
                }
                Text(content: panel, style: large_title)
                Text(
                    content: tr! { "en" => "This page isn't ready yet.", "zh-Hans" => "这一页还没做。" },
                    emphasis: secondary
                )
                Spacer
            }
            Shortcut(key: "escape") => draft.panel.set(Panel::None)
        }
    }
}

/// 首页：大图（继续播放里的一部，可以翻），下面是继续播放、今天更新、本季新番和媒体库。
/// 整页从窗口顶边开始，大图跟着一起滚；顶栏浮在上面。
#[component]
pub fn home(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Stack {
            Stack(under_bars: true) {
                Canvas {
                    Shape(kind: rect, fill: surface.canvas)
                }
                // 控件都在这个组件里：写在这一层上的话，Bern 会当成盖住窗口的页面，收起标题栏。
                home_scroll(app: app)
            }
            // 顶栏后面垫的东西，两种叠着放：换的时候一个淡出、一个淡入，放在同一列里会上下排成两行。
            Stack(under_bars: true) {
                Column(gap: 0) {
                    if app.recent_count > 0 && draft.home_at < 102 {
                        // 压在大图上：一层暗色渐隐托住白字。
                        Canvas(height: 60, appearance: dark, appear: fade, disappear: fade) {
                            Shape(kind: rect, fill: surface.canvas, fade: down, opacity: 0.55)
                        }
                    }
                    Spacer
                }
                Column(gap: 0) {
                    if app.recent_count == 0 || draft.home_at >= 102 {
                        // 大图滚出去了（或者没有）：页面底色，滚上来的内容不和导航叠在一起。
                        // 下沿渐隐，盖在大图的尾巴上时没有一条硬边。
                        Column(gap: 0, appear: fade, disappear: fade) {
                            Canvas(height: 48) {
                                Shape(kind: rect, fill: surface.canvas)
                            }
                            Canvas(height: 24) {
                                Shape(kind: rect, fill: surface.canvas, fade: down)
                            }
                        }
                    }
                    Spacer
                }
            }
            if app.recent_count > 0 && draft.home_at < 102 {
                // 什么也不画，只让顶栏和窗口按钮用深色外观（白字）。
                Stack(under_bars: true, appearance: dark) {
                    Canvas(width: 1, height: 1) {
                        Shape(kind: rect, fill: surface.canvas, opacity: 0)
                    }
                }
            }
            // 几部番慢慢轮换；点过箭头也接着从那一部往后换。
            Timer(interval: 8, enabled: app.recent_count > 1) => app.step_hero(1)
        }
    }
}

/// 首页滚动的内容。
#[component]
pub fn home_scroll(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView(position: draft.home_at, anchor: 0) {
            // 第一次显示首页时准备新番列表、播出时间和大图：有缓存读缓存，没有才去取。
            Watch(value: app.shin) => app.load_shin()
            Watch(value: app.shin) => app.load_home()
            // 换到别的页再回来，从头开始。
            Watch(value: draft.page) => draft.home_at.set(100)
            Stack(align: top) {
                if app.recent_count > 0 {
                    home_picture(app: app)
                }
                // 看不见的记号，`draft.home_at` 靠它们知道滚到了哪里。
                Column(gap: 0, width: 1) {
                    ForEach(items: app.home_marks, key: id) |mark| {
                        Canvas(width: 1, height: mark.height) {
                            Shape(kind: rect, fill: surface.canvas, opacity: 0)
                        }
                    }
                }
                Column(gap: 0) {
                    if app.recent_count > 0 {
                        home_hero(app: app)
                    }
                    Column(gap: 28) {
                        if app.recent_count == 0 {
                            Canvas(height: 36) {
                                Shape(kind: rect, fill: surface.canvas, opacity: 0)
                            }
                        }
                        continue_watching(app: app)
                        today_section(app: app)
                        shin_section(app: app)
                        library_section(app: app)
                        Canvas(height: 28) {
                            Shape(kind: rect, fill: surface.canvas, opacity: 0)
                        }
                    }
                }
            }
        }
    }
}

/// 大图：左半边压暗托住白字，底下化进页面底色。
#[component]
pub fn home_picture(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            Stack(height: 520) {
                // 没有图的视频，垫一层暗色，白字看得清。
                Canvas(appearance: dark) {
                    Shape(kind: rect, fill: fill.strong)
                    Shape(kind: rect, fill: surface.canvas, opacity: 0.6)
                }
                ForEach(items: app.hero, key: id) |shown| {
                    if shown.has_backdrop {
                        if shown.sharp {
                            Image(source: shown.backdrop, fit: cover, appear: fade, disappear: fade)
                        } else {
                            Image(source: shown.backdrop, fit: cover, blur: 40, appear: fade, disappear: fade)
                        }
                    }
                }
                Row(gap: 0) {
                    Canvas(appearance: dark) {
                        Shape(kind: rect, fill: surface.canvas, fade: right, opacity: 0.55)
                    }
                    Canvas {
                        Shape(kind: rect, fill: surface.canvas, opacity: 0)
                    }
                }
                Column(gap: 0) {
                    Spacer
                    Canvas(height: 208) {
                        Shape(kind: rect, fill: surface.canvas, fade: up)
                    }
                }
            }
        }
    }
}

/// 大图上的字：正在显示的那部番、看到哪里、“继续”和“详情”；右下角是左右翻页。
#[component]
pub fn home_hero(app: nkuplay::Client) -> impl View {
    ui! {
        Column(height: 450, padding_x: 56, padding_y: 40, appearance: dark) {
            Spacer
            Row(gap: 24, align: bottom) {
                ForEach(items: app.hero, key: id) |shown| {
                    Column(gap: 8, width: 520) {
                        Text(content: tr! { "en" => "Continue Watching", "zh-Hans" => "继续播放" }, style: caption)
                        Text(content: shown.title, font_size: 44, bold: true, lines: 2)
                        Row(gap: 6, align: center) {
                            if shown.kind != "" {
                                Stack {
                                    Canvas {
                                        Shape(kind: rounded, corner: 4, fill: text.primary, opacity: 0.2)
                                    }
                                    Row(padding_x: 8, padding_y: 1) {
                                        Text(content: shown.kind, style: caption)
                                    }
                                }
                            }
                            if shown.episode != "" {
                                Text(content: tr! {
                                    "en" => "{episode} · {p}% watched",
                                    "zh-Hans" => "你看到 {episode} · {p}%";
                                    episode: shown.episode,
                                    p: shown.percent
                                }, lines: 1)
                            } else {
                                Text(content: tr! { "en" => "{p}% watched", "zh-Hans" => "看到 {p}%"; p: shown.percent })
                            }
                        }
                        Canvas(height: 2) {
                            Shape(kind: rect, fill: surface.canvas, opacity: 0)
                        }
                        Row(gap: 10, align: center) {
                            if shown.part != "" {
                                Button(
                                    id: hero_play,
                                    label: tr! { "en" => "Continue {part}", "zh-Hans" => "继续 {part}"; part: shown.part },
                                    icon: "play.fill",
                                    variant: hero_play
                                ) => app.open_file(shown.source)
                            } else {
                                Button(
                                    id: hero_play_file,
                                    label: tr! { "en" => "Continue", "zh-Hans" => "继续" },
                                    icon: "play.fill",
                                    variant: hero_play
                                ) => app.open_file(shown.source)
                            }
                            if shown.anime_id > 0 {
                                Button(
                                    id: hero_details,
                                    label: tr! { "en" => "Details", "zh-Hans" => "详情" },
                                    variant: hero_details
                                ) => app.open_anime(shown.anime_id)
                            }
                        }
                    }
                }
                Spacer
                if app.recent_count > 1 {
                    Row(gap: 14, align: center) {
                        IconButton(id: hero_previous, icon: "chevron.left", label: tr! { "en" => "Previous", "zh-Hans" => "上一部" }, height: 34, variant: hero_arrow) => app.step_hero(-1)
                        Row(gap: 6, align: center) {
                            ForEach(items: app.hero_dots, key: id) |dot| {
                                if dot.on {
                                    Canvas(width: 22, height: 7) {
                                        Shape(kind: capsule, fill: text.primary)
                                    }
                                } else {
                                    Canvas(width: 7, height: 7) {
                                        Shape(kind: capsule, fill: text.primary, opacity: 0.6)
                                    }
                                }
                            }
                        }
                        IconButton(id: hero_next, icon: "chevron.right", label: tr! { "en" => "Next", "zh-Hans" => "下一部" }, height: 34, variant: hero_arrow) => app.step_hero(1)
                    }
                }
            }
        }
    }
}

/// 继续播放：每部番一张卡片（最近看的那一集）：上次离开时的画面（没有或全黑时用剧照，再没有用海报），
/// 底边是进度，下面是作品名和“看了百分之几 · 第几话”。点一下从离开的地方接着播放。
#[component]
pub fn continue_watching(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 12) {
            Row(gap: 10, align: center, padding_x: 56) {
                Text(content: tr! { "en" => "Continue Watching", "zh-Hans" => "继续播放" }, style: title2, bold: true)
                Spacer
                Button(
                    id: open_history,
                    label: tr! { "en" => "History ›", "zh-Hans" => "观看历史 ›" },
                    variant: link,
                    prominence: plain
                ) => draft.page.set(Page::History)
            }
            if app.recent_count == 0 {
                Row(padding_x: 56) {
                    Text(
                        content: tr! {
                            "en" => "Videos you open show up here. Drop one on the window to start.",
                            "zh-Hans" => "打开过的视频会出现在这里。把视频拖到窗口上就能播放。"
                        },
                        emphasis: secondary
                    )
                }
            } else {
                // 横排从左边距开始，一直到窗口右边。
                ScrollView(axis: horizontal, indicators: mouse) {
                    Row(gap: 16, align: top, padding_x: 56) {
                        ForEach(items: app.recents, key: id) |recent| {
                            Button(
                                id: resume,
                                label: tr! { "en" => "Play {title}", "zh-Hans" => "播放{title}"; title: recent.title }
                            ) {
                                recent_card(recent: recent)
                            } => app.open_file(recent.source)
                        }
                    }
                }
            }
        }
    }
}

/// 继续播放的一张卡片。
#[component]
pub fn recent_card(recent: nkuplay::Recent) -> impl View {
    ui! {
        Column(gap: 8, width: 260) {
            Stack(width: 260, height: 146, variant: card_picture) {
                if recent.has_shot {
                    Image(source: recent.shot, width: 260, height: 146, fit: cover)
                } else {
                    if recent.has_poster {
                        Image(source: recent.poster, width: 260, height: 146, fit: cover)
                    } else {
                        Canvas(width: 260, height: 146) {
                            Shape(kind: rect, fill: accent, opacity: 0.18)
                        }
                        Label(title: recent.title, icon: "play.fill", display: icon)
                    }
                }
                // 进度压在画面的底边上。
                Column(gap: 0, width: 260, height: 146) {
                    Spacer
                    Stack(width: 260, height: 4) {
                        Canvas(width: 260, height: 4, appearance: dark) {
                            Shape(kind: rect, fill: surface.canvas, opacity: 0.35)
                        }
                        Rail {
                            Canvas(from: 0, to: recent.bar) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                }
            }
            Column(gap: 2) {
                Text(content: recent.title, bold: true, lines: 1)
                if recent.episode != "" {
                    Text(
                        content: tr! {
                            "en" => "{p}% · {episode}",
                            "zh-Hans" => "{p}% · {episode}";
                            p: recent.percent,
                            episode: recent.episode
                        },
                        style: caption,
                        emphasis: secondary,
                        lines: 1
                    )
                } else {
                    Text(
                        content: tr! { "en" => "{p}%", "zh-Hans" => "{p}%"; p: recent.percent },
                        style: caption,
                        emphasis: secondary
                    )
                }
            }
        }
    }
}

/// 一张新番海报：左上角是播出时间，下面是作品名。
#[component]
pub fn anime_card(anime: nkuplay::ShinAnime) -> impl View {
    ui! {
        Column(gap: 8, width: 136) {
            Stack(width: 136, height: 192) {
                if anime.has_poster {
                    Image(source: anime.poster, width: 136, height: 192, fit: cover, corner: 7)
                } else {
                    Canvas(width: 136, height: 192) {
                        Shape(kind: rounded, corner: 7, fill: fill.strong)
                    }
                }
                if anime.time != "" {
                    Column(padding: 6, width: 136, height: 192) {
                        Row {
                            Stack(appearance: dark, width: 40, height: 18) {
                                Canvas {
                                    Shape(kind: rounded, corner: 4, fill: surface.canvas, opacity: 0.8)
                                }
                                Row(padding_x: 6, padding_y: 1) {
                                    Text(content: anime.time, font_size: 11)
                                }
                            }
                            Spacer
                        }
                        Spacer
                    }
                }
            }
            Text(content: anime.title, font_size: 13, lines: 1)
        }
    }
}

/// 今天更新：本季新番里今天更新的，一排海报，按播出时间排。
#[component]
pub fn today_section(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        if app.today_count > 0 {
            Column(gap: 12) {
                Row(gap: 10, align: center, padding_x: 56) {
                    Text(content: tr! { "en" => "New Today", "zh-Hans" => "今天更新" }, style: title2, bold: true)
                    Text(
                        content: tr! {
                            "en" => "{day} · {count}",
                            "zh-Hans" => "{day} · {count} 部";
                            day: app.today_day,
                            count: app.today_count
                        },
                        font_size: 13,
                        emphasis: secondary
                    )
                    Spacer
                    Button(
                        id: open_schedule,
                        label: tr! { "en" => "Schedule ›", "zh-Hans" => "新番时间表 ›" },
                        variant: link,
                        prominence: plain
                    ) => draft.page.set(Page::Schedule)
                }
                ScrollView(axis: horizontal, indicators: mouse) {
                    Row(gap: 16, align: top, padding_x: 56) {
                        ForEach(items: app.today, key: id) |anime| {
                            Button(
                                id: open_today,
                                label: tr! { "en" => "Open {title}", "zh-Hans" => "打开{title}"; title: anime.title }
                            ) {
                                anime_card(anime: anime)
                            } => app.open_anime(anime.id)
                        }
                    }
                }
            }
        }
    }
}

/// 本季新番：按星期筛选的一排海报。数据来自弹弹play，读缓存；重新取的按钮在新番时间表页，
/// 这里只在列表过期或取失败时出现。
#[component]
pub fn shin_section(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 12) {
            Row(gap: 10, align: center, padding_x: 56) {
                Text(content: tr! { "en" => "This Season", "zh-Hans" => "本季新番" }, style: title2, bold: true)
                if app.shin_year != "" {
                    Text(
                        content: tr! {
                            "en" => "{year}/{month}",
                            "zh-Hans" => "{year} 年 {month} 月";
                            year: app.shin_year,
                            month: app.shin_month
                        },
                        font_size: 13,
                        emphasis: secondary
                    )
                }
                Spacer
                if app.shin_outdated || app.shin_error != "" {
                    Button(
                        id: refresh_shin,
                        label: tr! { "en" => "Refresh", "zh-Hans" => "刷新" },
                        icon: "arrow.clockwise",
                        prominence: plain,
                        enabled: app.shin != ShinStatus::Loading
                    ) => app.refresh_shin()
                }
            }
            Row(gap: 4, align: center, padding_x: 56) {
                ForEach(items: app.shin_days, key: id) |chip| {
                    if chip.on {
                        Button(id: day_on, label: tr! { "en" => "{day}", "zh-Hans" => "{day}"; day: chip.day }, variant: day_on) => app.show_day(chip.day)
                    } else {
                        Button(id: day, label: tr! { "en" => "{day}", "zh-Hans" => "{day}"; day: chip.day }, variant: day) => app.show_day(chip.day)
                    }
                }
            }
            if app.shin_outdated {
                Row(padding_x: 56) {
                    Text(
                        content: tr! {
                            "en" => "This list is from an earlier season. Refresh to get the current one.",
                            "zh-Hans" => "这是之前一季的列表，点刷新获取本季的。"
                        },
                        color: status.warning
                    )
                }
            }
            if app.shin_error != "" {
                Row(padding_x: 56) {
                    Text(
                        content: tr! { "en" => "Couldn't get the list: {reason}", "zh-Hans" => "没能取到新番列表：{reason}"; reason: app.shin_error },
                        color: status.error
                    )
                }
            }
            match app.shin {
                ShinStatus::Loaded => {
                    if app.shin_count == 0 {
                        Row(padding_x: 56) {
                            Text(
                                content: tr! { "en" => "Nothing airs on this day.", "zh-Hans" => "这一天没有更新的新番。" },
                                emphasis: secondary
                            )
                        }
                    } else {
                        shin_shelf(app: app, loading: false)
                    }
                }
                ShinStatus::Loading => {
                    shin_shelf(app: app, loading: true)
                }
                ShinStatus::Unavailable => {
                    Row(padding_x: 56) {
                        Text(
                            content: tr! {
                                "en" => "This build has no dandanplay credentials.",
                                "zh-Hans" => "这个版本没有弹弹play 的凭证，取不到新番列表。"
                            },
                            emphasis: secondary
                        )
                    }
                }
                ShinStatus::Failed => {
                    // 原因已经显示在上面了；这里只提示还能做什么。
                    Row(padding_x: 56) {
                        Text(
                            content: tr! { "en" => "Try Refresh again later.", "zh-Hans" => "稍后可以点刷新再试。" },
                            emphasis: secondary
                        )
                    }
                }
                ShinStatus::NotLoaded => {
                    shin_shelf(app: app, loading: true)
                }
            }
        }
    }
}

/// 一排新番海报。`loading` 为真时（正在向弹弹play 取，或还没开始取）显示骨架图。
#[component]
pub fn shin_shelf(app: nkuplay::Client, loading: bool) -> impl View {
    ui! {
        ScrollView(axis: horizontal, indicators: mouse) {
            Row(gap: 16, align: top, padding_x: 56) {
                ForEach(items: app.shin_shown, key: id, skeleton: loading, skeleton_count: 8) |anime| {
                    Button(
                        id: open_shin,
                        label: tr! { "en" => "Open {title}", "zh-Hans" => "打开{title}"; title: anime.title }
                    ) {
                        anime_card(anime: anime)
                    } => app.open_anime(anime.id)
                }
            }
        }
    }
}

/// 首页最下面的媒体库：一排海报，或者一个虚线框提示怎么添加。
#[component]
pub fn library_section(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 12) {
            Row(gap: 10, align: center, padding_x: 56) {
                Text(content: tr! { "en" => "Library", "zh-Hans" => "媒体库" }, style: title2, bold: true)
                Spacer
                Button(
                    id: open_library,
                    label: tr! { "en" => "All ›", "zh-Hans" => "全部 ›" },
                    variant: link,
                    prominence: plain
                ) => draft.page.set(Page::Library)
            }
            if app.series_count > 0 {
                ScrollView(axis: horizontal, indicators: mouse) {
                    Row(gap: 16, align: top, padding_x: 56) {
                        ForEach(items: app.series, key: id) |show| {
                            Button(
                                id: open_shelf,
                                label: tr! { "en" => "Open {title}", "zh-Hans" => "打开{title}"; title: show.title }
                            ) {
                                Column(gap: 8, width: 136) {
                                    if show.has_poster {
                                        Image(source: show.poster, width: 136, height: 192, fit: cover, corner: 7)
                                    } else {
                                        Canvas(width: 136, height: 192) {
                                            Shape(kind: rounded, corner: 7, fill: fill.strong)
                                        }
                                    }
                                    Text(content: show.title, font_size: 13, lines: 1)
                                }
                            } => app.open_series(show.id)
                        }
                    }
                }
            } else {
                Row(padding_x: 56) {
                    Stack(height: 120) {
                        Canvas(height: 120) {
                            Shape(kind: rounded, corner: 8, stroke: text.placeholder, stroke_width: 1, dash: 3, opacity: 0.4)
                        }
                        Row(gap: 10, align: center) {
                            Text(
                                content: tr! {
                                    "en" => "Add a folder of shows: each show in it appears here with its poster.",
                                    "zh-Hans" => "添加放番剧的文件夹，番剧会带着海报显示在这里"
                                },
                                font_size: 13,
                                emphasis: secondary
                            )
                            Button(
                                id: add_folder,
                                label: tr! { "en" => "Add Folder", "zh-Hans" => "添加文件夹" },
                                icon: "plus",
                                pick: draft.folder,
                                files: "folder"
                            ) => app.add_folder(draft.folder)
                        }
                    }
                }
            }
        }
    }
}

/// 媒体库页：只有海报墙。左上按观看状态筛选，右上刷新、“⋯”（打开文件、文件夹、排序）、添加文件夹。
#[component]
pub fn library_page(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 8, padding_x: 64, padding_y: 12) {
                Row(height: 64, gap: 4, align: center) {
                    library_filter(app: app)
                    Spacer
                    IconButton(
                        id: refresh_library_page,
                        icon: "arrow.clockwise",
                        label: tr! { "en" => "Refresh", "zh-Hans" => "刷新" },
                        height: 34,
                        enabled: !app.library_scanning,
                        variant: round
                    ) => app.refresh_library()
                    Row(width: 4) {}
                    PopoverButton(
                        id: library_more,
                        label: tr! { "en" => "More", "zh-Hans" => "更多" },
                        icon: "ellipsis",
                        height: 34,
                        placement: below
                    ) {
                        library_more(app: app)
                    }
                    Row(width: 4) {}
                    IconButton(
                        id: add_folder_page,
                        icon: "plus",
                        label: tr! { "en" => "Add Folder", "zh-Hans" => "添加文件夹" },
                        height: 34,
                        variant: add_round,
                        pick: draft.folder,
                        files: "folder"
                    ) => app.add_folder(draft.folder)
                }
                library_wall(app: app)
            }
        }
    }
}

/// 左上的筛选：全部、在看、没看、看完，后面是数量。选中的是白色胶囊。
#[component]
pub fn library_filter(app: nkuplay::Client) -> impl View {
    ui! {
        Row(gap: 4, align: center) {
            if app.library_filter == LibraryFilter::All {
                Button(id: filter_all_on, label: tr! { "en" => "All  {n}", "zh-Hans" => "全部  {n}"; n: app.count_all }, variant: nav_on) => app.set_library_filter(LibraryFilter::All)
            } else {
                Button(id: filter_all, label: tr! { "en" => "All  {n}", "zh-Hans" => "全部  {n}"; n: app.count_all }, variant: nav, prominence: plain) => app.set_library_filter(LibraryFilter::All)
            }
            if app.library_filter == LibraryFilter::Watching {
                Button(id: filter_watching_on, label: tr! { "en" => "Watching  {n}", "zh-Hans" => "在看  {n}"; n: app.count_watching }, variant: nav_on) => app.set_library_filter(LibraryFilter::Watching)
            } else {
                Button(id: filter_watching, label: tr! { "en" => "Watching  {n}", "zh-Hans" => "在看  {n}"; n: app.count_watching }, variant: nav, prominence: plain) => app.set_library_filter(LibraryFilter::Watching)
            }
            if app.library_filter == LibraryFilter::Unwatched {
                Button(id: filter_unwatched_on, label: tr! { "en" => "Not started  {n}", "zh-Hans" => "没看  {n}"; n: app.count_unwatched }, variant: nav_on) => app.set_library_filter(LibraryFilter::Unwatched)
            } else {
                Button(id: filter_unwatched, label: tr! { "en" => "Not started  {n}", "zh-Hans" => "没看  {n}"; n: app.count_unwatched }, variant: nav, prominence: plain) => app.set_library_filter(LibraryFilter::Unwatched)
            }
            if app.library_filter == LibraryFilter::Finished {
                Button(id: filter_finished_on, label: tr! { "en" => "Finished  {n}", "zh-Hans" => "看完  {n}"; n: app.count_finished }, variant: nav_on) => app.set_library_filter(LibraryFilter::Finished)
            } else {
                Button(id: filter_finished, label: tr! { "en" => "Finished  {n}", "zh-Hans" => "看完  {n}"; n: app.count_finished }, variant: nav, prominence: plain) => app.set_library_filter(LibraryFilter::Finished)
            }
        }
    }
}

/// “⋯”里：打开文件、加入的文件夹（每行可以移除）、排序。
#[component]
pub fn library_more(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 6, width: 280) {
            Button(
                id: open_file_more,
                label: tr! { "en" => "Open File…", "zh-Hans" => "打开文件…" },
                icon: "doc",
                width: fill,
                prominence: plain,
                pick: draft.file,
                files: "video",
                shortcut: "cmd+o"
            ) => app.open_file(draft.file)
            Divider
            Text(content: tr! { "en" => "Folders", "zh-Hans" => "文件夹" }, font_size: 12, emphasis: secondary)
            if app.folder_count == 0 {
                Text(content: tr! { "en" => "No folders yet.", "zh-Hans" => "还没有加入文件夹。" }, font_size: 13, emphasis: secondary)
            }
            ForEach(items: app.folders, key: id) |folder| {
                Row(gap: 10, align: center) {
                    Label(title: folder.name, icon: "folder", display: icon)
                    Column(gap: 0) {
                        Text(content: folder.name, font_size: 13, lines: 1)
                        Text(
                            content: tr! { "en" => "{count} videos", "zh-Hans" => "{count} 个视频"; count: folder.videos },
                            font_size: 12,
                            emphasis: secondary,
                            skeleton: folder.scanning
                        )
                    }
                    Spacer
                    IconButton(id: remove_folder, icon: "xmark", label: tr! { "en" => "Remove", "zh-Hans" => "移除" }, font_size: 11, prominence: plain) => app.remove_folder(folder.id)
                }
            }
            Divider
            Text(content: tr! { "en" => "Sort", "zh-Hans" => "排序" }, font_size: 12, emphasis: secondary)
            Picker(id: library_sort, label: tr! { "en" => "Sort", "zh-Hans" => "排序" }, label_hidden: true, selected: app.library_sort, style: segmented) => app.set_library_sort(app.library_sort)
        }
    }
}

/// 番剧海报墙；还没有番时说明为什么。第一次扫描时显示骨架图。
#[component]
pub fn library_wall(app: nkuplay::Client) -> impl View {
    ui! {
        if app.series_count > 0 {
            series_grid(app: app, loading: false)
        } else {
            if app.library_scanning {
                series_grid(app: app, loading: true)
            } else {
                if app.folder_count == 0 {
                    Text(
                        content: tr! {
                            "en" => "Add a folder of shows: each show in it appears here with its poster.",
                            "zh-Hans" => "添加放番剧的文件夹，里面的每部番都会带着海报显示在这里。"
                        },
                        emphasis: secondary
                    )
                } else {
                    Text(
                        content: tr! {
                            "en" => "No shows found yet. Refresh to scan the folders again.",
                            "zh-Hans" => "还没有找到番剧，点“刷新”重新扫描文件夹。"
                        },
                        emphasis: secondary
                    )
                }
            }
        }
    }
}

/// 海报墙本身：每部番一张海报，下面是作品名和集数、看到哪里；点开进详情页，右键有更多。
/// 看了一部分的海报底边有进度，看完的右上角有勾。没认出的番没有海报，用浅色底加播放图标。
#[component]
pub fn series_grid(app: nkuplay::Client, loading: bool) -> impl View {
    ui! {
        Grid(min_width: 148, gap: 18, skeleton: loading, skeleton_count: 8) {
            ForEach(items: app.library_shown, key: id) |show| {
                Button(
                    id: open_show,
                    label: tr! { "en" => "Open {title}", "zh-Hans" => "打开{title}"; title: show.title }
                ) {
                    Column(gap: 4) {
                        Stack(height: 210, variant: poster) {
                            if show.has_poster {
                                Image(source: show.poster, height: 210, fit: cover, corner: 6)
                            } else {
                                Stack(height: 210) {
                                    Canvas(height: 210) {
                                        Shape(kind: rounded, corner: 6, fill: accent, opacity: 0.12)
                                    }
                                    Label(title: show.title, icon: "play", display: icon)
                                }
                            }
                            if show.finished {
                                Column(padding_x: 8, padding_y: 8) {
                                    Row {
                                        Spacer
                                        Stack(width: 22, height: 22, variant: check_badge) {
                                            Label(title: tr! { "en" => "Finished", "zh-Hans" => "看完了" }, icon: "checkmark", display: icon)
                                        }
                                    }
                                    Spacer
                                }
                            } else {
                                if show.bar > 0.0 {
                                    Column {
                                        Spacer
                                        Progress(value: show.bar)
                                    }
                                }
                            }
                        }
                        Text(content: show.title, bold: true, lines: 2)
                        if show.finished {
                            Text(content: tr! { "en" => "{n} episodes · finished", "zh-Hans" => "{n} 集 · 看完了"; n: show.episodes }, font_size: 12, emphasis: secondary)
                        } else {
                            if show.watched_up_to > 0 {
                                Text(content: tr! { "en" => "{n} episodes · up to {m}", "zh-Hans" => "{n} 集 · 看到第 {m} 话"; n: show.episodes, m: show.watched_up_to }, font_size: 12, emphasis: secondary)
                            } else {
                                Text(content: tr! { "en" => "{n, plural, one {# episode} other {# episodes}}", "zh-Hans" => "{n} 集"; n: show.episodes }, font_size: 12, emphasis: secondary)
                            }
                        }
                    }
                    ContextMenu {
                        MenuItem(id: resume_show, label: tr! { "en" => "Continue Episode {n}", "zh-Hans" => "继续第 {n} 话"; n: show.resume_number }, icon: "play") => app.resume_series(show.id)
                        MenuItem(id: show_details, label: tr! { "en" => "Details", "zh-Hans" => "详情" }, icon: "info.circle") => app.open_series(show.id)
                        Divider
                        MenuItem(id: finish_show, label: tr! { "en" => "Mark as Finished", "zh-Hans" => "标记为看完" }, icon: "checkmark.circle", enabled: !show.finished) => app.finish_series(show.id)
                        MenuItem(id: reidentify_show, label: tr! { "en" => "Identify Again…", "zh-Hans" => "重新识别…" }, icon: "arrow.clockwise") => app.reidentify_series(show.id)
                        MenuItem(id: reveal_show, label: tr! { "en" => "Show in Folder", "zh-Hans" => "在文件夹中显示" }, icon: "folder") => app.reveal_series(show.id)
                        Divider
                        MenuItem(id: hide_show, label: tr! { "en" => "Hide from Library", "zh-Hans" => "从媒体库隐藏" }, icon: "eye.slash", role: destructive) => app.hide_series(show.id)
                    }
                } => app.open_series(show.id)
            }
        }
    }
}

/// 番剧详情页：从海报墙或新番点进来，盖满窗口。上半部是选中那一集的剧照（没有时是虚化的海报），
/// 左边和下面用页面底色渐隐托住文字；往下依次是横幅、剧集条、评论和资料，一起滚动。
#[component]
pub fn details_page(app: nkuplay::Client) -> impl View {
    ui! {
        Stack(under_bars: true, variant: panel_ground, appear: page_in, disappear: fade) {
            ScrollView {
                Stack(align: top) {
                    detail_backdrop(app: app)
                    Column(gap: 30, padding_x: 56, padding_y: 40) {
                        // 返回写在这里，不放进横幅：有自己的控件，Bern 才把这一页当成盖住窗口的一页，
                        // 收起标题栏上的图标和名字。
                        Row(gap: 12, align: center) {
                            IconButton(
                                id: back_from_details,
                                icon: "arrow.left",
                                label: tr! { "en" => "Back", "zh-Hans" => "返回" },
                                height: 34,
                                variant: round
                            ) => app.close_details()
                            Text(content: app.detail_title, lines: 1)
                            Spacer
                        }
                        detail_banner(app: app)
                        detail_episodes(app: app)
                        if app.detail_known {
                            Divider
                            detail_bottom(app: app)
                        }
                    }
                }
            }
            // 轮换时每 8 秒换下一张。
            Timer(interval: 8, enabled: app.detail_rotates) => app.next_backdrop()
            // 播放时详情页还挂在下面：这时 Esc 留给播放页（退出全屏），不关看不见的详情页。
            if !app.has_video {
                Shortcut(key: "escape") => app.close_details()
            }
        }
    }
}

/// 背景：剧照铺在上半部，左边和下面渐隐成页面底色。
#[component]
pub fn detail_backdrop(app: nkuplay::Client) -> impl View {
    ui! {
        Column {
            Stack(height: 640) {
                // 换图时旧的淡出、新的淡入。
                ForEach(items: app.detail_backdrop, key: id) |backdrop| {
                    if backdrop.still {
                        Image(source: backdrop.source, fit: cover, appear: fade, disappear: fade)
                    } else {
                        Image(source: backdrop.source, fit: cover, blur: 48, appear: fade, disappear: fade)
                    }
                }
                Row(gap: 0) {
                    Canvas(width: 340) {
                        Shape(kind: rect, fill: surface.canvas)
                    }
                    Canvas(width: 460) {
                        Shape(kind: rect, fill: surface.canvas, fade: right)
                    }
                    Spacer
                }
                Column {
                    // 托住右上角的窗口按钮。
                    Canvas(height: 72) {
                        Shape(kind: rect, fill: surface.canvas, fade: down, opacity: 0.8)
                    }
                    Spacer
                    Canvas(height: 260) {
                        Shape(kind: rect, fill: surface.canvas, fade: up)
                    }
                }
            }
            Spacer
        }
    }
}

/// 横幅：大号作品名、日文名、概况、标签、简介；“继续”、标记看完、刷新、背景设置。
#[component]
pub fn detail_banner(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 30) {
            Column(gap: 12, width: 480) {
                Text(content: app.detail_title, font_size: 44, bold: true, lines: 2)
                if app.detail_native_title != "" {
                    Text(content: app.detail_native_title, emphasis: secondary, lines: 1)
                }
                if app.detail_known {
                    detail_overview(app: app)
                    Flow(gap: 8) {
                        ForEach(items: app.detail_tags, key: id) |tag| {
                            Stack {
                                Canvas {
                                    Shape(kind: capsule, fill: surface.raised)
                                }
                                Row(padding_x: 14, padding_y: 4) {
                                    Text(content: tag.name, style: caption)
                                }
                            }
                        }
                    }
                    if app.detail_summary != "" {
                        Text(content: app.detail_summary, lines: 3, line_height: 26)
                    }
                }
                if app.detail_error != "" {
                    Text(
                        content: tr! { "en" => "Couldn't get the details: {reason}", "zh-Hans" => "没能取到资料：{reason}"; reason: app.detail_error },
                        color: status.error
                    )
                }
            }
            detail_actions(app: app)
        }
    }
}

/// 概况：年份 · 类别 · 一集多长 · 几话 · 评分，后面是连载中或已完结。
#[component]
pub fn detail_overview(app: nkuplay::Client) -> impl View {
    ui! {
        Row(gap: 8, align: center) {
            if app.detail_has_premiere {
                Text(content: app.detail_year, emphasis: secondary)
                Text(content: tr! { "en" => "·", "zh-Hans" => "·" }, emphasis: tertiary)
            }
            if app.detail_kind != "" {
                Text(content: app.detail_kind, emphasis: secondary)
                Text(content: tr! { "en" => "·", "zh-Hans" => "·" }, emphasis: tertiary)
            }
            if app.detail_minutes > 0 {
                Text(content: tr! { "en" => "{n} min", "zh-Hans" => "{n} 分钟"; n: app.detail_minutes }, emphasis: secondary)
                Text(content: tr! { "en" => "·", "zh-Hans" => "·" }, emphasis: tertiary)
            }
            Text(
                content: tr! {
                    "en" => "{count, plural, one {# episode} other {# episodes}}",
                    "zh-Hans" => "{count} 话";
                    count: app.detail_episode_count
                },
                emphasis: secondary
            )
            if app.detail_rating > 0.0 {
                Text(content: tr! { "en" => "·", "zh-Hans" => "·" }, emphasis: tertiary)
                Label(title: app.detail_rating_text, icon: "star.fill", display: icon, color: status.error, font_size: 13)
                Text(content: app.detail_rating_text, bold: true, color: status.error)
            }
            Stack {
                Canvas {
                    Shape(kind: rounded, corner: 4, stroke: control.border, stroke_width: 1)
                }
                Row(padding_x: 6, padding_y: 1) {
                    if app.detail_on_air {
                        Text(content: tr! { "en" => "Airing", "zh-Hans" => "连载中" }, style: caption, emphasis: secondary)
                    } else {
                        Text(content: tr! { "en" => "Finished", "zh-Hans" => "已完结" }, style: caption, emphasis: secondary)
                    }
                }
            }
        }
    }
}

/// 一排按钮：继续（底下的浅色是看到的进度）、标记看完、刷新资料、背景设置。
#[component]
pub fn detail_actions(app: nkuplay::Client) -> impl View {
    ui! {
        Row(gap: 12, align: center) {
            Button(
                id: resume_show,
                label: tr! { "en" => "Continue", "zh-Hans" => "继续" },
                enabled: app.detail_can_resume
            ) {
                Stack(width: 120, height: 40, align: center) {
                    Canvas(width: 120, height: 40) {
                        Shape(kind: capsule, fill: surface.raised)
                        Shape(kind: capsule, stroke: control.border, stroke_width: 1)
                    }
                    Rail {
                        Canvas(from: 0, to: app.detail_resume_fraction) {
                            Shape(kind: capsule, fill: accent, opacity: 0.2)
                        }
                    }
                    Row(gap: 8, align: center) {
                        Label(title: app.detail_title, icon: "play.fill", display: icon, color: accent)
                        if app.detail_started {
                            Text(content: tr! { "en" => "Continue", "zh-Hans" => "继续" }, bold: true)
                        } else {
                            Text(content: tr! { "en" => "Play", "zh-Hans" => "播放" }, bold: true)
                        }
                    }
                }
            } => app.open_file(app.detail_resume)
            IconButton(
                id: mark_watched,
                icon: "checkmark",
                label: tr! { "en" => "Mark as Watched", "zh-Hans" => "标记看完" },
                height: 40,
                variant: round_large,
                enabled: app.detail_can_resume
            ) => app.mark_watched()
            IconButton(
                id: refresh_details,
                icon: "arrow.clockwise",
                label: tr! { "en" => "Refresh Details", "zh-Hans" => "刷新资料" },
                height: 40,
                variant: round_large,
                enabled: !app.detail_loading
            ) => app.refresh_details()
            PopoverButton(
                id: backdrop_menu,
                label: tr! { "en" => "Background", "zh-Hans" => "背景" },
                icon: "ellipsis",
                height: 40,
                shape: capsule
            ) {
                Column(gap: 2, width: 232) {
                    Row(padding_x: 10, padding_y: 4) {
                        Text(content: tr! { "en" => "Background", "zh-Hans" => "背景" }, style: caption, emphasis: secondary)
                    }
                    Button(id: backdrop_rotate, label: tr! { "en" => "Rotate", "zh-Hans" => "轮换" }) {
                        Row(padding_x: 10, padding_y: 8, align: center) {
                            Text(content: tr! { "en" => "Rotate", "zh-Hans" => "轮换" })
                            Spacer
                            if !app.detail_fixed {
                                Label(title: app.detail_title, icon: "checkmark", display: icon, color: accent)
                            }
                        }
                    } => app.set_backdrop_fixed(false)
                    Button(id: backdrop_fixed, label: tr! { "en" => "Fixed", "zh-Hans" => "固定" }) {
                        Row(padding_x: 10, padding_y: 8, align: center) {
                            Text(content: tr! { "en" => "Fixed", "zh-Hans" => "固定" })
                            Spacer
                            if app.detail_fixed {
                                Label(title: app.detail_title, icon: "checkmark", display: icon, color: accent)
                            }
                        }
                    } => app.set_backdrop_fixed(true)
                    Button(id: backdrop_next, label: tr! { "en" => "Switch", "zh-Hans" => "切换" }, enabled: app.detail_can_switch) {
                        Row(padding_x: 10, padding_y: 8, align: center) {
                            Text(content: tr! { "en" => "Switch", "zh-Hans" => "切换" })
                            Spacer
                        }
                    } => app.next_backdrop()
                }
            }
            if app.detail_loading {
                Progress()
            }
        }
    }
}

/// 剧集：正片 / 特别篇 / OP·ED 的筛选和左右翻页，下面一排剧照缩略图，打开时滚到“继续”那一集附近。
#[component]
pub fn detail_episodes(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 14) {
            // 引擎说滚到哪一集就滚到哪一集；用户自己滚时 `draft.strip` 跟着变。
            Watch(value: app.detail_strip_serial) => draft.strip.set(app.detail_strip_at)
            Row(gap: 6, align: center) {
                if app.detail_group == DetailGroup::Main {
                    Button(id: group_main_on, label: tr! { "en" => "Episodes {n}", "zh-Hans" => "正片 {n}"; n: app.detail_main_count }, variant: group_on) => app.show_group(DetailGroup::Main)
                } else {
                    Button(id: group_main, label: tr! { "en" => "Episodes {n}", "zh-Hans" => "正片 {n}"; n: app.detail_main_count }, variant: group, prominence: plain) => app.show_group(DetailGroup::Main)
                }
                if app.detail_special_count > 0 {
                    if app.detail_group == DetailGroup::Special {
                        Button(id: group_special_on, label: tr! { "en" => "Specials {n}", "zh-Hans" => "特别篇 {n}"; n: app.detail_special_count }, variant: group_on) => app.show_group(DetailGroup::Special)
                    } else {
                        Button(id: group_special, label: tr! { "en" => "Specials {n}", "zh-Hans" => "特别篇 {n}"; n: app.detail_special_count }, variant: group, prominence: plain) => app.show_group(DetailGroup::Special)
                    }
                }
                if app.detail_extra_count > 0 {
                    if app.detail_group == DetailGroup::Extra {
                        Button(id: group_extra_on, label: tr! { "en" => "OP / ED {n}", "zh-Hans" => "OP / ED {n}"; n: app.detail_extra_count }, variant: group_on) => app.show_group(DetailGroup::Extra)
                    } else {
                        Button(id: group_extra, label: tr! { "en" => "OP / ED {n}", "zh-Hans" => "OP / ED {n}"; n: app.detail_extra_count }, variant: group, prominence: plain) => app.show_group(DetailGroup::Extra)
                    }
                }
                Spacer
                IconButton(id: strip_back, icon: "chevron.left", label: tr! { "en" => "Earlier", "zh-Hans" => "往前" }, height: 34, variant: round) => app.page_episodes(draft.strip, -1)
                IconButton(id: strip_forward, icon: "chevron.right", label: tr! { "en" => "Later", "zh-Hans" => "往后" }, height: 34, variant: round) => app.page_episodes(draft.strip, 1)
            }
            ScrollView(axis: horizontal, position: draft.strip, snap: start, indicators: hidden) {
                Row(gap: 22, align: top) {
                    ForEach(items: app.detail_episodes, key: id) |episode| {
                        if episode.local {
                            Button(
                                id: play_episode,
                                label: tr! { "en" => "Play {title}", "zh-Hans" => "播放{title}"; title: episode.title }
                            ) {
                                episode_card(episode: episode)
                            } => app.open_file(episode.source)
                        } else {
                            episode_card(episode: episode)
                        }
                    }
                }
            }
        }
    }
}

/// 一集：缩略图（看完的带勾，“继续”那一集有黑框和进度，本地有的盖着播放图标，本地没有的变灰变淡），
/// 下面是“第N话 · 标题”和状态。
#[component]
pub fn episode_card(episode: nkuplay::DetailEpisode) -> impl View {
    ui! {
        Column(gap: 6, width: 248) {
            Stack(width: 248, height: 140) {
                if episode.has_thumb {
                    if episode.local {
                        Image(source: episode.thumb, width: 248, height: 140, fit: cover, corner: 8)
                    } else {
                        // 变淡的图后面垫上页面底色，不透出背景。
                        Canvas(width: 248, height: 140) {
                            Shape(kind: rounded, corner: 8, fill: surface.canvas)
                        }
                        Image(source: episode.thumb, width: 248, height: 140, fit: cover, corner: 8, saturation: 0, opacity: 0.45)
                    }
                } else {
                    Canvas(width: 248, height: 140) {
                        Shape(kind: rounded, corner: 8, fill: fill.strong)
                    }
                }
                if episode.current {
                    Canvas(width: 248, height: 140) {
                        Shape(kind: rounded, corner: 8, stroke: text.primary, stroke_width: 3)
                    }
                }
                if episode.finished {
                    Column(padding: 8, width: 248, height: 140, align: trailing) {
                        Label(title: episode.title, icon: "checkmark.circle.fill", display: icon, color: accent, font_size: 18)
                        Spacer
                    }
                }
                if episode.local && !episode.finished && !episode.current {
                    Column(width: 248, height: 140, align: center) {
                        Spacer
                        Label(title: episode.title, icon: "play.fill", display: icon, font_size: 28)
                        Spacer
                    }
                }
                if episode.current && episode.percent > 0 && !episode.finished {
                    Column(padding: 8, width: 248, height: 140) {
                        Spacer
                        Progress(value: episode.percent, total: 100)
                    }
                }
            }
            if episode.local {
                if episode.code != "" {
                    Text(content: tr! { "en" => "{code} · {title}", "zh-Hans" => "{code} · {title}"; code: episode.code, title: episode.title }, bold: true, lines: 1)
                } else {
                    if episode.number > 0 {
                        Text(content: tr! { "en" => "Ep {n} · {title}", "zh-Hans" => "第{n}话 · {title}"; n: episode.number, title: episode.title }, bold: true, lines: 1)
                    } else {
                        Text(content: episode.title, bold: true, lines: 1)
                    }
                }
            } else {
                // 本地没有的，标题也变灰。
                if episode.code != "" {
                    Text(content: tr! { "en" => "{code} · {title}", "zh-Hans" => "{code} · {title}"; code: episode.code, title: episode.title }, emphasis: tertiary, lines: 1)
                } else {
                    if episode.number > 0 {
                        Text(content: tr! { "en" => "Ep {n} · {title}", "zh-Hans" => "第{n}话 · {title}"; n: episode.number, title: episode.title }, emphasis: tertiary, lines: 1)
                    } else {
                        Text(content: episode.title, emphasis: tertiary, lines: 1)
                    }
                }
            }
            if !episode.local {
                Text(content: tr! { "en" => "Not found", "zh-Hans" => "未找到" }, style: caption, emphasis: tertiary)
            } else {
                if episode.finished {
                    Text(content: tr! { "en" => "Watched", "zh-Hans" => "已看完" }, style: caption, emphasis: secondary)
                } else {
                    if episode.percent > 0 {
                        Text(content: tr! { "en" => "{p}% watched", "zh-Hans" => "看到 {p}%"; p: episode.percent }, style: caption, color: accent)
                    } else {
                        Text(content: tr! { "en" => "On disk", "zh-Hans" => "本地有" }, style: caption, emphasis: secondary)
                    }
                }
            }
        }
    }
}

/// 底部两栏：左边评论（先显示 2×2 四条，可以展开这一页全部），右边各站评分和制作信息。
#[component]
pub fn detail_bottom(app: nkuplay::Client) -> impl View {
    ui! {
        Row(gap: 48, align: top) {
            Column(gap: 12) {
                Row(gap: 8, align: baseline) {
                    Text(content: tr! { "en" => "Reviews", "zh-Hans" => "评论" }, style: headline, font_size: 16, bold: true)
                    if app.detail_comment_source != "" {
                        Text(
                            content: tr! {
                                "en" => "{source} · {count}",
                                "zh-Hans" => "{source} · {count} 条";
                                source: app.detail_comment_source,
                                count: app.detail_comment_count
                            },
                            style: caption,
                            emphasis: secondary
                        )
                    }
                }
                Grid(min_width: 300, gap: 28, row_gap: 14) {
                    ForEach(items: app.detail_comments, key: id) |comment| {
                        Column(gap: 4) {
                            Row(gap: 4, align: baseline) {
                                Text(content: comment.user, bold: true, lines: 1)
                                Spacer
                                if comment.score > 0 {
                                    Text(content: comment.score, bold: true, color: status.error)
                                    Text(content: tr! { "en" => "/ 10", "zh-Hans" => "/ 10" }, style: caption, emphasis: secondary)
                                }
                            }
                            if app.detail_comments_expanded {
                                Text(content: comment.text, emphasis: secondary)
                            } else {
                                Text(content: comment.text, lines: 2, emphasis: secondary)
                            }
                            Divider
                        }
                    }
                }
                if app.detail_comments_more {
                    Button(
                        id: expand_comments,
                        label: tr! { "en" => "Show All", "zh-Hans" => "展开全部" },
                        icon: "chevron.down",
                        prominence: plain
                    ) => app.toggle_comments()
                }
                if app.detail_comments_expanded {
                    Button(
                        id: collapse_comments,
                        label: tr! { "en" => "Show Less", "zh-Hans" => "收起" },
                        icon: "chevron.up",
                        prominence: plain
                    ) => app.toggle_comments()
                }
            }
            Column(gap: 26, width: 440) {
                Column(gap: 12) {
                    Text(content: tr! { "en" => "Ratings", "zh-Hans" => "各站评分" }, style: headline, font_size: 16, bold: true)
                    Flow(gap: 14) {
                        ForEach(items: app.detail_scores, key: id) |score| {
                            Text(
                                content: tr! { "en" => "{site}: {score}", "zh-Hans" => "{site}：{score}"; site: score.site, score: score.score },
                                style: caption,
                                emphasis: secondary
                            )
                        }
                    }
                }
                Column(gap: 12) {
                    Text(content: tr! { "en" => "Credits", "zh-Hans" => "制作信息" }, style: headline, font_size: 16, bold: true)
                    Column(gap: 6) {
                        ForEach(items: app.detail_facts, key: id) |fact| {
                            Row(gap: 8, align: top) {
                                Text(content: fact.key, style: caption, bold: true)
                                Text(content: fact.value, style: caption, lines: 2)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 新番时间表（还没做）：先放重新取新番列表的按钮。
#[component]
pub fn schedule_page(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 12, padding_x: 56, padding_y: 28) {
            Row(gap: 8, align: center) {
                Text(content: tr! { "en" => "Schedule", "zh-Hans" => "新番时间表" }, style: large_title)
                Spacer
                Button(
                    id: refresh_schedule,
                    label: tr! { "en" => "Refresh", "zh-Hans" => "刷新" },
                    icon: "arrow.clockwise",
                    enabled: app.shin != ShinStatus::Loading
                ) => app.refresh_shin()
            }
            Text(
                content: tr! { "en" => "This page isn't ready yet.", "zh-Hans" => "这一页还没做。" },
                emphasis: secondary
            )
        }
    }
}

/// 观看历史（还没做）：先放去掉文件已经不在的视频的按钮。
#[component]
pub fn history_page(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 12, padding_x: 56, padding_y: 28) {
            Row(gap: 8, align: center) {
                Text(content: tr! { "en" => "History", "zh-Hans" => "观看历史" }, style: large_title)
                Spacer
                Button(
                    id: refresh_recents,
                    label: tr! { "en" => "Refresh", "zh-Hans" => "刷新" },
                    icon: "arrow.clockwise"
                ) => app.refresh_recents()
            }
            Text(
                content: tr! { "en" => "This page isn't ready yet.", "zh-Hans" => "这一页还没做。" },
                emphasis: secondary
            )
        }
    }
}

// 视频页的动画和底色。
style! {
    transition {
        // 详情页从稍小一点放大着淡入。
        page_in: pop(from: 0.97, smooth),
        // 顶栏从上方滑进、滑出，底栏从下方。
        bar_top: slide(top, 16, smooth),
        bar_bottom: slide(bottom, 16, smooth),
    }
    // 顶栏导航、登录和剧集分组：胶囊。选中的有底色，没选中的只有字。
    Button.nav_on {
        background: color(light: "#FFFFFF", dark: "#F3F3F3"),
        foreground: color(light: "#1B1B1B", dark: "#1B1B1B"),
        corner: 16,
    }
    Button.nav {
        corner: 16,
    }
    // 顶栏的设置：浅色页面上是白圆，大图上是灰圆。
    IconButton.nav_round {
        background: color(light: "#FFFFFF", dark: "#5C5C5C"),
        corner: 17,
    }
    // 首页各栏右边的“观看历史 ›”。
    Button.link {
        foreground: accent,
        padding: 0,
    }
    // 首页大图上的按钮，不跟随深浅色。
    Button.hero_play {
        background: color(light: "#005FB8", dark: "#005FB8"),
        foreground: color(light: "#FFFFFF", dark: "#FFFFFF"),
        corner: 5,
    }
    Button.hero_details {
        background: color(light: "#FFFFFF", dark: "#FFFFFF"),
        foreground: color(light: "#1B1B1B", dark: "#1B1B1B"),
        corner: 5,
    }
    IconButton.hero_arrow {
        background: color(light: "#F5F5F5", dark: "#F5F5F5"),
        foreground: color(light: "#1B1B1B", dark: "#1B1B1B"),
        corner: 17,
    }
    // 本季新番的星期。
    Button.day_on {
        background: color(light: "#1B1B1B", dark: "#F3F3F3"),
        foreground: color(light: "#FFFFFF", dark: "#1B1B1B"),
        corner: 16,
    }
    Button.day {
        background: color(light: "#E9E9E9", dark: "#2D2D2D"),
        foreground: color(light: "#444444", dark: "#CCCCCC"),
        corner: 16,
    }
    // 继续播放卡片的画面：圆角连同底边的进度一起切。
    Stack.card_picture {
        corner: 7,
    }
    Button.group_on {
        background: color(light: "#1B1B1B", dark: "#F3F3F3"),
        foreground: color(light: "#FFFFFF", dark: "#1B1B1B"),
        corner: 16,
    }
    Button.group {
        corner: 16,
    }
    // 圆钮：和 `height` 一半一样的圆角就是圆的。
    IconButton.round {
        background: color(light: "#FFFFFF", dark: "#3A3A3A"),
        corner: 17,
    }
    IconButton.round_large {
        background: color(light: "#FFFFFF", dark: "#3A3A3A"),
        corner: 20,
    }
    // 账号页、设置页盖在页面上，用不透明的页面底色。
    Stack.panel_ground {
        background: color(light: "#F3F3F3", dark: "#202020"),
    }
    // 设置页：选中的分类是白色胶囊，没选中的只有字。
    Button.settings_tab_on {
        background: color(light: "#FFFFFF", dark: "#F3F3F3"),
        foreground: color(light: "#1B1B1B", dark: "#1B1B1B"),
        corner: 18,
    }
    Button.settings_tab {
        corner: 18,
    }
    // 设置页一组行的白卡片。
    Stack.card {
        background: color(light: "#FFFFFF", dark: "#2B2B2B"),
        border: border(color(light: "#E5E5E5", dark: "#3A3A3A"), width: 1),
        corner: 8,
    }
    Row.setting_row {
        height: 48,
        padding_x: 16,
    }
    // 快捷键的键帽：浅底细边。
    Stack.key_cap {
        background: color(light: "#FBFBFB", dark: "#2D2D2D"),
        border: border(color(light: "#E0E0E0", dark: "#3D3D3D"), width: 1),
        corner: 4,
    }
    Row.key_row {
        height: 42,
        padding_x: 14,
    }
    // 外观页里顶栏的样子。
    Row.bar_preview {
        height: 52,
        padding_x: 16,
    }
    // 屏蔽词和弹幕类型的胶囊：开着的是深色。
    Button.chip {
        background: color(light: "#E9E9E9", dark: "#3A3A3A"),
        foreground: color(light: "#1B1B1B", dark: "#F3F3F3"),
        corner: 14,
    }
    Button.chip_on {
        background: color(light: "#1B1B1B", dark: "#F3F3F3"),
        foreground: color(light: "#FFFFFF", dark: "#1B1B1B"),
        corner: 14,
    }
    // 关于页的一行链接：整行是按钮，没有底色。
    Button.link_row {
        padding: 0,
    }
    // 播放页底栏的图标按钮：没有底色，指上去才有；设置面板开着时是白圆。
    // 底栏的弹幕开关：打开时圆点是白的（WinUI 深色下本来是黑的）。
    Switch.bar_switch {
        foreground: color(light: "#FFFFFF", dark: "#FFFFFF"),
    }
    IconButton.bar_button {
        corner: 17,
    }
    IconButton.bar_button_large {
        corner: 19,
    }
    IconButton.bar_button_on {
        background: color(light: "#FFFFFF", dark: "#FFFFFF"),
        foreground: color(light: "#1B1B1B", dark: "#1B1B1B"),
        corner: 17,
    }
    IconButton.round_small {
        background: color(light: "#E9E9E9", dark: "#3A3A3A"),
        corner: 15,
    }
    // 播放页右下角的面板和下一话卡片：深浅跟着系统，不透明（颜色不能带透明度）。
    Stack.panel {
        background: color(light: "#F3F3F3", dark: "#1C1C1C"),
        corner: 10,
    }
    Button.panel_tab_on {
        background: color(light: "#FFFFFF", dark: "#FFFFFF"),
        foreground: color(light: "#1B1B1B", dark: "#1B1B1B"),
        corner: 14,
        padding: 10,
    }
    Button.panel_tab {
        corner: 14,
        padding: 10,
    }
    // 字幕页的一条字幕轨：选中的一条浅蓝底、蓝字。
    Button.subtitle_row {
        corner: 6,
        padding: 0,
    }
    Button.subtitle_row_on {
        background: color(light: "#E5F0FB", dark: "#1F3346"),
        foreground: color(light: "#005FB8", dark: "#60CDFF"),
        corner: 6,
        padding: 0,
    }
    // 面板里的内容：浅色时是白卡片，深色时和面板同色。
    Stack.panel_card {
        background: color(light: "#FFFFFF", dark: "#1C1C1C"),
        border: border(color(light: "#E5E5E5", dark: "#1C1C1C"), width: 1),
        corner: 8,
    }
    // 时间偏移：减、数值、加放在一个带边的胶囊里。
    Stack.stepper {
        border: border(color(light: "#D9D9D9", dark: "#3A3A3A"), width: 1),
        corner: 16,
    }
    IconButton.round_plain {
        corner: 14,
    }
    // 选集里的一集、手动匹配里的一行：整行是按钮。
    Button.episode_row {
        corner: 8,
        padding: 0,
    }
    Button.episode_row_on {
        background: color(light: "#F0F0F0", dark: "#2A2A2A"),
        corner: 8,
        padding: 0,
    }
    Stack.thumb {
        background: color(light: "#D9D9D9", dark: "#2E2E2E"),
        corner: 6,
    }
    // 跳过片头的按钮、长按快进的提示：压在视频上的深色胶囊。
    Button.overlay_button {
        background: color(light: "#1C1C1C", dark: "#1C1C1C"),
        foreground: color(light: "#FFFFFF", dark: "#FFFFFF"),
        corner: 18,
    }
    Stack.toast {
        background: color(light: "#1C1C1C", dark: "#1C1C1C"),
        corner: 18,
    }
    // 媒体库右上的添加文件夹：蓝色圆钮。
    IconButton.add_round {
        background: accent,
        foreground: color(light: "#FFFFFF", dark: "#FFFFFF"),
        corner: 17,
    }
    // 海报：圆角连同底边的进度一起切；看完的勾是黑圆白勾。
    Stack.poster {
        corner: 6,
    }
    Stack.check_badge {
        background: color(light: "#1B1B1B", dark: "#1B1B1B"),
        foreground: color(light: "#FFFFFF", dark: "#FFFFFF"),
        corner: 11,
    }
    // 视频以外的地方（比例不同时的黑边）是纯黑，不露出窗口背景。
    Stack.player_black {
        background: color(light: "#000000", dark: "#000000"),
    }
}

/// 视频页：画面铺满窗口，上面不压任何界面。鼠标一动，透明的顶栏和底栏滑进来，
/// 1 秒不动就滑出去，指针也随之隐藏（窗口是 immersive 的，Bern 在 `window.idle`
/// 时隐藏指针）。全屏由用户自己切换（按钮、F 键、双击画面；全屏时 Esc 退出）。
/// 单击画面暂停或继续，右键打开菜单；上下键调音量，左右键后退、前进 5 秒，D 显示或隐藏弹幕。
#[component]
pub fn player(
    app: nkuplay::Client,
    windows: AppWindows,
    players: Players,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        // 鼠标停在画面上 1 秒才收起上下栏（画面那一层标了 idle_area）；停在上下栏、面板上不收，
        // 免得对着按钮还没点下去栏就收了，点下去落在画面上。
        Stack(variant: player_black) {
            Video(
                id: video,
                player: players.video,
                label: tr! { "en" => "Video", "zh-Hans" => "视频" },
                controls: none,
                sizing: fill
            )
            // 盖在画面上的透明一层，管单击（暂停/继续）和双击（全屏），见 video_gestures.winui3.cpp。
            // Bern 的指针事件没有单击，而且单击和双击要一起分辨，所以用原生代码写。
            // 只有 Windows 有实现；其他平台上这一层什么也不显示。
            NativeView(
                id: gestures,
                label: tr! { "en" => "Video", "zh-Hans" => "视频" },
                winui3: "video_gestures.winui3.cpp",
                idle_area: 1
            ) {
                NativeValue(name: playing, value: players.video.playing)
                NativeValue(name: idle, value: window.idle)
                NativeValue(name: fullscreen, value: window.fullscreen)
                NativeText(name: label, content: tr! { "en" => "Video", "zh-Hans" => "视频" })
                NativeAction(id: tap_play, label: tr! { "en" => "Play", "zh-Hans" => "播放" }) => players.video.play()
                NativeAction(id: tap_pause, label: tr! { "en" => "Pause", "zh-Hans" => "暂停" }) => players.video.pause()
                NativeAction(
                    id: tap_fullscreen,
                    label: tr! { "en" => "Toggle Full Screen", "zh-Hans" => "切换全屏" }
                ) => windows.main.toggle_fullscreen()
                ContextMenu {
                    // 菜单里不能写条件，播放和暂停各一项，用 enabled 区分。
                    MenuItem(
                        id: menu_play,
                        label: tr! { "en" => "Play", "zh-Hans" => "播放" },
                        icon: "play.fill",
                        enabled: !players.video.playing
                    ) => players.video.play()
                    MenuItem(
                        id: menu_pause,
                        label: tr! { "en" => "Pause", "zh-Hans" => "暂停" },
                        icon: "pause.fill",
                        enabled: players.video.playing
                    ) => players.video.pause()
                    MenuItem(
                        id: menu_fullscreen,
                        label: tr! { "en" => "Toggle Full Screen", "zh-Hans" => "切换全屏" },
                        icon: "arrow.up.left.and.arrow.down.right"
                    ) => windows.main.toggle_fullscreen()
                    Divider
                    MenuItem(
                        id: menu_open,
                        label: tr! { "en" => "Open…", "zh-Hans" => "打开…" },
                        pick: draft.file,
                        files: "video"
                    ) => app.open_file(draft.file)
                    MenuItem(id: menu_back, label: tr! { "en" => "Back", "zh-Hans" => "返回" }) => app.close()
                }
            }
            // 引擎每打开一次视频 `opened` 就变一次，这时让播放器载入它。
            Watch(value: app.opened) => players.video.open(app.source)
            // 音量由引擎记着（方向键要在当前音量上加减），变了就交给播放器。
            Watch(value: app.volume) => players.video.set_volume(app.volume)
            // 进度条给的是比例，引擎换算成毫秒后由这里交给播放器。
            // 换视频时 `seek_serial` 归零，旧的跳转不会用到新视频上。
            if app.seek_serial > 0 {
                Watch(value: app.seek_serial) => players.video.seek(app.seek_ms)
            }
            // 打开时要自动全屏：还没全屏就进入，进入以后告诉引擎。
            if app.fullscreen_pending {
                if window.fullscreen {
                    Watch(value: app.opened) => app.fullscreen_done()
                } else {
                    Watch(value: app.opened) => windows.main.toggle_fullscreen()
                }
            }
            player_keys(app: app, windows: windows, players: players)
            // 观看进度：播放时每 5 秒记一次，暂停、播完时也记（回首页时在 main_window 里记最后一次）。
            Timer(interval: 5, enabled: players.video.playing) => app.record_progress(players.video.position_ms, players.video.duration_ms)
            Watch(value: players.video.playing) => app.record_progress(players.video.position_ms, players.video.duration_ms)
            // 每秒报一次位置：片头片尾的按钮、自动跳过、播完以后的倒计时都靠它。
            Timer(interval: 1) => app.tick(players.video.position_ms, players.video.duration_ms)
            // 上下栏压在视频上，用深色外观：文字和图标是浅色的。
            // 各放在一个叠层里：上下栏收起的动画还没放完又叫出来时，正在消失的旧栏和新栏叠在同一处，
            // 不会把新栏挤到它下面去（放在同一列里时顶栏会往下错开一个栏高）。
            Column(appearance: dark) {
                Stack(align: top) {
                    if !window.idle {
                        top_bar(app: app, windows: windows)
                    }
                }
                Spacer
                Stack(align: bottom) {
                    if !window.idle {
                        play_bar(app: app, windows: windows, players: players)
                    }
                }
            }
            // 片头片尾：左下角“跳过片头 到 2:09”。
            if app.skip_shown {
                Column(padding_x: 24, padding_y: 120) {
                    Spacer
                    Row {
                        if app.skip_outro {
                            Button(id: skip_outro, label: tr! { "en" => "Skip Ending", "zh-Hans" => "跳过片尾" }, variant: overlay_button) {
                                Row(gap: 8, align: center) {
                                    Text(content: tr! { "en" => "Skip Ending", "zh-Hans" => "跳过片尾" })
                                    Text(content: app.skip_label, emphasis: secondary)
                                }
                            } => app.skip_part()
                        } else {
                            Button(id: skip_intro, label: tr! { "en" => "Skip Opening", "zh-Hans" => "跳过片头" }, variant: overlay_button) {
                                Row(gap: 8, align: center) {
                                    Text(content: tr! { "en" => "Skip Opening", "zh-Hans" => "跳过片头" })
                                    Text(content: app.skip_label, emphasis: secondary)
                                }
                            } => app.skip_part()
                        }
                        Spacer
                    }
                }
            }
            // 播完一集：右下角下一话倒计时。
            if app.next_shown {
                Column(padding_x: 24, padding_y: 120) {
                    Spacer
                    Row {
                        Spacer
                        next_card(app: app)
                    }
                }
            }
            // 设置面板：右下角，在底栏上方。
            if draft.player_panel != PlayerTab::Closed {
                Column(padding_x: 24, padding_y: 104) {
                    Spacer
                    Row {
                        Spacer
                        player_panel(app: app, players: players)
                    }
                }
            }
            // 按住长按快进时，画面中间偏上提示一下。
            if app.fast {
                Column(padding_y: 72) {
                    Row {
                        Spacer
                        Stack(variant: toast) {
                            Row(gap: 8, padding_x: 14, padding_y: 8, align: center) {
                                Label(title: tr! { "en" => "Fast forward", "zh-Hans" => "快进" }, icon: "forward.end.fill", display: icon)
                                Text(content: app.hold_choice)
                            }
                        }
                        Spacer
                    }
                    Spacer
                }
            }
            if app.match_open {
                match_dialog(app: app)
            }
        }
    }
}

/// 播放页的快捷键，按键跟着设置走。
#[component]
pub fn player_keys(
    app: nkuplay::Client,
    windows: AppWindows,
    players: Players,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        Stack {
            // 同一个键只能挂一个快捷键：WinUI 只认先挂上的那个，另一个即使 enabled 也不触发。
            // 所以按状态换着挂，不用 enabled 区分。
            if players.video.playing {
                Shortcut(key: app.keys.play) => players.video.pause()
            } else {
                Shortcut(key: app.keys.play) => players.video.play()
            }
            Shortcut(key: app.keys.louder) => app.nudge_volume(0.05)
            Shortcut(key: app.keys.quieter) => app.nudge_volume(-0.05)
            // `quiet`：不算“有操作”，上下栏和指针不出来。快进键按住时是长按快进。
            Shortcut(key: app.keys.rewind, quiet: true) => app.seek_steps(-1, players.video.position_ms, players.video.duration_ms)
            Shortcut(key: app.keys.forward, quiet: true, hold: draft.fast) => app.seek_steps(1, players.video.position_ms, players.video.duration_ms)
            Watch(value: draft.fast) => app.set_fast(draft.fast)
            Shortcut(key: app.keys.previous) => app.play_previous()
            Shortcut(key: app.keys.next) => app.play_next()
            Shortcut(key: app.keys.skip) => app.skip_ahead(players.video.position_ms, players.video.duration_ms)
            Shortcut(key: app.keys.faster) => app.nudge_speed(0.1)
            Shortcut(key: app.keys.slower) => app.nudge_speed(-0.1)
            Shortcut(key: app.keys.normal_speed) => app.set_speed(1.0)
            if players.video.muted {
                Shortcut(key: app.keys.mute) => players.video.set_muted(false)
            } else {
                Shortcut(key: app.keys.mute) => players.video.set_muted(true)
            }
            Shortcut(key: app.keys.danmaku) => app.toggle_danmaku()
            Shortcut(key: app.keys.danmaku_panel) => draft.player_panel.set(PlayerTab::Danmaku)
            Shortcut(key: app.keys.subtitle) => app.cycle_subtitle()
            Shortcut(key: app.keys.audio) => app.cycle_audio()
            Shortcut(key: app.keys.fullscreen) => windows.main.toggle_fullscreen()
            Shortcut(key: app.keys.exit_fullscreen, enabled: window.fullscreen) => windows.main.toggle_fullscreen()
            Shortcut(key: app.keys.episodes) => draft.player_panel.set(PlayerTab::Episodes)
            Shortcut(key: app.keys.info) => draft.player_panel.set(PlayerTab::Info)
        }
    }
}

/// 顶栏：左上返回、番名和集名（没认出时是文件名）；右上角什么都没有。
/// 底下是一层从上往下渐隐的暗色。
#[component]
pub fn top_bar(app: nkuplay::Client, windows: AppWindows) -> impl View {
    ui! {
        Stack(appear: bar_top, disappear: bar_top) {
            Canvas {
                Shape(kind: rect, fill: surface.canvas, fade: down, opacity: 0.7)
            }
            Row(gap: 12, padding_x: 20, padding_y: 12, align: center) {
                // 全屏时“返回”先只退出全屏，再按一次才回首页（用户要求）。
                if window.fullscreen {
                    IconButton(id: back_from_fullscreen, icon: "arrow.left", label: tr! { "en" => "Back", "zh-Hans" => "返回" }, prominence: plain) => windows.main.toggle_fullscreen()
                } else {
                    IconButton(id: back, icon: "arrow.left", label: tr! { "en" => "Back", "zh-Hans" => "返回" }, prominence: plain) => app.close()
                }
                Column(gap: 0) {
                    Text(content: app.play_title, font_size: 16, bold: true, lines: 1)
                    if app.play_subtitle != "" {
                        Text(content: app.play_subtitle, font_size: 12, lines: 1)
                    }
                }
                Spacer
            }
        }
    }
}

/// 底栏：进度条；左边上一话、播放、下一话、时间，右边弹幕开关、倍速、设置、音量、全屏。
/// 底下是一层从下往上渐隐的暗色。菜单和弹出层都往上弹。
#[component]
pub fn play_bar(
    app: nkuplay::Client,
    windows: AppWindows,
    players: Players,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        Stack(appear: bar_bottom, disappear: bar_bottom) {
            Canvas {
                Shape(kind: rect, fill: surface.canvas, fade: up, opacity: 0.7)
            }
            Column(gap: 2, padding_x: 24, padding_y: 10) {
                // 进度条用原生代码画（seek_bar.winui3.cpp）：章节分段、弹幕热度、悬停时间。
                // 松开时把比例交给引擎，引擎换算成毫秒再让播放器跳。
                NativeView(
                    id: seek_bar,
                    label: tr! { "en" => "Position", "zh-Hans" => "播放进度" },
                    winui3: "seek_bar.winui3.cpp"
                ) {
                    NativeValue(name: duration, value: players.video.duration)
                    NativeValue(name: chapters, value: app.chapters)
                    NativeValue(name: heat, value: app.heat)
                    NativeNumber(
                        id: seek,
                        label: tr! { "en" => "Position", "zh-Hans" => "播放进度" },
                        current: players.video.progress,
                        min: 0,
                        max: 1
                    ) => app.seek(players.video.progress, players.video.duration_ms)
                    // 没有原生实现的平台用标准滑块，没有悬停时间。
                    Fallback {
                        Slider(
                            id: seek_fallback,
                            label: tr! { "en" => "Position", "zh-Hans" => "播放进度" },
                            label_hidden: true,
                            current: players.video.progress,
                            min: 0,
                            max: 1
                        ) => app.seek(players.video.progress, players.video.duration_ms)
                    }
                }
                Row(gap: 4, align: center) {
                    Tooltip(content: tr! { "en" => "Previous", "zh-Hans" => "上一话" }, keys: app.keys.previous, placement: above) {
                        IconButton(id: previous, icon: "resources/icons/prev.svg", label: tr! { "en" => "Previous", "zh-Hans" => "上一话" }, font_size: 22, height: 34, enabled: app.has_previous, prominence: plain, variant: bar_button) => app.play_previous()
                    }
                    if players.video.playing {
                        Tooltip(content: tr! { "en" => "Pause", "zh-Hans" => "暂停" }, keys: app.keys.play, placement: above) {
                            IconButton(id: pause, icon: "resources/icons/pause.svg", label: tr! { "en" => "Pause", "zh-Hans" => "暂停" }, font_size: 26, height: 38, prominence: plain, variant: bar_button_large) => players.video.pause()
                        }
                    } else {
                        Tooltip(content: tr! { "en" => "Play", "zh-Hans" => "播放" }, keys: app.keys.play, placement: above) {
                            IconButton(id: play, icon: "resources/icons/play.svg", label: tr! { "en" => "Play", "zh-Hans" => "播放" }, font_size: 26, height: 38, prominence: plain, variant: bar_button_large) => players.video.play()
                        }
                    }
                    Tooltip(content: tr! { "en" => "Next", "zh-Hans" => "下一话" }, keys: app.keys.next, placement: above) {
                        IconButton(id: next, icon: "resources/icons/next.svg", label: tr! { "en" => "Next", "zh-Hans" => "下一话" }, font_size: 22, height: 34, enabled: app.has_next, prominence: plain, variant: bar_button) => app.play_next()
                    }
                    Row(padding_x: 10) {
                        play_time(at: players.video.position, total: players.video.duration)
                    }
                    Spacer
                    // 弹幕开关：“弹”字图标和一个开关胶囊，点哪里都切换。
                    Row(gap: 6, align: center) {
                        Tooltip(content: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, keys: app.keys.danmaku, placement: above) {
                            IconButton(id: danmaku_icon, icon: "resources/icons/danmaku.svg", label: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, font_size: 22, height: 34, prominence: plain, variant: bar_button) => app.toggle_danmaku()
                        }
                        Switch(id: danmaku_switch, label: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, label_hidden: true, checked: app.danmaku_visible, variant: bar_switch) => app.toggle_danmaku()
                    }
                    Row(width: 6) {}
                    // 倍速：写着现在的倍速，点开往上弹。
                    Menu(id: speed, label: tr! { "en" => "{s}", "zh-Hans" => "{s}"; s: app.speed_text }, style: icon, shape: capsule, height: 34, placement: above, bold: true) {
                        MenuItem(id: speed_3, label: tr! { "en" => "3.0×", "zh-Hans" => "3.0×" }, checked: app.speed == 3.0) => app.set_speed(3.0)
                        MenuItem(id: speed_2, label: tr! { "en" => "2.0×", "zh-Hans" => "2.0×" }, checked: app.speed == 2.0) => app.set_speed(2.0)
                        MenuItem(id: speed_15, label: tr! { "en" => "1.5×", "zh-Hans" => "1.5×" }, checked: app.speed == 1.5) => app.set_speed(1.5)
                        MenuItem(id: speed_125, label: tr! { "en" => "1.25×", "zh-Hans" => "1.25×" }, checked: app.speed == 1.25) => app.set_speed(1.25)
                        MenuItem(id: speed_1, label: tr! { "en" => "1.0×", "zh-Hans" => "1.0×" }, checked: app.speed == 1.0) => app.set_speed(1.0)
                        MenuItem(id: speed_075, label: tr! { "en" => "0.75×", "zh-Hans" => "0.75×" }, checked: app.speed == 0.75) => app.set_speed(0.75)
                        MenuItem(id: speed_05, label: tr! { "en" => "0.5×", "zh-Hans" => "0.5×" }, checked: app.speed == 0.5) => app.set_speed(0.5)
                    }
                    // 设置：打开或收起右下角的面板。
                    if draft.player_panel == PlayerTab::Closed {
                        Tooltip(content: tr! { "en" => "Settings", "zh-Hans" => "设置" }, keys: app.keys.danmaku_panel, placement: above) {
                            IconButton(id: open_panel, icon: "resources/icons/settings.svg", label: tr! { "en" => "Settings", "zh-Hans" => "设置" }, font_size: 22, height: 34, prominence: plain, variant: bar_button) => draft.player_panel.set(PlayerTab::Danmaku)
                        }
                    } else {
                        IconButton(id: close_panel_bar, icon: "resources/icons/settings.svg", label: tr! { "en" => "Settings", "zh-Hans" => "设置" }, font_size: 22, height: 34, variant: bar_button_on) => draft.player_panel.set(PlayerTab::Closed)
                    }
                    // 音量：点开在图标上方弹出竖着的滑块。
                    PopoverButton(
                        id: volume,
                        label: tr! { "en" => "Volume", "zh-Hans" => "音量" },
                        icon: "resources/icons/volume.svg",
                        font_size: 22,
                        height: 34,
                        prominence: plain,
                        placement: above
                    ) {
                        Column(gap: 6, width: 44, height: 150, align: center) {
                            Text(content: tr! { "en" => "{n}", "zh-Hans" => "{n}"; n: app.volume_percent }, font_size: 12)
                            Slider(
                                id: volume_slider,
                                label: tr! { "en" => "Volume", "zh-Hans" => "音量" },
                                label_hidden: true,
                                current: app.volume,
                                continuous: true,
                                min: 0,
                                max: 1,
                                step: 0.01,
                                axis: vertical
                            ) => app.set_volume(app.volume)
                        }
                    }
                    if window.fullscreen {
                        Tooltip(content: tr! { "en" => "Exit Full Screen", "zh-Hans" => "退出全屏" }, keys: app.keys.exit_fullscreen, placement: above) {
                            IconButton(id: exit_fullscreen, icon: "resources/icons/exit_fullscreen.svg", label: tr! { "en" => "Exit Full Screen", "zh-Hans" => "退出全屏" }, font_size: 22, height: 34, prominence: plain, variant: bar_button) => windows.main.toggle_fullscreen()
                        }
                    } else {
                        Tooltip(content: tr! { "en" => "Full Screen", "zh-Hans" => "全屏" }, keys: app.keys.fullscreen, placement: above) {
                            IconButton(id: enter_fullscreen, icon: "resources/icons/fullscreen.svg", label: tr! { "en" => "Full Screen", "zh-Hans" => "全屏" }, font_size: 22, height: 34, prominence: plain, variant: bar_button) => windows.main.toggle_fullscreen()
                        }
                    }
                }
            }
        }
    }
}

/// 播到哪里和总时长，例如 `12:34 / 24:00`；一小时以上的视频写成 `1:02:03`。
#[component]
pub fn play_time(at: f64, total: f64) -> impl View {
    ui! {
        // 现在的位置白字，总时长灰一些。
        if total >= 3600.0 {
            Row(gap: 0, align: center) {
                Text(content: tr! { "en" => "{at, duration, h:mm:ss}", "zh-Hans" => "{at, duration, h:mm:ss}"; at: at }, font_size: 13, bold: true)
                Text(content: tr! { "en" => " / {total, duration, h:mm:ss}", "zh-Hans" => " / {total, duration, h:mm:ss}"; total: total }, font_size: 13, emphasis: secondary)
            }
        } else {
            Row(gap: 0, align: center) {
                Text(content: tr! { "en" => "{at, duration, m:ss}", "zh-Hans" => "{at, duration, m:ss}"; at: at }, font_size: 13, bold: true)
                Text(content: tr! { "en" => " / {total, duration, m:ss}", "zh-Hans" => " / {total, duration, m:ss}"; total: total }, font_size: 13, emphasis: secondary)
            }
        }
    }
}

/// 下一话倒计时卡片。
#[component]
pub fn next_card(app: nkuplay::Client) -> impl View {
    ui! {
        Stack(variant: panel) {
            Column(gap: 6, padding_x: 16, padding_y: 14, width: 300) {
                Text(content: tr! { "en" => "Next in {n} s", "zh-Hans" => "{n} 秒后播放"; n: app.next_left }, font_size: 12, emphasis: secondary)
                Text(content: app.next_code, bold: true)
                if app.next_title != "" {
                    Text(content: app.next_title, lines: 1)
                }
                Row(gap: 8) {
                    Button(id: play_next_now, label: tr! { "en" => "Play Now", "zh-Hans" => "马上播放" }, icon: "play.fill", prominence: prominent) => app.play_next()
                    Button(id: cancel_next, label: tr! { "en" => "Cancel", "zh-Hans" => "取消" }) => app.cancel_next()
                }
            }
        }
    }
}

/// 设置面板：顶上七页，弹幕页底下还有弹幕列表、来源、屏蔽词三层。深浅跟着系统。
#[component]
pub fn player_panel(
    app: nkuplay::Client,
    players: Players,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        Stack(variant: panel) {
            Column(width: 440, gap: 0) {
                match draft.player_panel {
                    PlayerTab::DanmakuList => { sub_header(title: PlayerTab::DanmakuList) }
                    PlayerTab::DanmakuSource => { sub_header(title: PlayerTab::DanmakuSource) }
                    PlayerTab::Blocked => { sub_header(title: PlayerTab::Blocked) }
                    _ => {
                        Row(gap: 0, padding_x: 14, height: 50, align: center) {
                            if draft.player_panel == PlayerTab::Danmaku {
                                Button(id: tab_danmaku_on, label: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, height: 28, variant: panel_tab_on) {
                                    Text(content: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Danmaku)
                            } else {
                                Button(id: tab_danmaku, label: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, height: 28, variant: panel_tab) {
                                    Text(content: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Danmaku)
                            }
                            if draft.player_panel == PlayerTab::Subtitle {
                                Button(id: tab_subtitle_on, label: tr! { "en" => "Subtitles", "zh-Hans" => "字幕" }, height: 28, variant: panel_tab_on) {
                                    Text(content: tr! { "en" => "Subtitles", "zh-Hans" => "字幕" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Subtitle)
                            } else {
                                Button(id: tab_subtitle, label: tr! { "en" => "Subtitles", "zh-Hans" => "字幕" }, height: 28, variant: panel_tab) {
                                    Text(content: tr! { "en" => "Subtitles", "zh-Hans" => "字幕" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Subtitle)
                            }
                            if draft.player_panel == PlayerTab::Picture {
                                Button(id: tab_picture_on, label: tr! { "en" => "Picture", "zh-Hans" => "画面" }, height: 28, variant: panel_tab_on) {
                                    Text(content: tr! { "en" => "Picture", "zh-Hans" => "画面" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Picture)
                            } else {
                                Button(id: tab_picture, label: tr! { "en" => "Picture", "zh-Hans" => "画面" }, height: 28, variant: panel_tab) {
                                    Text(content: tr! { "en" => "Picture", "zh-Hans" => "画面" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Picture)
                            }
                            if draft.player_panel == PlayerTab::Play {
                                Button(id: tab_play_on, label: tr! { "en" => "Playback", "zh-Hans" => "播放" }, height: 28, variant: panel_tab_on) {
                                    Text(content: tr! { "en" => "Playback", "zh-Hans" => "播放" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Play)
                            } else {
                                Button(id: tab_play, label: tr! { "en" => "Playback", "zh-Hans" => "播放" }, height: 28, variant: panel_tab) {
                                    Text(content: tr! { "en" => "Playback", "zh-Hans" => "播放" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Play)
                            }
                            if draft.player_panel == PlayerTab::Episodes {
                                Button(id: tab_episodes_on, label: tr! { "en" => "Episodes", "zh-Hans" => "选集" }, height: 28, variant: panel_tab_on) {
                                    Text(content: tr! { "en" => "Episodes", "zh-Hans" => "选集" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Episodes)
                            } else {
                                Button(id: tab_episodes, label: tr! { "en" => "Episodes", "zh-Hans" => "选集" }, height: 28, variant: panel_tab) {
                                    Text(content: tr! { "en" => "Episodes", "zh-Hans" => "选集" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Episodes)
                            }
                            if draft.player_panel == PlayerTab::Info {
                                Button(id: tab_info_on, label: tr! { "en" => "Info", "zh-Hans" => "信息" }, height: 28, variant: panel_tab_on) {
                                    Text(content: tr! { "en" => "Info", "zh-Hans" => "信息" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Info)
                            } else {
                                Button(id: tab_info, label: tr! { "en" => "Info", "zh-Hans" => "信息" }, height: 28, variant: panel_tab) {
                                    Text(content: tr! { "en" => "Info", "zh-Hans" => "信息" }, font_size: 13)
                                } => draft.player_panel.set(PlayerTab::Info)
                            }
                            Spacer
                            IconButton(id: close_player_panel, icon: "xmark", label: tr! { "en" => "Close", "zh-Hans" => "关闭" }, font_size: 12, prominence: plain) => draft.player_panel.set(PlayerTab::Closed)
                        }
                    }
                }
                Column(gap: 0, padding_x: 12) {
                Stack(variant: panel_card) {
                Column(gap: 0, padding_x: 16, padding_y: 4) {
                    match draft.player_panel {
                        PlayerTab::Danmaku => { panel_danmaku(app: app) }
                        PlayerTab::Subtitle => { panel_subtitles(app: app) }
                        PlayerTab::Picture => { panel_picture(app: app) }
                        PlayerTab::Play => { panel_play(app: app) }
                        PlayerTab::Episodes => { panel_episodes(app: app) }
                        PlayerTab::Info => { panel_info(app: app) }
                        PlayerTab::DanmakuList => { panel_danmaku_list(app: app, players: players) }
                        PlayerTab::DanmakuSource => { panel_danmaku_source(app: app) }
                        PlayerTab::Blocked => { panel_blocked(app: app) }
                        PlayerTab::Closed => {}
                    }
                }
                }
                Row(height: 12) {}
                }
            }
        }
    }
}

/// 弹幕页下一层的标题：左上角返回弹幕页。
#[component]
pub fn sub_header(title: PlayerTab, #[session] draft: MainDraft) -> impl View {
    ui! {
        Row(gap: 6, padding_x: 10, height: 52, align: center) {
            IconButton(id: back_to_danmaku, icon: "arrow.left", label: tr! { "en" => "Back", "zh-Hans" => "返回" }, font_size: 13, prominence: plain) => draft.player_panel.set(PlayerTab::Danmaku)
            Text(content: title, font_size: 16, bold: true)
            Spacer
            IconButton(id: close_sub_panel, icon: "xmark", label: tr! { "en" => "Close", "zh-Hans" => "关闭" }, font_size: 12, prominence: plain) => draft.player_panel.set(PlayerTab::Closed)
        }
    }
}

/// 面板里的一行滑块：名字、滑块、数值。
#[component]
pub fn panel_slider(app: nkuplay::Client, amount: Amount, value: f64) -> impl View {
    ui! {
        Row(height: 44, gap: 12, align: center) {
            Column(width: 148) {
                Text(content: amount)
            }
            Column(width: 170) {
                match amount {
                    Amount::Opacity => { Slider(id: p_opacity, label: amount, label_hidden: true, current: value, min: 10, max: 100, step: 1) => app.set_amount(amount, value) }
                    Amount::Size => { Slider(id: p_size, label: amount, label_hidden: true, current: value, min: 18, max: 64, step: 1) => app.set_amount(amount, value) }
                    Amount::ScrollTime => { Slider(id: p_scroll, label: amount, label_hidden: true, current: value, min: 4, max: 16, step: 0.5) => app.set_amount(amount, value) }
                    Amount::Area => { Slider(id: p_area, label: amount, label_hidden: true, current: value, min: 25, max: 100, step: 5) => app.set_amount(amount, value) }
                    _ => {}
                }
            }
            Spacer
            match amount {
                Amount::ScrollTime => { Text(content: tr! { "en" => "{n} s", "zh-Hans" => "{n} 秒"; n: value }, font_size: 13) }
                Amount::Size => { Text(content: tr! { "en" => "{n}", "zh-Hans" => "{n}"; n: value }, font_size: 13) }
                _ => { Text(content: tr! { "en" => "{n}%", "zh-Hans" => "{n}%"; n: value }, font_size: 13) }
            }
        }
    }
}

/// 面板里的一行开关。
#[component]
pub fn panel_toggle(app: nkuplay::Client, toggle: Toggle, on: bool) -> impl View {
    ui! {
        Row(height: 44, align: center) {
            Text(content: toggle)
            Spacer
            Switch(id: panel_switch, label: toggle, label_hidden: true, checked: on) => app.set_toggle(toggle, on)
        }
    }
}

/// 弹幕页：样式、类型、偏移、屏蔽词，下面是弹幕列表和来源。
#[component]
pub fn panel_danmaku(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            panel_slider(app: app, amount: Amount::Opacity, value: app.d_opacity)
            panel_slider(app: app, amount: Amount::Size, value: app.d_size)
            panel_slider(app: app, amount: Amount::ScrollTime, value: app.d_seconds)
            panel_slider(app: app, amount: Amount::Area, value: app.d_area)
            Divider
            Row(height: 44, align: center) {
                Text(content: tr! { "en" => "Kinds", "zh-Hans" => "类型" })
                Spacer
                danmaku_kinds(app: app)
            }
            panel_toggle(app: app, toggle: Toggle::Merge, on: app.d_merge)
            Divider
            Row(height: 44, align: center) {
                Text(content: tr! { "en" => "Time offset", "zh-Hans" => "时间偏移" })
                Spacer
                Stack(variant: stepper) {
                    Row(gap: 0, padding_x: 2, align: center) {
                        IconButton(id: shift_back, icon: "minus", label: tr! { "en" => "Earlier", "zh-Hans" => "提前" }, font_size: 12, height: 28, prominence: plain, variant: round_plain) => app.nudge_shift(-0.5)
                        Column(width: 56) {
                            Row {
                                Spacer
                                Text(content: app.danmaku_shift_text, font_size: 13)
                                Spacer
                            }
                        }
                        IconButton(id: shift_later, icon: "plus", label: tr! { "en" => "Later", "zh-Hans" => "推后" }, font_size: 12, height: 28, prominence: plain, variant: round_plain) => app.nudge_shift(0.5)
                    }
                }
            }
            panel_blocked_row(app: app)
            Divider
            Button(id: open_danmaku_list, label: tr! { "en" => "Danmaku list", "zh-Hans" => "弹幕列表" }, width: fill, variant: link_row) {
                Row(height: 44, gap: 8, align: center) {
                    Text(content: tr! { "en" => "Danmaku list", "zh-Hans" => "弹幕列表" })
                    Spacer
                    Text(content: tr! { "en" => "{n} comments", "zh-Hans" => "{n} 条"; n: app.danmaku_count }, font_size: 13, emphasis: secondary)
                    Label(title: tr! { "en" => "Danmaku list", "zh-Hans" => "弹幕列表" }, icon: "chevron.right", display: icon)
                }
            } => draft.player_panel.set(PlayerTab::DanmakuList)
            Button(id: open_danmaku_source, label: tr! { "en" => "Source", "zh-Hans" => "来源" }, width: fill, variant: link_row) {
                Row(height: 44, gap: 8, align: center) {
                    Text(content: tr! { "en" => "Source", "zh-Hans" => "来源" })
                    Spacer
                    if app.danmaku_from_file {
                        Text(content: tr! { "en" => "Local file", "zh-Hans" => "本地文件" }, font_size: 13, emphasis: secondary)
                    } else {
                        Text(content: tr! { "en" => "dandanplay", "zh-Hans" => "弹弹play" }, font_size: 13, emphasis: secondary)
                    }
                    Label(title: tr! { "en" => "Source", "zh-Hans" => "来源" }, icon: "chevron.right", display: icon)
                }
            } => draft.player_panel.set(PlayerTab::DanmakuSource)
        }
    }
}

/// 屏蔽词一行：写着有几个，点开是屏蔽词那一层。
#[component]
pub fn panel_blocked_row(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Row(height: 44, align: center) {
            Text(content: tr! { "en" => "Blocked words", "zh-Hans" => "屏蔽词" })
            Spacer
            Button(id: edit_blocked, label: tr! { "en" => "{n} words", "zh-Hans" => "{n} 个"; n: app.blocked_count }, icon: "square.and.pencil") => draft.player_panel.set(PlayerTab::Blocked)
        }
    }
}

/// 屏蔽词那一层：每个词一个胶囊，点了删掉；下面输入新的。
#[component]
pub fn panel_blocked(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 10, padding_y: 6) {
            Flow(gap: 8) {
                ForEach(items: app.blocked, key: id) |word| {
                    Button(id: panel_unblock, label: tr! { "en" => "{w} ×", "zh-Hans" => "{w} ×"; w: word.text }, variant: chip) => app.remove_blocked(word.id)
                }
            }
            TextInput(
                id: panel_add_word,
                label: tr! { "en" => "Add a blocked word", "zh-Hans" => "添加屏蔽词" },
                label_hidden: true,
                value: draft.word
            ) => app.add_blocked(draft.word)
        }
    }
}

/// 字幕页：上面加载本地字幕文件，下面是视频里的字幕轨（和导入过的），点一条就换成它。
/// 默认显示视频自带的字幕（引擎让 mpv 优先选中文的）。
#[component]
pub fn panel_subtitles(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 0, padding_y: 8) {
            Watch(value: app.opened) => app.load_info()
            Button(id: import_subtitle, label: tr! { "en" => "Load a Subtitle File", "zh-Hans" => "加载本地字幕文件" }, icon: "plus.circle", width: fill, pick: draft.track_file, files: ".ass .ssa .srt .vtt .sup .sub") => app.add_subtitle(draft.track_file)
            Row(height: 40, align: bottom) {
                Text(content: tr! { "en" => "Subtitles in the video", "zh-Hans" => "内嵌字幕" }, font_size: 13, bold: true)
            }
            if app.subtitle_count == 0 {
                Row(height: 48, align: center) {
                    Text(content: tr! { "en" => "This video has no subtitles.", "zh-Hans" => "这个视频没有字幕。" }, font_size: 13, emphasis: secondary)
                }
            } else {
                Column(height: 300) {
                    ScrollView {
                        Column(gap: 2, padding_y: 4) {
                            ForEach(items: app.subtitles, key: id) |track| {
                                if track.on {
                                    Button(id: subtitle_on, label: track.name, width: fill, variant: subtitle_row_on) {
                                        Row(gap: 12, padding_x: 10, padding_y: 7, align: center) {
                                            Label(title: track.name, icon: "checkmark.circle.fill", display: icon)
                                            Column(gap: 1) {
                                                Text(content: track.name, font_size: 13, bold: true, lines: 1)
                                                Text(content: track.note, font_size: 12, emphasis: secondary, lines: 1)
                                            }
                                            Spacer
                                        }
                                    } => app.select_subtitle(track.id)
                                } else {
                                    Button(id: subtitle_off, label: track.name, width: fill, variant: subtitle_row) {
                                        Row(gap: 12, padding_x: 10, padding_y: 7, align: center) {
                                            Label(title: track.name, icon: "circle", display: icon)
                                            Column(gap: 1) {
                                                Text(content: track.name, font_size: 13, lines: 1)
                                                Text(content: track.note, font_size: 12, emphasis: secondary, lines: 1)
                                            }
                                            Spacer
                                        }
                                    } => app.select_subtitle(track.id)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 画面页：比例和 Anime4K 两排胶囊。
#[component]
pub fn panel_picture(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 8, padding_y: 4) {
            Text(content: tr! { "en" => "Aspect ratio", "zh-Hans" => "比例" }, font_size: 12, emphasis: secondary)
            Picker(id: panel_aspect, label: tr! { "en" => "Aspect ratio", "zh-Hans" => "比例" }, label_hidden: true, selected: app.aspect, style: chips, shape: capsule) => app.set_aspect(app.aspect)
            Text(content: tr! { "en" => "Anime4K upscaling", "zh-Hans" => "Anime4K 超分" }, font_size: 12, emphasis: secondary)
            Picker(id: panel_upscale, label: tr! { "en" => "Anime4K upscaling", "zh-Hans" => "Anime4K 超分" }, label_hidden: true, selected: app.upscale, style: chips, shape: capsule) => app.set_upscale(app.upscale)
        }
    }
}

/// 播放页：快进快退的秒数和长按快进的倍速，和设置页“播放”里的是同一份。
#[component]
pub fn panel_play(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 4) {
            Row(height: 44, gap: 8, align: center) {
                // 名字一列定宽，右边的胶囊挤不着它。
                Column(width: 64) {
                    Text(content: tr! { "en" => "Seek step", "zh-Hans" => "快进快退" }, lines: 1)
                }
                Spacer
                Picker(id: panel_seek, label: tr! { "en" => "Seek step", "zh-Hans" => "快进快退" }, label_hidden: true, selected: app.seek_choice, style: chips, shape: capsule) => app.set_seek_step(app.seek_choice)
            }
            Row(height: 44, gap: 8, align: center) {
                Column(width: 64) {
                    Text(content: tr! { "en" => "Hold speed", "zh-Hans" => "长按快进" }, lines: 1)
                }
                Spacer
                Picker(id: panel_hold, label: tr! { "en" => "Hold speed", "zh-Hans" => "长按快进" }, label_hidden: true, selected: app.hold_choice, style: chips, shape: capsule) => app.set_hold_rate(app.hold_choice)
            }
        }
    }
}

/// 选集页：这部番在媒体库里的每一集。
#[component]
pub fn panel_episodes(app: nkuplay::Client) -> impl View {
    ui! {
        if app.episode_count == 0 {
            Row(height: 60, align: center) {
                Text(content: tr! { "en" => "This video is not in the library.", "zh-Hans" => "这个视频不在媒体库里。" }, emphasis: secondary)
            }
        } else {
            Column(height: 520) {
                ScrollView {
                    Column(gap: 4) {
                        // 每一集：缩略图（看到一半的下面有进度）、集数、标题、状态。正在播放的一行灰底；
                        // 本地没有的整行变灰、点不了。
                        ForEach(items: app.episodes, key: id) |episode| {
                            if episode.current {
                                Button(id: current_episode, label: episode.code, width: fill, variant: episode_row_on) {
                                    episode_line(episode: episode)
                                } => app.play_episode(episode.id)
                            } else {
                                if episode.local {
                                    Button(id: pick_episode, label: episode.code, width: fill, variant: episode_row) {
                                        episode_line(episode: episode)
                                    } => app.play_episode(episode.id)
                                } else {
                                    Stack(opacity: 0.45) {
                                        episode_line(episode: episode)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 选集的一行：缩略图、集数、标题、状态。
#[component]
pub fn episode_line(episode: PlayEpisode) -> impl View {
    ui! {
        Row(gap: 14, padding_x: 6, padding_y: 6, align: center) {
            Stack(width: 112, height: 63, variant: thumb) {
                if episode.has_thumb {
                    if episode.local {
                        Image(source: episode.thumb, width: 112, height: 63, fit: cover, corner: 6)
                    } else {
                        Image(source: episode.thumb, width: 112, height: 63, fit: cover, corner: 6, saturation: 0)
                    }
                }
                if episode.bar > 0.0 {
                    Column {
                        Spacer
                        Progress(value: episode.bar)
                    }
                }
            }
            Column(gap: 1) {
                Text(content: episode.code, font_size: 13, bold: true)
                if episode.title != "" {
                    Text(content: episode.title, font_size: 13, lines: 1)
                }
                if episode.current {
                    Text(content: episode.note, font_size: 12, color: accent)
                } else {
                    Text(content: episode.note, font_size: 12, emphasis: secondary)
                }
            }
            Spacer
        }
    }
}

/// 信息页：编码信息，打开这一页时读一次。
#[component]
pub fn panel_info(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 10, padding_y: 10) {
            Watch(value: app.opened) => app.load_info()
            ForEach(items: app.info, key: id) |row| {
                Row(gap: 0, align: top) {
                    Column(width: 72) {
                        Text(content: row.key, font_size: 13, emphasis: secondary)
                    }
                    Text(content: row.value, font_size: 13, bold: true, selectable: true)
                }
            }
        }
    }
}

/// 弹幕列表：现在附近的弹幕，或搜到的；每条可以屏蔽。
#[component]
pub fn panel_danmaku_list(
    app: nkuplay::Client,
    players: Players,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        Column(gap: 6) {
            Watch(value: app.opened) => app.list_danmaku(players.video.position_ms, draft.dm_query)
            TextInput(
                id: search_danmaku,
                label: tr! { "en" => "Search danmaku", "zh-Hans" => "搜索弹幕" },
                label_hidden: true,
                value: draft.dm_query
            ) => app.list_danmaku(players.video.position_ms, draft.dm_query)
            Column(height: 420) {
                ScrollView {
                    Column(gap: 0) {
                        ForEach(items: app.danmaku_lines, key: id) |line| {
                            Row(height: 32, gap: 10, align: center) {
                                Column(width: 40) {
                                    Text(content: line.time, font_size: 12, emphasis: secondary)
                                }
                                if line.now {
                                    Text(content: line.text, font_size: 13, bold: true, lines: 1)
                                } else {
                                    Text(content: line.text, font_size: 13, lines: 1)
                                }
                                Spacer
                                IconButton(id: block_line, icon: "eye.slash", label: tr! { "en" => "Block", "zh-Hans" => "屏蔽" }, font_size: 12, prominence: plain) => app.block_line(line.id)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 来源：弹弹play 或本地文件；换一集（手动匹配）、重新获取、加载弹幕文件。
#[component]
pub fn panel_danmaku_source(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 2) {
            Row(height: 56, gap: 10, align: center) {
                if !app.danmaku_from_file {
                    Label(title: tr! { "en" => "In use", "zh-Hans" => "正在用" }, icon: "checkmark", display: icon)
                }
                Column {
                    Text(content: tr! { "en" => "dandanplay", "zh-Hans" => "弹弹play" })
                    if app.danmaku_matched {
                        Text(content: tr! { "en" => "{a} · {e}", "zh-Hans" => "{a} · {e}"; a: app.danmaku_anime, e: app.danmaku_episode }, font_size: 12, emphasis: secondary, lines: 1)
                    } else {
                        Text(content: app.danmaku, font_size: 12, emphasis: secondary)
                    }
                }
                Spacer
                if !app.danmaku_from_file {
                    Text(content: tr! { "en" => "{n} comments", "zh-Hans" => "{n} 条"; n: app.danmaku_count }, font_size: 12, emphasis: secondary)
                }
            }
            Row(height: 56, gap: 10, align: center) {
                if app.danmaku_from_file {
                    Label(title: tr! { "en" => "In use", "zh-Hans" => "正在用" }, icon: "checkmark", display: icon)
                }
                Column {
                    Text(content: tr! { "en" => "Local file", "zh-Hans" => "本地文件" })
                    if app.danmaku_from_file {
                        Text(content: app.danmaku_source, font_size: 12, emphasis: secondary, lines: 1)
                    }
                }
                Spacer
                if app.danmaku_from_file {
                    Text(content: tr! { "en" => "{n} comments", "zh-Hans" => "{n} 条"; n: app.danmaku_count }, font_size: 12, emphasis: secondary)
                }
            }
            Divider
            Row(height: 44, align: center) {
                Text(content: tr! { "en" => "Wrong episode", "zh-Hans" => "识别结果不对" })
                Spacer
                Button(id: rematch, label: tr! { "en" => "Choose", "zh-Hans" => "换一集" }) => app.open_match()
            }
            Row(height: 44, align: center) {
                Text(content: tr! { "en" => "Fetch again", "zh-Hans" => "重新获取弹幕" })
                Spacer
                Button(id: refetch, label: tr! { "en" => "Fetch", "zh-Hans" => "获取" }, icon: "arrow.clockwise") => app.refetch_danmaku()
            }
            Row(height: 44, align: center) {
                Text(content: tr! { "en" => "Load a danmaku file", "zh-Hans" => "加载弹幕文件" })
                Spacer
                Button(id: load_dm_file, label: tr! { "en" => "Choose", "zh-Hans" => "选择" }, icon: "folder", pick: draft.track_file, files: ".xml .json") => app.load_danmaku_file(draft.track_file)
            }
        }
    }
}

/// 手动匹配：左边作品，右边剧集；勾上时文件夹里的其他集也按这部番。
#[component]
pub fn match_dialog(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Modal(width: 760, height: 560) {
            Column(gap: 0) {
                Row(gap: 10, padding_x: 20, height: 56, align: center) {
                    Text(content: tr! { "en" => "Which episode is this?", "zh-Hans" => "这是哪一集" }, font_size: 16, bold: true)
                    Text(content: app.match_file, font_size: 13, emphasis: secondary, lines: 1)
                    Spacer
                    IconButton(id: close_match, icon: "xmark", label: tr! { "en" => "Close", "zh-Hans" => "关闭" }, font_size: 12, prominence: plain) => app.close_match()
                }
                Row(padding_x: 20, padding_y: 6) {
                    TextInput(
                        id: match_query,
                        label: tr! { "en" => "Search shows", "zh-Hans" => "搜索作品" },
                        label_hidden: true,
                        value: draft.match_query
                    ) => app.search_match(draft.match_query)
                }
                Divider
                Row(gap: 0, align: top) {
                    Column(width: 340, height: 380) {
                        ScrollView {
                            Column(gap: 2, padding_x: 12, padding_y: 8) {
                                if app.match_busy {
                                    Progress()
                                }
                                ForEach(items: app.match_animes, key: id) |anime| {
                                    Button(id: choose_anime, label: anime.title, width: fill, variant: episode_row) {
                                        Row(gap: 10, padding_x: 6, padding_y: 6, align: center) {
                                            if anime.chosen {
                                                Label(title: tr! { "en" => "Chosen", "zh-Hans" => "选中" }, icon: "checkmark", display: icon)
                                            }
                                            Column {
                                                Text(content: anime.title, lines: 1)
                                                Text(content: anime.note, font_size: 12, emphasis: secondary)
                                            }
                                            Spacer
                                        }
                                    } => app.choose_anime(anime.id)
                                }
                            }
                        }
                    }
                    Column(height: 380) {
                        ScrollView {
                            Column(gap: 2, padding_x: 12, padding_y: 8) {
                                ForEach(items: app.match_episodes, key: id) |episode| {
                                    Button(id: choose_episode, label: episode.title, width: fill, variant: episode_row) {
                                        Row(gap: 10, padding_x: 6, padding_y: 6, align: center) {
                                            if episode.chosen {
                                                Label(title: tr! { "en" => "Chosen", "zh-Hans" => "选中" }, icon: "checkmark", display: icon)
                                            }
                                            Text(content: episode.title, lines: 1)
                                            Spacer
                                        }
                                    } => app.choose_episode(episode.id)
                                }
                            }
                        }
                    }
                }
                Divider
                Row(gap: 10, padding_x: 20, height: 60, align: center) {
                    Switch(id: match_folder, label: tr! { "en" => "Use this show for the other episodes in this folder", "zh-Hans" => "这个文件夹里的其他集也按这部番" }, checked: app.match_folder) => app.set_match_folder(app.match_folder)
                    Spacer
                    Button(id: no_danmaku, label: tr! { "en" => "No Danmaku", "zh-Hans" => "不加载弹幕" }) => app.skip_danmaku()
                    Button(id: confirm_match, label: tr! { "en" => "OK", "zh-Hans" => "确定" }, prominence: prominent, enabled: app.match_ready) => app.confirm_match()
                }
            }
        } => app.close_match()
    }
}

/// 设置页：盖满窗口。标题栏那一行是返回和“设置”；左边胶囊分类，右边一组组的行。
/// 返回按钮写在这一层（不放进子组件），Bern 才收起标题栏的图标和名字。
#[component]
pub fn settings_page(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Stack(under_bars: true, variant: panel_ground) {
            // 返回和“设置”在标题栏那一行：盖满窗口的页面自己的 Toolbar 由 Bern 放进标题栏。
            Toolbar {
                IconButton(
                    id: close_settings,
                    icon: "arrow.left",
                    label: tr! { "en" => "Back", "zh-Hans" => "返回" },
                    height: 36,
                    variant: round
                ) => draft.panel.set(Panel::None)
                Text(content: tr! { "en" => "Settings", "zh-Hans" => "设置" }, font_size: 15, bold: true)
                Spacer
            }
            Column(gap: 0) {
                // 标题栏那一条（48 高）的下面才是页面。
                Row(height: 48) {}
                Row(align: top) {
                    settings_nav()
                    ScrollView {
                        Row(padding_x: 20, padding_y: 16, align: top) {
                        // 快捷键三列，比别的页宽。
                        if draft.settings_tab == SettingsTab::Keys {
                            Column(width: 1113) {
                                settings_keys(app: app)
                            }
                        } else {
                            Column(width: 860) {
                                match draft.settings_tab {
                                    SettingsTab::Look => { settings_look(app: app) }
                                    SettingsTab::Play => { settings_play(app: app) }
                                    SettingsTab::Picture => { settings_picture(app: app) }
                                    SettingsTab::Danmaku => { settings_danmaku(app: app) }
                                    SettingsTab::About => { settings_about(app: app) }
                                    _ => {}
                                }
                            }
                        }
                        Spacer
                        }
                    }
                }
            }
            Shortcut(key: "escape") => draft.panel.set(Panel::None)
        }
    }
}

/// 设置页左边的分类：选中的是白色胶囊。
#[component]
pub fn settings_nav(#[session] draft: MainDraft) -> impl View {
    ui! {
        Column(width: 244, gap: 4, padding_x: 28, padding_y: 20) {
            if draft.settings_tab == SettingsTab::Look {
                Button(id: look_on, label: tr! { "en" => "Appearance", "zh-Hans" => "外观" }, width: fill, height: 36, variant: settings_tab_on) {
                    tab_label(tab: SettingsTab::Look, on: true)
                } => draft.settings_tab.set(SettingsTab::Look)
            } else {
                Button(id: look, label: tr! { "en" => "Appearance", "zh-Hans" => "外观" }, width: fill, height: 36, variant: settings_tab) {
                    tab_label(tab: SettingsTab::Look, on: false)
                } => draft.settings_tab.set(SettingsTab::Look)
            }
            if draft.settings_tab == SettingsTab::Play {
                Button(id: play_on, label: tr! { "en" => "Playback", "zh-Hans" => "播放" }, width: fill, height: 36, variant: settings_tab_on) {
                    tab_label(tab: SettingsTab::Play, on: true)
                } => draft.settings_tab.set(SettingsTab::Play)
            } else {
                Button(id: play, label: tr! { "en" => "Playback", "zh-Hans" => "播放" }, width: fill, height: 36, variant: settings_tab) {
                    tab_label(tab: SettingsTab::Play, on: false)
                } => draft.settings_tab.set(SettingsTab::Play)
            }
            if draft.settings_tab == SettingsTab::Picture {
                Button(id: picture_on, label: tr! { "en" => "Picture", "zh-Hans" => "画面" }, width: fill, height: 36, variant: settings_tab_on) {
                    tab_label(tab: SettingsTab::Picture, on: true)
                } => draft.settings_tab.set(SettingsTab::Picture)
            } else {
                Button(id: picture, label: tr! { "en" => "Picture", "zh-Hans" => "画面" }, width: fill, height: 36, variant: settings_tab) {
                    tab_label(tab: SettingsTab::Picture, on: false)
                } => draft.settings_tab.set(SettingsTab::Picture)
            }
            if draft.settings_tab == SettingsTab::Danmaku {
                Button(id: danmaku_on, label: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, width: fill, height: 36, variant: settings_tab_on) {
                    tab_label(tab: SettingsTab::Danmaku, on: true)
                } => draft.settings_tab.set(SettingsTab::Danmaku)
            } else {
                Button(id: danmaku, label: tr! { "en" => "Danmaku", "zh-Hans" => "弹幕" }, width: fill, height: 36, variant: settings_tab) {
                    tab_label(tab: SettingsTab::Danmaku, on: false)
                } => draft.settings_tab.set(SettingsTab::Danmaku)
            }
            if draft.settings_tab == SettingsTab::Keys {
                Button(id: keys_on, label: tr! { "en" => "Shortcuts", "zh-Hans" => "快捷键" }, width: fill, height: 36, variant: settings_tab_on) {
                    tab_label(tab: SettingsTab::Keys, on: true)
                } => draft.settings_tab.set(SettingsTab::Keys)
            } else {
                Button(id: keys, label: tr! { "en" => "Shortcuts", "zh-Hans" => "快捷键" }, width: fill, height: 36, variant: settings_tab) {
                    tab_label(tab: SettingsTab::Keys, on: false)
                } => draft.settings_tab.set(SettingsTab::Keys)
            }
            if draft.settings_tab == SettingsTab::About {
                Button(id: about_on, label: tr! { "en" => "About", "zh-Hans" => "关于" }, width: fill, height: 36, variant: settings_tab_on) {
                    tab_label(tab: SettingsTab::About, on: true)
                } => draft.settings_tab.set(SettingsTab::About)
            } else {
                Button(id: about, label: tr! { "en" => "About", "zh-Hans" => "关于" }, width: fill, height: 36, variant: settings_tab) {
                    tab_label(tab: SettingsTab::About, on: false)
                } => draft.settings_tab.set(SettingsTab::About)
            }
        }
    }
}

/// 分类按钮里的图标和名字。
#[component]
pub fn tab_label(tab: SettingsTab, on: bool) -> impl View {
    ui! {
        Row(gap: 12, padding_x: 16, align: center) {
            match tab {
                SettingsTab::Look => { Label(title: tab, icon: "paintpalette", display: icon) }
                SettingsTab::Play => { Label(title: tab, icon: "play", display: icon) }
                SettingsTab::Picture => { Label(title: tab, icon: "tv", display: icon) }
                SettingsTab::Danmaku => { Label(title: tab, icon: "text.bubble", display: icon) }
                SettingsTab::Keys => { Label(title: tab, icon: "keyboard", display: icon) }
                SettingsTab::About => { Label(title: tab, icon: "info.circle", display: icon) }
            }
            Text(content: tab, bold: on)
            Spacer
        }
    }
}

/// 一页的大标题；能恢复默认的右边有按钮。
#[component]
pub fn settings_heading(title: SettingsTab) -> impl View {
    ui! {
        Row(height: 40, align: center) {
            Text(content: title, font_size: 28, bold: true)
            Spacer
        }
    }
}

#[component]
pub fn settings_heading_reset(
    app: nkuplay::Client,
    title: SettingsTab,
    section: Section,
) -> impl View {
    ui! {
        Row(height: 40, align: center) {
            Text(content: title, font_size: 28, bold: true)
            Spacer
            Button(
                id: reset_section,
                label: tr! { "en" => "Restore Defaults", "zh-Hans" => "恢复默认" },
                icon: "arrow.uturn.backward",
                height: 28
            ) => app.reset_section(section)
        }
    }
}

/// 外观：主题、语言、启动时打开；顶栏有哪些页、什么顺序。
#[component]
pub fn settings_look(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 8) {
            settings_heading(title: SettingsTab::Look)
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Interface", "zh-Hans" => "界面" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Theme", "zh-Hans" => "主题" })
                        Spacer
                        Picker(id: appearance, label: tr! { "en" => "Theme", "zh-Hans" => "主题" }, label_hidden: true, selected: app.appearance, style: segmented, shape: capsule) => app.set_appearance(app.appearance)
                    }
                    Divider
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Language", "zh-Hans" => "语言" })
                        Spacer
                        Column(width: 160) {
                            LanguagePicker(id: language, label: tr! { "en" => "Language", "zh-Hans" => "语言" }, label_hidden: true)
                        }
                    }
                    Divider
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Open at start", "zh-Hans" => "启动时打开" })
                        Spacer
                        Column(width: 160) {
                            Picker(id: start_page, label: tr! { "en" => "Open at start", "zh-Hans" => "启动时打开" }, label_hidden: true, selected: app.start_page, style: menu) => app.set_start_page(app.start_page)
                        }
                    }
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Top bar", "zh-Hans" => "顶栏" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    // 顶栏的样子：按现在的顺序，开着的页面。
                    Stack(variant: panel_ground, height: 52) {
                    Row(variant: bar_preview, align: center) {
                        Text(content: tr! { "en" => "Nkuplay", "zh-Hans" => "Nkuplay" }, font_size: 16, bold: true)
                        Row(gap: 4, padding_x: 10) {
                            ForEach(items: app.nav_shown, key: id) |item| {
                                if item.id == 0 {
                                    Button(id: preview_on, label: item.page, variant: nav_on) => draft.page.set(item.page)
                                } else {
                                    Button(id: preview_page, label: item.page, variant: nav, prominence: plain) => draft.page.set(item.page)
                                }
                            }
                        }
                        Spacer
                    }
                    }
                    Reorderable(
                        id: nav_order,
                        label: tr! { "en" => "Top bar pages", "zh-Hans" => "顶栏的页面" },
                        items: app.nav,
                        key: id,
                        move_from: draft.move_from,
                        move_to: draft.move_to
                    ) |item| {
                        Row(variant: setting_row, gap: 14, align: center) {
                            Text(content: item.page)
                            Spacer
                            Switch(id: page_on, label: item.page, label_hidden: true, checked: item.on) => app.show_page(item.page, item.on)
                        }
                    } => app.move_page(draft.move_from, draft.move_to)
                }
            }
        }
    }
}

/// 一行：左边名字（开关的标签），右边一个开关。
#[component]
pub fn toggle_row(app: nkuplay::Client, toggle: Toggle, on: bool) -> impl View {
    ui! {
        Row(variant: setting_row, align: center) {
            Text(content: toggle)
            Spacer
            Switch(id: setting_switch, label: toggle, label_hidden: true, checked: on) => app.set_toggle(toggle, on)
        }
    }
}

/// 一行：左边名字（数值的标签），右边滑块和它的数值。滑块的范围按是哪个数值写在这里。
#[component]
pub fn slider_row(app: nkuplay::Client, amount: Amount, value: f64) -> impl View {
    ui! {
        Row(variant: setting_row, gap: 14, align: center) {
            Text(content: amount)
            Spacer
            Column(width: 220) {
                match amount {
                    Amount::Countdown => { Slider(id: countdown, label: amount, label_hidden: true, current: value, min: 3, max: 30, step: 1) => app.set_amount(amount, value) }
                    Amount::SkipSeconds => { Slider(id: skip_seconds, label: amount, label_hidden: true, current: value, min: 10, max: 300, step: 5) => app.set_amount(amount, value) }
                    Amount::Cache => { Slider(id: cache, label: amount, label_hidden: true, current: value, min: 32, max: 1024, step: 2) => app.set_amount(amount, value) }
                    Amount::Opacity => { Slider(id: opacity, label: amount, label_hidden: true, current: value, min: 10, max: 100, step: 1) => app.set_amount(amount, value) }
                    Amount::Size => { Slider(id: size, label: amount, label_hidden: true, current: value, min: 18, max: 64, step: 1) => app.set_amount(amount, value) }
                    Amount::Outline => { Slider(id: outline, label: amount, label_hidden: true, current: value, min: 0, max: 4, step: 0.5) => app.set_amount(amount, value) }
                    Amount::Area => { Slider(id: area, label: amount, label_hidden: true, current: value, min: 25, max: 100, step: 5) => app.set_amount(amount, value) }
                    Amount::ScrollTime => { Slider(id: scroll_time, label: amount, label_hidden: true, current: value, min: 4, max: 16, step: 0.5) => app.set_amount(amount, value) }
                    Amount::Gap => { Slider(id: gap, label: amount, label_hidden: true, current: value, min: 0, max: 50, step: 1) => app.set_amount(amount, value) }
                    _ => {}
                }
            }
            Column(width: 58) {
                Row {
                    Spacer
                    match amount {
                        Amount::Countdown => { Text(content: tr! { "en" => "{n} s", "zh-Hans" => "{n} 秒"; n: value }, emphasis: secondary) }
                        Amount::SkipSeconds => { Text(content: tr! { "en" => "{n} s", "zh-Hans" => "{n} 秒"; n: value }, emphasis: secondary) }
                        Amount::ScrollTime => { Text(content: tr! { "en" => "{n} s", "zh-Hans" => "{n} 秒"; n: value }, emphasis: secondary) }
                        Amount::Cache => { Text(content: tr! { "en" => "{n} MB", "zh-Hans" => "{n} MB"; n: value }, emphasis: secondary) }
                        Amount::Opacity => { Text(content: tr! { "en" => "{n}%", "zh-Hans" => "{n}%"; n: value }, emphasis: secondary) }
                        Amount::Area => { Text(content: tr! { "en" => "{n}%", "zh-Hans" => "{n}%"; n: value }, emphasis: secondary) }
                        Amount::Gap => { Text(content: tr! { "en" => "{n}%", "zh-Hans" => "{n}%"; n: value }, emphasis: secondary) }
                        _ => { Text(content: tr! { "en" => "{n}", "zh-Hans" => "{n}"; n: value }, emphasis: secondary) }
                    }
                }
            }
        }
    }
}

/// 播放：开始、播完以后、片头片尾、操作、控制栏、解码。
#[component]
pub fn settings_play(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 8) {
            settings_heading_reset(app: app, title: SettingsTab::Play, section: Section::Play)
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Start", "zh-Hans" => "开始" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    toggle_row(app: app, toggle: Toggle::Resume, on: app.resume)
                    Divider
                    toggle_row(app: app, toggle: Toggle::AutoFullscreen, on: app.auto_fullscreen)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "After an episode", "zh-Hans" => "播完以后" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "When an episode ends", "zh-Hans" => "播完一集" })
                        Spacer
                        Picker(id: after_end, label: tr! { "en" => "When an episode ends", "zh-Hans" => "播完一集" }, label_hidden: true, selected: app.after_end, style: segmented, shape: capsule) => app.set_after_end(app.after_end)
                    }
                    Divider
                    slider_row(app: app, amount: Amount::Countdown, value: app.countdown)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Openings and endings", "zh-Hans" => "片头片尾" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Skip openings and endings", "zh-Hans" => "跳过片头片尾" })
                        Spacer
                        Picker(id: skip_mode, label: tr! { "en" => "Skip openings and endings", "zh-Hans" => "跳过片头片尾" }, label_hidden: true, selected: app.skip_mode, style: segmented, shape: capsule) => app.set_skip_mode(app.skip_mode)
                    }
                    Divider
                    slider_row(app: app, amount: Amount::SkipSeconds, value: app.skip_seconds)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Controls", "zh-Hans" => "操作" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Seek step", "zh-Hans" => "快进快退" })
                        Spacer
                        Column(width: 110) {
                            Picker(id: seek_step, label: tr! { "en" => "Seek step", "zh-Hans" => "快进快退" }, label_hidden: true, selected: app.seek_choice, style: menu) => app.set_seek_step(app.seek_choice)
                        }
                    }
                    Divider
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Hold speed", "zh-Hans" => "长按倍速" })
                        Spacer
                        Column(width: 110) {
                            Picker(id: hold_speed, label: tr! { "en" => "Hold speed", "zh-Hans" => "长按倍速" }, label_hidden: true, selected: app.hold_choice, style: menu) => app.set_hold_rate(app.hold_choice)
                        }
                    }
                    Divider
                    toggle_row(app: app, toggle: Toggle::KeepPitch, on: app.keep_pitch)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Control bar", "zh-Hans" => "控制栏" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Stays for", "zh-Hans" => "停留" })
                        Spacer
                        Column(width: 110) {
                            Picker(id: bar_stay, label: tr! { "en" => "Stays for", "zh-Hans" => "停留" }, label_hidden: true, selected: app.stay_choice, style: menu) => app.set_bar_stay(app.stay_choice)
                        }
                    }
                    Divider
                    toggle_row(app: app, toggle: Toggle::Chapters, on: app.chapter_marks)
                    Divider
                    toggle_row(app: app, toggle: Toggle::Heat, on: app.heat_curve)
                    Divider
                    toggle_row(app: app, toggle: Toggle::Preview, on: app.hover_preview)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Decoding", "zh-Hans" => "解码" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    toggle_row(app: app, toggle: Toggle::Hwdec, on: app.hwdec)
                    Divider
                    slider_row(app: app, amount: Amount::Cache, value: app.cache_mb)
                }
            }
        }
    }
}

/// 画面：Anime4K、画面比例、填充方式。
#[component]
pub fn settings_picture(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 8) {
            settings_heading_reset(app: app, title: SettingsTab::Picture, section: Section::Picture)
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Enhancement", "zh-Hans" => "增强" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Anime4K upscaling", "zh-Hans" => "Anime4K 超分" })
                        Spacer
                        Picker(id: upscale, label: tr! { "en" => "Anime4K upscaling", "zh-Hans" => "Anime4K 超分" }, label_hidden: true, selected: app.upscale, style: segmented, shape: capsule) => app.set_upscale(app.upscale)
                    }
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Display", "zh-Hans" => "显示" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Aspect ratio", "zh-Hans" => "画面比例" })
                        Spacer
                        Column(width: 160) {
                            Picker(id: aspect, label: tr! { "en" => "Aspect ratio", "zh-Hans" => "画面比例" }, label_hidden: true, selected: app.aspect, style: menu) => app.set_aspect(app.aspect)
                        }
                    }
                    Divider
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Fit", "zh-Hans" => "填充方式" })
                        Spacer
                        Picker(id: fill, label: tr! { "en" => "Fit", "zh-Hans" => "填充方式" }, label_hidden: true, selected: app.fill, style: segmented, shape: capsule) => app.set_fill(app.fill)
                    }
                }
            }
        }
    }
}

/// 弹幕：样式、排布、内容、屏蔽词。
#[component]
pub fn settings_danmaku(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 8) {
            settings_heading_reset(app: app, title: SettingsTab::Danmaku, section: Section::Danmaku)
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Style", "zh-Hans" => "样式" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    slider_row(app: app, amount: Amount::Opacity, value: app.d_opacity)
                    Divider
                    slider_row(app: app, amount: Amount::Size, value: app.d_size)
                    Divider
                    slider_row(app: app, amount: Amount::Outline, value: app.d_outline)
                    Divider
                    Row(variant: setting_row, align: center) {
                        Text(content: tr! { "en" => "Font", "zh-Hans" => "字体" })
                        Spacer
                        Column(width: 160) {
                            Picker(id: d_font, label: tr! { "en" => "Font", "zh-Hans" => "字体" }, label_hidden: true, selected: app.d_font, style: menu) => app.set_danmaku_font(app.d_font)
                        }
                    }
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Layout", "zh-Hans" => "排布" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    slider_row(app: app, amount: Amount::Area, value: app.d_area)
                    Divider
                    slider_row(app: app, amount: Amount::ScrollTime, value: app.d_seconds)
                    Divider
                    slider_row(app: app, amount: Amount::Gap, value: app.d_gap)
                    Divider
                    toggle_row(app: app, toggle: Toggle::Overlap, on: app.d_overlap)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Content", "zh-Hans" => "内容" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    Row(variant: setting_row, gap: 8, align: center) {
                        Text(content: tr! { "en" => "Show", "zh-Hans" => "显示" })
                        Spacer
                        danmaku_kinds(app: app)
                    }
                    Divider
                    toggle_row(app: app, toggle: Toggle::Merge, on: app.d_merge)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Blocked words", "zh-Hans" => "屏蔽词" }, bold: true) }
            Stack(variant: card) {
                Column(padding_x: 10, padding_y: 10) {
                Flow(gap: 8) {
                    ForEach(items: app.blocked, key: id) |word| {
                        Button(id: unblock, label: tr! { "en" => "{w} ×", "zh-Hans" => "{w} ×"; w: word.text }, variant: chip) => app.remove_blocked(word.id)
                    }
                    Column(width: 200) {
                        TextInput(
                            id: add_word,
                            label: tr! { "en" => "Add a blocked word", "zh-Hans" => "添加屏蔽词" },
                            label_hidden: true,
                            value: draft.word
                        ) => app.add_blocked(draft.word)
                    }
                }
                }
            }
        }
    }
}

/// 滚动、顶部、底部、彩色四个开关，开着的是深色胶囊。
#[component]
pub fn danmaku_kinds(app: nkuplay::Client) -> impl View {
    ui! {
        Row(gap: 8) {
            kind_chip(app: app, toggle: Toggle::Scroll, on: app.d_scroll)
            kind_chip(app: app, toggle: Toggle::Top, on: app.d_top)
            kind_chip(app: app, toggle: Toggle::Bottom, on: app.d_bottom)
            kind_chip(app: app, toggle: Toggle::Colored, on: app.d_colored)
        }
    }
}

#[component]
pub fn kind_chip(app: nkuplay::Client, toggle: Toggle, on: bool) -> impl View {
    ui! {
        if on {
            Button(id: kind_on, label: toggle, variant: chip_on) => app.set_toggle(toggle, false)
        } else {
            Button(id: kind_off, label: toggle, variant: chip) => app.set_toggle(toggle, true)
        }
    }
}

/// 一行快捷键：左边操作，右边录键的按钮。
#[component]
pub fn key_row(app: nkuplay::Client, action: KeyAction, keys: String) -> impl View {
    ui! {
        Row(variant: key_row, align: center) {
            Text(content: action)
            Spacer
            ShortcutInput(
                id: key_input,
                label: action,
                label_hidden: true,
                current: keys,
                prompt: tr! { "en" => "Press keys", "zh-Hans" => "按下按键" },
                empty: tr! { "en" => "None", "zh-Hans" => "无" }
            ) => app.set_key(action, keys)
        }
    }
}

/// 长按快进那一行：一个键帽写“按住 →”，跟着快进键走，不能单独改。
#[component]
pub fn hold_row(app: nkuplay::Client, action: KeyAction) -> impl View {
    ui! {
        Row(variant: key_row, align: center) {
            Text(content: action)
            Spacer
            Stack(variant: key_cap) {
                Row(padding_x: 7, padding_y: 2) {
                    Text(content: tr! { "en" => "Hold {k}", "zh-Hans" => "按住 {k}"; k: app.keys.hold_label }, font_size: 12)
                }
            }
        }
    }
}

/// 跳过那一行：名字里写跳过多少秒。
#[component]
pub fn skip_key_row(app: nkuplay::Client, keys: String) -> impl View {
    ui! {
        Row(variant: key_row, align: center) {
            Text(content: tr! { "en" => "Skip {n} s", "zh-Hans" => "跳过 {n} 秒"; n: app.skip_seconds })
            Spacer
            ShortcutInput(
                id: key_input,
                label: tr! { "en" => "Skip", "zh-Hans" => "跳过" },
                label_hidden: true,
                current: keys,
                prompt: tr! { "en" => "Press keys", "zh-Hans" => "按下按键" },
                empty: tr! { "en" => "None", "zh-Hans" => "无" }
            ) => app.set_key(KeyAction::Skip, keys)
        }
    }
}

/// 快捷键：三列。
#[component]
pub fn settings_keys(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 8) {
            settings_heading_reset(app: app, title: SettingsTab::Keys, section: Section::Keys)
            Row(gap: 24, align: top) {
                Column(width: 355, gap: 6) {
                    Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Playback", "zh-Hans" => "播放" }, bold: true) }
                    Stack(variant: card) {
                        Column(gap: 0) {
                            key_row(app: app, action: KeyAction::Play, keys: app.keys.play)
                            Divider
                            key_row(app: app, action: KeyAction::Rewind, keys: app.keys.rewind)
                            Divider
                            key_row(app: app, action: KeyAction::Forward, keys: app.keys.forward)
                            Divider
                            key_row(app: app, action: KeyAction::Previous, keys: app.keys.previous)
                            Divider
                            key_row(app: app, action: KeyAction::Next, keys: app.keys.next)
                            Divider
                            skip_key_row(app: app, keys: app.keys.skip)
                        }
                    }
                    Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Speed", "zh-Hans" => "倍速" }, bold: true) }
                    Stack(variant: card) {
                        Column(gap: 0) {
                            key_row(app: app, action: KeyAction::Faster, keys: app.keys.faster)
                            Divider
                            key_row(app: app, action: KeyAction::Slower, keys: app.keys.slower)
                            Divider
                            key_row(app: app, action: KeyAction::NormalSpeed, keys: app.keys.normal_speed)
                            Divider
                            // 长按快进就是按住快进键，跟着快进键走，不单独改。
                            hold_row(app: app, action: KeyAction::Hold)
                        }
                    }
                }
                Column(width: 355, gap: 6) {
                    Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Sound", "zh-Hans" => "声音" }, bold: true) }
                    Stack(variant: card) {
                        Column(gap: 0) {
                            key_row(app: app, action: KeyAction::Louder, keys: app.keys.louder)
                            Divider
                            key_row(app: app, action: KeyAction::Quieter, keys: app.keys.quieter)
                            Divider
                            key_row(app: app, action: KeyAction::Mute, keys: app.keys.mute)
                        }
                    }
                    Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Danmaku and tracks", "zh-Hans" => "弹幕和轨道" }, bold: true) }
                    Stack(variant: card) {
                        Column(gap: 0) {
                            key_row(app: app, action: KeyAction::Danmaku, keys: app.keys.danmaku)
                            Divider
                            key_row(app: app, action: KeyAction::DanmakuPanel, keys: app.keys.danmaku_panel)
                            Divider
                            key_row(app: app, action: KeyAction::Subtitle, keys: app.keys.subtitle)
                            Divider
                            key_row(app: app, action: KeyAction::Audio, keys: app.keys.audio)
                        }
                    }
                }
                Column(width: 355, gap: 6) {
                    Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Picture and window", "zh-Hans" => "画面和窗口" }, bold: true) }
                    Stack(variant: card) {
                        Column(gap: 0) {
                            key_row(app: app, action: KeyAction::Fullscreen, keys: app.keys.fullscreen)
                            Divider
                            key_row(app: app, action: KeyAction::ExitFullscreen, keys: app.keys.exit_fullscreen)
                            Divider
                            key_row(app: app, action: KeyAction::Episodes, keys: app.keys.episodes)
                            Divider
                            key_row(app: app, action: KeyAction::Info, keys: app.keys.info)
                        }
                    }
                }
            }
        }
    }
}

/// 一行链接：整行点开浏览器。
#[component]
pub fn link_row(app: nkuplay::Client, link: Link) -> impl View {
    ui! {
        Button(id: open_link, label: link, width: fill, variant: link_row) {
            Row(variant: setting_row, gap: 10, align: center) {
                Text(content: link)
                Spacer
                match link {
                    Link::Source => { Text(content: tr! { "en" => "github.com/SubaRudesu7/Nkuplay", "zh-Hans" => "github.com/SubaRudesu7/Nkuplay" }, emphasis: secondary) }
                    Link::BangumiData => { Text(content: tr! { "en" => "CC BY 4.0", "zh-Hans" => "CC BY 4.0" }, emphasis: secondary) }
                    _ => {}
                }
                Label(title: link, icon: "arrow.up.right.square", display: icon)
            }
        } => app.open_link(link)
    }
}

/// 关于：源代码和数据来源。
#[component]
pub fn settings_about(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 8) {
            settings_heading(title: SettingsTab::About)
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Source code", "zh-Hans" => "源代码" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    link_row(app: app, link: Link::Source)
                }
            }
            Row(height: 34, align: bottom) { Text(content: tr! { "en" => "Data sources", "zh-Hans" => "数据来源" }, bold: true) }
            Stack(variant: card) {
                Column(gap: 0) {
                    link_row(app: app, link: Link::Dandanplay)
                    Divider
                    link_row(app: app, link: Link::Tmdb)
                    Divider
                    link_row(app: app, link: Link::BangumiData)
                }
            }
        }
    }
}
