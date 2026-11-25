//! 给每条弹幕分一行，并给出某一刻屏上弹幕的 ASS 文本。

use crate::{CANVAS_HEIGHT, Mode, Track};
use std::fmt::Write;
use std::sync::Arc;

/// 分到了行的一条弹幕。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    /// 在 `Track::entries` 里的位置。
    entry: usize,
    lane: usize,
}

/// 一行里最后放进去的滚动弹幕：出现时间、宽度、速度（像素每秒）。
#[derive(Clone, Copy)]
struct Last {
    time: f64,
    width: f64,
    speed: f64,
}

/// 按某个画布宽度排好的弹幕。
#[derive(Clone, Debug)]
pub struct Layout {
    track: Arc<Track>,
    /// 画布宽（高是 [`CANVAS_HEIGHT`]）。
    width: f64,
    /// 一行多高。
    lane_height: f64,
    /// 底部弹幕从哪里往上排：显示区域的下沿。
    bottom: f64,
    /// 按出现时间排好。
    placed: Vec<Placed>,
}

/// 最小的那个值在第几个。
fn earliest(values: impl Iterator<Item = f64>) -> usize {
    values
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(index, _)| index)
}

impl Layout {
    /// 在 `width` 宽、[`CANVAS_HEIGHT`] 高的画布上排 `track`。
    pub fn new(track: Arc<Track>, width: f64) -> Self {
        let config = track.config();
        let width = if width.is_finite() && width > 0.0 {
            width
        } else {
            CANVAS_HEIGHT * 16.0 / 9.0
        };
        let lane_height = config.font_size * (1.0 + config.lane_gap) + config.outline * 2.0;
        let bottom = CANVAS_HEIGHT * config.area.clamp(0.1, 1.0);
        let fit = ((bottom / lane_height).floor() as usize).max(1);
        let lanes = config.max_lanes.map_or(fit, |max| max.clamp(1, fit));
        // 滚动弹幕之间至少隔一个字号。
        let spacing = config.font_size;
        let duration = config.scroll_seconds.max(0.5);
        let fixed = config.fixed_seconds.max(0.5);

        let mut scrolling: Vec<Option<Last>> = vec![None; lanes];
        // 顶部、底部每行空出来的时刻。
        let mut top = vec![f64::NEG_INFINITY; lanes];
        let mut low = vec![f64::NEG_INFINITY; lanes];
        let mut placed = Vec::new();
        for (index, entry) in track.entries.iter().enumerate() {
            let lane = match entry.mode {
                Mode::Scroll => {
                    let speed = (width + entry.width) / duration;
                    let lane = scrolling
                        .iter()
                        .position(|last| match last {
                            None => true,
                            Some(last) => fits(last, entry.time, speed, width, spacing, duration),
                        })
                        .or_else(|| {
                            // 允许重叠：叠在最早放进去的那一行上。
                            config.overlap.then(|| {
                                earliest(
                                    scrolling.iter().map(|last| {
                                        last.map_or(f64::NEG_INFINITY, |last| last.time)
                                    }),
                                )
                            })
                        });
                    if let Some(lane) = lane {
                        scrolling[lane] = Some(Last {
                            time: entry.time,
                            width: entry.width,
                            speed,
                        });
                    }
                    lane
                }
                Mode::Top | Mode::Bottom => {
                    let free = if entry.mode == Mode::Top {
                        &mut top
                    } else {
                        &mut low
                    };
                    let lane = free
                        .iter()
                        .position(|until| *until <= entry.time)
                        .or_else(|| config.overlap.then(|| earliest(free.iter().copied())));
                    if let Some(lane) = lane {
                        free[lane] = entry.time + fixed;
                    }
                    lane
                }
            };
            if let Some(lane) = lane {
                placed.push(Placed { entry: index, lane });
            }
        }
        Self {
            track,
            width,
            lane_height,
            bottom,
            placed,
        }
    }

    /// 排进去了几条（其余因为放不下丢掉了）。
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// 画布宽。
    pub fn width(&self) -> f64 {
        self.width
    }

    /// `time` 这一刻屏上所有弹幕的 ASS 文本，一条一行；没有弹幕时为空。
    pub fn events(&self, time: f64) -> String {
        let config = self.track.config();
        let duration = config.scroll_seconds.max(0.5);
        let fixed = config.fixed_seconds.max(0.5);
        let longest = duration.max(fixed);
        let entries = &self.track.entries;
        // 只看出现时间在 (time - 最长停留, time] 里的：两次二分查找。
        let from = self
            .placed
            .partition_point(|p| entries[p.entry].time < time - longest);
        let to = self
            .placed
            .partition_point(|p| entries[p.entry].time <= time);
        let mut out = String::new();
        for placed in &self.placed[from..to] {
            let entry = &entries[placed.entry];
            let elapsed = time - entry.time;
            let lane_top = placed.lane as f64 * self.lane_height;
            match entry.mode {
                Mode::Scroll => {
                    if elapsed > duration {
                        continue;
                    }
                    let speed = (self.width + entry.width) / duration;
                    let x = self.width - elapsed * speed;
                    let _ = write!(out, "{{\\an7\\pos({x:.1},{lane_top:.1})");
                }
                Mode::Top | Mode::Bottom => {
                    if elapsed > fixed {
                        continue;
                    }
                    let y = if entry.mode == Mode::Top {
                        lane_top
                    } else {
                        self.bottom - (placed.lane + 1) as f64 * self.lane_height
                    };
                    let _ = write!(out, "{{\\an8\\pos({:.1},{y:.1})", self.width / 2.0);
                }
            }
            out.push_str(&entry.styled);
            out.push('\n');
        }
        out
    }
}

/// 在已有 `last` 的一行里，`time` 时出现、速度为 `speed` 的新弹幕放不放得下：
/// 出现时前一条的尾巴已经完全进来并空出 `spacing`，而且直到前一条从左边出去，新的都追不上它。
fn fits(last: &Last, time: f64, speed: f64, width: f64, spacing: f64, duration: f64) -> bool {
    let entered = (time - last.time) * last.speed >= last.width + spacing;
    if !entered {
        return false;
    }
    if speed <= last.speed {
        return true;
    }
    // 前一条的尾巴走到左边缘（x = 0）的时刻，新弹幕的头还在它右边。
    let gone = last.time + duration;
    width - (gone - time) * speed >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Comment, Config, Measure};

    fn track(comments: &[(f64, Mode, &str)], config: Config) -> Arc<Track> {
        let comments: Vec<Comment> = comments
            .iter()
            .map(|(time, mode, text)| Comment {
                time: *time,
                mode: *mode,
                color: 0xFFFFFF,
                text: (*text).into(),
            })
            .collect();
        Arc::new(Track::new(&comments, 0.0, config, &Measure::estimate()))
    }

    /// `events` 里每条的位置。
    fn positions(events: &str) -> Vec<(f64, f64)> {
        events
            .lines()
            .map(|line| {
                let inside = line.split("\\pos(").nth(1).unwrap();
                let inside = inside.split(')').next().unwrap();
                let (x, y) = inside.split_once(',').unwrap();
                (x.parse().unwrap(), y.parse().unwrap())
            })
            .collect()
    }

    #[test]
    fn scrolling_comments_in_one_lane_never_overlap() {
        // 一秒 5 条、长短不一，排 60 秒。
        let texts = [
            "短",
            "稍微长一点的弹幕",
            "这是一条非常非常非常长的弹幕内容啊啊啊",
            "ok",
            "哈哈哈哈",
        ];
        let comments: Vec<(f64, Mode, &str)> = (0..300)
            .map(|i| (i as f64 * 0.2, Mode::Scroll, texts[i % texts.len()]))
            .collect();
        let track = track(&comments, Config::default());
        let layout = Layout::new(Arc::clone(&track), 1920.0);
        assert!(layout.len() > 100, "{}", layout.len());
        let entries = &track.entries;
        // 每隔 0.1 秒看一次：同一行的任意两条，左右不重叠。
        for step in 0..700 {
            let t = step as f64 * 0.1;
            let mut shown: Vec<(usize, f64, f64)> = Vec::new();
            for placed in &layout.placed {
                let entry = &entries[placed.entry];
                let elapsed = t - entry.time;
                if !(0.0..=8.0).contains(&elapsed) {
                    continue;
                }
                let x = 1920.0 - elapsed * (1920.0 + entry.width) / 8.0;
                shown.push((placed.lane, x, x + entry.width));
            }
            for (i, a) in shown.iter().enumerate() {
                for b in &shown[i + 1..] {
                    if a.0 == b.0 {
                        assert!(a.2 <= b.1 || b.2 <= a.1, "overlap at {t}: {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_comment_that_fits_nowhere_is_dropped() {
        let config = Config {
            max_lanes: Some(2),
            ..Config::default()
        };
        let track = track(
            &[
                (0.0, Mode::Scroll, "一"),
                (0.0, Mode::Scroll, "二"),
                (0.0, Mode::Scroll, "三"),
                (0.0, Mode::Top, "上一"),
                (1.0, Mode::Top, "上二"),
                (2.0, Mode::Top, "上三"),
                (5.0, Mode::Top, "上四"),
            ],
            config,
        );
        let layout = Layout::new(track, 1920.0);
        let lanes: Vec<(usize, usize)> = layout.placed.iter().map(|p| (p.entry, p.lane)).collect();
        // “三”没地方；“上三”时两行都占着；“上四”时第一行空出来了。
        assert_eq!(lanes, [(0, 0), (1, 1), (3, 0), (4, 1), (6, 0)]);
    }

    #[test]
    fn events_show_what_is_on_screen_at_that_moment() {
        let track = track(
            &[
                (10.0, Mode::Scroll, "滚动"),
                (10.0, Mode::Top, "顶部"),
                (10.0, Mode::Bottom, "底部"),
                (10.5, Mode::Bottom, "底部二"),
            ],
            Config::default(),
        );
        let layout = Layout::new(track, 1920.0);
        assert_eq!(layout.events(9.9), "");
        let at_start = layout.events(10.0);
        assert_eq!(at_start.lines().count(), 3);
        // 滚动弹幕刚从右边缘进来；顶部的居中贴着上沿；底部的在显示区域下沿之上一行。
        let lane = 34.0 * 1.15 + 4.0;
        assert_eq!(
            positions(&at_start),
            [
                (1920.0, 0.0),
                (960.0, 0.0),
                (960.0, ((1080.0 * 0.85 - lane) * 10.0_f64).round() / 10.0)
            ]
        );
        assert!(at_start.starts_with("{\\an7\\pos(1920.0,0.0)\\fnMicrosoft YaHei"));
        // 第二条底部弹幕排在第一条上面。
        let later = positions(&layout.events(10.5));
        assert!(later[3].1 < later[2].1);
        // 滚动弹幕往左走；顶部底部的 5 秒后消失。
        let moving = positions(&layout.events(14.0));
        assert!(moving[0].0 < 1000.0);
        assert_eq!(layout.events(15.2).lines().count(), 2);
        // 8 秒后滚动的也走完了。
        assert_eq!(layout.events(18.1), "");
    }

    #[test]
    fn a_wider_canvas_moves_comments_faster() {
        let track = track(&[(0.0, Mode::Scroll, "弹幕")], Config::default());
        let narrow = positions(&Layout::new(Arc::clone(&track), 1440.0).events(4.0));
        let wide = positions(&Layout::new(track, 2560.0).events(4.0));
        // 都走了一半路程。
        let half = |width: f64| width - 4.0 * (width + 34.0 * 1.8 + 4.0) / 8.0;
        assert!((narrow[0].0 - half(1440.0)).abs() < 0.1);
        assert!((wide[0].0 - half(2560.0)).abs() < 0.1);
    }
}
