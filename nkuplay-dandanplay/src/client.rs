//! HTTP 部分：向 API 发送带签名的请求并读取应答。

use crate::auth::Credentials;
use crate::comment::{Comment, CommentsBody};
use crate::error::Error;
use crate::model::{MatchRequest, MatchResult, SearchEpisodesBody, SearchedAnime, decode};
use serde::de::DeserializeOwned;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 弹弹play 文档给出的服务器地址。
pub const DEFAULT_BASE_URL: &str = "https://api.dandanplay.net";

const USER_AGENT: &str = concat!("nkuplay/", env!("CARGO_PKG_VERSION"));
const TIMEOUT: Duration = Duration::from_secs(20);

/// 是否让服务器对弹幕做简繁转换。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChineseConversion {
    #[default]
    None,
    Simplified,
    Traditional,
}

impl ChineseConversion {
    fn code(self) -> &'static str {
        match self {
            ChineseConversion::None => "0",
            ChineseConversion::Simplified => "1",
            ChineseConversion::Traditional => "2",
        }
    }
}

/// 弹弹play API 的客户端。调用会阻塞到服务器应答为止，所以不要在 UI 线程上调用。
pub struct Client {
    base_url: String,
    credentials: Credentials,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(credentials: Credentials) -> Self {
        Self::with_base_url(credentials, DEFAULT_BASE_URL)
    }

    /// 连接另一台使用同样 API 的服务器，例如代为保管 AppSecret 的中转服务器，或测试服务器。
    pub fn with_base_url(credentials: Credentials, base_url: impl Into<String>) -> Self {
        let agent = ureq::Agent::config_builder()
            // 拒绝应答由我们自己读：403 会在响应头里说明原因。
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .user_agent(USER_AGENT)
            .build()
            .into();
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            credentials,
            agent,
        }
    }

    /// 用编译进程序的凭证创建客户端（见 [`Credentials::built_in`]）。
    /// 构建时没有提供凭证则返回 [`Error::MissingCredentials`]。
    pub fn built_in() -> Result<Self, Error> {
        Credentials::built_in()
            .map(Self::new)
            .ok_or(Error::MissingCredentials)
    }

    /// 识别一个视频文件是哪一集。`is_matched` 为真时唯一的结果是确定的；
    /// 否则这些结果是候选，供用户选择。
    pub fn match_file(&self, request: &MatchRequest) -> Result<MatchResult, Error> {
        let body =
            serde_json::to_string(request).map_err(|error| Error::Decode(error.to_string()))?;
        self.post("/api/v2/match", body)
    }

    /// 搜索标题包含 `anime` 的作品的剧集（至少两个字符；支持中文、日语、罗马音、英语）。
    /// `episode` 只保留指定的那一集：一个数字，或用 `C1`/`S1`/`O1` 表示特别篇。
    pub fn search_episodes(
        &self,
        anime: &str,
        episode: Option<&str>,
    ) -> Result<Vec<SearchedAnime>, Error> {
        let mut query = vec![("anime", anime)];
        if let Some(episode) = episode.filter(|episode| !episode.is_empty()) {
            query.push(("episode", episode));
        }
        let body: SearchEpisodesBody = self.get("/api/v2/search/episodes", &query)?;
        Ok(body.animes)
    }

    /// 某一集的弹幕，按出现时间排序。`with_related` 会加上弹弹play 从其他网站
    /// 合并来的弹幕，这部分占大多数，也是弹弹play 推荐的用法。
    pub fn comments(
        &self,
        episode_id: i64,
        with_related: bool,
        conversion: ChineseConversion,
    ) -> Result<Vec<Comment>, Error> {
        let path = format!("/api/v2/comment/{episode_id}");
        let query = [
            ("withRelated", if with_related { "true" } else { "false" }),
            ("chConvert", conversion.code()),
        ];
        let body: CommentsBody = self.get(&path, &query)?;
        Ok(body.into_comments())
    }

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, Error> {
        let mut request = self.agent.get(format!("{}{path}", self.base_url));
        for (name, value) in self.headers(path, now()) {
            request = request.header(name, value);
        }
        for (name, value) in query {
            request = request.query(*name, *value);
        }
        answer(request.call())
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: String) -> Result<T, Error> {
        let mut request = self
            .agent
            .post(format!("{}{path}", self.base_url))
            .header("Content-Type", "application/json");
        for (name, value) in self.headers(path, now()) {
            request = request.header(name, value);
        }
        answer(request.send(body))
    }

    /// 为在 `timestamp` 时刻请求 `path` 签名所需的请求头。
    fn headers(&self, path: &str, timestamp: u64) -> [(&'static str, String); 4] {
        [
            ("Accept", "application/json".to_string()),
            ("X-AppId", self.credentials.app_id.clone()),
            ("X-Timestamp", timestamp.to_string()),
            ("X-Signature", self.credentials.signature(timestamp, path)),
        ]
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn answer<T: DeserializeOwned>(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<T, Error> {
    let mut response = response.map_err(|error| Error::Transport(error.to_string()))?;
    let code = response.status().as_u16();
    if !(200..300).contains(&code) {
        let reason = response
            .headers()
            .get("X-Error-Message")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        return Err(Error::Status { code, reason });
    }
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| Error::Transport(error.to_string()))?;
    decode(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{Receiver, channel};

    /// 本机上的一个服务器：用 `status`（状态行和附加的响应头行）和 `body` 应答一次请求，
    /// 并把收到的内容（请求行、请求头和请求体）交回来。
    fn serve_once(status: &'static str, body: &'static str) -> (String, Receiver<String>) {
        let response = format!(
            "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (sent, received) = channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                let end = line == "\r\n" || line.is_empty();
                request.push_str(&line);
                if end {
                    break;
                }
            }
            let mut sent_body = vec![0; length];
            reader.read_exact(&mut sent_body).unwrap();
            request.push_str(&String::from_utf8_lossy(&sent_body));
            reader.get_mut().write_all(response.as_bytes()).unwrap();
            let _ = sent.send(request);
        });
        (address, received)
    }

    fn client(address: String) -> Client {
        Client::with_base_url(Credentials::new("my-app", "my-secret"), address)
    }

    fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }

    #[test]
    fn requests_are_signed_for_their_path_and_time() {
        let client = client("http://unused".into());
        let headers = client.headers("/api/v2/match", 1735660800);
        let value = |name: &str| {
            headers
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.as_str())
                .unwrap()
        };
        assert_eq!(value("X-AppId"), "my-app");
        assert_eq!(value("X-Timestamp"), "1735660800");
        assert_eq!(
            value("X-Signature"),
            Credentials::new("my-app", "my-secret").signature(1735660800, "/api/v2/match")
        );
        // 密钥本身绝不会被发送。
        assert!(
            headers
                .iter()
                .all(|(_, value)| !value.contains("my-secret"))
        );
    }

    #[test]
    fn comments_are_fetched_with_the_signature_of_the_path_without_its_query() {
        let (address, received) = serve_once(
            "HTTP/1.1 200 OK",
            r#"{"count":1,"comments":[{"cid":7,"p":"3.5,1,16777215,u","m":"hi"}]}"#,
        );
        let comments = client(address)
            .comments(123450001, true, ChineseConversion::Simplified)
            .unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "hi");

        let request = received.recv().unwrap();
        let line = request.lines().next().unwrap();
        assert!(
            line.starts_with("GET /api/v2/comment/123450001?withRelated=true&chConvert=1 "),
            "{line}"
        );
        let timestamp: u64 = header(&request, "X-Timestamp").unwrap().parse().unwrap();
        assert_eq!(
            header(&request, "X-Signature").unwrap(),
            Credentials::new("my-app", "my-secret")
                .signature(timestamp, "/api/v2/comment/123450001")
        );
        assert_eq!(header(&request, "X-AppId"), Some("my-app"));
        assert!(!request.contains("my-secret"));
    }

    #[test]
    fn a_file_is_matched_by_posting_its_fingerprint_as_json() {
        let (address, received) = serve_once(
            "HTTP/1.1 200 OK",
            r#"{"isMatched":true,"matches":[{"episodeId":9,"animeId":1,"animeTitle":"A","episodeTitle":"E"}],"success":true}"#,
        );
        let result = client(address)
            .match_file(&MatchRequest {
                file_name: "Show - 01".into(),
                file_hash: "658d05841b9476ccc7420b3f0bb21c3b".into(),
                file_size: 5,
                video_duration: 0,
                match_mode: Default::default(),
            })
            .unwrap();
        assert!(result.is_matched);
        assert_eq!(result.matches[0].episode_id, 9);

        let request = received.recv().unwrap();
        assert!(request.starts_with("POST /api/v2/match "), "{request}");
        assert_eq!(header(&request, "Content-Type"), Some("application/json"));
        let body = request.split("\r\n\r\n").nth(1).unwrap();
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(sent["fileHash"], "658d05841b9476ccc7420b3f0bb21c3b");
        assert_eq!(sent["matchMode"], "hashAndFileName");
    }

    #[test]
    fn a_refusal_carries_the_server_s_reason() {
        let (address, _received) = serve_once(
            "HTTP/1.1 403 Forbidden\r\nX-Error-Message: Invalid Signature",
            "",
        );
        let error = client(address)
            .search_episodes("show", Some("1"))
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Status { code: 403, reason: Some(ref reason) } if reason == "Invalid Signature"
        ));
    }

    #[test]
    fn an_unreachable_server_is_a_transport_error() {
        // 监听器关闭后，它用过的端口上没有任何服务。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let error = client(address).search_episodes("show", None).unwrap_err();
        assert!(matches!(error, Error::Transport(_)), "{error:?}");
    }
}
