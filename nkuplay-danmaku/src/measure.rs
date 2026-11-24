//! 量一段文字有多宽，单位是“字号”：宽度乘以字号就是画出来的像素宽。
//!
//! 弹幕画在 mpv 的 OSD 上，由 libass 排字。libass 照 Windows GDI 的做法解释字号：
//! 字号是字体的 `usWinAscent + usWinDescent` 那么高，而不是 1 em。所以一个字的宽度是
//! `前进宽度 / (usWinAscent + usWinDescent) × 字号`。这里读同一个字体文件，按同样的比例算，
//! 排版时算的宽度就和屏幕上的一致，同一行的弹幕不会因为估宽不准而叠在一起。
//!
//! 字体里没有的字（比如表情）按“中日韩和全角一个字号的 0.9 倍宽、其余一半”估算；
//! 字体文件读不到时全部这样估算。

use std::sync::OnceLock;
use ttf_parser::Face;

/// 一个读出来的字体，以及它的“字号高度”（`usWinAscent + usWinDescent`，字体单位）。
struct Font {
    face: Face<'static>,
    cell: f64,
}

/// 量文字宽度的工具。
pub struct Measure {
    font: Option<&'static Font>,
}

/// 弹幕可以选的字体：交给 libass 的名字和 Windows 字体目录里的文件。
pub const FONTS: [(&str, &str); 4] = [
    ("Microsoft YaHei", "msyh.ttc"),
    ("SimHei", "simhei.ttf"),
    ("DengXian", "Deng.ttf"),
    ("SimSun", "simsun.ttc"),
];

/// 读过的字体，每种在进程里只读一次。
static LOADED: [OnceLock<Option<Font>>; FONTS.len()] = [const { OnceLock::new() }; FONTS.len()];

impl Measure {
    /// 用 Windows 自带的微软雅黑量；读不到就估算。字体文件只在第一次调用时读。
    pub fn system() -> Self {
        Self::for_font(FONTS[0].0)
    }

    /// 用 [`FONTS`] 里名叫 `name` 的字体量；不认识的名字用微软雅黑，读不到就估算。
    pub fn for_font(name: &str) -> Self {
        let index = FONTS
            .iter()
            .position(|(font, _)| *font == name)
            .unwrap_or(0);
        let font = LOADED[index].get_or_init(|| {
            let windows = std::env::var_os("WINDIR")?;
            let path = std::path::Path::new(&windows)
                .join("Fonts")
                .join(FONTS[index].1);
            let data = std::fs::read(path).ok()?;
            // 字体在整个进程里都要用，留着不释放。
            Font::parse(Box::leak(data.into_boxed_slice()))
        });
        Self {
            font: font.as_ref(),
        }
    }

    /// 不读字体，全部估算。测试用，结果不受这台电脑装了什么字体影响。
    pub fn estimate() -> Self {
        Self { font: None }
    }

    /// 是否读到了字体。
    pub fn has_font(&self) -> bool {
        self.font.is_some()
    }

    /// `text` 有多宽，单位是字号。
    pub fn width(&self, text: &str) -> f64 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    fn advance(&self, c: char) -> f64 {
        if let Some(font) = self.font
            && let Some(glyph) = font.face.glyph_index(c)
            && let Some(advance) = font.face.glyph_hor_advance(glyph)
        {
            return f64::from(advance) / font.cell;
        }
        if is_wide(c) { 0.9 } else { 0.5 }
    }
}

impl Font {
    /// 读字体集合里的第一个字体。
    fn parse(data: &'static [u8]) -> Option<Self> {
        let face = Face::parse(data, 0).ok()?;
        let cell = face
            .tables()
            .os2
            .map(|os2| f64::from(os2.windows_ascender()) + f64::from(os2.windows_descender()).abs())
            .filter(|cell| *cell > 0.0)
            .unwrap_or_else(|| f64::from(face.ascender()) - f64::from(face.descender()));
        (cell > 0.0).then_some(Self { face, cell })
    }
}

/// 中日韩文字、全角符号和表情，大致是一个字号宽。
fn is_wide(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF
        | 0x20000..=0x3FFFD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_give_wide_characters_about_one_size_and_others_half() {
        let measure = Measure::estimate();
        assert!((measure.width("弹幕") - 1.8).abs() < 1e-9);
        assert!((measure.width("ab") - 1.0).abs() < 1e-9);
        assert!((measure.width("ｗ😀") - 1.8).abs() < 1e-9);
        assert_eq!(measure.width(""), 0.0);
    }

    #[test]
    fn the_system_font_measures_chinese_wider_than_latin() {
        let measure = Measure::system();
        if !measure.has_font() {
            // 这台电脑没有微软雅黑（不是 Windows）：只能估算，没什么可比。
            return;
        }
        let chinese = measure.width("中");
        let latin = measure.width("i");
        assert!(chinese > 0.5 && chinese < 1.0, "{chinese}");
        assert!(latin < chinese / 2.0, "{latin}");
        // 同一段文字量两次结果一样。
        assert_eq!(measure.width("再见菈菈"), measure.width("再见菈菈"));
    }
}
