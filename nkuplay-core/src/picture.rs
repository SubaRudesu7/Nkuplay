//! 看一张画面是不是几乎全黑（片头、过场）：继续播放的卡片不用这样的画面。

use std::path::Path;
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

/// 亮度（0 到 255）超过多少才算看得见。
const VISIBLE: u8 = 80;
/// 看得见的地方不到几分之一，就算全黑。
const MIN_VISIBLE: usize = 20;

/// 黑白画面（每个像素一个亮度）是不是几乎全黑。
fn mostly_black(luma: &[u8]) -> bool {
    let visible = luma.iter().filter(|&&value| value > VISIBLE).count();
    !luma.is_empty() && visible * MIN_VISIBLE < luma.len()
}

/// `path` 处的 JPEG 是不是几乎全黑；读不出来时当作不是。会阻塞。
pub fn is_black(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma);
    JpegDecoder::new_with_options(ZCursor::new(bytes), options)
        .decode()
        .is_ok_and(|luma| mostly_black(&luma))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_with_almost_nothing_visible_is_black() {
        assert!(mostly_black(&[16; 100]));
        // 几个亮点（字幕）不算。
        let mut subtitle = [16; 100];
        subtitle[..4].fill(235);
        assert!(mostly_black(&subtitle));
        // 夜景的亮处多一些。
        let mut night = [30; 100];
        night[..20].fill(120);
        assert!(!mostly_black(&night));
        assert!(!mostly_black(&[]));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_not_black() {
        assert!(!is_black(Path::new("no-such-file.jpg")));
    }

    /// 草稿里的几张真实画面；别的机器上没有这些文件，跳过。
    #[test]
    fn real_shots_are_told_apart() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/home/shots");
        for (name, black) in [
            ("672a4ec1140f0ec0-1795185222.jpg", true),
            ("95ef55bb88638111-1795185246.jpg", true),
            ("cbd0426fe9824392-1795183028.jpg", false),
            ("1a4fa6a1100802c2-1795180926.jpg", false),
        ] {
            let path = folder.join(name);
            if path.is_file() {
                assert_eq!(is_black(&path), black, "{name}");
            }
        }
    }
}
