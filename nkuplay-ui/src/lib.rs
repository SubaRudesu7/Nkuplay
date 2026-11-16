//! Nkuplay 的界面：一个窗口，左边是导航，右边是所选的页面；打开视频后换成视频页。
//! 每段文字都在显示它的地方写出所有语言的版本，每个按钮都执行协议里的一条命令。

use bern::prelude::*;
use nkuplay_protocol::nkuplay::{self, ShinStatus};

/// 左侧导航能去的页面。
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
    #[label(tr! { "en" => "Settings", "zh-Hans" => "设置" })]
    Settings,
}

/// 主窗口的草稿状态。
#[bern::session]
pub struct MainDraft {
    /// 最近一次选择或拖入的视频。
    pub file: MediaSource,
    /// 是否有视频正悬停在窗口上方。
    pub over: bool,
    /// 左侧导航选中的页面。
    pub page: Page,
    /// 最近一次选择的要加入媒体库的文件夹。
    pub folder: MediaSource,
}

#[component]
pub fn app(app: nkuplay::Client, windows: AppWindows) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main) {
            Window(
                id: main,
                title: tr! { "en" => "Nkuplay", "zh-Hans" => "Nkuplay" },
                initial: open,
                width: 1440,
                height: 900,
                min_width: 800,
                min_height: 560
            ) {
                main_window(app: app)
            }
        }
    }
}

/// 整个窗口都接受拖入的视频；里面要么是导航加页面，要么是视频页。
#[component]
pub fn main_window(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Drop(
            id: drop,
            label: tr! { "en" => "Drop a video here", "zh-Hans" => "把视频拖到这里" },
            value: draft.file,
            files: "video",
            over: draft.over
        ) {
            if app.has_video {
                player(app: app)
            } else {
                SplitView {
                    Sidebar(selection: draft.page, width: 280) {
                        SidebarItem(value: Page::Home, label: tr! { "en" => "Home", "zh-Hans" => "首页" }, icon: "house")
                        SidebarItem(value: Page::Library, label: tr! { "en" => "Library", "zh-Hans" => "媒体库" }, icon: "folder")
                        SidebarItem(value: Page::Schedule, label: tr! { "en" => "Schedule", "zh-Hans" => "新番时间表" }, icon: "calendar")
                        SidebarItem(value: Page::History, label: tr! { "en" => "History", "zh-Hans" => "观看历史" }, icon: "clock.arrow.circlepath")
                        SidebarItem(value: Page::Settings, label: tr! { "en" => "Settings", "zh-Hans" => "设置" }, icon: "gearshape")
                    }
                    Detail(titled: false) {
                        match draft.page {
                            Page::Home => { home(app: app) }
                            Page::Library => { library_page(app: app) }
                            Page::Schedule => { not_yet(page: Page::Schedule) }
                            Page::History => { not_yet(page: Page::History) }
                            Page::Settings => { not_yet(page: Page::Settings) }
                        }
                    }
                }
            }
        } => app.open_file(draft.file)
    }
}

/// 首页：继续观看、本季新番、本地媒体库。
#[component]
pub fn home(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 28, padding_x: 56, padding_y: 28) {
                // 第一次显示首页时准备新番列表：有缓存读缓存，没有才向弹弹play 取一次。
                Watch(value: app.shin) => app.load_shin()
                Row(gap: 8, align: center) {
                    Text(content: tr! { "en" => "Home", "zh-Hans" => "首页" }, style: large_title)
                    Spacer
                    Button(
                        id: open,
                        label: tr! { "en" => "Open File", "zh-Hans" => "打开文件" },
                        icon: "folder",
                        prominence: prominent,
                        pick: draft.file,
                        files: "video"
                    ) => app.open_file(draft.file)
                }
                continue_watching(app: app)
                shin_section(app: app)
                folders_section(app: app)
            }
        }
    }
}

/// 继续观看：最近打开过的视频，点一下接着播放。
#[component]
pub fn continue_watching(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 12) {
            Text(content: tr! { "en" => "Continue Watching", "zh-Hans" => "继续观看" }, style: title2)
            if app.recent_count == 0 {
                Text(
                    content: tr! {
                        "en" => "Videos you open show up here.",
                        "zh-Hans" => "打开过的视频会出现在这里。"
                    },
                    emphasis: secondary
                )
            } else {
                Grid(min_width: 240, gap: 16) {
                    ForEach(items: app.recents, key: id) |recent| {
                        Button(
                            id: resume,
                            label: tr! { "en" => "Play {title}", "zh-Hans" => "播放{title}"; title: recent.title }
                        ) {
                            Column(gap: 6) {
                                if recent.has_poster {
                                    Image(source: recent.poster, height: 135, fit: cover, corner: 6)
                                } else {
                                    Stack(height: 135) {
                                        Canvas(height: 135) {
                                            Shape(kind: rounded, corner: 6, fill: accent, opacity: 0.18)
                                        }
                                        Label(title: recent.title, icon: "play.fill", display: icon)
                                    }
                                }
                                Text(content: recent.title, lines: 1)
                                if recent.episode != "" {
                                    Text(content: recent.episode, style: caption, emphasis: secondary, lines: 1)
                                }
                            }
                        } => app.open_file(recent.source)
                    }
                }
            }
        }
    }
}

/// 本季新番：按星期筛选的海报墙。数据来自弹弹play，读缓存，点“刷新”才重新取。
#[component]
pub fn shin_section(app: nkuplay::Client) -> impl View {
    ui! {
        Column(gap: 12) {
            Row(gap: 8, align: center) {
                Text(content: tr! { "en" => "This Season", "zh-Hans" => "本季新番" }, style: title2)
                Spacer
                Button(
                    id: refresh_shin,
                    label: tr! { "en" => "Refresh", "zh-Hans" => "刷新" },
                    icon: "arrow.clockwise",
                    enabled: app.shin != ShinStatus::Loading
                ) => app.refresh_shin()
            }
            Picker(
                id: shin_day,
                label: tr! { "en" => "Day", "zh-Hans" => "星期" },
                selected: app.shin_day,
                label_hidden: true,
                style: segmented
            ) => app.show_day(app.shin_day)
            if app.shin_outdated {
                Text(
                    content: tr! {
                        "en" => "This list is from an earlier season. Refresh to get the current one.",
                        "zh-Hans" => "这是之前一季的列表，点刷新获取本季的。"
                    },
                    color: status.warning
                )
            }
            if app.shin_error != "" {
                Text(
                    content: tr! { "en" => "Couldn't get the list: {reason}", "zh-Hans" => "没能取到新番列表：{reason}"; reason: app.shin_error },
                    color: status.error
                )
            }
            match app.shin {
                ShinStatus::Loaded => {
                    if app.shin_count == 0 {
                        Text(
                            content: tr! { "en" => "Nothing airs on this day.", "zh-Hans" => "这一天没有更新的新番。" },
                            emphasis: secondary
                        )
                    } else {
                        Grid(min_width: 140, gap: 16) {
                            ForEach(items: app.shin_shown, key: id) |anime| {
                                Column(gap: 6) {
                                    if anime.has_poster {
                                        Image(source: anime.poster, label: anime.title, height: 187, fit: cover, corner: 6)
                                    }
                                    Text(content: anime.title, lines: 2)
                                }
                            }
                        }
                    }
                }
                ShinStatus::Loading => {
                    Row(gap: 8, align: center) {
                        Progress
                        Text(content: tr! { "en" => "Loading…", "zh-Hans" => "正在获取…" }, emphasis: secondary)
                    }
                }
                ShinStatus::Unavailable => {
                    Text(
                        content: tr! {
                            "en" => "This build has no dandanplay credentials.",
                            "zh-Hans" => "这个版本没有弹弹play 的凭证，取不到新番列表。"
                        },
                        emphasis: secondary
                    )
                }
                ShinStatus::Failed => {
                    // 原因已经显示在上面了；这里只提示还能做什么。
                    Text(
                        content: tr! { "en" => "Try Refresh again later.", "zh-Hans" => "稍后可以点刷新再试。" },
                        emphasis: secondary
                    )
                }
                ShinStatus::NotLoaded => {
                    Progress
                }
            }
        }
    }
}

/// 首页上的本地媒体库：加入的文件夹和里面的视频数。
#[component]
pub fn folders_section(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 12) {
            Row(gap: 8, align: center) {
                Text(content: tr! { "en" => "Local Library", "zh-Hans" => "本地媒体库" }, style: title2)
                Spacer
                Button(
                    id: add_folder,
                    label: tr! { "en" => "Add Folder", "zh-Hans" => "添加文件夹" },
                    icon: "plus",
                    pick: draft.folder,
                    files: "folder"
                ) => app.add_folder(draft.folder)
            }
            folder_list(app: app)
        }
    }
}

/// 媒体库页：和首页上的那一块一样，单独占一页。
#[component]
pub fn library_page(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 20, padding_x: 56, padding_y: 28) {
                Row(gap: 8, align: center) {
                    Text(content: tr! { "en" => "Library", "zh-Hans" => "媒体库" }, style: large_title)
                    Spacer
                    Button(
                        id: add_folder_page,
                        label: tr! { "en" => "Add Folder", "zh-Hans" => "添加文件夹" },
                        icon: "plus",
                        prominence: prominent,
                        pick: draft.folder,
                        files: "folder"
                    ) => app.add_folder(draft.folder)
                }
                folder_list(app: app)
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
                    if folder.scanning {
                        Text(content: tr! { "en" => "Counting…", "zh-Hans" => "正在统计…" }, emphasis: secondary)
                    } else {
                        Text(
                            content: tr! { "en" => "{count} videos", "zh-Hans" => "{count} 个视频"; count: folder.videos },
                            emphasis: secondary
                        )
                    }
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

/// 还没做的页面。
#[component]
pub fn not_yet(page: Page) -> impl View {
    ui! {
        Column(gap: 12, padding_x: 56, padding_y: 28) {
            Text(content: page, style: large_title)
            Text(
                content: tr! { "en" => "This page isn't ready yet.", "zh-Hans" => "这一页还没做。" },
                emphasis: secondary
            )
        }
    }
}

/// 视频页：视频，下面是返回首页的按钮和弹幕状态。
#[component]
pub fn player(app: nkuplay::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 8, padding: 8) {
            Video(
                id: video,
                source: app.source,
                label: tr! { "en" => "Video", "zh-Hans" => "视频" }
            )
            Row(gap: 8) {
                Button(
                    id: back,
                    label: tr! { "en" => "Back", "zh-Hans" => "返回" },
                    icon: "chevron.left"
                ) => app.close()
                Spacer
                Button(
                    id: open_another,
                    label: tr! { "en" => "Open…", "zh-Hans" => "打开…" },
                    pick: draft.file,
                    files: "video"
                ) => app.open_file(draft.file)
            }
            danmaku_status(app: app)
            // 用 `bern new --module` 添加的模块显示在这里。
            Slot(id: extras)
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
