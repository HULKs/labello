use std::{fs, io::Read, path::Path};

use anyhow::{Result, ensure};
use flate2::{Compression, read::GzEncoder};

/// Build sidecars before inventorying the release. Runtime configuration is
/// deliberately excluded: it belongs to the deployment configuration tree.
pub fn compress_browser_assets(root: impl AsRef<Path>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "browser assets must not contain symlinks"
        );
        let path = entry.path();
        if kind.is_dir() {
            compress_browser_assets(path)?;
        } else if kind.is_file()
            && matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("wasm" | "js")
            )
        {
            let input = fs::read(&path)?;
            let mut gzip = Vec::new();
            GzEncoder::new(input.as_slice(), Compression::best()).read_to_end(&mut gzip)?;
            let mut brotli = Vec::new();
            brotli::CompressorReader::new(input.as_slice(), 64 * 1024, 11, 22)
                .read_to_end(&mut brotli)?;
            for (extension, bytes) in [("gz", gzip), ("br", brotli)] {
                let mut name = path.as_os_str().to_os_string();
                name.push(format!(".{extension}"));
                ensure!(
                    !fs::symlink_metadata(&name).is_ok_and(|metadata| metadata.is_symlink()),
                    "browser sidecars must not be symlinks"
                );
                fs::write(name, bytes)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecars_decode_to_the_original_and_are_reproducible() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("snippets");
        fs::create_dir(&scripts).unwrap();
        let input = b"browser module fixture\n".repeat(256);
        fs::write(root.path().join("client.wasm"), &input).unwrap();
        fs::write(scripts.join("loader.js"), &input).unwrap();
        fs::write(root.path().join("labello.client.json"), b"{}").unwrap();
        compress_browser_assets(root.path()).unwrap();
        for path in [root.path().join("client.wasm"), scripts.join("loader.js")] {
            let gzip_path = path.with_file_name(format!(
                "{}.gz",
                path.file_name().unwrap().to_str().unwrap()
            ));
            let brotli_path = path.with_file_name(format!(
                "{}.br",
                path.file_name().unwrap().to_str().unwrap()
            ));
            let gzip = fs::read(&gzip_path).unwrap();
            let brotli = fs::read(&brotli_path).unwrap();
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(gzip.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            assert_eq!(decoded, input);
            decoded.clear();
            brotli::Decompressor::new(brotli.as_slice(), 4096)
                .read_to_end(&mut decoded)
                .unwrap();
            assert_eq!(decoded, input);
            compress_browser_assets(root.path()).unwrap();
            assert_eq!(fs::read(gzip_path).unwrap(), gzip);
            assert_eq!(fs::read(brotli_path).unwrap(), brotli);
        }
        assert!(!root.path().join("labello.client.json.br").exists());
    }

    #[test]
    fn sidecars_cannot_overwrite_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), b"retained").unwrap();
        fs::write(root.path().join("client.wasm"), b"wasm").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("client.wasm.br")).unwrap();
        assert!(compress_browser_assets(root.path()).is_err());
        assert_eq!(fs::read(outside.path()).unwrap(), b"retained");
    }
}
