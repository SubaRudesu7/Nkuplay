//! 本季新番：来自弹弹play 的新番列表（`GET /api/v2/bangumi/shin`）。
//!
//! 取到的列表存进缓存文件夹：第一次打开首页时取一次，之后只读缓存，
//! 用户点“刷新”才重新取。缓存里记着是哪一季取的，换季后界面提示刷新。

use chrono::{Datelike, NaiveDate};
use nkuplay_dandanplay::{BangumiIntro, Client, Credentials, Error};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CACHE_FILE: &str = "shin.json";

/// 弹弹play 的调用里这里用到的那一个，测试时可以换成假的。
pub trait Api: Send + Sync {
    /// 本季新番，不含限制级作品。
    fn shin(&self) -> Result<Vec<BangumiIntro>, Error>;
}

impl Api for Client {
    fn shin(&self) -> Result<Vec<BangumiIntro>, Error> {
        self.shin_bangumi(true)
    }
}

/// 新番列表里的一部作品。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// 弹弹play 的作品编号。
    pub id: i64,
    pub title: String,
    /// 0 到 6 是周一到周日。
    pub day: u8,
    pub poster: Option<String>,
}

/// 一次取到的新番列表。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Season {
    /// 是哪一季取的，例如 `202610`：年份加上这一季开始的月份（01、04、07、10）。
    pub id: String,
    pub anime: Vec<Entry>,
}

/// `date` 所在的那一季的编号。
pub fn season_of(date: NaiveDate) -> String {
    let month = (date.month0() / 3) * 3 + 1;
    format!("{}{month:02}", date.year())
}

/// 弹弹play 的 `airDay`（0 是星期日，1 到 6 是星期一到星期六）换成 0 是周一的写法。
fn day_of(air_day: i32) -> u8 {
    match air_day {
        1..=6 => (air_day - 1) as u8,
        _ => 6,
    }
}

fn entry_of(intro: BangumiIntro) -> Entry {
    Entry {
        id: intro.anime_id,
        title: intro.anime_title,
        day: day_of(intro.air_day),
        poster: intro.image_url.filter(|url| !url.is_empty()),
    }
}

/// 新番列表的来源：向弹弹play 取，并存进缓存文件夹。
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
        Self::new(api, bern::storage::cache_dir())
    }

    /// 能不能向弹弹play 取。
    pub fn available(&self) -> bool {
        self.api.is_some()
    }

    /// 存在缓存里的列表；没有或读不出来时为 `None`。
    pub fn cached(&self) -> Option<Season> {
        let path = self.cache.as_ref()?.join(CACHE_FILE);
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// 向弹弹play 取新番列表，记作 `current` 这一季（例如 `202610`），存进缓存。会阻塞。
    pub fn download(&self, current: &str) -> Result<Season, String> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| "this build has no dandanplay credentials".to_string())?;
        let anime = api
            .shin()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|intro| !intro.is_restricted)
            .map(entry_of)
            .collect();
        let season = Season {
            id: current.to_string(),
            anime,
        };
        if let Some(folder) = &self.cache {
            // 缓存写不进去不影响这次的结果。
            let _ = std::fs::create_dir_all(folder);
            if let Ok(bytes) = serde_json::to_vec(&season) {
                let _ = std::fs::write(folder.join(CACHE_FILE), bytes);
            }
        }
        Ok(season)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 假的弹弹play：应答固定的新番列表，并数被调用了几次。
    #[derive(Default)]
    pub(crate) struct FakeApi {
        pub list: Vec<BangumiIntro>,
        pub error: Option<&'static str>,
        pub calls: Arc<AtomicUsize>,
    }

    pub(crate) fn intro(id: i64, title: &str, air_day: i32) -> BangumiIntro {
        BangumiIntro {
            anime_id: id,
            anime_title: title.into(),
            image_url: Some(format!("https://img.example/{id}.jpg")),
            air_day,
            is_on_air: true,
            is_restricted: false,
            rating: 0.0,
        }
    }

    impl FakeApi {
        /// 周一一部、周四一部、周日一部，外加一部限制级的。
        pub(crate) fn season() -> Self {
            let mut restricted = intro(4, "限制级", 1);
            restricted.is_restricted = true;
            Self {
                list: vec![
                    intro(1, "周一的番", 1),
                    intro(2, "周四的番", 4),
                    intro(3, "周日的番", 0),
                    restricted,
                ],
                ..Self::default()
            }
        }
    }

    impl Api for FakeApi {
        fn shin(&self) -> Result<Vec<BangumiIntro>, Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.error {
                Some(reason) => Err(Error::Transport(reason.into())),
                None => Ok(self.list.clone()),
            }
        }
    }

    #[test]
    fn the_season_follows_the_date() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(season_of(date(2026, 11, 16)), "202610");
        assert_eq!(season_of(date(2026, 1, 1)), "202601");
        assert_eq!(season_of(date(2026, 3, 31)), "202601");
        assert_eq!(season_of(date(2026, 7, 1)), "202607");
    }

    #[test]
    fn the_weekday_starts_on_monday_and_restricted_anime_is_left_out() {
        let source = Source::new(Some(Box::new(FakeApi::season())), None);
        let season = source.download("202610").unwrap();
        let days: Vec<_> = season
            .anime
            .iter()
            .map(|a| (a.title.as_str(), a.day))
            .collect();
        assert_eq!(days, [("周一的番", 0), ("周四的番", 3), ("周日的番", 6)]);
        assert_eq!(
            season.anime[0].poster.as_deref(),
            Some("https://img.example/1.jpg")
        );
    }

    #[test]
    fn a_download_is_kept_in_the_cache() {
        let folder = std::env::temp_dir().join(format!("nkuplay-shin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let source = Source::new(Some(Box::new(FakeApi::season())), Some(folder.clone()));
        assert_eq!(source.cached(), None);
        let season = source.download("202610").unwrap();
        assert_eq!(season.id, "202610");
        // 下一次运行没有凭证也读得到缓存。
        assert_eq!(
            Source::new(None, Some(folder.clone())).cached(),
            Some(season)
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn without_credentials_nothing_is_asked() {
        let source = Source::new(None, None);
        assert!(!source.available());
        assert!(source.download("202610").is_err());
    }
}
