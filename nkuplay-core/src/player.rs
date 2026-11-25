//! 播放页和设置：设置的存取和生效，播放页的选集、章节、片头片尾、下一话、倍速、播放信息、
//! 弹幕列表、弹幕时间偏移和手动匹配。
//!
//! 倍速、字幕、音轨、画面这些 Bern 的标准媒体命令里没有，和弹幕一样由引擎直接交给
//! `nkuplay_mpv`。

use crate::danmaku::Choice;
use crate::settings::{self, Settings};
use crate::{Arrival, NkuplayEngine, details, library, no_source};
use bern::CommandError;
use bern::media::MediaSource;
use nkuplay_dandanplay::{Comment, CommentMode, SearchedAnime};
use nkuplay_mpv::{DanmakuConfig, MediaInfo};
use nkuplay_protocol::nkuplay::{
    BarStay, DanmakuLine, DanmakuStatus, HoldRate, InfoRow, Link, SubtitleRow, MatchAnime, MatchEpisode,
    NavItem, PlayEpisode, SeekStep, Word,
};
use std::sync::Arc;

/// 弹幕热度分几段。
const HEAT_BINS: usize = 120;
/// 弹幕列表里现在之前、之后各列几条。
const LINES_BEFORE: usize = 6;
const LINES_AFTER: usize = 8;
/// 搜索弹幕时最多列几条。
const LINES_FOUND: usize = 60;
/// 合并重复弹幕时，多少秒内算重复。
const MERGE_WITHIN: f64 = 10.0;

/// 引擎为播放页记着的东西；换片时清掉。
#[derive(Default)]
pub(crate) struct Play {
    /// 正在播放的视频所在的那部番的剧集（海报墙上同一部番），按集数排；不在媒体库里时为空。
    episodes: Vec<library::EpisodeFile>,
    /// 视频的章节：从第几秒开始、叫什么。
    chapters: Vec<(f64, String)>,
    /// 已经问过播放内核章节和编码信息。
    asked: bool,
    /// 总时长，秒；还不知道时为 0。
    duration: f64,
    /// 弹弹play 给的时间偏移；用户调的另外加在上面。
    base_shift: f64,
    /// 自动跳过过的片头片尾（在 `parts` 里的位置），一段只跳一次。
    skipped: Vec<usize>,
    /// 已经播到结尾，按“播完以后”处理过。
    ended: bool,
    /// 手动匹配：搜到的作品，选中的作品和剧集。
    found: Vec<SearchedAnime>,
    anime: Option<i64>,
    episode: Option<i64>,
    /// 选集的每一行（`PlayEpisode::id` 是下标）对应的本地文件；本地没有的为空。
    row_urls: Vec<Option<String>>,
    /// 已经在后台要过这部番的资料和各集缩略图。
    images_asked: bool,
}

/// 片头还是片尾，从哪里到哪里（秒）。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Part {
    start: f64,
    end: f64,
    outro: bool,
}

/// 章节名像片头或片尾的，就是可以跳过的一段。
fn parts(chapters: &[(f64, String)], duration: f64) -> Vec<Part> {
    let mut found = Vec::new();
    for (index, (start, title)) in chapters.iter().enumerate() {
        let name = title.trim().to_lowercase();
        let named = |short: &[&str], long: &[&str]| {
            short
                .iter()
                .any(|word| name == *word || name.starts_with(&format!("{word} ")))
                || long.iter().any(|word| name.contains(word))
        };
        let intro = named(
            &["op", "intro"],
            &["opening", "片头", "片頭", "オープニング"],
        );
        let outro = named(&["ed"], &["ending", "credits", "片尾", "エンディング"]);
        if !intro && !outro {
            continue;
        }
        let end = chapters.get(index + 1).map_or(duration, |(next, _)| *next);
        if end > *start {
            found.push(Part {
                start: *start,
                end,
                outro: outro && !intro,
            });
        }
    }
    found
}

/// 秒数写成 `m:ss`。
pub(crate) fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0) as i64
    } else {
        0
    };
    if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

/// 倍速写成 `1.0×`、`1.25×`。
fn speed_text(speed: f64) -> String {
    let hundredths = (speed * 100.0).round() as i64;
    if hundredths % 10 == 0 {
        format!("{:.1}×", speed)
    } else {
        format!("{:.2}×", speed)
    }
}

/// 弹幕时间偏移写成 `+1.5 秒`、`0.0 秒`。
fn shift_text(shift: f64) -> String {
    if shift.abs() < 0.05 {
        "0.0 秒".into()
    } else {
        format!("{shift:+.1} 秒")
    }
}

/// 一集在选集里叫什么：`第8话`；文件名里认不出集数时是文件名。
fn episode_code(file: &library::EpisodeFile) -> String {
    match file.episode {
        Some(number) => format!("第{number}话"),
        None => file.name.clone(),
    }
}

/// 弹幕热度：总时长分成 [`HEAT_BINS`] 段，数每段有多少条，相邻三段平滑，再按最多的一段换成 0 到 1。
fn heat_of(comments: &[Comment], duration: f64) -> Vec<f64> {
    if duration <= 0.0 || comments.is_empty() {
        return Vec::new();
    }
    let mut counts = vec![0.0; HEAT_BINS];
    for comment in comments {
        if comment.time.is_finite() && comment.time >= 0.0 {
            let bin = ((comment.time / duration) * HEAT_BINS as f64) as usize;
            counts[bin.min(HEAT_BINS - 1)] += 1.0;
        }
    }
    let smooth: Vec<f64> = (0..HEAT_BINS)
        .map(|i| {
            let before = counts[i.saturating_sub(1)];
            let after = counts[(i + 1).min(HEAT_BINS - 1)];
            (before + counts[i] * 2.0 + after) / 4.0
        })
        .collect();
    let most = smooth.iter().copied().fold(0.0, f64::max);
    if most <= 0.0 {
        return Vec::new();
    }
    smooth.into_iter().map(|count| count / most).collect()
}

/// 播放信息的几行。
fn info_rows(info: &MediaInfo) -> Vec<InfoRow> {
    let mut rows = Vec::new();
    let mut push = |key: &str, value: String| {
        if !value.is_empty() {
            rows.push(InfoRow {
                id: rows.len() as i64,
                key: key.into(),
                value,
            });
        }
    };
    let join = |parts: Vec<Option<String>>| {
        parts
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let kbps = |bits: Option<f64>| {
        bits.filter(|bits| *bits > 0.0).map(|bits| {
            if bits >= 1_000_000.0 {
                format!("{:.1} Mbps", bits / 1_000_000.0)
            } else {
                format!("{:.0} kbps", bits / 1000.0)
            }
        })
    };
    push(
        "视频",
        join(vec![
            info.video_codec.as_ref().map(|codec| codec.to_uppercase()),
            (info.width > 0).then(|| format!("{}×{}", info.width, info.height)),
            info.fps
                .map(|fps| format!("{fps:.3} fps").replace(".000", "")),
            kbps(info.video_bitrate),
        ]),
    );
    let dynamic = info.gamma.as_deref().map(|gamma| {
        if matches!(gamma, "pq" | "hlg") {
            "HDR"
        } else {
            "SDR"
        }
        .to_string()
    });
    push(
        "颜色",
        join(vec![
            info.pixel_format.clone(),
            info.color_matrix.clone(),
            dynamic,
        ]),
    );
    push(
        "解码",
        info.decoder.clone().unwrap_or_else(|| "软件".into()),
    );
    push(
        "音频",
        join(vec![
            info.audio_codec.as_ref().map(|codec| codec.to_uppercase()),
            info.channels.map(|count| match count {
                1 => "单声道".to_string(),
                2 => "立体声".to_string(),
                count => format!("{count} 声道"),
            }),
            info.sample_rate.map(|rate| format!("{} kHz", rate / 1000)),
            kbps(info.audio_bitrate),
        ]),
    );
    push(
        "字幕",
        join(vec![
            info.subtitle_codec
                .as_ref()
                .map(|codec| codec.to_uppercase()),
            info.subtitle_language.as_deref().map(language_name),
        ]),
    );
    push(
        "文件",
        join(vec![
            info.file_format
                .as_ref()
                .map(|format| format.to_uppercase()),
            info.file_size.map(|size| {
                let gb = size as f64 / 1024.0 / 1024.0 / 1024.0;
                if gb >= 1.0 {
                    format!("{gb:.2} GB")
                } else {
                    format!("{:.0} MB", size as f64 / 1024.0 / 1024.0)
                }
            }),
        ]),
    );
    push("丢帧", info.dropped_frames.to_string());
    rows
}

/// 弹弹play 的弹幕文件：xml（`<d p="时间,模式,字号,颜色,…">文字</d>`）或 json（`{"comments":[{"p":"时间,模式,颜色,用户","m":"文字"}]}`）。
pub(crate) fn parse_danmaku_file(text: &str) -> Vec<Comment> {
    let mode_of = |code: &str| match code.trim() {
        "4" => CommentMode::Bottom,
        "5" => CommentMode::Top,
        _ => CommentMode::Scroll,
    };
    let mut comments = Vec::new();
    if text.trim_start().starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            for (index, item) in value["comments"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let p = item["p"].as_str().unwrap_or_default();
                let fields: Vec<&str> = p.split(',').collect();
                let Some(time) = fields
                    .first()
                    .and_then(|time| time.trim().parse::<f64>().ok())
                else {
                    continue;
                };
                comments.push(Comment {
                    id: index as i64,
                    time,
                    mode: mode_of(fields.get(1).copied().unwrap_or("1")),
                    color: fields
                        .get(2)
                        .and_then(|color| color.trim().parse().ok())
                        .unwrap_or(0xFFFFFF),
                    user: String::new(),
                    text: item["m"].as_str().unwrap_or_default().to_string(),
                });
            }
        }
    } else {
        let mut rest = text;
        while let Some(start) = rest.find("<d p=\"") {
            rest = &rest[start + 6..];
            let Some(quote) = rest.find('"') else { break };
            let p = &rest[..quote];
            let Some(open) = rest.find('>') else { break };
            let Some(close) = rest.find("</d>") else {
                break;
            };
            if close < open {
                rest = &rest[close + 4..];
                continue;
            }
            let body = &rest[open + 1..close];
            rest = &rest[close + 4..];
            let fields: Vec<&str> = p.split(',').collect();
            let Some(time) = fields
                .first()
                .and_then(|time| time.trim().parse::<f64>().ok())
            else {
                continue;
            };
            comments.push(Comment {
                id: comments.len() as i64,
                time,
                mode: mode_of(fields.get(1).copied().unwrap_or("1")),
                color: fields
                    .get(3)
                    .and_then(|color| color.trim().parse().ok())
                    .unwrap_or(0xFFFFFF),
                user: String::new(),
                text: unescape(body),
            });
        }
    }
    comments.sort_by(|a, b| a.time.total_cmp(&b.time));
    comments
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

impl NkuplayEngine {
    // ——设置——

    /// 把设置写进界面的状态。
    pub(crate) fn publish_settings(&mut self) {
        let s = &self.settings;
        let state = &mut self.state;
        state.appearance = settings::appearance_of(&s.appearance);
        state.start_page = settings::page_of(&s.start_page);
        state.nav = s
            .nav
            .iter()
            .enumerate()
            .map(|(id, (page, on))| NavItem {
                id: id as i64,
                page: settings::page_of(page),
                on: *on,
            })
            .collect();
        state.nav_shown = state.nav.iter().filter(|item| item.on).cloned().collect();
        state.resume = s.resume;
        state.auto_fullscreen = s.auto_fullscreen;
        state.after_end = settings::after_end_of(&s.after_end);
        state.countdown = s.countdown;
        state.skip_mode = settings::skip_mode_of(&s.skip_mode);
        state.skip_seconds = s.skip_seconds;
        state.seek_step = s.seek_step;
        state.hold_speed = s.hold_speed;
        state.keep_pitch = s.keep_pitch;
        state.bar_stay = s.bar_stay;
        state.seek_choice = match s.seek_step {
            1 => SeekStep::One,
            10 => SeekStep::Ten,
            15 => SeekStep::Fifteen,
            30 => SeekStep::Thirty,
            _ => SeekStep::Five,
        };
        state.hold_choice = match s.hold_speed {
            15 => HoldRate::OneAndHalf,
            30 => HoldRate::Three,
            40 => HoldRate::Four,
            _ => HoldRate::Two,
        };
        state.stay_choice = match s.bar_stay {
            2 => BarStay::Two,
            3 => BarStay::Three,
            5 => BarStay::Five,
            _ => BarStay::One,
        };
        state.chapter_marks = s.chapter_marks;
        state.heat_curve = s.heat_curve;
        state.hover_preview = s.hover_preview;
        state.hwdec = s.hwdec;
        state.cache_mb = s.cache_mb;
        state.upscale = settings::upscale_of(&s.upscale);
        state.aspect = settings::aspect_of(&s.aspect);
        state.fill = settings::fill_of(&s.fill);
        state.d_opacity = s.d_opacity;
        state.d_size = s.d_size;
        state.d_outline = s.d_outline;
        state.d_font = settings::font_of(&s.d_font);
        state.d_area = s.d_area;
        state.d_seconds = s.d_seconds;
        state.d_gap = s.d_gap;
        state.d_overlap = s.d_overlap;
        state.d_scroll = s.d_scroll;
        state.d_top = s.d_top;
        state.d_bottom = s.d_bottom;
        state.d_colored = s.d_colored;
        state.d_merge = s.d_merge;
        state.blocked = s
            .blocked
            .iter()
            .enumerate()
            .map(|(id, text)| Word {
                id: id as i64,
                text: text.clone(),
            })
            .collect();
        state.blocked_count = s.blocked.len() as i64;
        state.keys = s.keys();
    }

    /// 设置交给播放内核。
    pub(crate) fn apply_settings(&self) {
        let s = &self.settings;
        nkuplay_mpv::set_property("hwdec", if s.hwdec { "auto-safe" } else { "no" });
        nkuplay_mpv::set_property(
            "demuxer-max-bytes",
            &format!("{}MiB", s.cache_mb.round() as i64),
        );
        nkuplay_mpv::set_property(
            "audio-pitch-correction",
            if s.keep_pitch { "yes" } else { "no" },
        );
        let (aspect, keep) = match s.aspect.as_str() {
            "16:9" => ("16:9", "yes"),
            "4:3" => ("4:3", "yes"),
            "21:9" => ("2.333", "yes"),
            "stretch" => ("-1", "no"),
            _ => ("-1", "yes"),
        };
        nkuplay_mpv::set_property("video-aspect-override", aspect);
        nkuplay_mpv::set_property("keepaspect", keep);
        nkuplay_mpv::set_property("panscan", if s.fill == "crop" { "1.0" } else { "0.0" });
        nkuplay_mpv::set_property("glsl-shaders", &self.shaders());
        nkuplay_mpv::set_property(
            "loop-file",
            if s.after_end == "loop" { "inf" } else { "no" },
        );
        nkuplay_mpv::set_danmaku_config(self.danmaku_config());
    }

    /// Anime4K 的着色器：可执行文件旁边 `shaders/anime4k/<档位>/` 里的 `.glsl`，按文件名排；
    /// 关着或没有这些文件时为空。
    fn shaders(&self) -> String {
        if self.settings.upscale == "off" {
            return String::new();
        }
        let Some(folder) = std::env::current_exe().ok().and_then(|exe| {
            exe.parent().map(|dir| {
                dir.join("shaders")
                    .join("anime4k")
                    .join(&self.settings.upscale)
            })
        }) else {
            return String::new();
        };
        let mut files: Vec<_> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "glsl"))
            .collect();
        files.sort();
        // mpv 在 Windows 上用 `;` 分隔路径列表。
        files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(";")
    }

    fn danmaku_config(&self) -> DanmakuConfig {
        let s = &self.settings;
        DanmakuConfig {
            font: settings::font_family(&settings::font_of(&s.d_font)).into(),
            font_size: s.d_size,
            outline: s.d_outline,
            opacity: s.d_opacity / 100.0,
            scroll_seconds: s.d_seconds,
            area: s.d_area / 100.0,
            lane_gap: s.d_gap / 100.0,
            merge_within: s.d_merge.then_some(MERGE_WITHIN),
            blocked: s.blocked.clone(),
            scroll: s.d_scroll,
            top: s.d_top,
            bottom: s.d_bottom,
            colored: s.d_colored,
            overlap: s.d_overlap,
            ..DanmakuConfig::default()
        }
    }

    /// 设置改了：存盘、更新界面、交给播放内核。
    pub(crate) fn settings_changed(&mut self) {
        settings::save(self.settings_folder.as_ref(), &self.settings);
        self.publish_settings();
        self.apply_settings();
        self.publish_play();
    }

    pub(crate) fn edit_settings(&mut self, edit: impl FnOnce(&mut Settings)) {
        edit(&mut self.settings);
        self.settings_changed();
    }

    pub(crate) fn open_link_at(&self, link: &Link) -> Result<(), CommandError> {
        let url = match link {
            Link::Source => "https://github.com/SubaRudesu7/Nkuplay",
            Link::Dandanplay => "https://www.dandanplay.com/",
            Link::Tmdb => "https://www.themoviedb.org/",
            Link::BangumiData => "https://github.com/bangumi-data/bangumi-data",
        };
        bern::platform::open_https(url)
    }

    // ——播放页——

    /// 打开一个视频时：记下它所在的那部番，重置播放页。
    pub(crate) fn begin_play(&mut self, url: &str) {
        self.play = Play::default();
        self.play.episodes = self
            .shelf
            .iter()
            .find(|item| item.episodes.iter().any(|episode| episode.url == url))
            .map(|item| item.episodes.clone())
            .unwrap_or_default();
        self.state.danmaku_shift = 0.0;
        self.state.danmaku_shift_text = shift_text(0.0);
        self.state.danmaku_lines.clear();
        self.state.danmaku_source = String::new();
        self.state.danmaku_from_file = false;
        self.state.info.clear();
        self.state.subtitles.clear();
        self.state.subtitle_count = 0;
        self.state.next_shown = false;
        self.state.skip_shown = false;
        self.state.preview_ready = false;
        self.state.match_open = false;
        self.state.fullscreen_pending = self.settings.auto_fullscreen;
        self.apply_settings();
        self.publish_play();
    }

    /// 正在播放的是剧集里的第几个。
    fn index(&self) -> Option<usize> {
        let url = &self.state.source.url;
        self.play
            .episodes
            .iter()
            .position(|episode| &episode.url == url)
    }

    /// 播放页的番名、选集、上一话下一话、章节、热度。
    pub(crate) fn publish_play(&mut self) {
        let url = self.state.source.url.clone();
        let recent = self.library.recents.iter().find(|recent| recent.url == url);
        let index = self.index();
        if self.state.danmaku_matched {
            self.state.play_title = self.state.danmaku_anime.clone();
            self.state.play_subtitle = self.state.danmaku_episode.clone();
        } else if let Some(recent) = recent.filter(|recent| !recent.episode.is_empty()) {
            self.state.play_title = recent.title.clone();
            self.state.play_subtitle = recent.episode.clone();
        } else {
            // 没认出：顶栏写文件名。
            self.state.play_title = self
                .state
                .source
                .file_name()
                .map(|name| match name.rsplit_once('.') {
                    Some((stem, _)) => stem.to_string(),
                    None => name,
                })
                .unwrap_or_default();
            self.state.play_subtitle = String::new();
        }
        self.state.has_previous = index.is_some_and(|index| index > 0);
        self.state.has_next = index.is_some_and(|index| index + 1 < self.play.episodes.len());
        // 选集：认出了作品、缓存里有它的资料时，列出全部集数（本地没有的写“未找到”），
        // 缩略图用 TMDB 的每集缩略图；否则只列媒体库里这部番的文件。
        let anime = self.play_anime();
        let found = anime
            .and_then(|id| self.details_source.cached(id))
            .and_then(|info| info.details);
        let tmdb = found.as_ref().and_then(details::tmdb);
        // 不在媒体库里的视频：只有它自己，集数按识别出的剧集名。
        let alone;
        let local = if self.play.episodes.is_empty() && !url.is_empty() {
            let recent = self.library.recents.iter().find(|recent| recent.url == url);
            alone = vec![library::EpisodeFile {
                url: url.clone(),
                name: self.state.source.file_name().unwrap_or_default(),
                episode: recent.and_then(|recent| crate::first_number(&recent.episode)),
            }];
            &alone[..]
        } else {
            &self.play.episodes[..]
        };
        let rows = details::rows(found.as_ref(), local, &self.library.progress);
        self.play.row_urls = rows.iter().map(|row| row.url.clone()).collect();
        self.state.episodes = rows
            .iter()
            .enumerate()
            .map(|(id, row)| {
                let current = row.url.as_deref() == Some(url.as_str());
                let local = row.url.is_some();
                let percent = row.watched.map_or(0, |watched| watched.percent());
                let finished = row.watched.is_some_and(|watched| watched.finished());
                let code = match row.number {
                    Some(number) => format!("第{number}话"),
                    None if !row.code.is_empty() => row.code.clone(),
                    None => row.title.clone(),
                };
                let title = if found.is_some() {
                    row.title.clone()
                } else {
                    // 没有资料时用识别出的剧集名；认出的剧集名开头是“第7话”，选集里已经写了，去掉。
                    row.url
                        .as_ref()
                        .and_then(|row_url| self.library.recents.iter().find(|recent| &recent.url == row_url))
                        .map(|recent| recent.episode.trim_start_matches(code.as_str()).trim().to_string())
                        .unwrap_or_default()
                };
                let thumb = tmdb
                    .zip(row.number)
                    .and_then(|((tv, season), number)| self.stills.cached_thumb(tv, season, number));
                PlayEpisode {
                    id: id as i64,
                    code,
                    title,
                    has_thumb: thumb.is_some(),
                    thumb: thumb.map_or_else(no_source, MediaSource::file),
                    note: if current {
                        "正在播放".into()
                    } else if !local {
                        "未找到".into()
                    } else if finished {
                        "已看完".into()
                    } else if percent > 0 {
                        format!("看到 {percent}%")
                    } else {
                        String::new()
                    },
                    current,
                    local,
                    bar: if finished || !local { 0.0 } else { percent as f64 / 100.0 },
                }
            })
            .collect();
        self.state.episode_count = self.state.episodes.len() as i64;
        let duration = self.play.duration;
        self.state.chapters = if self.settings.chapter_marks && duration > 0.0 {
            self.play
                .chapters
                .iter()
                .map(|(start, _)| start / duration)
                .filter(|fraction| *fraction > 0.002 && *fraction < 0.998)
                .collect()
        } else {
            Vec::new()
        };
        self.state.heat = if self.settings.heat_curve {
            heat_of(&self.comments, duration)
        } else {
            Vec::new()
        };
        self.state.speed_text = speed_text(self.state.speed);
        self.publish_next(index);
    }

    /// 下一话倒计时卡片上的那一集。
    fn publish_next(&mut self, index: Option<usize>) {
        let next = index.and_then(|index| self.play.episodes.get(index + 1));
        match next {
            Some(file) => {
                self.state.next_code = episode_code(file);
                self.state.next_title = self
                    .library
                    .recents
                    .iter()
                    .find(|recent| recent.url == file.url)
                    .map(|recent| recent.episode.clone())
                    .unwrap_or_default();
            }
            None => {
                self.state.next_code = String::new();
                self.state.next_title = String::new();
            }
        }
        self.state.next_thumb = no_source();
        self.state.next_has_thumb = false;
    }

    /// 播放页认出的作品：这个视频识别过的作品编号。
    pub(crate) fn play_anime(&self) -> Option<i64> {
        let url = &self.state.source.url;
        self.library
            .recents
            .iter()
            .find(|recent| &recent.url == url)
            .and_then(|recent| recent.anime_id)
    }

    /// 选集第 `id` 行：本地有文件时换到它。
    pub(crate) fn play_row(&mut self, id: i64) -> Result<(), CommandError> {
        let Some(Some(row_url)) = usize::try_from(id).ok().and_then(|id| self.play.row_urls.get(id)).cloned() else {
            return Ok(());
        };
        match self.play.episodes.iter().position(|episode| episode.url == row_url) {
            Some(index) => self.play_at(index),
            None => Ok(()),
        }
    }

    /// 播放页认出了作品、还没要过时，在后台取它的资料（缓存里没有的话）和各集缩略图，取完重排选集。
    /// 要在命令里调：后台取完要唤醒引擎。
    pub(crate) fn want_play_images(&mut self) {
        if self.play.images_asked {
            return;
        }
        let Some(anime) = self.play_anime() else {
            return;
        };
        self.play.images_asked = true;
        let source = Arc::clone(&self.details_source);
        let stills = Arc::clone(&self.stills);
        let generation = self.generation;
        self.in_background_many(move |send| {
            let found = source
                .cached(anime)
                .filter(|info| info.details.is_some())
                .or_else(|| source.download(anime, crate::unix_now()).ok())
                .and_then(|info| info.details);
            send(Arrival::PlayImages(generation));
            let Some(found) = found else {
                return;
            };
            let Some((tv, season)) = details::tmdb(&found).filter(|_| stills.available()) else {
                return;
            };
            for number in found
                .episodes
                .iter()
                .filter_map(|episode| episode.episode_number.parse::<u32>().ok())
            {
                if stills.cached_thumb(tv, season, number).is_none() && stills.fetch_thumb(tv, season, number).is_some() {
                    send(Arrival::PlayImages(generation));
                }
            }
        });
    }

    /// 换到剧集里的第 `index` 个。
    pub(crate) fn play_at(&mut self, index: usize) -> Result<(), CommandError> {
        let Some(file) = self.play.episodes.get(index).cloned() else {
            return Ok(());
        };
        let episodes = std::mem::take(&mut self.play.episodes);
        <Self as nkuplay_protocol::nkuplay::Commands>::open_file(
            self,
            MediaSource::new(&file.url),
        )?;
        // 打开时按海报墙重新找了一遍；找不到（比如媒体库刚刷新）就沿用原来的。
        if self.play.episodes.is_empty() {
            self.play.episodes = episodes;
            self.publish_play();
        }
        Ok(())
    }

    pub(crate) fn play_offset(&mut self, offset: i64) -> Result<(), CommandError> {
        let Some(index) = self.index() else {
            return Ok(());
        };
        let target = index as i64 + offset;
        if target < 0 {
            return Ok(());
        }
        self.play_at(target as usize)
    }

    /// 每秒一次：读章节、片头片尾、播完以后。
    pub(crate) fn tick_at(&mut self, position_ms: i64, duration_ms: Option<i64>) {
        if !self.state.has_video {
            return;
        }
        let duration = duration_ms
            .filter(|duration| *duration > 0)
            .map_or(0.0, |duration| duration as f64 / 1000.0);
        if duration > 0.0 && (self.play.duration - duration).abs() > 0.5 {
            self.play.duration = duration;
            self.publish_play();
        }
        if duration > 0.0 && !self.play.asked {
            self.play.asked = true;
            self.ask_info();
        }
        self.want_play_images();
        let position = position_ms as f64 / 1000.0;
        self.update_skip(position);
        self.update_end(position, duration);
    }

    /// 问播放内核要章节和编码信息，在后台等它回话。
    fn ask_info(&self) {
        let Some(answer) = nkuplay_mpv::media_info() else {
            return;
        };
        let generation = self.generation;
        self.in_background(move || {
            let info = answer
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or_default();
            Arrival::Media(generation, info)
        });
    }

    /// 播放内核报回了章节和编码信息。
    pub(crate) fn take_media(&mut self, generation: u64, info: MediaInfo) {
        if generation != self.generation {
            return;
        }
        if !info.chapters.is_empty() || self.play.chapters.is_empty() {
            self.play.chapters = info.chapters.clone();
        }
        self.state.info = info_rows(&info);
        self.state.subtitles = subtitle_rows(&info.subtitles);
        self.state.subtitle_count = self.state.subtitles.len() as i64;
        self.publish_play();
    }

    pub(crate) fn load_info_now(&mut self) {
        self.ask_info();
    }

    /// 片头片尾的按钮，或自动跳过。
    fn update_skip(&mut self, position: f64) {
        let mode = self.settings.skip_mode.as_str();
        let found = parts(&self.play.chapters, self.play.duration);
        let here = found
            .iter()
            .enumerate()
            .find(|(_, part)| position >= part.start && position < part.end - 1.0);
        match (mode, here) {
            ("auto", Some((index, part))) if !self.play.skipped.contains(&index) => {
                self.play.skipped.push(index);
                self.seek_to((part.end * 1000.0) as i64, std::time::Instant::now());
                self.state.skip_shown = false;
            }
            ("button", Some((_, part))) => {
                self.state.skip_shown = true;
                self.state.skip_outro = part.outro;
                self.state.skip_label = format!("到 {}", clock(part.end));
            }
            _ => self.state.skip_shown = false,
        }
    }

    pub(crate) fn skip_part_now(&mut self) {
        let found = parts(&self.play.chapters, self.play.duration);
        // 按钮显示着时，位置就在某一段里；跳到那一段的结尾。
        let label = self.state.skip_label.clone();
        if let Some(part) = found
            .iter()
            .find(|part| format!("到 {}", clock(part.end)) == label)
        {
            self.seek_to((part.end * 1000.0) as i64, std::time::Instant::now());
        }
        self.state.skip_shown = false;
    }

    /// 播到结尾：按“播完以后”处理；下一话要倒计时的，每秒减一。
    fn update_end(&mut self, position: f64, duration: f64) {
        if duration <= 0.0 {
            return;
        }
        let at_end = position >= duration - 0.6;
        if !at_end {
            if self.play.ended {
                self.play.ended = false;
                self.state.next_shown = false;
            }
            return;
        }
        if self.play.ended {
            if self.state.next_shown {
                self.state.next_left -= 1;
                if self.state.next_left <= 0 {
                    self.state.next_shown = false;
                    let _ = self.play_offset(1);
                }
            }
            return;
        }
        self.play.ended = true;
        match self.settings.after_end.as_str() {
            "next" if self.state.has_next => {
                self.state.next_left = self.settings.countdown.round() as i64;
                self.state.next_shown = true;
            }
            "back" => {
                let _ = <Self as nkuplay_protocol::nkuplay::Commands>::close(self);
            }
            _ => {}
        }
    }

    pub(crate) fn cancel_next_now(&mut self) {
        self.state.next_shown = false;
    }

    /// 倍速：按住长按快进时内核用长按的倍速，松开后回到这里设的。
    pub(crate) fn set_speed_to(&mut self, speed: f64) -> Result<(), CommandError> {
        if !speed.is_finite() {
            return Err(CommandError::new(
                "invalid_speed",
                "the speed must be a number",
            ));
        }
        self.state.speed = (speed * 100.0).round().clamp(25.0, 400.0) / 100.0;
        self.state.speed_text = speed_text(self.state.speed);
        if !self.state.fast {
            nkuplay_mpv::set_property("speed", &format!("{}", self.state.speed));
        }
        Ok(())
    }

    pub(crate) fn set_fast_to(&mut self, fast: bool) {
        self.state.fast = fast;
        let speed = if fast {
            self.settings.hold_speed as f64 / 10.0
        } else {
            self.state.speed
        };
        nkuplay_mpv::set_property("speed", &format!("{speed}"));
    }

    // ——弹幕——

    /// 弹幕交给播放内核，带上弹弹play 的偏移和用户调的偏移。
    pub(crate) fn send_danmaku(&self) {
        nkuplay_mpv::set_danmaku(
            crate::for_kernel(&self.comments),
            self.play.base_shift + self.state.danmaku_shift,
        );
    }

    /// 取到一集的弹幕以后：记下弹弹play 的偏移，更新热度和来源。
    pub(crate) fn danmaku_arrived(&mut self, base_shift: f64) {
        self.play.base_shift = base_shift;
        self.state.danmaku_source = "弹弹play".into();
        self.state.danmaku_from_file = false;
        self.send_danmaku();
        self.publish_play();
    }

    pub(crate) fn nudge_shift_by(&mut self, step: f64) {
        if !step.is_finite() {
            return;
        }
        let shift = ((self.state.danmaku_shift + step) * 10.0).round() / 10.0;
        self.state.danmaku_shift = shift.clamp(-60.0, 60.0);
        self.state.danmaku_shift_text = shift_text(self.state.danmaku_shift);
        if !self.comments.is_empty() {
            self.send_danmaku();
        }
    }

    pub(crate) fn list_danmaku_at(&mut self, position_ms: i64, query: &str) {
        let position =
            position_ms as f64 / 1000.0 - self.play.base_shift - self.state.danmaku_shift;
        let query = query.trim();
        let picked: Vec<&Comment> = if query.is_empty() {
            let now = self
                .comments
                .partition_point(|comment| comment.time < position);
            let from = now.saturating_sub(LINES_BEFORE);
            let to = (now + LINES_AFTER).min(self.comments.len());
            self.comments[from..to].iter().collect()
        } else {
            self.comments
                .iter()
                .filter(|comment| comment.text.contains(query))
                .take(LINES_FOUND)
                .collect()
        };
        let nearest = picked
            .iter()
            .enumerate()
            .min_by(|a, b| {
                (a.1.time - position)
                    .abs()
                    .total_cmp(&(b.1.time - position).abs())
            })
            .map(|(index, _)| index);
        self.state.danmaku_lines = picked
            .iter()
            .enumerate()
            .map(|(index, comment)| DanmakuLine {
                id: comment.id,
                time: clock(comment.time + self.play.base_shift + self.state.danmaku_shift),
                text: comment.text.clone(),
                now: query.is_empty() && Some(index) == nearest,
            })
            .collect();
    }

    pub(crate) fn block_line_now(&mut self, id: i64) {
        let Some(text) = self
            .state
            .danmaku_lines
            .iter()
            .find(|line| line.id == id)
            .map(|line| line.text.clone())
        else {
            return;
        };
        if self.settings.add_blocked(&text) {
            self.state.danmaku_lines.retain(|line| line.id != id);
            self.settings_changed();
        }
    }

    /// 弹幕文件里的弹幕代替弹弹play 的。
    pub(crate) fn load_file_comments(&mut self, file: &MediaSource) -> Result<(), CommandError> {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file.open()?, &mut bytes)
            .map_err(|error| CommandError::new("unreadable", error.to_string()))?;
        let comments = parse_danmaku_file(&String::from_utf8_lossy(&bytes));
        if comments.is_empty() {
            return Err(CommandError::new("no_danmaku", "the file holds no danmaku"));
        }
        // 正在路上的弹弹play 结果作废。
        self.generation += 1;
        self.state.danmaku = DanmakuStatus::Loaded;
        self.state.danmaku_count = comments.len() as i64;
        self.comments = Arc::from(comments);
        self.play.base_shift = 0.0;
        self.state.danmaku_source = file.file_name().unwrap_or_default();
        self.state.danmaku_from_file = true;
        self.send_danmaku();
        self.publish_play();
        Ok(())
    }

    /// 重新取弹幕，不用缓存。
    pub(crate) fn refetch(&mut self) {
        let Some(loader) = self.danmaku.clone() else {
            return;
        };
        let source = self.state.source.clone();
        self.reset_danmaku(DanmakuStatus::Matching);
        let generation = self.generation;
        self.in_background(move || Arrival::Danmaku(generation, loader.reload(&source)));
    }

    // ——手动匹配——

    pub(crate) fn open_match_now(&mut self) {
        self.state.match_open = true;
        self.state.match_file = self.state.source.file_name().unwrap_or_default();
        self.state.match_folder = true;
        self.state.match_animes.clear();
        self.state.match_episodes.clear();
        self.state.match_ready = false;
        self.play.found.clear();
        self.play.anime = None;
        self.play.episode = None;
        // 先按认出的作品名搜；没认出时按文件名猜的作品名。
        let guess = if self.state.danmaku_matched {
            self.state.danmaku_anime.clone()
        } else {
            crate::naming::parse(&self.state.match_file).title
        };
        self.search_match_for(&guess);
    }

    pub(crate) fn search_match_for(&mut self, query: &str) {
        let query = query.trim().to_string();
        let Some(loader) = self.danmaku.clone().filter(|_| !query.is_empty()) else {
            return;
        };
        self.state.match_busy = true;
        let generation = self.generation;
        self.in_background(move || Arrival::Searched(generation, loader.search(&query)));
    }

    pub(crate) fn take_search(
        &mut self,
        generation: u64,
        found: Result<Vec<SearchedAnime>, String>,
    ) {
        if generation != self.generation {
            return;
        }
        self.state.match_busy = false;
        self.play.found = found.unwrap_or_default();
        self.play.anime = self.play.found.first().map(|anime| anime.anime_id);
        self.play.episode = None;
        // 选中作品里集数和文件名对上的那一集。
        let number = self
            .play
            .episodes
            .get(self.index().unwrap_or(usize::MAX))
            .and_then(|file| file.episode)
            .or_else(|| crate::naming::parse(&self.state.match_file).episode);
        if let (Some(anime), Some(number)) = (self.chosen_anime(), number) {
            self.play.episode = anime
                .episodes
                .iter()
                .find(|episode| episode_number(&episode.episode_title) == Some(number))
                .map(|episode| episode.episode_id);
        }
        self.publish_match();
    }

    fn chosen_anime(&self) -> Option<&SearchedAnime> {
        let id = self.play.anime?;
        self.play.found.iter().find(|anime| anime.anime_id == id)
    }

    pub(crate) fn publish_match(&mut self) {
        self.state.match_animes = self
            .play
            .found
            .iter()
            .map(|anime| MatchAnime {
                id: anime.anime_id,
                title: anime.anime_title.clone(),
                note: if anime.type_description.is_empty() {
                    format!("{} 话", anime.episodes.len())
                } else {
                    format!("{} · {} 话", anime.type_description, anime.episodes.len())
                },
                chosen: Some(anime.anime_id) == self.play.anime,
            })
            .collect();
        let chosen = self.play.episode;
        self.state.match_episodes = self
            .chosen_anime()
            .map(|anime| {
                anime
                    .episodes
                    .iter()
                    .map(|episode| MatchEpisode {
                        id: episode.episode_id,
                        title: episode.episode_title.clone(),
                        chosen: Some(episode.episode_id) == chosen,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.state.match_ready = chosen.is_some();
    }

    pub(crate) fn choose_anime_now(&mut self, id: i64) {
        self.play.anime = Some(id);
        self.play.episode = None;
        self.publish_match();
    }

    pub(crate) fn choose_episode_now(&mut self, id: i64) {
        self.play.episode = Some(id);
        self.publish_match();
    }

    /// 按选好的那一集取弹幕；勾着“文件夹里的其他集”时，按集数把它们也记成这部番。
    pub(crate) fn confirm_match_now(&mut self) {
        let (Some(loader), Some(anime), Some(episode_id)) = (
            self.danmaku.clone(),
            self.chosen_anime().cloned(),
            self.play.episode,
        ) else {
            return;
        };
        let title = anime
            .episodes
            .iter()
            .find(|episode| episode.episode_id == episode_id)
            .map(|episode| episode.episode_title.clone())
            .unwrap_or_default();
        let choice = Choice {
            anime_id: anime.anime_id,
            anime_title: anime.anime_title.clone(),
            episode_id,
            episode_title: title,
        };
        let others: Vec<(MediaSource, Choice)> = if self.state.match_folder {
            let url = self.state.source.url.clone();
            self.play
                .episodes
                .iter()
                .filter(|file| file.url != url)
                .filter_map(|file| {
                    let number = file.episode?;
                    let found = anime
                        .episodes
                        .iter()
                        .find(|episode| episode_number(&episode.episode_title) == Some(number))?;
                    Some((
                        MediaSource::new(&file.url),
                        Choice {
                            anime_id: anime.anime_id,
                            anime_title: anime.anime_title.clone(),
                            episode_id: found.episode_id,
                            episode_title: found.episode_title.clone(),
                        },
                    ))
                })
                .collect()
        } else {
            Vec::new()
        };
        self.state.match_open = false;
        let source = self.state.source.clone();
        self.reset_danmaku(DanmakuStatus::Matching);
        let generation = self.generation;
        self.in_background(move || {
            let outcome = loader.choose(&source, &choice);
            for (other, choice) in &others {
                let _ = loader.remember(other, choice);
            }
            Arrival::Danmaku(generation, outcome)
        });
    }

    pub(crate) fn skip_danmaku_now(&mut self) {
        self.state.match_open = false;
        self.reset_danmaku(DanmakuStatus::Off);
        self.publish_play();
    }
}

/// 弹弹play 剧集名开头的集数：“第7话 …”是 7。
fn episode_number(title: &str) -> Option<u32> {
    let rest = title.trim().strip_prefix('第')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// 字幕页的每一行：名字写语言和轨道标题，下面写语言。同名的几条在下面写上轨道号好区分。
fn subtitle_rows(tracks: &[nkuplay_mpv::SubtitleTrack]) -> Vec<SubtitleRow> {
    let named: Vec<(String, Option<String>)> = tracks
        .iter()
        .map(|track| {
            let language = track.language.as_deref().map(language_name);
            let name = match (&language, &track.title) {
                (Some(language), Some(title)) if language != title => format!("{language} ({title})"),
                (Some(language), _) => language.clone(),
                (None, Some(title)) => title.clone(),
                (None, None) => format!("字幕 {}", track.id),
            };
            (name, language)
        })
        .collect();
    tracks
        .iter()
        .zip(&named)
        .map(|(track, (name, language))| {
            let same = named.iter().filter(|(other, _)| other == name).count() > 1;
            let mut note = if track.external {
                "导入的文件".to_string()
            } else {
                format!("语言：{}", language.as_deref().unwrap_or("未标明"))
            };
            if same {
                note.push_str(&format!(" · 轨道 {}", track.id));
            }
            SubtitleRow { id: track.id, name: name.clone(), note, on: track.selected }
        })
        .collect()
}

/// 语言代码的中文名字；中文分得出简繁的写简体、繁体。不认识的照原样写。
fn language_name(code: &str) -> String {
    let lower = code.to_ascii_lowercase();
    let simplified = ["hans", "chs", "sc", "zh-cn", "zh-sg", "gb"];
    let traditional = ["hant", "cht", "tc", "zh-tw", "zh-hk", "big5"];
    let has = |marks: &[&str]| marks.iter().any(|mark| lower.split(['-', '_', '.']).any(|part| part == *mark) || lower == *mark);
    let name = match lower.split(['-', '_']).next().unwrap_or("") {
        "chi" | "zho" | "zh" | "chs" | "cht" | "sc" | "tc" => {
            if has(&simplified) || lower.starts_with("zh-cn") || lower.starts_with("zh-sg") {
                "简体中文"
            } else if has(&traditional) || lower.starts_with("zh-tw") || lower.starts_with("zh-hk") {
                "繁体中文"
            } else {
                "中文"
            }
        }
        "eng" | "en" => "英文",
        "jpn" | "ja" => "日文",
        "kor" | "ko" => "韩文",
        "ind" | "id" => "印尼语",
        "may" | "msa" | "ms" => "马来语",
        "ger" | "deu" | "de" => "德语",
        "fre" | "fra" | "fr" => "法语",
        "spa" | "es" => "西班牙语",
        "por" | "pt" => "葡萄牙语",
        "ita" | "it" => "意大利语",
        "rus" | "ru" => "俄语",
        "ara" | "ar" => "阿拉伯语",
        "tha" | "th" => "泰语",
        "vie" | "vi" => "越南语",
        _ => return code.to_string(),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_rows_name_the_language_and_the_title() {
        let track = |id: i64, title: Option<&str>, language: Option<&str>, external: bool| nkuplay_mpv::SubtitleTrack {
            id,
            title: title.map(Into::into),
            language: language.map(Into::into),
            selected: id == 1,
            external,
        };
        let tracks = [
            track(1, None, Some("zh-Hans"), false),
            track(2, None, Some("zh-Hant"), false),
            track(3, Some("Subtitle track 2"), Some("eng"), false),
            track(4, None, Some("may"), false),
            track(5, None, Some("spa"), false),
            track(6, None, Some("spa"), false),
            track(7, Some("a.ass"), None, true),
        ];
        let rows = subtitle_rows(&tracks);
        assert_eq!(rows[0].name, "简体中文");
        assert_eq!(rows[0].note, "语言：简体中文");
        assert!(rows[0].on);
        assert_eq!(rows[1].name, "繁体中文");
        assert_eq!(rows[2].name, "英文 (Subtitle track 2)");
        assert_eq!(rows[3].name, "马来语");
        // 同名的写上轨道号。
        assert_eq!(rows[4].note, "语言：西班牙语 · 轨道 5");
        assert_eq!(rows[6].name, "a.ass");
        assert_eq!(rows[6].note, "导入的文件");
        assert_eq!(language_name("chi"), "中文");
    }

    #[test]
    fn chapters_named_like_openings_and_endings_can_be_skipped() {
        let chapters = vec![
            (0.0, "Prologue".to_string()),
            (40.0, "Opening".to_string()),
            (129.0, "Part A".to_string()),
            (1330.0, "ED".to_string()),
            (1423.0, "Preview".to_string()),
        ];
        let found = parts(&chapters, 1440.0);
        assert_eq!(
            found,
            [
                Part {
                    start: 40.0,
                    end: 129.0,
                    outro: false
                },
                Part {
                    start: 1330.0,
                    end: 1423.0,
                    outro: true
                },
            ]
        );
    }

    #[test]
    fn heat_is_scaled_to_the_busiest_part() {
        let comments: Vec<Comment> = [1.0, 1.0, 1.0, 50.0]
            .iter()
            .enumerate()
            .map(|(id, time)| Comment {
                id: id as i64,
                time: *time,
                mode: CommentMode::Scroll,
                color: 0xFFFFFF,
                user: String::new(),
                text: "x".into(),
            })
            .collect();
        let heat = heat_of(&comments, 120.0);
        assert_eq!(heat.len(), HEAT_BINS);
        assert_eq!(heat[1], 1.0);
        assert!(heat[50] > 0.0 && heat[50] < 1.0);
        assert!(heat_of(&comments, 0.0).is_empty());
    }

    #[test]
    fn danmaku_files_in_xml_and_json_are_read() {
        let xml =
            r#"<i><d p="2.5,1,25,16777215,0,0,0,0">第二</d><d p="1.0,5,25,255">顶&amp;部</d></i>"#;
        let comments = parse_danmaku_file(xml);
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].text, "顶&部");
        assert_eq!(comments[0].mode, CommentMode::Top);
        assert_eq!(comments[0].color, 255);
        let json = r#"{"count":1,"comments":[{"cid":1,"p":"3.2,4,16711680,u","m":"底"}]}"#;
        let comments = parse_danmaku_file(json);
        assert_eq!(comments[0].mode, CommentMode::Bottom);
        assert_eq!(comments[0].color, 0xFF0000);
    }

    #[test]
    fn texts_are_written_as_the_design_shows_them() {
        assert_eq!(speed_text(1.0), "1.0×");
        assert_eq!(speed_text(1.25), "1.25×");
        assert_eq!(shift_text(0.0), "0.0 秒");
        assert_eq!(shift_text(-1.5), "-1.5 秒");
        assert_eq!(clock(129.0), "2:09");
        assert_eq!(episode_number("第12话 告别"), Some(12));
    }
}
