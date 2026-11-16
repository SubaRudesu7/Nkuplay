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
