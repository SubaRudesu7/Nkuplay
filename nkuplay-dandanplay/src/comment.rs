//! API 返回的弹幕：文本 `m`，以及形如 `时间,模式,颜色,用户` 的参数串 `p`。

use serde::Deserialize;

/// 弹幕显示的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentMode {
    /// 从右侧滚动穿过画面。
    Scroll,
    /// 停在底部。
    Bottom,
    /// 停在顶部。
    Top,
}

/// 某一集的一条弹幕。
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub id: i64,
    /// 出现的时间，视频开始后的秒数。
    pub time: f64,
    pub mode: CommentMode,
    /// `0xRRGGBB`。
    pub color: u32,
    /// 发送者；从其他网站合并来的弹幕会用方括号标出来源网站，例如 `[BiliBili]…`。
    pub user: String,
    pub text: String,
}

#[derive(Deserialize)]
struct Wire {
    #[serde(default)]
    cid: i64,
    #[serde(default)]
    p: Option<String>,
    #[serde(default)]
    m: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CommentsBody {
    #[serde(default)]
    comments: Option<Vec<Wire>>,
}

impl CommentsBody {
    /// 按在视频中出现的顺序排列的弹幕。参数读不出来的弹幕会被跳过，不影响其余弹幕。
    pub(crate) fn into_comments(self) -> Vec<Comment> {
        let mut comments = self
            .comments
            .unwrap_or_default()
            .into_iter()
            .filter_map(|wire| {
                parse(
                    wire.cid,
                    wire.p.as_deref().unwrap_or(""),
                    wire.m.unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        comments.sort_by(|a, b| a.time.total_cmp(&b.time));
        comments
    }
}

fn parse(id: i64, parameters: &str, text: String) -> Option<Comment> {
    // 用户在最后一段，它本身可能含有逗号。
    let mut fields = parameters.splitn(4, ',');
    let time = fields.next()?.trim().parse::<f64>().ok()?;
    if !time.is_finite() || time < 0.0 {
        return None;
    }
    let mode = match fields.next()?.trim().parse::<u32>().ok()? {
        4 => CommentMode::Bottom,
        5 => CommentMode::Top,
        _ => CommentMode::Scroll,
    };
    let color = fields.next()?.trim().parse::<u32>().ok()? & 0x00FF_FFFF;
    let user = fields.next().unwrap_or("").to_string();
    Some(Comment {
        id,
        time,
        mode,
        color,
        user,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(body: &str) -> Vec<Comment> {
        serde_json::from_str::<CommentsBody>(body)
            .unwrap()
            .into_comments()
    }

    #[test]
    fn comments_are_read_and_put_in_order_of_appearance() {
        let comments = read(
            r#"{"count": 3, "comments": [
                {"cid": 2, "p": "30.50,5,16711680,[BiliBili]abc", "m": "顶部"},
                {"cid": 1, "p": "1.25,1,16777215,42", "m": "第一条"},
                {"cid": 3, "p": "12.00,4,255,user,with,commas", "m": "底部"}
            ]}"#,
        );
        assert_eq!(
            comments
                .iter()
                .map(|comment| comment.id)
                .collect::<Vec<_>>(),
            [1, 3, 2]
        );
        assert_eq!(
            comments[0],
            Comment {
                id: 1,
                time: 1.25,
                mode: CommentMode::Scroll,
                color: 0xFFFFFF,
                user: "42".into(),
                text: "第一条".into(),
            }
        );
        assert_eq!(comments[1].mode, CommentMode::Bottom);
        assert_eq!(comments[1].user, "user,with,commas");
        assert_eq!(comments[2].mode, CommentMode::Top);
        assert_eq!(comments[2].color, 0xFF0000);
    }

    #[test]
    fn a_comment_that_cannot_be_read_is_left_out() {
        let comments = read(
            r#"{"comments": [
                {"cid": 1, "p": "not-a-time,1,0,u", "m": "x"},
                {"cid": 2, "p": "5.0,1", "m": "x"},
                {"cid": 3, "p": null, "m": "x"},
                {"cid": 4, "p": "5.0,1,0,u", "m": "kept"}
            ]}"#,
        );
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "kept");
    }

    #[test]
    fn an_episode_without_comments_is_an_empty_list() {
        assert!(read(r#"{"count": 0, "comments": null}"#).is_empty());
        assert!(read(r#"{"count": 0}"#).is_empty());
    }
}
