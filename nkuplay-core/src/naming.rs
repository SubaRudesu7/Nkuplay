//! 从视频文件名里认出作品名和集数，用来把媒体库里同一部番的剧集归到一起。
//!
//! 只看文件名，不联网。常见的写法：
//!
//! - `[ANi] 再見菈菈 - 11 [1080P][Baha][WEB-DL][AAC AVC][CHT]`
//! - `[Nix-Raws] Sayonara Lara S01E06 [CR WEB-DL 1080p AVC AAC][SC_TC]`
//! - `[Skymoon] 學生會也有洞！ [01][CHS]`
//! - `Sayonara.Lara.E06.1080p`、`第06话`
//!
//! 开头的字幕组、末尾的画质和来源标签（方括号里的）去掉，剩下集数前面的部分当作品名。
//! 认不出集数时整个文件名（去掉标签）当作品名，集数为空。

use regex::Regex;
use std::sync::LazyLock;

/// 从一个文件名里认出的东西。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// 作品名；认不出时为空。
    pub title: String,
    /// 第几集；认不出时为空。
    pub episode: Option<u32>,
}

/// 集数的几种写法，按可信程度排列。每个模式的第一组是集数。
static EPISODE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // S01E06
        r"(?i)\bS\d{1,2}\s*E(\d{1,4})\b",
        // 第06话、第 6 集
        r"第\s*(\d{1,4})\s*[话話集]",
        // - 11、- 11v2、- 11.5（取整数部分）
        r"\s-\s+(\d{1,4})(?:\.\d)?(?:v\d)?(?:\s|\[|\(|$)",
        // [01]、[01v2]
        r"\[(\d{1,4})(?:v\d)?\]",
        // EP06、E06（前后不是字母）
        r"(?i)(?:^|[^a-z])EP?(\d{1,4})(?:[^a-z0-9]|$)",
        // 整个文件名就是集数，例如 `01`：作品名要从文件夹名里找。
        r"^\s*(\d{1,4})(?:v\d)?\s*$",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("a valid pattern"))
    .collect()
});

/// 方括号、全角方括号里的标签。
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[[^\]]*\]|【[^】]*】").expect("a valid pattern"));

/// 末尾的季数标记，例如 `S01`、`Season 2`；同一季的剧集文件名里有的带、有的不带。
static SEASON_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s+(?:S\d{1,2}|Season\s*\d{1,2})$").expect("a valid pattern")
});

/// 认出文件名 `stem`（不带扩展名）里的作品名和集数。
pub fn parse(stem: &str) -> Parsed {
    let found = EPISODE.iter().find_map(|pattern| {
        let captures = pattern.captures(stem)?;
        let number = captures.get(1)?;
        Some((
            captures.get(0)?.start(),
            number.as_str().parse::<u32>().ok()?,
        ))
    });
    let (before, episode) = match found {
        Some((at, number)) => (&stem[..at], Some(number)),
        None => (stem, None),
    };
    let mut title = clean(&TAG.replace_all(before, " "));
    if title.is_empty() {
        // 作品名也在方括号里，例如 `[字幕组][作品名][01]`：取第一个不是字幕组的标签。
        title = TAG
            .find_iter(before)
            .skip(1)
            .map(|tag| {
                clean(&tag.as_str()[tag.as_str().chars().next().map_or(0, char::len_utf8)..])
            })
            .find(|text| !text.is_empty())
            .unwrap_or_default();
    }
    Parsed { title, episode }
}

/// 去掉首尾的空白和分隔符；没有空格、用点或下划线分词的写法换成空格；去掉末尾的季数。
fn clean(text: &str) -> String {
    let mut text = text
        .trim_matches(|c: char| c.is_whitespace() || "-_.~·|]】".contains(c))
        .to_string();
    if !text.contains(' ') && (text.contains('.') || text.contains('_')) {
        text = text.replace(['.', '_'], " ");
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    SEASON_TAIL.replace(&collapsed, "").trim().to_string()
}

/// 归类用的键：作品名去掉标点和空白、转成小写。写法略有不同的同一部番归到一起。
pub fn key(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(stem: &str, title: &str, episode: Option<u32>) {
        assert_eq!(
            parse(stem),
            Parsed {
                title: title.into(),
                episode
            },
            "{stem}"
        );
    }

    #[test]
    fn common_release_names_give_the_title_and_episode() {
        check(
            "[ANi] 再見菈菈 - 11 [1080P][Baha][WEB-DL][AAC AVC][CHT]",
            "再見菈菈",
            Some(11),
        );
        check(
            "[ANi] Re：從零開始的異世界生活 第四季 - 14 [1080P][Baha][WEB-DL][AAC AVC][CHT]",
            "Re：從零開始的異世界生活 第四季",
            Some(14),
        );
        check(
            "[hyakuhuyu&LoliHouse] Re Zero kara Hajimeru Isekai Seikatsu - 78 [WebRip 1080p HEVC-10bit AAC ASSx2]",
            "Re Zero kara Hajimeru Isekai Seikatsu",
            Some(78),
        );
        check(
            "[Nix-Raws] Sayonara Lara S01E06 [CR WEB-DL 1080p AVC AAC][SC_TC]",
            "Sayonara Lara",
            Some(6),
        );
        check(
            "[Skymoon] 學生會也有洞！ [01][CHS]",
            "學生會也有洞！",
            Some(1),
        );
        check(
            "[Skymoon-Raws] Kamiina Botan, Yoeru Sugata wa Yuri no Hana - 06 [ViuTV][WEB-DL][CHT][SRT][1080p][AVC AAC]",
            "Kamiina Botan, Yoeru Sugata wa Yuri no Hana",
            Some(6),
        );
        check(
            "[Prejudice-Studio] BanG Dream！梦想妄想世界 BanG Dream！YUME∞MITA - 05 [Bilibili WEB-DL 1080P AVC 8Bit AAC MP4][简体内嵌][VideoVer]",
            "BanG Dream！梦想妄想世界 BanG Dream！YUME∞MITA",
            Some(5),
        );
    }

    #[test]
    fn other_ways_of_writing_the_episode() {
        check("Sayonara.Lara.E06.1080p", "Sayonara Lara", Some(6));
        check("Sayonara_Lara_EP07", "Sayonara Lara", Some(7));
        check("再见菈菈 第06话", "再见菈菈", Some(6));
        check("[Group][再见菈菈][08][1080P]", "再见菈菈", Some(8));
        check(
            "[LoliHouse] Yani Neko - 08v2 [WebRip 1080p]",
            "Yani Neko",
            Some(8),
        );
        check("Yani Neko S01 - 09", "Yani Neko", Some(9));
    }

    #[test]
    fn a_name_without_an_episode_is_all_title() {
        check("[Group] 剧场版 某作品 [1080P]", "剧场版 某作品", None);
        check("oobe-intro", "oobe-intro", None);
    }

    #[test]
    fn a_bare_number_is_an_episode_without_a_title() {
        check("01", "", Some(1));
        check("12v2", "", Some(12));
    }

    #[test]
    fn keys_ignore_case_spacing_and_punctuation() {
        assert_eq!(key("Yani Neko"), key("yani-neko"));
        assert_eq!(key("再見菈菈"), "再見菈菈");
        assert_ne!(key("Yani Neko"), key("Yani Neko 2"));
    }
}
