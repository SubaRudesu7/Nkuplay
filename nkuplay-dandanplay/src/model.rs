//! API 的请求和应答，用 Rust 的命名。传输格式是 camelCase 键名的 JSON；
//! 服务器留成 `null` 的字符串和列表按空值读取。

use crate::Error;
use crate::fingerprint::Fingerprint;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// 识别 API 比较的内容。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchMode {
    /// 先比较哈希，再比较文件名。
    #[default]
    HashAndFileName,
    FileNameOnly,
    HashOnly,
}

/// 要识别的视频文件（`POST /api/v2/match`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRequest {
    /// 文件名，不含文件夹和扩展名。
    pub file_name: String,
    /// 文件前 16 MiB 的 MD5，32 位十六进制。
    pub file_hash: String,
    /// 整个文件的长度，单位为字节。
    pub file_size: u64,
    /// 视频时长，单位为秒；未知时为 0。
    pub video_duration: u32,
    pub match_mode: MatchMode,
}

impl From<Fingerprint> for MatchRequest {
    fn from(fingerprint: Fingerprint) -> Self {
        Self {
            file_name: fingerprint.file_name,
            file_hash: fingerprint.file_hash,
            file_size: fingerprint.file_size,
            video_duration: 0,
            match_mode: MatchMode::default(),
        }
    }
}

/// 文件可能对应的一集。
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeMatch {
    /// 标识这一集及其弹幕库。
    pub episode_id: i64,
    pub anime_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub anime_title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub episode_title: String,
    /// 作品类别的文字说明，例如 `TV动画`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub type_description: String,
    /// 弹幕应延后出现的秒数；负数表示提前。
    #[serde(default)]
    pub shift: f64,
    /// 作品的海报。
    #[serde(default)]
    pub image_url: Option<String>,
}

/// 识别 API 的应答。
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchResult {
    /// 文件被精确识别：`matches` 里只有那一集。
    /// 否则 `matches` 是候选，供用户选择。
    #[serde(default)]
    pub is_matched: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    pub matches: Vec<EpisodeMatch>,
}

/// 搜索结果中的一集。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeSummary {
    pub episode_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub episode_title: String,
}

/// 搜索结果中的一部作品，以及其中匹配到的剧集。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchedAnime {
    pub anime_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub anime_title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub type_description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub episodes: Vec<EpisodeSummary>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchEpisodesBody {
    #[serde(default, deserialize_with = "null_as_default")]
    pub animes: Vec<SearchedAnime>,
}

/// 新番列表里的一部作品（`GET /api/v2/bangumi/shin` 的 `BangumiIntro`）。
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiIntro {
    pub anime_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub anime_title: String,
    /// 海报地址。
    #[serde(default)]
    pub image_url: Option<String>,
    /// 每周哪天更新：0 是星期日，1 到 6 是星期一到星期六。
    #[serde(default)]
    pub air_day: i32,
    /// 正在连载。
    #[serde(default)]
    pub is_on_air: bool,
    /// 限制级内容。
    #[serde(default)]
    pub is_restricted: bool,
    /// 综合评分，0 到 10。
    #[serde(default)]
    pub rating: f64,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BangumiListBody {
    #[serde(default, deserialize_with = "null_as_default")]
    pub bangumi_list: Vec<BangumiIntro>,
}

/// 作品详情里的一集（`BangumiEpisode`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiEpisode {
    pub episode_id: i64,
    /// 例如 `第1话 人鱼公主菈菈`、`S1 ララと…`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub episode_title: String,
    /// 正片是数字（`1`），特别篇是 `S1`，OP/ED 是 `C1`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub episode_number: String,
    /// 首播时间，例如 `2026-07-06T00:00:00`。
    #[serde(default)]
    pub air_date: Option<String>,
}

/// 作品的另一个标题（`BangumiTitle`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiTitle {
    /// 例如 `官方标题，日语`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub language: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
}

/// 作品的一个标签（`BangumiTag`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiTag {
    #[serde(default, deserialize_with = "null_as_default")]
    pub name: String,
    /// 有多少人打了这个标签。
    #[serde(default)]
    pub count: i64,
}

/// 作品在其他资料站上的页面（`BangumiOnlineDatabase`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineDatabase {
    #[serde(default, deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub url: String,
}

/// 一部作品的详情（`GET /api/v2/bangumi/{animeId}` 的 `BangumiDetails`），只取界面用到的字段。
/// 也原样存进缓存文件，所以能序列化。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiDetails {
    pub anime_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub anime_title: String,
    #[serde(default)]
    pub image_url: Option<String>,
    /// 作品类别的文字说明，例如 `TV动画`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub type_description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub titles: Vec<BangumiTitle>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub episodes: Vec<BangumiEpisode>,
    /// 番剧简介。
    #[serde(default, deserialize_with = "null_as_default")]
    pub summary: String,
    /// 制作信息，每条是 `键: 值`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub metadata: Vec<String>,
    /// 各站评分：站名和分数，按弹弹play 给的顺序。
    #[serde(default, deserialize_with = "ordered_scores")]
    pub rating_details: Vec<(String, f64)>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub tags: Vec<BangumiTag>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub online_databases: Vec<OnlineDatabase>,
    #[serde(default)]
    pub is_on_air: bool,
    /// 综合评分，0 到 10。
    #[serde(default)]
    pub rating: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BangumiDetailsBody {
    #[serde(default)]
    pub bangumi: Option<BangumiDetails>,
}

/// 把接口的 `{"站名": 分数, …}` 按出现顺序读成列表；也读缓存文件里存成的
/// `[["站名", 分数], …]`。`null` 读成空列表。
fn ordered_scores<'de, D>(deserializer: D) -> Result<Vec<(String, f64)>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Scores;
    impl<'de> serde::de::Visitor<'de> for Scores {
        type Value = Vec<(String, f64)>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an object of site names to scores")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut scores = Vec::new();
            while let Some((site, score)) = map.next_entry::<String, Option<f64>>()? {
                if let Some(score) = score {
                    scores.push((site, score));
                }
            }
            Ok(scores)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut scores = Vec::new();
            while let Some(score) = seq.next_element::<(String, f64)>()? {
                scores.push(score);
            }
            Ok(scores)
        }
    }
    deserializer.deserialize_any(Scores)
}

/// 作品的一条短评（`BangumiComment`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiComment {
    #[serde(default)]
    pub id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub user_name: String,
    /// 来自哪个网站，例如 `Bangumi`。
    #[serde(default, deserialize_with = "null_as_default")]
    pub source: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub text: String,
    /// 0 到 10；0 表示没打分。
    #[serde(default)]
    pub rating: i64,
}

/// 一页短评（`GET /api/v2/bangumi/{animeId}/comments`）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BangumiComments {
    /// 这一部作品一共多少条（不是这一页的条数）。
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    pub comments: Vec<BangumiComment>,
}

/// 调用失败时，每个应答都可能带有的内容。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Failure {
    #[serde(default = "succeeded")]
    success: bool,
    #[serde(default)]
    error_code: i32,
    #[serde(default)]
    error_message: Option<String>,
}

fn succeeded() -> bool {
    true
}

/// 读取应答体：API 用 200 加 `success: false` 报告业务错误，
/// 其余情况按文档里 `T` 的结构读取。
pub(crate) fn decode<T: DeserializeOwned>(body: &str) -> Result<T, Error> {
    let failure: Failure =
        serde_json::from_str(body).map_err(|error| Error::Decode(error.to_string()))?;
    if !failure.success {
        return Err(Error::Api {
            code: failure.error_code,
            message: failure.error_message.unwrap_or_default(),
        });
    }
    serde_json::from_str(body).map_err(|error| Error::Decode(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_match_request_is_sent_in_the_api_s_names() {
        let request = MatchRequest {
            file_name: "Show - 01".into(),
            file_hash: "658d05841b9476ccc7420b3f0bb21c3b".into(),
            file_size: 123,
            video_duration: 1440,
            match_mode: MatchMode::HashAndFileName,
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "fileName": "Show - 01",
                "fileHash": "658d05841b9476ccc7420b3f0bb21c3b",
                "fileSize": 123,
                "videoDuration": 1440,
                "matchMode": "hashAndFileName",
            })
        );
    }

    #[test]
    fn a_match_answer_is_read() {
        let body = r#"{
            "isMatched": true,
            "matches": [{
                "episodeId": 123450001, "animeId": 12345,
                "animeTitle": "某作品", "episodeTitle": "第1话 开始",
                "type": "tvseries", "typeDescription": "TV动画",
                "shift": -1.5, "imageUrl": null
            }],
            "errorCode": 0, "success": true, "errorMessage": ""
        }"#;
        let result: MatchResult = decode(body).unwrap();
        assert!(result.is_matched);
        assert_eq!(result.matches.len(), 1);
        let episode = &result.matches[0];
        assert_eq!(episode.episode_id, 123450001);
        assert_eq!(episode.anime_title, "某作品");
        assert_eq!(episode.shift, -1.5);
        assert_eq!(episode.image_url, None);
    }

    #[test]
    fn null_lists_and_strings_read_as_empty() {
        let result: MatchResult =
            decode(r#"{"isMatched": false, "matches": null, "success": true}"#).unwrap();
        assert_eq!(result, MatchResult::default());
        let body: SearchEpisodesBody = decode(
            r#"{"animes": [{"animeId": 1, "animeTitle": null, "episodes": null}], "success": true}"#,
        )
        .unwrap();
        assert_eq!(body.animes[0].anime_title, "");
        assert!(body.animes[0].episodes.is_empty());
    }

    #[test]
    fn a_shin_bangumi_answer_is_read() {
        let body: BangumiListBody = decode(
            r#"{
                "bangumiList": [
                    {"animeId": 1, "bangumiId": "b1", "animeTitle": "某新番",
                     "imageUrl": "https://img.example/1.jpg", "searchKeyword": "某新番",
                     "isOnAir": true, "airDay": 4, "isFavorited": false,
                     "isRestricted": false, "rating": 8.5},
                    {"animeId": 2, "animeTitle": null, "imageUrl": null, "airDay": 0}
                ],
                "errorCode": 0, "success": true, "errorMessage": null
            }"#,
        )
        .unwrap();
        assert_eq!(body.bangumi_list.len(), 2);
        let first = &body.bangumi_list[0];
        assert_eq!(first.anime_title, "某新番");
        assert_eq!(
            first.image_url.as_deref(),
            Some("https://img.example/1.jpg")
        );
        assert_eq!(first.air_day, 4);
        assert!(first.is_on_air);
        assert_eq!(first.rating, 8.5);
        assert_eq!(body.bangumi_list[1].anime_title, "");
        assert_eq!(body.bangumi_list[1].image_url, None);
    }

    #[test]
    fn bangumi_details_are_read_with_the_scores_in_their_order() {
        let body: BangumiDetailsBody = decode(
            r#"{
                "bangumi": {
                    "type": "tvseries", "typeDescription": "TV动画",
                    "titles": [{"language": "官方标题，日语", "title": "さよならララ"}],
                    "seasons": [],
                    "episodes": [
                        {"seasonId": null, "episodeId": 186430001, "episodeTitle": "第1话 人鱼公主菈菈",
                         "episodeNumber": "1", "lastWatched": null, "airDate": "2026-07-06T00:00:00"},
                        {"episodeId": 186439001, "episodeTitle": "S1 ララと", "episodeNumber": "S1"}
                    ],
                    "summary": "很久很久以前", "intro": "", "metadata": ["话数: 12"],
                    "ratingDetails": {"弹弹play连载中评分": 9.11, "Bangumi评分": 6.4, "空": null},
                    "relateds": [], "similars": null,
                    "tags": [{"id": 2, "name": "恋爱", "count": 825}],
                    "onlineDatabases": [{"name": "TMDB", "url": "https://www.themoviedb.org/tv/295071"}],
                    "trailers": [],
                    "animeId": 18643, "bangumiId": "18643", "animeTitle": "再见，拉拉",
                    "imageUrl": "https://img.example/18643.jpg", "isOnAir": false, "airDay": 1,
                    "rating": 6.9
                },
                "errorCode": 0, "success": true, "errorMessage": ""
            }"#,
        )
        .unwrap();
        let details = body.bangumi.unwrap();
        assert_eq!(details.anime_id, 18643);
        assert_eq!(details.type_description, "TV动画");
        assert_eq!(details.episodes[1].episode_number, "S1");
        assert_eq!(details.episodes[1].air_date, None);
        assert_eq!(
            details.rating_details,
            [
                ("弹弹play连载中评分".to_string(), 9.11),
                ("Bangumi评分".to_string(), 6.4)
            ]
        );
        assert_eq!(details.tags[0].name, "恋爱");
        // 存进缓存再读回来，内容不变。
        let stored = serde_json::to_string(&details).unwrap();
        assert_eq!(
            serde_json::from_str::<BangumiDetails>(&stored).unwrap(),
            details
        );
    }

    #[test]
    fn a_missing_bangumi_and_a_page_of_comments_are_read() {
        let body: BangumiDetailsBody = decode(r#"{"bangumi": null, "success": true}"#).unwrap();
        assert!(body.bangumi.is_none());
        let comments: BangumiComments = decode(
            r#"{"count": 20, "hasMore": true, "comments": [
                {"id": 1, "userId": 0, "userName": "104期的杂鱼", "imageUrl": null,
                 "source": "Bangumi", "text": "第一集", "rating": 5, "updatedTime": "2026-11-18T04:19:34"}
            ], "success": true}"#,
        )
        .unwrap();
        assert_eq!(comments.count, 20);
        assert_eq!(comments.comments[0].user_name, "104期的杂鱼");
        assert_eq!(comments.comments[0].rating, 5);
    }

    #[test]
    fn a_business_error_is_reported_with_its_code_and_message() {
        let error = decode::<MatchResult>(
            r#"{"success": false, "errorCode": 1, "errorMessage": "服务器内部错误"}"#,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::Api { code: 1, ref message } if message == "服务器内部错误"
        ));
    }

    #[test]
    fn something_that_is_not_the_documented_json_is_a_decode_error() {
        assert!(matches!(
            decode::<MatchResult>("<html>blocked</html>").unwrap_err(),
            Error::Decode(_)
        ));
    }
}
