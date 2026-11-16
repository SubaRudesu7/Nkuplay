//! 把媒体位置转成 mpv 接受的形式。

/// 对 `url` 应该交给 `loadfile` 的内容：`file:` 地址转成它指向的路径，
/// 其他地址（http、https）原样传递。
pub fn to_mpv_location(url: &str) -> String {
    let Some(rest) = url.strip_prefix("file://") else {
        return url.to_string();
    };
    let decoded = percent_decode(rest);
    // `file:///C:/dir/a.mkv` 指的是 `C:/dir/a.mkv`；`file://server/share/a.mkv`
    // 指的是网络路径 `//server/share/a.mkv`。
    match decoded.strip_prefix('/') {
        Some(path) if has_drive(path) => path.to_string(),
        Some(_) => decoded,
        None => format!("//{decoded}"),
    }
}

fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths_and_other_urls_pass_through() {
        assert_eq!(
            to_mpv_location("file:///C:/videos/a.mkv"),
            "C:/videos/a.mkv"
        );
        assert_eq!(
            to_mpv_location("file:///C:/my%20videos/%E7%95%AA.mkv"),
            "C:/my videos/番.mkv"
        );
        assert_eq!(
            to_mpv_location("file://nas/share/a.mkv"),
            "//nas/share/a.mkv"
        );
        assert_eq!(to_mpv_location("file:///home/me/a.mkv"), "/home/me/a.mkv");
        assert_eq!(
            to_mpv_location("https://example.com/a%20b.mp4"),
            "https://example.com/a%20b.mp4"
        );
    }

    #[test]
    fn a_stray_percent_sign_is_kept() {
        assert_eq!(to_mpv_location("file:///C:/100%.mkv"), "C:/100%.mkv");
    }
}
