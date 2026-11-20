//! 弹弹play 开放弹幕网络（`api.dandanplay.net`，API v2）的客户端：
//! 识别视频文件、按标题搜索剧集、获取某一集的弹幕、本季新番列表，以及作品的详情和短评。
//!
//! 弹弹play 文档描述的常规流程：
//!
//! 1. 用 [`fingerprint`] 计算视频文件的指纹，再用 [`Client::match_file`]
//!    查出它是哪一集（得到 `episode_id`）。
//! 2. 识别不出来时，用 [`Client::search_episodes`] 按标题搜索，让用户选择。
//! 3. 用 `episode_id` 调用 [`Client::comments`]。
//!
//! 每个请求都用弹弹play 为应用签发的 AppId 和 AppSecret 签名，见 [`Credentials`]。
//! 返回的结果很少变化，弹弹play 要求客户端缓存它们，并且只在用户操作时才调用。

mod auth;
mod client;
mod comment;
mod error;
mod fingerprint;
mod model;

pub use auth::Credentials;
pub use client::{ChineseConversion, Client, DEFAULT_BASE_URL};
pub use comment::{Comment, CommentMode};
pub use error::Error;
pub use fingerprint::{Fingerprint, fingerprint, fingerprint_reader};
pub use model::{
    BangumiComment, BangumiComments, BangumiDetails, BangumiEpisode, BangumiIntro, BangumiTag,
    BangumiTitle, EpisodeMatch, EpisodeSummary, MatchMode, MatchRequest, MatchResult,
    OnlineDatabase, SearchedAnime,
};
