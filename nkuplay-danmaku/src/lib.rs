//! 弹幕的排版和显示文本：纯 Rust，不依赖播放器，可以单测。
//!
//! 用法分三步：
//!
//! 1. [`Track::new`]：把一集的弹幕整理一遍：去掉屏蔽词和空弹幕，加上时间偏移，
//!    （可选）合并短时间内的重复弹幕，量好每条的宽度。
//! 2. [`Layout::new`]：按画面的宽高比给每条弹幕分一行。滚动弹幕从右往左，同一行里后一条
//!    不会碰上前一条；顶部、底部弹幕各自从上往下、从下往上排。哪一行都放不下的丢掉。
//!    这一步只依赖画面宽高比，窗口尺寸变了才重新算。
//! 3. [`Layout::events`]：给出某一刻屏上所有弹幕的 ASS 文本，交给 mpv 的 `osd-overlay`
//!    （`format=ass-events`）。位置完全由时间算出，所以暂停、跳转、倍速都不用另外处理。
//!
//! 坐标系：画布高 [`CANVAS_HEIGHT`]，宽按画面宽高比算，交给 mpv 时用同样的 `res_x`/`res_y`。
//! 思路参考了 NipaPlay（MIT）的 DFM+ 弹幕内核和 uosc_danmaku（MIT）的 `render.lua`：
//! 位置由播放时间算出、按时间窗口取屏上的弹幕、用真实字体量宽度。没有照抄它们的代码。

mod clock;
mod layout;
mod measure;

pub use clock::Clock;
pub use layout::Layout;
pub use measure::{FONTS, Measure};

use std::collections::HashMap;

/// 画布的高。字号、描边等都按这个高度给。
pub const CANVAS_HEIGHT: f64 = 1080.0;

/// 弹幕显示的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// 从右往左滚过画面。
    Scroll,
    /// 停在上方。
    Top,
    /// 停在下方。
    Bottom,
}

/// 一条弹幕。
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    /// 视频开始后第几秒出现（还没加时间偏移）。
    pub time: f64,
    pub mode: Mode,
    /// `0xRRGGBB`。
    pub color: u32,
    pub text: String,
}

/// 弹幕怎么显示。默认值见 [`Config::default`]。
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// 字体名，交给 libass；量宽度用的字体要和它一致（见 [`Measure::system`]）。
    pub font: String,
    /// 字号，按 [`CANVAS_HEIGHT`] 高的画布算。
    pub font_size: f64,
    /// 描边粗细。
    pub outline: f64,
    /// 不透明度，0 到 1。
    pub opacity: f64,
    /// 滚动弹幕从右边进来到左边完全出去用几秒。
    pub scroll_seconds: f64,
    /// 顶部、底部弹幕停留几秒。
    pub fixed_seconds: f64,
    /// 弹幕只占画面上方的这个比例，下面留给字幕。
    pub area: f64,
    /// 行与行之间的空隙，按字号的比例。
    pub lane_gap: f64,
    /// 每种弹幕最多几行；为空时排满 `area`。
    pub max_lanes: Option<usize>,
    /// 这么多秒内内容相同的弹幕合成一条，后面写上“×次数”；为空时不合并。
    pub merge_within: Option<f64>,
    /// 含有其中任何一个词的弹幕不显示；写成 `/…/` 的是正则表达式。
    pub blocked: Vec<String>,
    /// 显示哪几类：滚动、顶部、底部。
    pub scroll: bool,
    pub top: bool,
    pub bottom: bool,
    /// 为假时彩色弹幕也画成白色。
    pub colored: bool,
    /// 哪一行都放不下时叠在最早空出来的那一行上，而不是丢掉。
    pub overlap: bool,
}

impl Default for Config {
    /// 字号 34（1080 高的画面上约 3%）、描边 2、不透明度 0.7、滚动 8 秒、顶部底部停 5 秒、
    /// 占上方 85%、行距 0.15 个字号、不合并、不屏蔽。
    fn default() -> Self {
        Self {
            font: "Microsoft YaHei".into(),
            font_size: 34.0,
            outline: 2.0,
            opacity: 0.7,
            scroll_seconds: 8.0,
            fixed_seconds: 5.0,
            area: 0.85,
            lane_gap: 0.15,
            max_lanes: None,
            merge_within: None,
            blocked: Vec::new(),
            scroll: true,
            top: true,
            bottom: true,
            colored: true,
            overlap: false,
        }
    }
}

/// 整理好的一条弹幕。
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// 加上时间偏移以后出现的时间。
    time: f64,
    mode: Mode,
    /// 画出来多宽（像素，含描边）。
    width: f64,
    /// ASS 里位置之后的部分：样式和文字，例如 `\fs34…}文字`。
    styled: String,
}

/// 一集整理好的弹幕，按出现时间排好。
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    entries: Vec<Entry>,
    config: Config,
}

impl Track {
    /// 整理一集的弹幕。`shift` 是弹弹play 给的时间偏移：弹幕要延后出现的秒数，负数是提前。
    pub fn new(comments: &[Comment], shift: f64, config: Config, measure: &Measure) -> Self {
        let shift = if shift.is_finite() { shift } else { 0.0 };
        let mut kept: Vec<(f64, Mode, u32, String, u32)> = Vec::new();
        // 合并时：每种内容最近保留的那一条在 `kept` 里的位置。
        let mut latest: HashMap<(Mode, String), usize> = HashMap::new();
        let mut sorted: Vec<&Comment> = comments
            .iter()
            .filter(|c| c.time.is_finite() && shows(&config, c.mode))
            .collect();
        sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
        let blocked = Blocked::new(&config.blocked);
        for comment in sorted {
            let text = clean(&comment.text);
            if text.is_empty() || blocked.matches(&text) {
                continue;
            }
            let time = (comment.time + shift).max(0.0);
            if let Some(window) = config.merge_within {
                let key = (comment.mode, text.clone());
                if let Some(&index) = latest.get(&key)
                    && time - kept[index].0 <= window
                {
                    kept[index].4 += 1;
                    continue;
                }
                latest.insert(key, kept.len());
            }
            let color = if config.colored {
                comment.color
            } else {
                0xFFFFFF
            };
            kept.push((time, comment.mode, color, text, 1));
        }
        let entries = kept
            .into_iter()
            .map(|(time, mode, color, text, count)| {
                let shown = if count > 1 {
                    format!("{text} ×{count}")
                } else {
                    text
                };
                Entry {
                    time,
                    mode,
                    width: measure.width(&shown) * config.font_size + config.outline * 2.0,
                    styled: styled(&shown, color, &config),
                }
            })
            .collect();
        Self { entries, config }
    }

    /// 整理后还剩几条。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// 这一类弹幕要不要显示。
fn shows(config: &Config, mode: Mode) -> bool {
    match mode {
        Mode::Scroll => config.scroll,
        Mode::Top => config.top,
        Mode::Bottom => config.bottom,
    }
}

/// 屏蔽词：普通的词按包含比较，`/…/` 按正则表达式；写错的正则当普通词。
struct Blocked {
    words: Vec<String>,
    patterns: Vec<regex::Regex>,
}

impl Blocked {
    fn new(blocked: &[String]) -> Self {
        let mut words = Vec::new();
        let mut patterns = Vec::new();
        for word in blocked
            .iter()
            .map(|word| word.trim())
            .filter(|word| !word.is_empty())
        {
            let pattern = word
                .strip_prefix('/')
                .and_then(|rest| rest.strip_suffix('/'))
                .filter(|inner| !inner.is_empty())
                .and_then(|inner| regex::Regex::new(inner).ok());
            match pattern {
                Some(pattern) => patterns.push(pattern),
                None => words.push(word.to_string()),
            }
        }
        Self { words, patterns }
    }

    fn matches(&self, text: &str) -> bool {
        self.words.iter().any(|word| text.contains(word.as_str()))
            || self.patterns.iter().any(|pattern| pattern.is_match(text))
    }
}

/// 弹幕文字放进 ASS 前的处理：换行变空格、去掉首尾空白，`\`、`{`、`}` 换成全角，
/// 免得被 libass 当成控制符。
fn clean(text: &str) -> String {
    text.trim()
        .chars()
        .map(|c| match c {
            '\\' => '＼',
            '{' => '｛',
            '}' => '｝',
            '\r' | '\n' | '\t' => ' ',
            c => c,
        })
        .collect()
}

/// ASS 的颜色写法 `&HBBGGRR&`。
fn ass_color(rgb: u32) -> String {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    format!("&H{b:02X}{g:02X}{r:02X}&")
}

/// 一条弹幕位置之后的样式和文字。深色的字用白色描边，其余用黑色描边。
fn styled(text: &str, color: u32, config: &Config) -> String {
    let (r, g, b) = ((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF);
    let luminance = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
    let border = if luminance < 60.0 { 0xFFFFFF } else { 0x000000 };
    // ASS 的透明度：0 不透明，255 全透明。
    let alpha = ((1.0 - config.opacity.clamp(0.0, 1.0)) * 255.0).round() as u8;
    format!(
        "\\fn{}\\fs{}\\bord{}\\shad0\\c{}\\3c{}\\alpha&H{alpha:02X}&}}{text}",
        config.font,
        config.font_size,
        config.outline,
        ass_color(color),
        ass_color(border),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(time: f64, mode: Mode, text: &str) -> Comment {
        Comment {
            time,
            mode,
            color: 0xFFFFFF,
            text: text.into(),
        }
    }

    #[test]
    fn blocked_and_empty_comments_are_left_out_and_the_rest_shifted_and_sorted() {
        let config = Config {
            blocked: vec!["剧透".into(), String::new()],
            ..Config::default()
        };
        let track = Track::new(
            &[
                comment(5.0, Mode::Scroll, "第二条"),
                comment(1.0, Mode::Scroll, "第一条"),
                comment(2.0, Mode::Scroll, "前方剧透"),
                comment(3.0, Mode::Top, "   "),
                comment(0.5, Mode::Scroll, "提前到零"),
            ],
            -1.0,
            config,
            &Measure::estimate(),
        );
        let times: Vec<f64> = track.entries.iter().map(|entry| entry.time).collect();
        assert_eq!(times, [0.0, 0.0, 4.0]);
        assert!(track.entries[2].styled.ends_with("}第二条"));
    }

    #[test]
    fn repeats_close_together_are_merged_when_asked() {
        let comments = [
            comment(1.0, Mode::Scroll, "awsl"),
            comment(2.0, Mode::Scroll, "awsl"),
            comment(3.0, Mode::Top, "awsl"),
            comment(4.0, Mode::Scroll, "awsl"),
            comment(30.0, Mode::Scroll, "awsl"),
        ];
        let merged = Track::new(
            &comments,
            0.0,
            Config {
                merge_within: Some(10.0),
                ..Config::default()
            },
            &Measure::estimate(),
        );
        let shown: Vec<&str> = merged
            .entries
            .iter()
            .map(|entry| entry.styled.rsplit('}').next().unwrap())
            .collect();
        assert_eq!(shown, ["awsl ×3", "awsl", "awsl"]);
        // 合并后的文字更长，宽度也跟着变。
        assert!(merged.entries[0].width > merged.entries[2].width);
        let kept = Track::new(&comments, 0.0, Config::default(), &Measure::estimate());
        assert_eq!(kept.len(), 5);
    }

    #[test]
    fn text_is_made_safe_for_ass_and_styled() {
        assert_eq!(clean(" a{\\b}c\nd "), "a｛＼b｝c d");
        assert_eq!(ass_color(0x112233), "&H332211&");
        let style = styled("字", 0xFF0000, &Config::default());
        assert_eq!(
            style,
            "\\fnMicrosoft YaHei\\fs34\\bord2\\shad0\\c&H0000FF&\\3c&H000000&\\alpha&H4D&}字"
        );
        // 黑字用白边。
        assert!(styled("字", 0x000000, &Config::default()).contains("\\3c&HFFFFFF&"));
    }

    #[test]
    fn kinds_colours_and_patterns_follow_the_config() {
        let mut red = comment(1.0, Mode::Scroll, "999");
        red.color = 0xFF0000;
        let comments = [
            red,
            comment(2.0, Mode::Top, "顶"),
            comment(3.0, Mode::Bottom, "底"),
            comment(4.0, Mode::Scroll, "99a"),
        ];
        let config = Config {
            top: false,
            colored: false,
            blocked: vec!["/^9+$/".into(), "/[/".into()],
            ..Config::default()
        };
        let track = Track::new(&comments, 0.0, config, &Measure::estimate());
        let shown: Vec<&str> = track
            .entries
            .iter()
            .map(|entry| entry.styled.rsplit('}').next().unwrap())
            .collect();
        // 顶部的不显示，纯数字的被正则挡掉；写错的正则当普通词。
        assert_eq!(shown, ["底", "99a"]);
        let plain = Track::new(
            &comments[..1],
            0.0,
            Config {
                colored: false,
                ..Config::default()
            },
            &Measure::estimate(),
        );
        assert!(plain.entries[0].styled.contains("\\c&HFFFFFF&"));
    }

    #[test]
    fn width_includes_the_outline() {
        let track = Track::new(
            &[comment(0.0, Mode::Scroll, "ab")],
            0.0,
            Config::default(),
            &Measure::estimate(),
        );
        assert!((track.entries[0].width - (34.0 + 4.0)).abs() < 1e-9);
    }
}
