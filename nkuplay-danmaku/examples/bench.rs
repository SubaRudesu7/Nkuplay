//! 量一量排版和每帧生成 ASS 文本要多久：`cargo run -p nkuplay-danmaku --example bench --release`。
//!
//! 造一集 24 分钟、3000 条的弹幕，其中第 600 秒附近 5 秒内挤进 400 条（高能时刻），
//! 用系统字体量宽度，再按 60 Hz 把整集的每一帧都生成一遍。

use nkuplay_danmaku::{Comment, Config, Layout, Measure, Mode, Track};
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let texts = [
        "前方高能",
        "哈哈哈哈哈哈",
        "awsl",
        "这一段作画太好了吧，制作组加鸡腿",
        "2333",
        "名场面来了",
        "泪目",
        "OP 好听",
    ];
    let mut comments = Vec::new();
    for i in 0..2600 {
        comments.push(Comment {
            time: i as f64 * 1440.0 / 2600.0,
            mode: match i % 20 {
                0 => Mode::Top,
                1 => Mode::Bottom,
                _ => Mode::Scroll,
            },
            color: 0xFFFFFF,
            text: texts[i % texts.len()].into(),
        });
    }
    for i in 0..400 {
        comments.push(Comment {
            time: 600.0 + i as f64 * 5.0 / 400.0,
            mode: Mode::Scroll,
            color: 0xFFCC00,
            text: texts[i % texts.len()].into(),
        });
    }

    let started = Instant::now();
    let measure = Measure::system();
    let font = started.elapsed();
    let started = Instant::now();
    let track = Arc::new(Track::new(&comments, 0.0, Config::default(), &measure));
    let prepared = started.elapsed();
    let started = Instant::now();
    let layout = Layout::new(Arc::clone(&track), 1920.0);
    let laid_out = started.elapsed();
    println!(
        "字体 {:?}（读到字体：{}），整理 {} 条 {prepared:?}，排版 {laid_out:?}，排进 {} 条",
        font,
        measure.has_font(),
        track.len(),
        layout.len()
    );

    let mut slowest = (std::time::Duration::ZERO, 0.0, 0);
    let mut total = std::time::Duration::ZERO;
    let frames = 1440 * 60;
    for frame in 0..frames {
        let time = frame as f64 / 60.0;
        let started = Instant::now();
        let events = layout.events(time);
        let took = started.elapsed();
        total += took;
        if took > slowest.0 {
            slowest = (took, time, events.lines().count());
        }
    }
    println!(
        "每帧平均 {:?}，最慢 {:?}（第 {:.1} 秒，同屏 {} 条）",
        total / frames,
        slowest.0,
        slowest.1,
        slowest.2
    );
}
