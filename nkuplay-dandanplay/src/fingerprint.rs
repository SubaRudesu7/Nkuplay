//! 弹弹play 识别视频文件的依据：文件前 16 MiB 的 MD5，加上文件大小和文件名。

use md5::{Digest, Md5};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// 文件中参与哈希的部分。
const HASHED_BYTES: u64 = 16 * 1024 * 1024;

/// 供识别 API 辨认一个视频文件的信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// 文件名，不含文件夹和扩展名。
    pub file_name: String,
    /// 前 16 MiB 的 MD5，32 位小写十六进制。
    pub file_hash: String,
    /// 整个文件的长度，单位为字节。
    pub file_size: u64,
}

/// 读取 `path` 处文件的指纹。无论文件多大，只读前 16 MiB。
pub fn fingerprint(path: &Path) -> std::io::Result<Fingerprint> {
    let file = File::open(path)?;
    let file_size = file.metadata()?.len();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    fingerprint_reader(&name, file, file_size)
}

/// 从一个已经打开的文件读取指纹：`name` 是用户看到的文件名（可带扩展名），
/// `reader` 从文件开头读起，`file_size` 是整个文件的长度。只读前 16 MiB。
pub fn fingerprint_reader(
    name: &str,
    reader: impl Read,
    file_size: u64,
) -> std::io::Result<Fingerprint> {
    let mut hash = Md5::new();
    let mut head = reader.take(HASHED_BYTES);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = head.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let file_hash = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let file_name = Path::new(name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Fingerprint {
        file_name,
        file_hash,
        file_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 系统临时文件夹里的一个文件，销毁时删除。
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn with(name: &str, write: impl FnOnce(&mut File)) -> Self {
            let path = std::env::temp_dir()
                .join(format!("nkuplay-dandanplay-{}-{name}", std::process::id()));
            let mut file = File::create(&path).unwrap();
            write(&mut file);
            file.flush().unwrap();
            TempFile(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_small_file_is_hashed_whole() {
        let file = TempFile::with("[Sub] Show - 01.mkv", |file| {
            file.write_all(b"hello dandanplay").unwrap();
        });
        let print = fingerprint(&file.0).unwrap();
        assert!(print.file_name.ends_with("[Sub] Show - 01"));
        assert_eq!(print.file_size, 16);
        // md5("hello dandanplay")，用 Python 的 hashlib 算出。
        assert_eq!(print.file_hash, "a9759ca91fb7420de375cc159dab29a0");
    }

    #[test]
    fn only_the_first_16_mib_are_hashed() {
        // 16 MiB 的 0x61 后面跟 1 KiB 的 0x62：末尾这段不应改变哈希。
        let file = TempFile::with("large.bin", |file| {
            let block = vec![0x61u8; 1024 * 1024];
            for _ in 0..16 {
                file.write_all(&block).unwrap();
            }
            file.write_all(&[0x62u8; 1024]).unwrap();
        });
        let print = fingerprint(&file.0).unwrap();
        assert_eq!(print.file_size, 16 * 1024 * 1024 + 1024);
        // 16 MiB 的 0x61 的 md5，用 Python 的 hashlib 算出。
        assert_eq!(print.file_hash, "f4820540fc0ac02750739896fe028d56");
    }

    #[test]
    fn a_reader_is_fingerprinted_like_the_file_it_reads() {
        let print = fingerprint_reader("Show - 02.mp4", &b"hello dandanplay"[..], 16).unwrap();
        assert_eq!(print.file_name, "Show - 02");
        assert_eq!(print.file_size, 16);
        assert_eq!(print.file_hash, "a9759ca91fb7420de375cc159dab29a0");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(fingerprint(Path::new("no/such/file.mkv")).is_err());
    }
}
