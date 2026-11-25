//! 新番的播出时间：弹弹play 只给到星期几，几点来自 bangumi-data（CC BY 4.0）。
//!
//! 整份数据有好几兆，只留下还在播的作品的名字和时间，存进缓存文件夹，几天更新一次。
//! 两边的编号对不上，按名字对。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const URL: &str = "https://unpkg.com/bangumi-data@0.3/dist/data.json";
const CACHE_FILE: &str = "airtimes.json";
/// 整份数据最大多少字节；超过的不要。
const MAX_DATA: u64 = 64 * 1024 * 1024;
/// 缓存多久算旧（秒）：三天。
const FRESH: u64 = 3 * 24 * 60 * 60;
/// 只要最近这么久（秒）开播的：更早的作品，开播时的时间多半已经不准。
const RECENT: i64 = 366 * 24 * 60 * 60;
/// 一次取不下来时试几次。
const ATTEMPTS: usize = 3;
/// 名字只对上开头时，开头至少要有几个字。
const MIN_PREFIX: usize = 4;

/// 取 bangumi-data 的那一步，测试时可以换成假的。
pub trait Api: Send + Sync {
    fn data(&self) -> Result<Vec<u8>, String>;
}

/// 真的 bangumi-data。
struct Http;

impl Api for Http {
    fn data(&self) -> Result<Vec<u8>, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .user_agent(concat!("nkuplay/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let response = agent.get(URL).call().map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_DATA)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }
}

#[derive(Deserialize)]
struct Data {
    items: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    title: String,
    #[serde(default)]
    title_translate: HashMap<String, Vec<String>>,
    #[serde(default)]
    begin: String,
    #[serde(default)]
    broadcast: Option<String>,
    #[serde(default)]
    end: String,
}

/// 一部作品的名字（[`key`]）和第一次播出的时刻（Unix 秒），之后每周同一时刻播出。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Slot {
    names: Vec<String>,
    begin: i64,
}

/// 还在播的作品的播出时间。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    /// 取到的时间（Unix 秒）。
    fetched: u64,
    slots: Vec<Slot>,
}

/// 比较用的名字：去掉标点和空白，转小写；“第二期”和“第二季”算一样。
fn key(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .map(|c| if c == '期' { '季' } else { c })
        .collect()
}

fn moment(text: &str) -> Option<i64> {
    Some(DateTime::parse_from_rfc3339(text).ok()?.timestamp())
}

impl Item {
    /// 第一次播出的时刻：有每周的播出时间（`R/<时刻>/P7D`）就用它。
    fn first(&self) -> Option<i64> {
        self.broadcast
            .as_deref()
            .and_then(|text| moment(text.split('/').nth(1)?))
            .or_else(|| moment(&self.begin))
    }
}

impl Table {
    /// 从 bangumi-data 的 JSON 里留下 `now`（Unix 秒）时还在播的。
    fn read(bytes: &[u8], now: u64) -> Result<Self, String> {
        let data: Data = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        let at = now as i64;
        let slots = data
            .items
            .iter()
            .filter(|item| moment(&item.end).is_none_or(|end| end > at))
            .filter_map(|item| {
                let begin = item.first().filter(|begin| at - begin < RECENT)?;
                let chinese = ["zh-Hans", "zh-Hant"]
                    .iter()
                    .filter_map(|language| item.title_translate.get(*language))
                    .flatten();
                let mut names: Vec<String> = std::iter::once(&item.title)
                    .chain(chinese)
                    .map(|name| key(name))
                    .filter(|name| !name.is_empty())
                    .collect();
                names.dedup();
                Some(Slot { names, begin })
            })
            .collect();
        Ok(Self {
            fetched: now,
            slots,
        })
    }

    /// 在 `now`（Unix 秒）时还算不算新。
    pub fn fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched) < FRESH
    }

    /// 名叫 `title` 的作品第一次播出的时刻；对不上时为空。先找一样的名字，再找开头一样的。
    pub fn first_airing(&self, title: &str) -> Option<DateTime<Utc>> {
        let wanted = key(title);
        let exact = self.slots.iter().find(|slot| slot.names.contains(&wanted));
        let alike = |name: &String| {
            let shorter = name.chars().count().min(wanted.chars().count());
            shorter >= MIN_PREFIX
                && (name.starts_with(&wanted) || wanted.starts_with(name.as_str()))
        };
        let slot = exact.or_else(|| self.slots.iter().find(|slot| slot.names.iter().any(alike)))?;
        DateTime::from_timestamp(slot.begin, 0)
    }
}

/// 播出时间的来源：向 bangumi-data 取，并存进缓存文件夹。
pub struct Source {
    /// 不联网时为 `None`（测试里）。
    api: Option<Box<dyn Api>>,
    /// 缓存文件夹；没有时不缓存。
    cache: Option<PathBuf>,
}

impl Source {
    pub fn new(api: Option<Box<dyn Api>>, cache: Option<PathBuf>) -> Self {
        Self { api, cache }
    }

    pub fn built_in() -> Self {
        Self::new(Some(Box::new(Http)), bern::storage::cache_dir())
    }

    pub fn available(&self) -> bool {
        self.api.is_some()
    }

    /// 存在缓存里的；没有或读不出来时为 `None`。
    pub fn cached(&self) -> Option<Table> {
        let path = self.cache.as_ref()?.join(CACHE_FILE);
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// 取一份，记作 `now`（Unix 秒）时取到，存进缓存。会阻塞。
    pub fn download(&self, now: u64) -> Result<Table, String> {
        let api = self.api.as_ref().ok_or_else(|| "offline".to_string())?;
        let mut last = api.data();
        for _ in 1..ATTEMPTS {
            if last.is_ok() {
                break;
            }
            last = api.data();
        }
        let table = Table::read(&last?, now)?;
        if let Some(folder) = &self.cache {
            // 缓存写不进去不影响这次的结果。
            let _ = std::fs::create_dir_all(folder);
            if let Ok(bytes) = serde_json::to_vec(&table) {
                let _ = std::fs::write(folder.join(CACHE_FILE), bytes);
            }
        }
        Ok(table)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 2026 年 11 月 16 日 00:00（UTC）。
    pub(crate) const NOW: u64 = 1_794_787_200;

    /// 假的 bangumi-data：前 `failures` 次失败，并数被调用了几次。
    #[derive(Default)]
    pub(crate) struct FakeApi {
        pub failures: usize,
        pub calls: Arc<AtomicUsize>,
    }

    impl Api for FakeApi {
        fn data(&self) -> Result<Vec<u8>, String> {
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.failures {
                return Err("reset".into());
            }
            Ok(r#"{"items": [
                {"title": "月曜のアニメ", "titleTranslate": {"zh-Hans": ["周一的番"]},
                 "begin": "2026-10-05T14:00:00.000Z", "broadcast": "R/2026-10-05T15:30:00.000Z/P7D", "end": ""},
                {"title": "剣になりました", "titleTranslate": {"zh-Hans": ["成为魔剑 第二季"]},
                 "begin": "2026-10-07T15:00:00.000Z", "end": ""},
                {"title": "播完的番", "begin": "2026-07-01T12:00:00.000Z", "end": "2026-09-30T12:00:00.000Z"},
                {"title": "很久以前开播", "begin": "1999-10-20T09:30:00.000Z", "end": ""},
                {"title": "没有时间", "begin": "", "end": ""}
            ]}"#
                .into())
        }
    }

    fn table() -> Table {
        Table::read(&FakeApi::default().data().unwrap(), NOW).unwrap()
    }

    #[test]
    fn shows_still_airing_are_found_by_name() {
        let table = table();
        let at = |title: &str| table.first_airing(title).map(|at| at.to_rfc3339());
        // 有每周的播出时间就用它，不用开播时间。
        assert_eq!(at("周一的番").as_deref(), Some("2026-10-05T15:30:00+00:00"));
        assert_eq!(at("月曜のアニメ"), at("周一的番"));
        // 标点空白不管，“第二期”和“第二季”算一样，开头一样也算。
        assert_eq!(
            at("成为魔剑  第二期").as_deref(),
            Some("2026-10-07T15:00:00+00:00")
        );
        assert!(at("周一的番 特别版").is_some());
        for missing in ["播完的番", "很久以前开播", "没有时间", "周一", "别的番"]
        {
            assert_eq!(at(missing), None, "{missing}");
        }
    }

    #[test]
    fn a_download_is_tried_again_and_kept_in_the_cache() {
        let folder = std::env::temp_dir().join(format!("nkuplay-airtimes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let api = FakeApi {
            failures: 2,
            ..FakeApi::default()
        };
        let calls = Arc::clone(&api.calls);
        let source = Source::new(Some(Box::new(api)), Some(folder.clone()));
        assert_eq!(source.cached(), None);
        let table = source.download(NOW).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(source.cached().as_ref(), Some(&table));
        assert!(table.fresh(NOW + 60) && !table.fresh(NOW + FRESH));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_failure_is_reported() {
        let api = FakeApi {
            failures: 9,
            ..FakeApi::default()
        };
        assert!(
            Source::new(Some(Box::new(api)), None)
                .download(NOW)
                .is_err()
        );
        assert!(Source::new(None, None).download(NOW).is_err());
    }
}
