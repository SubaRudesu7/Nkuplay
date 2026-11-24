//! 设置：存在数据文件夹的 `settings.json` 里，读不出来或缺的项用默认值。
//!
//! 协议里的类型没有序列化，所以存盘时枚举写成名字（`"dark"`），按键写成 Bern 的写法
//! （`"shift+right"`），读回来再换成协议的类型。

use nkuplay_protocol::nkuplay::{
    AfterEnd, Amount, Appearance, Aspect, DanmakuFont, Fill, KeyAction, Keys, LibrarySort, Page,
    Section, SkipMode, Toggle, Upscale,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const FILE: &str = "settings.json";

/// 存盘的设置。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: String,
    pub start_page: String,
    /// 顶栏的页面，按顺序，各自开没开。
    pub nav: Vec<(String, bool)>,
    pub resume: bool,
    pub auto_fullscreen: bool,
    pub after_end: String,
    pub countdown: f64,
    pub skip_mode: String,
    pub skip_seconds: f64,
    pub seek_step: i64,
    pub hold_speed: i64,
    pub keep_pitch: bool,
    pub bar_stay: i64,
    pub chapter_marks: bool,
    pub heat_curve: bool,
    pub hover_preview: bool,
    pub hwdec: bool,
    pub cache_mb: f64,
    pub upscale: String,
    pub aspect: String,
    pub fill: String,
    pub d_opacity: f64,
    pub d_size: f64,
    pub d_outline: f64,
    pub d_font: String,
    pub d_area: f64,
    pub d_seconds: f64,
    pub d_gap: f64,
    pub d_overlap: bool,
    pub d_scroll: bool,
    pub d_top: bool,
    pub d_bottom: bool,
    pub d_colored: bool,
    pub d_merge: bool,
    pub blocked: Vec<String>,
    /// 操作的名字到按键；缺的用默认。
    pub keys: BTreeMap<String, String>,
    /// 媒体库的排序。
    pub library_sort: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: "system".into(),
            start_page: "home".into(),
            nav: PAGES.iter().map(|page| (page.to_string(), true)).collect(),
            resume: true,
            auto_fullscreen: false,
            after_end: "next".into(),
            countdown: 10.0,
            skip_mode: "button".into(),
            skip_seconds: 90.0,
            seek_step: 5,
            hold_speed: 20,
            keep_pitch: true,
            bar_stay: 1,
            chapter_marks: true,
            heat_curve: true,
            hover_preview: true,
            hwdec: true,
            cache_mb: 150.0,
            upscale: "off".into(),
            aspect: "auto".into(),
            fill: "fit".into(),
            d_opacity: 70.0,
            d_size: 34.0,
            d_outline: 2.0,
            d_font: "yahei".into(),
            d_area: 85.0,
            d_seconds: 8.0,
            d_gap: 15.0,
            d_overlap: false,
            d_scroll: true,
            d_top: true,
            d_bottom: true,
            d_colored: true,
            d_merge: false,
            blocked: Vec::new(),
            keys: BTreeMap::new(),
            library_sort: "added".into(),
        }
    }
}

/// 顶栏能放的页面，默认的顺序。
const PAGES: [&str; 4] = ["home", "library", "schedule", "history"];

/// 读存下的设置；没有或读不出来时是默认的。
pub fn load(folder: Option<&PathBuf>) -> Settings {
    let mut settings: Settings = folder
        .and_then(|folder| std::fs::read(folder.join(FILE)).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    settings.tidy();
    settings
}

/// 存下设置；没有数据文件夹时不存。
pub fn save(folder: Option<&PathBuf>, settings: &Settings) {
    let Some(folder) = folder else {
        return;
    };
    let _ = std::fs::create_dir_all(folder);
    if let Ok(bytes) = serde_json::to_vec_pretty(settings) {
        let _ = std::fs::write(folder.join(FILE), bytes);
    }
}

impl Settings {
    /// 去掉不认识和重复的顶栏页面，补上缺的（关着放在最后），至少开一页。
    fn tidy(&mut self) {
        let mut nav: Vec<(String, bool)> = Vec::new();
        for (page, on) in &self.nav {
            if PAGES.contains(&page.as_str()) && !nav.iter().any(|(seen, _)| seen == page) {
                nav.push((page.clone(), *on));
            }
        }
        for page in PAGES {
            if !nav.iter().any(|(seen, _)| seen == page) {
                nav.push((page.to_string(), false));
            }
        }
        if !nav.iter().any(|(_, on)| *on) {
            nav[0].1 = true;
        }
        self.nav = nav;
    }

    /// 顶栏显示或不显示这一页；最后一页不能关。
    pub fn show_page(&mut self, page: &Page, on: bool) {
        let name = page_name(page);
        let open = self.nav.iter().filter(|(_, shown)| *shown).count();
        if let Some(entry) = self.nav.iter_mut().find(|(seen, _)| seen == name)
            && (on || !entry.1 || open > 1)
        {
            entry.1 = on;
        }
    }

    /// 把第 `from` 个页面挪到第 `to` 个。
    pub fn move_page(&mut self, from: i64, to: i64) {
        let count = self.nav.len() as i64;
        if !(0..count).contains(&from) {
            return;
        }
        let item = self.nav.remove(from as usize);
        let to = to.clamp(0, count - 1) as usize;
        self.nav.insert(to, item);
    }

    pub fn toggle(&mut self, toggle: &Toggle) -> &mut bool {
        match toggle {
            Toggle::Resume => &mut self.resume,
            Toggle::AutoFullscreen => &mut self.auto_fullscreen,
            Toggle::KeepPitch => &mut self.keep_pitch,
            Toggle::Chapters => &mut self.chapter_marks,
            Toggle::Heat => &mut self.heat_curve,
            Toggle::Preview => &mut self.hover_preview,
            Toggle::Hwdec => &mut self.hwdec,
            Toggle::Overlap => &mut self.d_overlap,
            Toggle::Scroll => &mut self.d_scroll,
            Toggle::Top => &mut self.d_top,
            Toggle::Bottom => &mut self.d_bottom,
            Toggle::Colored => &mut self.d_colored,
            Toggle::Merge => &mut self.d_merge,
        }
    }

    /// 设一个数值，限制在它的范围里、按它的步长取整；不是数时不变。
    pub fn set_amount(&mut self, amount: &Amount, value: f64) {
        if !value.is_finite() {
            return;
        }
        let snap = |value: f64, low: f64, high: f64, step: f64| {
            (((value.clamp(low, high) - low) / step).round() * step + low).clamp(low, high)
        };
        match amount {
            Amount::Countdown => self.countdown = snap(value, 3.0, 30.0, 1.0),
            Amount::SkipSeconds => self.skip_seconds = snap(value, 10.0, 300.0, 5.0),
            Amount::SeekStep => self.seek_step = snap(value, 1.0, 30.0, 1.0) as i64,
            Amount::HoldSpeed => self.hold_speed = snap(value, 15.0, 40.0, 5.0) as i64,
            Amount::BarStay => self.bar_stay = snap(value, 1.0, 10.0, 1.0) as i64,
            Amount::Cache => self.cache_mb = snap(value, 32.0, 1024.0, 2.0),
            Amount::Opacity => self.d_opacity = snap(value, 10.0, 100.0, 1.0),
            Amount::Size => self.d_size = snap(value, 18.0, 64.0, 1.0),
            Amount::Outline => self.d_outline = snap(value, 0.0, 4.0, 0.5),
            Amount::Area => self.d_area = snap(value, 25.0, 100.0, 5.0),
            Amount::ScrollTime => self.d_seconds = snap(value, 4.0, 16.0, 0.5),
            Amount::Gap => self.d_gap = snap(value, 0.0, 50.0, 1.0),
        }
    }

    /// 加一个屏蔽词；空的、重复的不加。返回加没加。
    pub fn add_blocked(&mut self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || self.blocked.iter().any(|word| word == text) {
            return false;
        }
        self.blocked.push(text.to_string());
        true
    }

    /// 一个操作的按键：设过的，否则默认的。
    pub fn key(&self, action: &KeyAction) -> String {
        let name = key_name(action);
        self.keys
            .get(name)
            .cloned()
            .unwrap_or_else(|| default_key(action).to_string())
    }

    /// 换一个操作的按键；和默认一样就不另外记。
    pub fn set_key(&mut self, action: &KeyAction, keys: &str) {
        let name = key_name(action).to_string();
        let keys = keys.trim().to_ascii_lowercase();
        if keys == default_key(action) {
            self.keys.remove(&name);
        } else {
            self.keys.insert(name, keys);
        }
    }

    /// 这一类恢复默认。
    pub fn reset(&mut self, section: &Section) {
        let fresh = Settings::default();
        match section {
            Section::Play => {
                self.resume = fresh.resume;
                self.auto_fullscreen = fresh.auto_fullscreen;
                self.after_end = fresh.after_end;
                self.countdown = fresh.countdown;
                self.skip_mode = fresh.skip_mode;
                self.skip_seconds = fresh.skip_seconds;
                self.seek_step = fresh.seek_step;
                self.hold_speed = fresh.hold_speed;
                self.keep_pitch = fresh.keep_pitch;
                self.bar_stay = fresh.bar_stay;
                self.chapter_marks = fresh.chapter_marks;
                self.heat_curve = fresh.heat_curve;
                self.hover_preview = fresh.hover_preview;
                self.hwdec = fresh.hwdec;
                self.cache_mb = fresh.cache_mb;
            }
            Section::Picture => {
                self.upscale = fresh.upscale;
                self.aspect = fresh.aspect;
                self.fill = fresh.fill;
            }
            // 屏蔽词是用户自己加的，不算“设置”，留着。
            Section::Danmaku => {
                self.d_opacity = fresh.d_opacity;
                self.d_size = fresh.d_size;
                self.d_outline = fresh.d_outline;
                self.d_font = fresh.d_font;
                self.d_area = fresh.d_area;
                self.d_seconds = fresh.d_seconds;
                self.d_gap = fresh.d_gap;
                self.d_overlap = fresh.d_overlap;
                self.d_scroll = fresh.d_scroll;
                self.d_top = fresh.d_top;
                self.d_bottom = fresh.d_bottom;
                self.d_colored = fresh.d_colored;
                self.d_merge = fresh.d_merge;
            }
            Section::Keys => self.keys.clear(),
        }
    }

    /// 界面上的按键表。
    pub fn keys(&self) -> Keys {
        Keys {
            play: self.key(&KeyAction::Play),
            rewind: self.key(&KeyAction::Rewind),
            forward: self.key(&KeyAction::Forward),
            previous: self.key(&KeyAction::Previous),
            next: self.key(&KeyAction::Next),
            skip: self.key(&KeyAction::Skip),
            faster: self.key(&KeyAction::Faster),
            slower: self.key(&KeyAction::Slower),
            normal_speed: self.key(&KeyAction::NormalSpeed),
            // 长按快进就是按住快进键。
            hold: self.key(&KeyAction::Forward),
            hold_label: key_label(&self.key(&KeyAction::Forward)),
            louder: self.key(&KeyAction::Louder),
            quieter: self.key(&KeyAction::Quieter),
            mute: self.key(&KeyAction::Mute),
            danmaku: self.key(&KeyAction::Danmaku),
            danmaku_panel: self.key(&KeyAction::DanmakuPanel),
            subtitle: self.key(&KeyAction::Subtitle),
            audio: self.key(&KeyAction::Audio),
            fullscreen: self.key(&KeyAction::Fullscreen),
            exit_fullscreen: self.key(&KeyAction::ExitFullscreen),
            episodes: self.key(&KeyAction::Episodes),
            info: self.key(&KeyAction::Info),
        }
    }
}

/// 名字和协议的枚举互相换：`$value => "name"`，第一项是读不出来时的值。
macro_rules! names {
    ($to:ident, $from:ident, $ty:ident { $($variant:ident => $name:literal),+ $(,)? }) => {
        pub fn $to(value: &$ty) -> &'static str {
            match value {
                $($ty::$variant => $name),+
            }
        }

        pub fn $from(name: &str) -> $ty {
            match name {
                $($name => $ty::$variant,)+
                _ => names!(@first $ty $($variant)+),
            }
        }
    };
    (@first $ty:ident $first:ident $($rest:ident)*) => { $ty::$first };
}

names!(appearance_name, appearance_of, Appearance { System => "system", Light => "light", Dark => "dark" });
names!(page_name, page_of, Page { Home => "home", Library => "library", Schedule => "schedule", History => "history" });
names!(after_end_name, after_end_of, AfterEnd { Next => "next", Loop => "loop", Stop => "stop", Back => "back" });
names!(skip_mode_name, skip_mode_of, SkipMode { Button => "button", Off => "off", Auto => "auto" });
names!(upscale_name, upscale_of, Upscale { Off => "off", Light => "light", Standard => "standard", High => "high" });
names!(aspect_name, aspect_of, Aspect { Auto => "auto", Wide => "16:9", Classic => "4:3", Cinema => "21:9", Stretch => "stretch" });
names!(fill_name, fill_of, Fill { Fit => "fit", Crop => "crop" });
names!(library_sort_name, library_sort_of, LibrarySort { Added => "added", Name => "name", Watched => "watched" });
names!(font_name, font_of, DanmakuFont { YaHei => "yahei", Hei => "hei", Deng => "deng", Song => "song" });

/// 弹幕字体交给 libass 的名字。
pub fn font_family(font: &DanmakuFont) -> &'static str {
    match font {
        DanmakuFont::YaHei => "Microsoft YaHei",
        DanmakuFont::Hei => "SimHei",
        DanmakuFont::Deng => "DengXian",
        DanmakuFont::Song => "SimSun",
    }
}

fn key_name(action: &KeyAction) -> &'static str {
    match action {
        KeyAction::Play => "play",
        KeyAction::Rewind => "rewind",
        KeyAction::Forward => "forward",
        KeyAction::Previous => "previous",
        KeyAction::Next => "next",
        KeyAction::Skip => "skip",
        KeyAction::Faster => "faster",
        KeyAction::Slower => "slower",
        KeyAction::NormalSpeed => "normal_speed",
        KeyAction::Hold => "forward",
        KeyAction::Louder => "louder",
        KeyAction::Quieter => "quieter",
        KeyAction::Mute => "mute",
        KeyAction::Danmaku => "danmaku",
        KeyAction::DanmakuPanel => "danmaku_panel",
        KeyAction::Subtitle => "subtitle",
        KeyAction::Audio => "audio",
        KeyAction::Fullscreen => "fullscreen",
        KeyAction::ExitFullscreen => "exit_fullscreen",
        KeyAction::Episodes => "episodes",
        KeyAction::Info => "info",
    }
}

/// 键帽上写的键：方向键写箭头，空格写“空格”，字母大写；带修饰键的用“+”连起来。
fn key_label(keys: &str) -> String {
    keys.split('+')
        .map(|key| match key {
            "left" => "←".to_string(),
            "right" => "→".to_string(),
            "up" => "↑".to_string(),
            "down" => "↓".to_string(),
            "space" => "空格".to_string(),
            "shift" => "Shift".to_string(),
            "cmd" | "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn default_key(action: &KeyAction) -> &'static str {
    match action {
        KeyAction::Play => "space",
        KeyAction::Rewind => "left",
        KeyAction::Forward | KeyAction::Hold => "right",
        KeyAction::Previous => "shift+left",
        KeyAction::Next => "shift+right",
        KeyAction::Skip => "s",
        KeyAction::Faster => "]",
        KeyAction::Slower => "[",
        KeyAction::NormalSpeed => "delete",
        KeyAction::Louder => "up",
        KeyAction::Quieter => "down",
        KeyAction::Mute => "m",
        KeyAction::Danmaku => "d",
        KeyAction::DanmakuPanel => "c",
        KeyAction::Subtitle => "v",
        KeyAction::Audio => "a",
        KeyAction::Fullscreen => "f",
        KeyAction::ExitFullscreen => "escape",
        KeyAction::Episodes => "l",
        KeyAction::Info => "i",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_unknown_entries_fall_back_to_the_defaults() {
        let dir = std::env::temp_dir().join(format!("nkuplay-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"appearance":"dark","nav":[["library",true],["bogus",true],["library",false]],"seek_step":10}"#,
        )
        .unwrap();
        let settings = load(Some(&dir));
        assert_eq!(appearance_of(&settings.appearance), Appearance::Dark);
        assert_eq!(settings.seek_step, 10);
        assert!(settings.resume);
        let pages: Vec<_> = settings
            .nav
            .iter()
            .map(|(page, on)| (page.as_str(), *on))
            .collect();
        assert_eq!(
            pages,
            [
                ("library", true),
                ("home", false),
                ("schedule", false),
                ("history", false)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_last_page_stays_on_and_pages_move() {
        let mut settings = Settings::default();
        for page in [Page::Home, Page::Library, Page::Schedule, Page::History] {
            settings.show_page(&page, false);
        }
        assert_eq!(settings.nav.iter().filter(|(_, on)| *on).count(), 1);
        settings.move_page(3, 0);
        assert_eq!(settings.nav[0].0, "history");
    }

    #[test]
    fn amounts_snap_to_their_steps_and_ranges() {
        let mut settings = Settings::default();
        settings.set_amount(&Amount::SkipSeconds, 93.0);
        assert_eq!(settings.skip_seconds, 95.0);
        settings.set_amount(&Amount::Outline, 9.0);
        assert_eq!(settings.d_outline, 4.0);
        settings.set_amount(&Amount::HoldSpeed, f64::NAN);
        assert_eq!(settings.hold_speed, 20);
    }

    #[test]
    fn keys_keep_only_what_differs_and_reset_keeps_blocked_words() {
        let mut settings = Settings::default();
        settings.set_key(&KeyAction::Mute, "N");
        settings.set_key(&KeyAction::Play, "space");
        assert_eq!(settings.keys.len(), 1);
        assert_eq!(settings.keys().mute, "n");
        settings.add_blocked(" 剧透 ");
        assert!(!settings.add_blocked("剧透"));
        settings.d_size = 50.0;
        settings.reset(&Section::Danmaku);
        assert_eq!(settings.d_size, 34.0);
        assert_eq!(settings.blocked, ["剧透"]);
        settings.reset(&Section::Keys);
        assert_eq!(settings.keys().mute, "m");
    }
}
