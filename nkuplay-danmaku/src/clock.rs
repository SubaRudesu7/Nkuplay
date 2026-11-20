//! 弹幕用的播放时钟。
//!
//! mpv 的 `time-pos` 只在换一帧视频时才变：24 帧的片子一秒只变 24 次。弹幕每次屏幕刷新都要
//! 挪一点（60 Hz 或更高），直接用它会一顿一顿的。所以这里自己往前走：每次按真实流逝的时间
//! 乘播放速度往前推，再朝 mpv 报的位置轻轻拉一点，不让误差积累。差得太多（跳转、换片、卡顿）
//! 就直接对齐。暂停时直接用 mpv 的位置。

use std::time::Instant;

/// 和 mpv 的位置差多少秒以上就直接对齐，不再慢慢拉。
const SNAP: f64 = 0.5;
/// 每次刷新朝 mpv 的位置拉近多少（差距的比例）。
const PULL: f64 = 0.05;

/// 平滑的播放时间。
#[derive(Debug, Default)]
pub struct Clock {
    /// 上一次给出的时间，以及给出的时刻。
    last: Option<(f64, Instant)>,
}

impl Clock {
    pub fn new() -> Self {
        Self::default()
    }

    /// 现在应该画哪一刻的弹幕。`position` 是 mpv 报的播放位置（秒），
    /// `playing` 为假表示暂停、缓冲或播完，`speed` 是播放速度。
    pub fn tick(&mut self, position: f64, playing: bool, speed: f64, now: Instant) -> f64 {
        let time = match self.last {
            Some((last, at)) if playing => {
                let predicted = last + now.duration_since(at).as_secs_f64() * speed.max(0.0);
                let error = position - predicted;
                if error.abs() > SNAP {
                    position
                } else {
                    predicted + error * PULL
                }
            }
            _ => position,
        };
        self.last = Some((time, now));
        time
    }

    /// 忘掉之前的时间，下一次直接用 mpv 的位置。换片时调用。
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn it_moves_on_between_video_frames_and_follows_pauses_and_seeks() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut clock = Clock::new();
        assert_eq!(clock.tick(10.0, true, 1.0, at(0)), 10.0);
        // mpv 还停在上一帧，时钟照样往前走。
        let t = clock.tick(10.0, true, 1.0, at(16));
        assert!(t > 10.0 && t < 10.016, "{t}");
        let t = clock.tick(10.0, true, 1.0, at(33));
        assert!(t > 10.02 && t < 10.033, "{t}");
        // 两倍速走得快一倍。
        let before = clock.tick(10.042, true, 2.0, at(50));
        let after = clock.tick(10.042, true, 2.0, at(66));
        assert!((after - before - 0.032).abs() < 0.005, "{}", after - before);
        // 暂停：就是 mpv 的位置。
        assert_eq!(clock.tick(10.1, false, 1.0, at(1000)), 10.1);
        // 跳转：直接对齐。
        assert_eq!(clock.tick(300.0, true, 1.0, at(1016)), 300.0);
    }

    #[test]
    fn small_drift_is_pulled_back_without_jumping() {
        let start = Instant::now();
        let mut clock = Clock::new();
        clock.tick(0.0, true, 1.0, start);
        // mpv 一直报 0.2 秒以后的位置（比如音频时钟在前面）：时钟慢慢追上去，不会一下跳过去。
        let mut time = 0.0;
        for frame in 1..=600 {
            let now = start + Duration::from_micros(frame * 16_667);
            let position = frame as f64 / 60.0 + 0.2;
            let next = clock.tick(position, true, 1.0, now);
            assert!(next - time < 0.03, "jumped by {}", next - time);
            time = next;
        }
        assert!((time - (10.0 + 0.2)).abs() < 0.01, "{time}");
    }
}
