//! Nkuplay 的界面：一个窗口，顶上是导航（首页、媒体库、新番时间表、观看历史）和右侧的账号、设置；
//! 点开一部番换成它的详情页，打开视频后换成视频页。
//! 每段文字都在显示它的地方写出所有语言的版本，每个按钮都执行协议里的一条命令。

use bern::prelude::*;
use nkuplay_protocol::nkuplay::{self, DetailGroup, ShinStatus};

/// 顶栏导航能去的页面。
#[bern::value]
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
}

#[component]
pub fn app(app: nkuplay::Client, windows: AppWindows, players: Players) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main) {
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
                // 账号、设置和详情页盖满窗口，那时顶栏不挂。
                if !app.detail_open && draft.panel == Panel::None {
                    Toolbar {
                        // 开头的文字就是窗口的名字，标题栏不再另外显示图标和名字。
                        Text(content: tr! { "en" => "Nkuplay", "zh-Hans" => "Nkuplay" }, font_size: 19, bold: true)
                        if draft.page == Page::Home {
                            Button(id: nav_home_on, label: tr! { "en" => "Home", "zh-Hans" => "首页" }, variant: nav_on) => draft.page.set(Page::Home)
                        } else {
                            Button(id: nav_home, label: tr! { "en" => "Home", "zh-Hans" => "首页" }, variant: nav, prominence: plain) => draft.page.set(Page::Home)
                        }
                        if draft.page == Page::Library {
                            Button(id: nav_library_on, label: tr! { "en" => "Library", "zh-Hans" => "媒体库" }, variant: nav_on) => draft.page.set(Page::Library)
                        } else {
                            Button(id: nav_library, label: tr! { "en" => "Library", "zh-Hans" => "媒体库" }, variant: nav, prominence: plain) => draft.page.set(Page::Library)
                        }
                        if draft.page == Page::Schedule {
                            Button(id: nav_schedule_on, label: tr! { "en" => "Schedule", "zh-Hans" => "新番时间表" }, variant: nav_on) => draft.page.set(Page::Schedule)
                        } else {
                            Button(id: nav_schedule, label: tr! { "en" => "Schedule", "zh-Hans" => "新番时间表" }, variant: nav, prominence: plain) => draft.page.set(Page::Schedule)
                        }
                        if draft.page == Page::History {
                            Button(id: nav_history_on, label: tr! { "en" => "History", "zh-Hans" => "观看历史" }, variant: nav_on) => draft.page.set(Page::History)
                        } else {
                            Button(id: nav_history, label: tr! { "en" => "History", "zh-Hans" => "观看历史" }, variant: nav, prominence: plain) => draft.page.set(Page::History)
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
                    Panel::Settings => { panel_page(panel: Panel::Settings) }
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

/// 媒体库页：番剧海报墙，下面是加入的文件夹。
#[component]
pub fn library_page(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 20, padding_x: 56, padding_y: 28) {
                Row(gap: 8, align: center) {
                    Text(content: tr! { "en" => "Library", "zh-Hans" => "媒体库" }, style: large_title)
                    Spacer
                    Button(
                        id: open,
                        label: tr! { "en" => "Open File", "zh-Hans" => "打开文件" },
                        icon: "folder",
                        pick: draft.file,
                        files: "video"
                    ) => app.open_file(draft.file)
                    Button(
                        id: refresh_library_page,
                        label: tr! { "en" => "Refresh", "zh-Hans" => "刷新" },
                        icon: "arrow.clockwise",
                        enabled: !app.library_scanning
                    ) => app.refresh_library()
                    Button(
                        id: add_folder_page,
                        label: tr! { "en" => "Add Folder", "zh-Hans" => "添加文件夹" },
                        icon: "plus",
                        prominence: prominent,
                        pick: draft.folder,
                        files: "folder"
                    ) => app.add_folder(draft.folder)
                }
                library_wall(app: app)
                Text(content: tr! { "en" => "Folders", "zh-Hans" => "文件夹" }, style: title2)
                folder_list(app: app)
            }
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

/// 海报墙本身：每部番一张海报，下面是作品名和集数，点开进详情页。
/// 弹弹play 没认出的番没有海报，用一块浅色底加播放图标代替。
#[component]
pub fn series_grid(app: nkuplay::Client, loading: bool) -> impl View {
    ui! {
        Grid(min_width: 140, gap: 16, skeleton: loading, skeleton_count: 6) {
            ForEach(items: app.series, key: id) |show| {
                Button(
                    id: open_show,
                    label: tr! { "en" => "Open {title}", "zh-Hans" => "打开{title}"; title: show.title }
                ) {
                    Column(gap: 6) {
                        if show.has_poster {
                            Image(source: show.poster, height: 187, fit: cover, corner: 6)
                        } else {
                            Stack(height: 187) {
                                Canvas(height: 187) {
                                    Shape(kind: rounded, corner: 6, fill: accent, opacity: 0.18)
                                }
                                Label(title: show.title, icon: "play.fill", display: icon)
                            }
                        }
                        Text(content: show.title, lines: 2)
                        Text(
                            content: tr! {
                                "en" => "{count, plural, one {# episode} other {# episodes}}",
                                "zh-Hans" => "{count} 集";
                                count: show.episodes
                            },
                            style: caption,
                            emphasis: secondary
                        )
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
            Shortcut(key: "escape") => app.close_details()
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

/// 媒体库里的文件夹列表；空的时候提示怎么加。
#[component]
pub fn folder_list(app: nkuplay::Client) -> impl View {
    ui! {
        if app.folder_count == 0 {
            Text(
                content: tr! {
                    "en" => "Add a folder of videos to see it here.",
                    "zh-Hans" => "添加放视频的文件夹，它会出现在这里。"
                },
                emphasis: secondary
            )
        } else {
            List(items: app.folders, key: id) |folder| {
                Row(gap: 12, align: center) {
                    Label(title: folder.name, icon: "folder")
                    Spacer
                    // 还在数的时候，视频数显示成骨架条。
                    Text(
                        content: tr! { "en" => "{count} videos", "zh-Hans" => "{count} 个视频"; count: folder.videos },
                        emphasis: secondary,
                        skeleton: folder.scanning
                    )
                    Button(
                        id: remove_folder,
                        label: tr! { "en" => "Remove", "zh-Hans" => "移除" },
                        prominence: plain
                    ) => app.remove_folder(folder.id)
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
        // 不标 idle_area：标了以后，Bern 只在鼠标停在那块区域上时才算静止，
        // 实测在视频页上停多久都不算。不标时，鼠标在窗口里任何地方 1 秒不动就算静止。
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
                winui3: "video_gestures.winui3.cpp"
            ) {
                NativeValue(name: playing, value: players.video.playing)
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
            Shortcut(key: "space", enabled: players.video.playing) => players.video.pause()
            Shortcut(key: "space", enabled: !players.video.playing) => players.video.play()
            Shortcut(key: "up") => app.nudge_volume(0.05)
            Shortcut(key: "down") => app.nudge_volume(-0.05)
            // `ui!` 不能做加减，由引擎从当前位置算出要跳到哪里。`quiet`：不算“有操作”，上下栏和指针不出来。
            Shortcut(key: "left", quiet: true) => app.skip(-5.0, players.video.position_ms, players.video.duration_ms)
            Shortcut(key: "right", quiet: true) => app.skip(5.0, players.video.position_ms, players.video.duration_ms)
            Shortcut(key: "f") => windows.main.toggle_fullscreen()
            Shortcut(key: "d") => app.toggle_danmaku()
            Shortcut(key: "escape", enabled: window.fullscreen) => windows.main.toggle_fullscreen()
            // 观看进度：播放时每 5 秒记一次，暂停、播完时也记（回首页时在 main_window 里记最后一次）。
            Timer(interval: 5, enabled: players.video.playing) => app.record_progress(players.video.position_ms, players.video.duration_ms)
            Watch(value: players.video.playing) => app.record_progress(players.video.position_ms, players.video.duration_ms)
            // 上下栏压在视频上，用深色外观：文字和图标是浅色的。
            Column(appearance: dark) {
                if !window.idle {
                    top_bar(app: app, windows: windows)
                }
                Spacer
                if !window.idle {
                    play_bar(app: app, windows: windows, players: players)
                }
            }
        }
    }
}

/// 顶栏：返回（全屏时只退出全屏）、弹幕状态、打开。底下是一层从上往下渐隐的暗色，没有边框和面板。
#[component]
pub fn top_bar(
    app: nkuplay::Client,
    windows: AppWindows,
    #[session] draft: MainDraft,
) -> impl View {
    ui! {
        Stack(appear: bar_top, disappear: bar_top) {
            Canvas {
                Shape(kind: rect, fill: surface.canvas, fade: down, opacity: 0.7)
            }
            Row(gap: 8, padding_x: 16, padding_y: 12, align: center) {
                // 全屏时“返回”先只退出全屏，再按一次才回首页（用户要求）。
                if window.fullscreen {
                    Button(
                        id: back_from_fullscreen,
                        label: tr! { "en" => "Back", "zh-Hans" => "返回" },
                        icon: "chevron.left",
                        prominence: plain
                    ) => windows.main.toggle_fullscreen()
                } else {
                    Button(
                        id: back,
                        label: tr! { "en" => "Back", "zh-Hans" => "返回" },
                        icon: "chevron.left",
                        prominence: plain
                    ) => app.close()
                }
                Spacer
                danmaku_status(app: app)
                Button(
                    id: open_another,
                    label: tr! { "en" => "Open…", "zh-Hans" => "打开…" },
                    icon: "folder",
                    prominence: plain,
                    pick: draft.file,
                    files: "video"
                ) => app.open_file(draft.file)
            }
        }
    }
}

/// 底栏：进度条，下面是播放/暂停、时间、音量和全屏。底下是一层从下往上渐隐的暗色。
#[component]
pub fn play_bar(app: nkuplay::Client, windows: AppWindows, players: Players) -> impl View {
    ui! {
        Stack(appear: bar_bottom, disappear: bar_bottom) {
            Canvas {
                Shape(kind: rect, fill: surface.canvas, fade: up, opacity: 0.7)
            }
            Column(gap: 4, padding_x: 16, padding_y: 12) {
                // 进度条用原生代码画（seek_bar.winui3.cpp）：鼠标悬停时，时间气泡跟着指针走。
                // 松开时把比例交给引擎，引擎换算成毫秒再让播放器跳。
                NativeView(
                    id: seek_bar,
                    label: tr! { "en" => "Position", "zh-Hans" => "播放进度" },
                    winui3: "seek_bar.winui3.cpp"
                ) {
                    NativeValue(name: duration, value: players.video.duration)
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
                Row(gap: 8, align: center) {
                    if players.video.playing {
                        IconButton(
                            id: pause,
                            icon: "pause.fill",
                            label: tr! { "en" => "Pause", "zh-Hans" => "暂停" },
                            prominence: plain
                        ) => players.video.pause()
                    } else {
                        IconButton(
                            id: play,
                            icon: "play.fill",
                            label: tr! { "en" => "Play", "zh-Hans" => "播放" },
                            prominence: plain
                        ) => players.video.play()
                    }
                    play_time(at: players.video.position, total: players.video.duration)
                    Spacer
                    // 弹幕由播放内核画在画面上，这里只管显示还是隐藏（也可以按 D）。
                    if app.danmaku_visible {
                        IconButton(
                            id: hide_danmaku,
                            icon: "text.bubble.fill",
                            label: tr! { "en" => "Hide Danmaku", "zh-Hans" => "隐藏弹幕" },
                            prominence: plain
                        ) => app.toggle_danmaku()
                    } else {
                        IconButton(
                            id: show_danmaku,
                            icon: "text.bubble",
                            label: tr! { "en" => "Show Danmaku", "zh-Hans" => "显示弹幕" },
                            prominence: plain
                        ) => app.toggle_danmaku()
                    }
                    if players.video.muted {
                        IconButton(
                            id: unmute,
                            icon: "speaker.slash.fill",
                            label: tr! { "en" => "Unmute", "zh-Hans" => "取消静音" },
                            prominence: plain
                        ) => players.video.set_muted(false)
                    } else {
                        IconButton(
                            id: mute,
                            icon: "speaker.wave.2",
                            label: tr! { "en" => "Mute", "zh-Hans" => "静音" },
                            prominence: plain
                        ) => players.video.set_muted(true)
                    }
                    Column(width: 120) {
                        Slider(
                            id: volume,
                            label: tr! { "en" => "Volume", "zh-Hans" => "音量" },
                            label_hidden: true,
                            current: app.volume,
                            min: 0,
                            max: 1
                        ) => app.set_volume(app.volume)
                    }
                    if window.fullscreen {
                        IconButton(
                            id: exit_fullscreen,
                            icon: "arrow.down.right.and.arrow.up.left",
                            label: tr! { "en" => "Exit Full Screen", "zh-Hans" => "退出全屏" },
                            prominence: plain
                        ) => windows.main.toggle_fullscreen()
                    } else {
                        IconButton(
                            id: enter_fullscreen,
                            icon: "arrow.up.left.and.arrow.down.right",
                            label: tr! { "en" => "Full Screen", "zh-Hans" => "全屏" },
                            prominence: plain
                        ) => windows.main.toggle_fullscreen()
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
        if total >= 3600.0 {
            Text(content: tr! {
                "en" => "{at, duration, h:mm:ss} / {total, duration, h:mm:ss}",
                "zh-Hans" => "{at, duration, h:mm:ss} / {total, duration, h:mm:ss}";
                at: at,
                total: total
            })
        } else {
            Text(content: tr! {
                "en" => "{at, duration, m:ss} / {total, duration, m:ss}",
                "zh-Hans" => "{at, duration, m:ss} / {total, duration, m:ss}";
                at: at,
                total: total
            })
        }
    }
}

/// 当前视频的弹幕情况：处于哪个阶段；加载成功时是哪一集、多少条；失败时的原因。
#[component]
pub fn danmaku_status(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 2) {
            Text(
                content: tr! { "en" => "Danmaku: {status}", "zh-Hans" => "弹幕：{status}"; status: app.danmaku },
                emphasis: secondary
            )
            if app.danmaku_matched {
                Text(
                    content: tr! {
                        "en" => "{anime} - {episode} · {count} comments",
                        "zh-Hans" => "{anime} - {episode} · {count} 条";
                        anime: app.danmaku_anime,
                        episode: app.danmaku_episode,
                        count: app.danmaku_count
                    },
                    emphasis: secondary
                )
            }
            if app.danmaku_error != "" {
                Text(content: app.danmaku_error, emphasis: secondary)
            }
        }
    }
}
