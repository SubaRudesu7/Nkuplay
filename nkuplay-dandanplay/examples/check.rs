//! 检查弹弹play 凭证有没有编译进来，并可以对真实服务器试一次调用。
//!
//! ```text
//! cargo run -p nkuplay-dandanplay --example check                  只看凭证是否已注入
//! cargo run -p nkuplay-dandanplay --example check -- 葬送的芙莉莲    按标题搜索剧集
//! cargo run -p nkuplay-dandanplay --example check -- D:\番\01.mkv   识别一个视频文件
//! ```
//!
//! 后两种会向 `api.dandanplay.net` 发一次请求。

use nkuplay_dandanplay::{Client, Credentials, MatchRequest, fingerprint};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(credentials) = Credentials::built_in() else {
        println!("凭证：未注入。");
        println!("构建前设置环境变量 DANDANPLAY_APP_ID 和 DANDANPLAY_APP_SECRET，再重新构建。");
        return ExitCode::FAILURE;
    };
    // 只显示 AppId；AppSecret 不打印。
    println!("凭证：已注入（AppId：{}）。", credentials.app_id);

    let Some(argument) = std::env::args().nth(1) else {
        return ExitCode::SUCCESS;
    };
    let client = Client::new(credentials);
    let path = Path::new(&argument);
    let outcome = if path.is_file() {
        identify(&client, path)
    } else {
        search(&client, &argument)
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            println!("失败：{error}");
            ExitCode::FAILURE
        }
    }
}

fn identify(client: &Client, path: &Path) -> Result<(), nkuplay_dandanplay::Error> {
    let print = fingerprint(path)?;
    println!(
        "文件：{}（{} 字节，hash {}）",
        print.file_name, print.file_size, print.file_hash
    );
    let result = client.match_file(&MatchRequest::from(print))?;
    println!(
        "{}，共 {} 个结果：",
        if result.is_matched {
            "精确匹配"
        } else {
            "没有精确匹配，以下是候选"
        },
        result.matches.len()
    );
    for episode in result.matches.iter().take(10) {
        println!(
            "  {}  {} - {}",
            episode.episode_id, episode.anime_title, episode.episode_title
        );
    }
    if let Some(first) = result.matches.first().filter(|_| result.is_matched) {
        let comments = client.comments(first.episode_id, true, Default::default())?;
        println!("这一集有 {} 条弹幕。", comments.len());
    }
    Ok(())
}

fn search(client: &Client, title: &str) -> Result<(), nkuplay_dandanplay::Error> {
    let animes = client.search_episodes(title, None)?;
    println!("“{title}”搜到 {} 部作品：", animes.len());
    for anime in animes.iter().take(10) {
        println!(
            "  {}  {}（{} 集）",
            anime.anime_id,
            anime.anime_title,
            anime.episodes.len()
        );
    }
    Ok(())
}
