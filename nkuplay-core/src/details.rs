//! 番剧详情页的数据：弹弹play 的作品详情和短评，以及把剧集和本地文件、观看进度对在一起。
//!
//! 详情和短评（第一页）一起存进缓存文件夹（`bangumi/<作品编号>.json`）：连载中的作品保留
//! [`ON_AIR_KEEP`]，其余保留 [`OTHER_KEEP`]。过期的缓存照样先显示，同时在后台重新取；
//! 取不到时继续用旧的。用户点“刷新资料”才不管缓存直接取。
//!
//! 剧集按集数对上本地文件：弹弹play 的 `episodeNumber` 是数字的是正片，和文件名里认出的集数比；
//! 特别篇（`S1`）和 OP/ED（`C1`）排在正片后面；对不上的本地文件（比如绝对集数 `- 78`）放在最后。

use crate::library::{EpisodeFile, Watched};
use nkuplay_dandanplay::{BangumiComments, BangumiDetails, Client, Credentials, Error};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

/// 连载中的作品，详情和短评保留多久。
pub const ON_AIR_KEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// 其余作品保留多久。
pub const OTHER_KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 弹弹play 的调用里详情页用到的那两个，测试时可以换成假的。
pub trait Api: Send + Sync {
    fn details(&self, anime_id: i64) -> Result<Option<BangumiDetails>, Error>;
    /// 第一页短评。
    fn comments(&self, anime_id: i64) -> Result<BangumiComments, Error>;
}

impl Api for Client {
    fn details(&self, anime_id: i64) -> Result<Option<BangumiDetails>, Error> {
        self.bangumi(anime_id)
    }

    fn comments(&self, anime_id: i64) -> Result<BangumiComments, Error> {
        self.bangumi_comments(anime_id, 0)
    }
}

/// 一部作品的详情和短评，以及取到的时间。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// 取到的时间（Unix 秒）。
    pub saved_at: u64,
    /// 弹弹play 没有这部作品时为空。
    pub details: Option<BangumiDetails>,
    pub comments: BangumiComments,
}

impl Info {
    /// 在 `now`（Unix 秒）时还算不算新。
    pub fn fresh(&self, now: u64) -> bool {
        let on_air = self
            .details
            .as_ref()
            .is_some_and(|details| details.is_on_air);
        let keep = if on_air { ON_AIR_KEEP } else { OTHER_KEEP };
        now.saturating_sub(self.saved_at) < keep.as_secs()
    }
}

/// 详情的来源：向弹弹play 取，并存进缓存文件夹。
pub struct Source {
    /// 这个构建没有弹弹play 凭证时为 `None`。
    api: Option<Box<dyn Api>>,
    /// 缓存文件夹；没有时不缓存。
    cache: Option<PathBuf>,
}

impl Source {
    pub fn new(api: Option<Box<dyn Api>>, cache: Option<PathBuf>) -> Self {
        Self { api, cache }
    }

    /// 用编译进程序的凭证，缓存放在应用的缓存文件夹里。
    pub fn built_in() -> Self {
        let api = Credentials::built_in()
            .map(|credentials| Box::new(Client::new(credentials)) as Box<dyn Api>);
        Self::new(
            api,
            bern::storage::cache_dir().map(|dir| dir.join("bangumi")),
        )
    }

    /// 能不能向弹弹play 取。
    pub fn available(&self) -> bool {
        self.api.is_some()
    }

    fn file(&self, anime_id: i64) -> Option<PathBuf> {
        Some(self.cache.as_ref()?.join(format!("{anime_id}.json")))
    }

    /// 缓存里的详情，不管新旧；没有或读不出来时为 `None`。只读一个小文件。
    pub fn cached(&self, anime_id: i64) -> Option<Info> {
        serde_json::from_slice(&std::fs::read(self.file(anime_id)?).ok()?).ok()
    }

    /// 向弹弹play 取作品 `anime_id` 的详情和第一页短评，记作 `now` 时取到，存进缓存。会阻塞。
    /// 短评取不到不算失败，当作没有。
    pub fn download(&self, anime_id: i64, now: u64) -> Result<Info, String> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| "this build has no dandanplay credentials".to_string())?;
        let details = api.details(anime_id).map_err(|error| error.to_string())?;
        let comments = api.comments(anime_id).unwrap_or_default();
        let info = Info {
            saved_at: now,
            details,
            comments,
        };
        if let Some(path) = self.file(anime_id) {
            // 缓存写不进去不影响这次的结果。
            if let Some(folder) = path.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            if let Ok(bytes) = serde_json::to_vec(&info) {
                let _ = std::fs::write(path, bytes);
            }
        }
        Ok(info)
    }
}

/// 剧集的分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// 正片，以及对不上的本地文件。
    Main,
    /// 特别篇（`S1` 等）。
    Special,
    /// OP / ED（`C1`）。
    Extra,
}

/// 详情页剧集列表里的一行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub group: Group,
    /// 特别篇和 OP/ED 的短编号（`S1`、`C1`）；正片和本地文件为空。
    pub code: String,
    /// 第几集；特别篇和认不出集数的本地文件为空。
    pub number: Option<u32>,
    /// 剧集标题（不含“第N话”），本地多出来的文件是文件名。
    pub title: String,
    /// 本地文件的地址；本地没有时为空。
    pub url: Option<String>,
    pub watched: Option<Watched>,
}

impl Row {
    fn finished(&self) -> bool {
        self.watched.is_some_and(|watched| watched.finished())
    }
}

/// `第1话 人鱼公主菈菈` 去掉开头的编号，剩下 `人鱼公主菈菈`；只有编号时为空。
fn bare_title(title: &str, number: &str) -> String {
    let title = title.trim();
    let (first, rest) = title.split_once(' ').unwrap_or((title, ""));
    let numbered = first == number
        || first
            .strip_prefix('第')
            .and_then(|tail| tail.strip_suffix(['话', '話', '集']))
            .is_some_and(|middle| middle == number);
    if numbered {
        rest.trim().to_string()
    } else {
        title.to_string()
    }
}

/// 不带扩展名的文件名。
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// 剧集列表：弹弹play 的剧集（正片在前，特别篇和 OP/ED 在后），各自对上本地同一集的文件，
/// 再加上对不上的本地文件。没有详情时只有本地文件。`local` 按集数排好。
pub fn rows(
    details: Option<&BangumiDetails>,
    local: &[EpisodeFile],
    progress: &BTreeMap<String, Watched>,
) -> Vec<Row> {
    let mut used = vec![false; local.len()];
    let mut rows = Vec::new();
    let episodes = details.map_or(&[][..], |details| &details.episodes[..]);
    let (main, extra): (Vec<_>, Vec<_>) = episodes
        .iter()
        .partition(|episode| episode.episode_number.parse::<u32>().is_ok());
    for episode in main.into_iter().chain(extra) {
        let number = episode.episode_number.parse::<u32>().ok();
        // 正片按集数找本地文件（同一集有几个文件时用第一个）；特别篇在文件名里认不出来。
        let found = number.and_then(|number| {
            (0..local.len()).find(|&at| !used[at] && local[at].episode == Some(number))
        });
        if let Some(at) = found {
            used[at] = true;
        }
        let url = found.map(|at| local[at].url.clone());
        rows.push(Row {
            group: match number {
                Some(_) => Group::Main,
                None if episode.episode_number.starts_with(['C', 'c']) => Group::Extra,
                None => Group::Special,
            },
            code: if number.is_some() {
                String::new()
            } else {
                episode.episode_number.clone()
            },
            number,
            title: bare_title(&episode.episode_title, &episode.episode_number),
            watched: url.as_ref().and_then(|url| progress.get(url).copied()),
            url,
        });
    }
    for file in local
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(file, _)| file)
    {
        rows.push(Row {
            group: Group::Main,
            code: String::new(),
            number: file.episode,
            title: stem(&file.name).to_string(),
            url: Some(file.url.clone()),
            watched: progress.get(&file.url).copied(),
        });
    }
    rows
}

/// 剧集列表上方的统计，以及“继续”指向哪一行。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// 正片（含对不上的本地文件）、特别篇、OP / ED 各有几行。
    pub main: usize,
    pub special: usize,
    pub extra: usize,
    /// 本地有的集数。
    pub local: usize,
    /// 最近看过的那一行。
    pub last: Option<usize>,
    /// “继续”播放的那一行：最近看过且没看完的那集；最近一集已看完就取它后面本地有、没看完的；
    /// 什么都没看过就是本地有的第一集。本地什么都没有时为空。
    pub next: Option<usize>,
}

/// `rows` 的统计。
pub fn summarize(rows: &[Row]) -> Summary {
    let count = |group: Group| rows.iter().filter(|row| row.group == group).count();
    let local = rows.iter().filter(|row| row.url.is_some()).count();
    let last = rows
        .iter()
        .enumerate()
        .filter_map(|(at, row)| Some((at, row.watched?.at)))
        .max_by_key(|&(at, time)| (time, std::cmp::Reverse(at)))
        .map(|(at, _)| at);
    let first_local = || rows.iter().position(|row| row.url.is_some());
    let next = match last {
        Some(at) if !rows[at].finished() => Some(at),
        Some(at) => (at + 1..rows.len())
            .find(|&later| rows[later].url.is_some() && !rows[later].finished())
            .or(Some(at)),
        None => first_local(),
    };
    Summary {
        main: count(Group::Main),
        special: count(Group::Special),
        extra: count(Group::Extra),
        local,
        last,
        next,
    }
}

/// 详情页选中的那一行：正在播放的；没有时是最近看过的；都没有时是第一集。
pub fn selected(rows: &[Row], summary: &Summary, playing: Option<&str>) -> Option<usize> {
    let playing =
        playing.and_then(|url| rows.iter().position(|row| row.url.as_deref() == Some(url)));
    playing
        .or(summary.last)
        .or_else(|| rows.iter().position(|row| row.group == Group::Main))
}

/// 作品的日文标题，和主标题一样或没有时为空。
pub fn native_title(details: &BangumiDetails) -> String {
    details
        .titles
        .iter()
        .find(|title| title.language.contains("日语") && !title.language.contains("罗马"))
        .map(|title| title.title.trim().to_string())
        .filter(|title| title != details.anime_title.trim())
        .unwrap_or_default()
}

/// 弹弹play 列出的正片集数。
pub fn episode_count(details: &BangumiDetails) -> usize {
    details
        .episodes
        .iter()
        .filter(|episode| episode.episode_number.parse::<u32>().is_ok())
        .count()
}

/// 这部番一集多少分钟：本地文件里记下过的最长的时长；都没播放过时为空。
pub fn minutes(rows: &[Row]) -> Option<i64> {
    rows.iter()
        .filter_map(|row| row.watched)
        .map(|watched| (watched.duration_ms + 30_000) / 60_000)
        .filter(|minutes| *minutes > 0)
        .max()
}

/// 只留字母和数字、转成小写，用来认出同一个名字的不同写法（`KINEMA_CITRUS`、`KINEMA CITRUS`）。
fn normalized(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// `metadata` 里键为 `key` 的那条的值。
fn fact<'a>(details: &'a BangumiDetails, key: &str) -> Option<&'a str> {
    details.metadata.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim())
    })
}

/// 标签里不显示的：类别、地区、“原创”这类每部番都有、说明不了什么的。
const PLAIN_TAGS: &[&str] = &[
    "TV",
    "WEB",
    "OVA",
    "OAD",
    "剧场版",
    "电影",
    "日本",
    "中国",
    "原创",
];

/// 显示的标签，最多 `limit` 个，按弹弹play 给的顺序：去掉 [`PLAIN_TAGS`]、年份和季度
/// （`2026`、`2026年7月`）、制作公司和原作（已经写在制作信息里），以及同一个标签的不同写法。
pub fn tags(details: &BangumiDetails, limit: usize) -> Vec<String> {
    let mut seen: HashSet<String> = ["动画制作", "原作"]
        .into_iter()
        .filter_map(|key| fact(details, key))
        .map(normalized)
        .collect();
    details
        .tags
        .iter()
        .map(|tag| tag.name.trim())
        .filter(|name| {
            let dated = name.chars().any(|c| c.is_ascii_digit())
                && name
                    .chars()
                    .all(|c| c.is_ascii_digit() || "年月季".contains(c));
            !name.is_empty()
                && !PLAIN_TAGS.contains(name)
                && !dated
                && seen.insert(normalized(name))
        })
        .take(limit)
        .map(str::to_string)
        .collect()
}

/// 简介：去掉弹弹play 附在后面的 `[简介原文]` 等，连续的空行合成一个换行。
pub fn summary(details: &BangumiDetails) -> String {
    let text = details
        .summary
        .split("[简介原文]")
        .next()
        .unwrap_or_default();
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 开播的年和月：制作信息里的“放送开始”（`2026年7月5日`），没有时用第一集的首播日期。
pub fn premiere(details: &BangumiDetails) -> Option<(i32, u32)> {
    let numbers = |text: &str| -> Vec<u32> {
        text.split(|c: char| !c.is_ascii_digit())
            .filter_map(|part| part.parse().ok())
            .collect()
    };
    let from_fact = fact(details, "放送开始").map(numbers);
    let from_episode = details
        .episodes
        .iter()
        .find_map(|episode| episode.air_date.as_deref())
        .map(numbers);
    [from_fact, from_episode]
        .into_iter()
        .flatten()
        .find_map(|parts| match parts[..] {
            [year, month, ..] if (1900..3000).contains(&year) && (1..=12).contains(&month) => {
                Some((year as i32, month))
            }
            _ => None,
        })
}

/// 制作信息的前 `limit` 条，`(键, 值)`；值里的换行和多余空白合成一个空格。
pub fn facts(details: &BangumiDetails, limit: usize) -> Vec<(String, String)> {
    details
        .metadata
        .iter()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            (!key.trim().is_empty() && !value.is_empty()).then(|| (key.trim().to_string(), value))
        })
        .take(limit)
        .collect()
}

/// 评分写成一位小数。
pub fn score(value: f64) -> String {
    format!("{value:.1}")
}

/// 作品在 TMDB 上的剧集编号和第几季：从 `onlineDatabases` 里 TMDB 的链接
/// （`https://www.themoviedb.org/tv/295071`，可能带 `/season/2`）读出来；没写季时是第一季。
pub fn tmdb(details: &BangumiDetails) -> Option<(i64, u32)> {
    details.online_databases.iter().find_map(|database| {
        let (_, path) = database.url.split_once("themoviedb.org/tv/")?;
        let mut parts = path.split(['/', '?', '#']);
        let id = parts
            .next()?
            .split('-')
            .next()?
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0)?;
        let season = match (parts.next(), parts.next()) {
            (Some("season"), Some(season)) => season.parse().ok().filter(|season| *season > 0),
            _ => None,
        };
        Some((id, season.unwrap_or(1)))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nkuplay_dandanplay::{
        BangumiComment, BangumiEpisode, BangumiTag, BangumiTitle, OnlineDatabase,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn episode(id: i64, number: &str, title: &str) -> BangumiEpisode {
        BangumiEpisode {
            episode_id: id,
            episode_title: title.into(),
            episode_number: number.into(),
            air_date: None,
        }
    }

    /// 和《再见，拉拉》同样形状的详情：三集正片、一集特别篇、一个 OP。
    pub(crate) fn lara() -> BangumiDetails {
        BangumiDetails {
            anime_id: 18643,
            anime_title: "再见，拉拉".into(),
            image_url: Some("https://img.example/18643.jpg".into()),
            type_description: "TV动画".into(),
            titles: [
                ("主标题", "再见，拉拉"),
                ("主标题，日语罗马字", "Sayonara Lara"),
                ("官方标题，日语", "さよならララ"),
            ]
            .into_iter()
            .map(|(language, title)| BangumiTitle {
                language: language.into(),
                title: title.into(),
            })
            .collect(),
            episodes: vec![
                episode(1, "1", "第1话 人鱼公主菈菈"),
                episode(91, "S1", "S1 ララと茉里と過ごす四季 ～春～"),
                episode(2, "2", "第2话 奔走于滋贺"),
                episode(3, "3", "第3话"),
                episode(92, "C1", "C1 Opening"),
            ],
            summary: "很久很久以前，\r\n\r\n有一位人鱼公主。\r\n\r\n[简介原文]\r\n昔々".into(),
            metadata: vec![
                "中文名: 再见菈菈".into(),
                "话数: 12".into(),
                "放送开始: 2026年7月5日".into(),
                "原作: KINEMA CITRUS".into(),
                "别名: Sayonara Lara\n        别名: Goodbye, Lara".into(),
                "链接: ".into(),
            ],
            rating_details: vec![
                ("弹弹play连载中评分".into(), 0.0),
                ("Bangumi评分".into(), 6.4),
            ],
            tags: [
                "TV",
                "恋爱",
                "日本",
                "奇幻",
                "原创",
                "2026年7月",
                "KINEMACITRUS",
                "2026",
                "童话",
                "小出卓史",
                "KINEMA_CITRUS",
                "青春",
                "百合",
                "无法预测的命运之舞台",
            ]
            .into_iter()
            .map(|name| BangumiTag {
                name: name.into(),
                count: 1,
            })
            .collect(),
            online_databases: vec![
                OnlineDatabase {
                    name: "AniDB".into(),
                    url: "https://anidb.net/anime/18643".into(),
                },
                OnlineDatabase {
                    name: "TMDB".into(),
                    url: "https://www.themoviedb.org/tv/295071".into(),
                },
            ],
            is_on_air: false,
            rating: 6.9,
        }
    }

    fn file(name: &str, episode: Option<u32>) -> EpisodeFile {
        EpisodeFile {
            url: format!("file:///E:/Lara/{name}"),
            name: name.into(),
            episode,
        }
    }

    fn watched(position_ms: i64, at: u64) -> Watched {
        Watched {
            position_ms,
            duration_ms: 1_000,
            at,
        }
    }

    #[test]
    fn episodes_line_up_with_local_files_specials_last_and_leftovers_at_the_end() {
        let local = [
            file("Lara - 02.mkv", Some(2)),
            file("Lara - 02 v2.mkv", Some(2)),
            file("Lara - 78.mkv", Some(78)),
        ];
        let progress = BTreeMap::from([(local[0].url.clone(), watched(400, 5))]);
        let rows = rows(Some(&lara()), &local, &progress);
        let shown: Vec<(&str, Option<u32>, &str, bool)> = rows
            .iter()
            .map(|row| {
                (
                    row.code.as_str(),
                    row.number,
                    row.title.as_str(),
                    row.url.is_some(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("", Some(1), "人鱼公主菈菈", false),
                ("", Some(2), "奔走于滋贺", true),
                ("", Some(3), "", false),
                ("S1", None, "ララと茉里と過ごす四季 ～春～", false),
                ("C1", None, "Opening", false),
                ("", Some(2), "Lara - 02 v2", true),
                ("", Some(78), "Lara - 78", true),
            ]
        );
        assert_eq!(rows[1].watched.map(|w| w.percent()), Some(40));
        assert_eq!(rows[1].url.as_deref(), Some(local[0].url.as_str()));
    }

    #[test]
    fn without_details_only_the_local_files_are_listed() {
        let local = [file("01.mkv", Some(1)), file("extra.mkv", None)];
        let rows = rows(None, &local, &BTreeMap::new());
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[1].number, rows[1].title.as_str()), (None, "extra"));
        let summary = summarize(&rows);
        assert_eq!((summary.main, summary.special, summary.local), (2, 0, 2));
        assert_eq!((summary.last, summary.next), (None, Some(0)));
    }

    fn numbered(count: u32) -> Vec<Row> {
        (1..=count)
            .map(|number| Row {
                group: Group::Main,
                code: String::new(),
                number: Some(number),
                title: String::new(),
                url: (number != 3).then(|| format!("file:///{number}.mkv")),
                watched: None,
            })
            .collect()
    }

    #[test]
    fn continue_points_at_the_last_unfinished_episode_or_the_next_local_one() {
        let mut rows = numbered(5);
        let summary = summarize(&rows);
        assert_eq!((summary.main, summary.local, summary.next), (5, 4, Some(0)));

        // 第 2 集看了一半，之前看完过第 1 集：继续第 2 集。
        rows[0].watched = Some(watched(1_000, 1));
        rows[1].watched = Some(watched(300, 2));
        let summary = summarize(&rows);
        assert_eq!((summary.last, summary.next), (Some(1), Some(1)));

        // 第 2 集看完了：第 3 集本地没有，跳到第 4 集。
        rows[1].watched = Some(watched(950, 3));
        assert_eq!(summarize(&rows).next, Some(3));

        // 选中的是最近看过的；正在播放别的一集时是正在播放的。
        assert_eq!(selected(&rows, &summarize(&rows), None), Some(1));
        assert_eq!(
            selected(&rows, &summarize(&rows), Some("file:///5.mkv")),
            Some(4)
        );
        // 最后一集也看完了：后面没有了，就还是它。
        rows[4].watched = Some(watched(1_000, 4));
        let summary = summarize(&rows);
        assert_eq!((summary.last, summary.next), (Some(4), Some(4)));
    }

    #[test]
    fn the_counts_tell_main_episodes_from_specials() {
        let details = lara();
        let rows = rows(
            Some(&details),
            &[file("Lara - 01.mkv", Some(1))],
            &BTreeMap::new(),
        );
        let summary = summarize(&rows);
        assert_eq!(
            (summary.main, summary.special, summary.extra, summary.local),
            (3, 1, 1, 1)
        );
        assert_eq!(summary.next, Some(0));
    }

    #[test]
    fn the_banner_texts_are_tidied() {
        let details = lara();
        assert_eq!(native_title(&details), "さよならララ");
        assert_eq!(episode_count(&details), 3);
        assert_eq!(
            tags(&details, 6),
            ["恋爱", "奇幻", "童话", "小出卓史", "青春", "百合"]
        );
        assert_eq!(summary(&details), "很久很久以前，\n有一位人鱼公主。");
        assert_eq!(premiere(&details), Some((2026, 7)));
        assert_eq!(
            facts(&details, 9),
            [
                ("中文名".to_string(), "再见菈菈".to_string()),
                ("话数".to_string(), "12".to_string()),
                ("放送开始".to_string(), "2026年7月5日".to_string()),
                ("原作".to_string(), "KINEMA CITRUS".to_string()),
                (
                    "别名".to_string(),
                    "Sayonara Lara 别名: Goodbye, Lara".to_string()
                ),
            ]
        );
        assert_eq!(score(9.11), "9.1");
        assert_eq!(score(6.0), "6.0");
    }

    #[test]
    fn the_premiere_falls_back_to_the_first_air_date() {
        let mut details = lara();
        details.metadata.clear();
        assert_eq!(premiere(&details), None);
        details.episodes[0].air_date = Some("2026-07-06T00:00:00".into());
        assert_eq!(premiere(&details), Some((2026, 7)));
    }

    #[test]
    fn the_tmdb_show_and_season_come_from_its_link() {
        let mut details = lara();
        assert_eq!(tmdb(&details), Some((295071, 1)));
        details.online_databases[1].url =
            "https://www.themoviedb.org/tv/1234-some-show/season/2".into();
        assert_eq!(tmdb(&details), Some((1234, 2)));
        details.online_databases.remove(1);
        assert_eq!(tmdb(&details), None);
    }

    /// 假的弹弹play：应答固定的详情和短评，并数被调用了几次。
    #[derive(Default)]
    pub(crate) struct FakeApi {
        pub details: Option<BangumiDetails>,
        pub error: Option<&'static str>,
        pub comments_error: bool,
        /// 给几条评论；0 当作 1。
        pub comment_count: usize,
        pub calls: Arc<AtomicUsize>,
    }

    impl FakeApi {
        pub(crate) fn lara() -> Self {
            Self {
                details: Some(lara()),
                ..Self::default()
            }
        }
    }

    impl Api for FakeApi {
        fn details(&self, _anime_id: i64) -> Result<Option<BangumiDetails>, Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.error {
                Some(reason) => Err(Error::Transport(reason.into())),
                None => Ok(self.details.clone()),
            }
        }

        fn comments(&self, _anime_id: i64) -> Result<BangumiComments, Error> {
            if self.comments_error {
                return Err(Error::Transport("refused".into()));
            }
            Ok(BangumiComments {
                count: 20,
                has_more: true,
                comments: (1..=self.comment_count.max(1) as i64)
                    .map(|id| BangumiComment {
                        id,
                        user_name: "104期的杂鱼".into(),
                        source: "Bangumi".into(),
                        text: "第一集的观感真的很不错".into(),
                        rating: 5,
                    })
                    .collect(),
            })
        }
    }

    #[test]
    fn details_are_kept_on_disk_and_age_by_whether_the_show_is_airing() {
        let folder = std::env::temp_dir().join(format!("nkuplay-details-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let source = Source::new(Some(Box::new(FakeApi::lara())), Some(folder.clone()));
        assert!(source.cached(18643).is_none());
        let info = source.download(18643, 1_000).unwrap();
        assert_eq!(info.comments.comments.len(), 1);
        let cached = Source::new(None, Some(folder.clone()))
            .cached(18643)
            .unwrap();
        assert_eq!(cached, info);
        // 完结的作品保留 7 天，连载中的 1 天。
        assert!(cached.fresh(1_000 + 6 * 24 * 3600));
        assert!(!cached.fresh(1_000 + 7 * 24 * 3600));
        let mut airing = cached.clone();
        airing.details.as_mut().unwrap().is_on_air = true;
        assert!(airing.fresh(1_000 + 3600));
        assert!(!airing.fresh(1_000 + 24 * 3600));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn missing_comments_do_not_fail_the_details() {
        let source = Source::new(
            Some(Box::new(FakeApi {
                comments_error: true,
                ..FakeApi::lara()
            })),
            None,
        );
        let info = source.download(18643, 1).unwrap();
        assert!(info.details.is_some());
        assert!(info.comments.comments.is_empty());
        let failing = Source::new(
            Some(Box::new(FakeApi {
                error: Some("timed out"),
                ..FakeApi::lara()
            })),
            None,
        );
        assert!(
            failing
                .download(18643, 1)
                .unwrap_err()
                .contains("timed out")
        );
        assert!(Source::new(None, None).download(18643, 1).is_err());
    }
}
