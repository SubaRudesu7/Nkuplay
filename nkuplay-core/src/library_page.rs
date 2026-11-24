//! 媒体库页：每部番看到哪里、按观看状态筛选、排序，以及海报的右键菜单（继续、标记看完、
//! 隐藏、重新识别、在文件夹中显示）。

use crate::library::{ShelfItem, Watched};
use crate::settings;
use crate::{NkuplayEngine, unix_now};
use bern::CommandError;
use nkuplay_protocol::nkuplay::{LibraryFilter, LibrarySort, Series};
use std::collections::{BTreeMap, HashMap};

/// 一部番看到哪里。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Watch {
    /// 看过的最后一集是第几话；都没看过时为 0。
    pub up_to: i64,
    pub finished: bool,
    /// 看完的集占几分之几。
    pub bar: f64,
    /// “继续”要播的那一集在剧集里的位置。
    pub resume: usize,
    /// 那一集是第几话。
    pub resume_number: i64,
    /// 最近一次看这部番的时刻（Unix 秒）；没看过时为 0。
    pub last_at: u64,
}

/// 一集是第几话：文件名里认出的集数，认不出时按排好的位置。
fn number_of(item: &ShelfItem, index: usize) -> i64 {
    item.episodes[index]
        .episode
        .map_or(index as i64 + 1, i64::from)
}

/// 按观看进度算出一部番看到哪里、该从哪一集接着看。
pub(crate) fn watch_of(item: &ShelfItem, progress: &BTreeMap<String, Watched>) -> Watch {
    let total = item.episodes.len();
    if total == 0 {
        return Watch::default();
    }
    let mut watch = Watch::default();
    let mut finished = 0;
    // 最近看的那一集和它看完没有。
    let mut latest: Option<(u64, usize, bool)> = None;
    for (index, episode) in item.episodes.iter().enumerate() {
        let Some(seen) = progress.get(&episode.url) else {
            continue;
        };
        let done = seen.finished();
        if done {
            finished += 1;
        }
        if done || seen.position_ms > 0 {
            watch.up_to = watch.up_to.max(number_of(item, index));
        }
        if latest.is_none_or(|(at, ..)| seen.at >= at) {
            latest = Some((seen.at, index, done));
        }
    }
    watch.finished = finished == total;
    watch.bar = finished as f64 / total as f64;
    watch.resume = match latest {
        // 看到一半的接着看；看完的看下一集，已经是最后一集就重看它。
        Some((_, index, false)) => index,
        Some((_, index, true)) => (index + 1).min(total - 1),
        None => 0,
    };
    watch.last_at = latest.map_or(0, |(at, ..)| at);
    watch.resume_number = number_of(item, watch.resume);
    watch
}

/// 一部番属于哪个筛选。
fn filter_of(series: &Series) -> LibraryFilter {
    if series.finished {
        LibraryFilter::Finished
    } else if series.watched_up_to > 0 {
        LibraryFilter::Watching
    } else {
        LibraryFilter::Unwatched
    }
}

impl NkuplayEngine {
    /// 媒体库页的海报墙：筛选、排序，和各个筛选下的数量。
    pub(crate) fn publish_library_page(&mut self) {
        let all = &self.state.series;
        let count = |filter: LibraryFilter| {
            all.iter()
                .filter(|series| filter_of(series) == filter)
                .count() as i64
        };
        self.state.count_all = all.len() as i64;
        self.state.count_watching = count(LibraryFilter::Watching);
        self.state.count_unwatched = count(LibraryFilter::Unwatched);
        self.state.count_finished = count(LibraryFilter::Finished);
        let sort = settings::library_sort_of(&self.settings.library_sort);
        self.state.library_sort = sort.clone();
        let filter = self.state.library_filter.clone();
        let mut shown: Vec<Series> = all
            .iter()
            .filter(|series| filter == LibraryFilter::All || filter_of(series) == filter)
            .cloned()
            .collect();
        match sort {
            // 海报墙本来按作品名排。
            LibrarySort::Name => {}
            LibrarySort::Added => {
                let order = self.added_order();
                shown.sort_by_key(|series| {
                    let key = self.shelf_key(series.id);
                    std::cmp::Reverse(key.and_then(|key| order.get(key).copied()).unwrap_or(0))
                });
            }
            LibrarySort::Watched => {
                let last: HashMap<i64, u64> = shown
                    .iter()
                    .map(|series| (series.id, self.watch_at(series.id).last_at))
                    .collect();
                shown.sort_by_key(|series| std::cmp::Reverse(last[&series.id]));
            }
        }
        self.state.library_shown = shown;
    }

    /// 每部番第一次出现在第几个文件夹的第几部：越大越晚加入。
    fn added_order(&self) -> HashMap<String, usize> {
        let mut order = HashMap::new();
        let mut rank = 0;
        for series in self
            .library
            .folders
            .iter()
            .flat_map(|folder| &folder.series)
        {
            let key = match &series.identity {
                Some(identity) => format!("anime:{}", identity.anime_id),
                None => format!("title:{}", series.key),
            };
            order.entry(key).or_insert_with(|| {
                rank += 1;
                rank
            });
        }
        order
    }

    fn shelf_key(&self, id: i64) -> Option<&String> {
        Some(&self.shelf_item(id)?.key)
    }

    fn shelf_item(&self, id: i64) -> Option<&ShelfItem> {
        usize::try_from(id - 1)
            .ok()
            .and_then(|at| self.shelf.get(at))
    }

    fn watch_at(&self, id: i64) -> Watch {
        self.shelf_item(id)
            .map(|item| watch_of(item, &self.library.progress))
            .unwrap_or_default()
    }

    fn item_or_error(&self, id: i64) -> Result<&ShelfItem, CommandError> {
        self.shelf_item(id)
            .ok_or_else(|| CommandError::new("unknown_series", format!("there is no series {id}")))
    }

    /// “继续”要播的那一集的地址。
    pub(crate) fn resume_url(&self, id: i64) -> Result<String, CommandError> {
        let item = self.item_or_error(id)?;
        let watch = watch_of(item, &self.library.progress);
        item.episodes
            .get(watch.resume)
            .map(|episode| episode.url.clone())
            .ok_or_else(|| CommandError::new("no_episodes", "the series has no episodes"))
    }

    /// 每一集都记成看完；时长不知道的记成 1 毫秒的“看完”。
    pub(crate) fn finish_series_at(&mut self, id: i64) -> Result<(), CommandError> {
        let urls: Vec<String> = self
            .item_or_error(id)?
            .episodes
            .iter()
            .map(|episode| episode.url.clone())
            .collect();
        let now = unix_now();
        for url in urls {
            let duration = self
                .library
                .progress
                .get(&url)
                .map_or(1, |seen| seen.duration_ms.max(1));
            self.library.progress.insert(
                url,
                Watched {
                    position_ms: duration,
                    duration_ms: duration,
                    at: now,
                },
            );
        }
        self.save_library();
        Ok(())
    }

    pub(crate) fn hide_series_at(&mut self, id: i64) -> Result<(), CommandError> {
        let key = self.item_or_error(id)?.key.clone();
        if !self.library.hidden.contains(&key) {
            self.library.hidden.push(key);
        }
        self.save_library();
        Ok(())
    }

    /// 忘掉这部番认出的作品；接着重新扫描时会再问弹弹play。
    pub(crate) fn reidentify_series_at(&mut self, id: i64) -> Result<(), CommandError> {
        let key = self.item_or_error(id)?.key.clone();
        let anime = key
            .strip_prefix("anime:")
            .and_then(|id| id.parse::<i64>().ok());
        for series in self
            .library
            .folders
            .iter_mut()
            .flat_map(|folder| folder.series.iter_mut())
        {
            if anime.is_some()
                && series.identity.as_ref().map(|identity| identity.anime_id) == anime
            {
                series.identity = None;
            }
        }
        self.save_library();
        Ok(())
    }

    /// 打开“继续”那一集所在的文件夹。
    pub(crate) fn reveal_series_at(&self, id: i64) -> Result<(), CommandError> {
        let url = self.resume_url(id)?;
        let path = std::path::PathBuf::from(nkuplay_mpv::location(&url));
        let folder = path
            .parent()
            .ok_or_else(|| CommandError::new("no_folder", "the episode is not in a folder"))?;
        bern::platform::open_path(folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::EpisodeFile;

    fn item(count: u32) -> ShelfItem {
        ShelfItem {
            key: "title:x".into(),
            title: "X".into(),
            poster: None,
            episodes: (1..=count)
                .map(|number| EpisodeFile {
                    url: format!("file:///x/{number}.mkv"),
                    name: format!("{number}.mkv"),
                    episode: Some(number),
                })
                .collect(),
        }
    }

    fn seen(position_ms: i64, at: u64) -> Watched {
        Watched {
            position_ms,
            duration_ms: 1000,
            at,
        }
    }

    #[test]
    fn a_series_resumes_where_it_was_left() {
        let show = item(4);
        let none = watch_of(&show, &BTreeMap::new());
        assert_eq!(
            (none.up_to, none.resume_number, none.finished),
            (0, 1, false)
        );

        let mut progress = BTreeMap::new();
        progress.insert("file:///x/1.mkv".to_string(), seen(1000, 10));
        progress.insert("file:///x/2.mkv".to_string(), seen(400, 20));
        let half = watch_of(&show, &progress);
        assert_eq!((half.up_to, half.resume_number), (2, 2));
        assert_eq!(half.bar, 0.25);

        progress.insert("file:///x/2.mkv".to_string(), seen(1000, 30));
        assert_eq!(watch_of(&show, &progress).resume_number, 3);

        for number in 1..=4 {
            progress.insert(
                format!("file:///x/{number}.mkv"),
                seen(1000, 40 + number as u64),
            );
        }
        let done = watch_of(&show, &progress);
        assert!(done.finished);
        // 看完了最后一集：重看最后一集。
        assert_eq!(done.resume_number, 4);
    }
}
