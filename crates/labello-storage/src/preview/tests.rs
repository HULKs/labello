use super::*;
use image::{ImageBuffer, Rgba};
use std::{fs, sync::atomic::Ordering};

struct Fixture {
    _root: tempfile::TempDir,
    repo: DatasetRepository,
    cache: PreviewCache,
    record: ImageRecord,
}
impl Fixture {
    fn new(width: u32, height: u32, config: PreviewConfig) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = DatasetRepository::new(root.path().join("dataset"));
        fs::create_dir_all(repo.root()).unwrap();
        let pixels = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([
                (x % 251) as u8,
                (y % 241) as u8,
                ((x + y) % 239) as u8,
                if x % 5 == 0 { 0 } else { 255 },
            ])
        });
        pixels.save(repo.root().join("source.png")).unwrap();
        let bytes = fs::read(repo.root().join("source.png")).unwrap();
        let record = ImageRecord {
            image_id: "image".into(),
            blake3: blake3::hash(&bytes).to_hex().to_string(),
            canonical_path: "source.png".into(),
            known_paths: vec!["source.png".into()],
            duplicate_paths: vec![],
            file_name: "source.png".into(),
            byte_size: bytes.len() as u64,
            width,
            height,
            media_type: "image/png".into(),
            source_memberships: None,
        };
        let cache = PreviewCache::new(root.path().join("cache"), config).unwrap();
        Self {
            _root: root,
            repo,
            cache,
            record,
        }
    }
    async fn get(&self, profile: PreviewProfile) -> Result<EncodedPreview, PreviewError> {
        self.cache.get(&self.repo, &self.record, profile).await
    }
    fn generations(&self) -> usize {
        self.cache.inner.generations.load(Ordering::SeqCst)
    }
    fn cache_files(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.cache.inner.root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "preview"))
            .collect()
    }
}

#[tokio::test]
async fn standard_is_exact_legacy_rgba_for_small_portrait_landscape_and_transparent_pixels() {
    for (width, height) in [(19, 11), (17, 1701), (1701, 17)] {
        let fixture = Fixture::new(width, height, PreviewConfig::default());
        let encoded = fixture.get(PreviewProfile::StandardV1).await.unwrap();
        let old = image::open(fixture.repo.root().join("source.png")).unwrap();
        let old = if width.max(height) > 1600 {
            old.resize(1600, 1600, image::imageops::FilterType::Triangle)
        } else {
            old
        };
        let actual = image::load_from_memory_with_format(&encoded.webp, image::ImageFormat::WebP)
            .unwrap()
            .to_rgba8();
        assert_eq!(actual, old.to_rgba8());
        assert_eq!(
            (encoded.original_width, encoded.original_height),
            (width, height)
        );
        assert_eq!((encoded.width, encoded.height), actual.dimensions());
        assert_eq!(
            fixture.get(PreviewProfile::StandardV1).await.unwrap().webp,
            encoded.webp
        );
        assert_eq!(fixture.generations(), 1);
        let saver = fixture.get(PreviewProfile::DataSaverV1).await.unwrap();
        assert!(saver.width.max(saver.height) <= 1280);
        assert!(saver.width <= width && saver.height <= height);
        assert_eq!(
            saver.webp,
            codec::encode(
                &codec::resize(
                    &fs::read(fixture.repo.root().join("source.png")).unwrap(),
                    &fixture.record,
                    1280,
                    &PreviewConfig::default()
                )
                .unwrap(),
                PreviewProfile::DataSaverV1
            )
            .unwrap()
        );
        assert_eq!(fixture.generations(), 2);
    }
}

#[tokio::test]
async fn corruption_regenerates_and_restart_reuses_only_complete_entries() {
    let fixture = Fixture::new(19, 11, PreviewConfig::default());
    let original = fixture.get(PreviewProfile::StandardV1).await.unwrap();
    fs::write(&fixture.cache_files()[0], b"interrupted or corrupt").unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap().webp,
        original.webp
    );
    assert_eq!(fixture.generations(), 2);
    let root = fixture.cache.inner.root.clone();
    let competing = PreviewCache::new(&root, PreviewConfig::default()).unwrap();
    assert_eq!(
        competing
            .get(&fixture.repo, &fixture.record, PreviewProfile::StandardV1)
            .await
            .unwrap_err(),
        PreviewError::Busy
    );
    drop(competing);
    drop(fixture.cache);
    let temporary = root.join(format!(".preview-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, b"partial").unwrap();
    let restarted = PreviewCache::new(&root, PreviewConfig::default()).unwrap();
    assert_eq!(
        restarted
            .get(&fixture.repo, &fixture.record, PreviewProfile::StandardV1)
            .await
            .unwrap()
            .webp,
        original.webp
    );
    assert_eq!(restarted.inner.generations.load(Ordering::SeqCst), 0);
    assert!(!temporary.exists());
}

#[tokio::test]
async fn configured_quotas_evict_and_oversized_entry_never_publishes() {
    let fixture = Fixture::new(
        19,
        11,
        PreviewConfig {
            cache_entries: 1,
            ..PreviewConfig::default()
        },
    );
    fixture.get(PreviewProfile::StandardV1).await.unwrap();
    let first = fixture.cache_files()[0].clone();
    fixture.get(PreviewProfile::DataSaverV1).await.unwrap();
    assert!(!first.exists());
    assert_eq!(fixture.cache_files().len(), 1);
    fixture.get(PreviewProfile::StandardV1).await.unwrap();
    assert_eq!(fixture.generations(), 3);
    let tiny = Fixture::new(
        19,
        11,
        PreviewConfig {
            cache_bytes: 1,
            ..PreviewConfig::default()
        },
    );
    assert_eq!(
        tiny.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Quota
    );
    assert!(tiny.cache_files().is_empty());
    assert_eq!(fs::read_dir(&tiny.cache.inner.root).unwrap().count(), 1);
}

#[tokio::test]
async fn cache_hits_revalidate_original_and_dataset_identity() {
    let mut fixture = Fixture::new(19, 11, PreviewConfig::default());
    fixture.get(PreviewProfile::StandardV1).await.unwrap();
    let key = cache_key(&fixture.repo, &fixture.record, PreviewProfile::StandardV1);
    assert_ne!(
        key,
        cache_key(
            &DatasetRepository::new(fixture.repo.root().join("other")),
            &fixture.record,
            PreviewProfile::StandardV1
        )
    );
    fs::write(fixture.repo.root().join("source.png"), b"replaced").unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::SourceChanged
    );
    fs::remove_file(fixture.repo.root().join("source.png")).unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Source
    );
    fixture.record.canonical_path = "../outside.png".into();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Source
    );
    assert_eq!(fixture.generations(), 1);
}

#[tokio::test]
async fn source_pixel_decoder_and_worker_limits_cover_encoded_and_legacy_paths() {
    for (config, expected) in [
        (
            PreviewConfig {
                max_source_bytes: 1,
                ..PreviewConfig::default()
            },
            PreviewError::SourceLimit,
        ),
        (
            PreviewConfig {
                max_pixels: 1,
                ..PreviewConfig::default()
            },
            PreviewError::SourceLimit,
        ),
        (
            PreviewConfig {
                max_decoded_bytes: 1,
                ..PreviewConfig::default()
            },
            PreviewError::DecoderLimit,
        ),
    ] {
        let fixture = Fixture::new(19, 11, config);
        assert_eq!(
            fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
            expected
        );
        assert_eq!(
            fixture
                .cache
                .rgba(&fixture.repo, &fixture.record, 1600)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(fixture.generations(), 0);
    }
    let fixture = Fixture::new(
        19,
        11,
        PreviewConfig {
            workers: 1,
            ..PreviewConfig::default()
        },
    );
    let permit = fixture
        .cache
        .inner
        .workers
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Busy
    );
    assert_eq!(
        fixture
            .cache
            .rgba(&fixture.repo, &fixture.record, 1600)
            .await
            .unwrap_err(),
        PreviewError::Busy
    );
    drop(permit);
    fixture.get(PreviewProfile::StandardV1).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_identical_requests_coalesce_and_cancelled_waiter_does_not_publish() {
    let fixture = Arc::new(Fixture::new(
        73,
        47,
        PreviewConfig {
            workers: 1,
            ..PreviewConfig::default()
        },
    ));
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let fixture = fixture.clone();
        tasks.push(tokio::spawn(async move {
            fixture.get(PreviewProfile::StandardV1).await.unwrap().webp
        }));
    }
    let expected = tasks.remove(0).await.unwrap();
    for task in tasks {
        assert_eq!(task.await.unwrap(), expected);
    }
    assert_eq!(fixture.generations(), 1);
    let key = cache_key(&fixture.repo, &fixture.record, PreviewProfile::DataSaverV1);
    let flight = Arc::new(AsyncMutex::new(()));
    fixture
        .cache
        .inner
        .flights
        .lock()
        .unwrap()
        .insert(key, Arc::downgrade(&flight));
    let guard = flight.lock().await;
    let waiter_fixture = fixture.clone();
    let waiter = tokio::spawn(async move { waiter_fixture.get(PreviewProfile::DataSaverV1).await });
    tokio::task::yield_now().await;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    drop(guard);
    assert_eq!(fixture.generations(), 1);
    fixture.get(PreviewProfile::DataSaverV1).await.unwrap();
    assert_eq!(fixture.generations(), 2);
}

#[cfg(unix)]
#[tokio::test]
async fn source_and_cache_symlinks_are_never_followed() {
    use std::os::unix::fs::symlink;
    let mut fixture = Fixture::new(19, 11, PreviewConfig::default());
    symlink(
        fixture.repo.root().join("source.png"),
        fixture.repo.root().join("alias.png"),
    )
    .unwrap();
    fixture.record.canonical_path = "alias.png".into();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Source
    );
    fixture.record.canonical_path = "source.png".into();
    let outside = fixture._root.path().join("untouched");
    fs::write(&outside, b"private").unwrap();
    fs::create_dir(&fixture.cache.inner.root).unwrap();
    symlink(&outside, fixture.cache.inner.root.join("cache.lock")).unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Cache
    );
    assert_eq!(fs::read(outside).unwrap(), b"private");
}

#[tokio::test]
async fn unsupported_sources_and_failed_cache_initialization_leave_no_published_entry() {
    let mut fixture = Fixture::new(19, 11, PreviewConfig::default());
    fixture.record.canonical_path = "source.unknown".into();
    fs::rename(
        fixture.repo.root().join("source.png"),
        fixture.repo.root().join("source.unknown"),
    )
    .unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Source
    );
    assert!(fixture.cache_files().is_empty());
    let fixture = Fixture::new(19, 11, PreviewConfig::default());
    fs::write(&fixture.cache.inner.root, b"unavailable").unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::StandardV1).await.unwrap_err(),
        PreviewError::Cache
    );
    assert_eq!(fs::read(&fixture.cache.inner.root).unwrap(), b"unavailable");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_started_worker_retains_permits_and_publishes_atomically() {
    let fixture = Arc::new(Fixture::new(
        73,
        47,
        PreviewConfig {
            workers: 1,
            ..Default::default()
        },
    ));
    // Holding the disk gate lets the real worker finish source validation, then
    // blocks publication while the caller disconnects.
    let disk = fixture.cache.inner.disk.lock().unwrap();
    let worker_fixture = fixture.clone();
    let caller = tokio::spawn(async move { worker_fixture.get(PreviewProfile::StandardV1).await });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while fixture.cache.inner.workers.available_permits() != 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    caller.abort();
    assert_eq!(fixture.cache.inner.workers.available_permits(), 0);
    drop(disk);
    assert!(caller.await.unwrap_err().is_cancelled());
    let result = fixture.get(PreviewProfile::StandardV1).await.unwrap();
    assert!(!result.webp.is_empty());
    assert_eq!(fixture.generations(), 1);
    assert_eq!(fixture.cache_files().len(), 1);
    assert_eq!(fs::read_dir(&fixture.cache.inner.root).unwrap().count(), 2);
}

#[tokio::test]
async fn standard_preserves_native_sixteen_bit_resize_before_rgba_conversion() {
    let mut fixture = Fixture::new(11, 1701, PreviewConfig::default());
    let pixels = image::ImageBuffer::from_fn(11, 1701, |x, y| {
        image::Luma([((x * 71 + y * 39) % 65536) as u16])
    });
    let path = fixture.repo.root().join("source.png");
    pixels.save(&path).unwrap();
    fixture.record.blake3 = blake3::hash(&fs::read(&path).unwrap()).to_hex().to_string();
    let expected = image::open(path)
        .unwrap()
        .resize(1600, 1600, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let encoded = fixture.get(PreviewProfile::StandardV1).await.unwrap();
    let actual = image::load_from_memory_with_format(&encoded.webp, image::ImageFormat::WebP)
        .unwrap()
        .to_rgba8();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn standard_preserves_existing_jpeg_orientation_policy() {
    use image::codecs::jpeg::JpegEncoder;
    let mut fixture = Fixture::new(37, 19, PreviewConfig::default());
    let rgb = image::ImageBuffer::from_fn(37, 19, |x, y| {
        image::Rgb([(x * 6) as u8, (y * 11) as u8, 83])
    });
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&rgb)
        .unwrap();
    // Valid EXIF orientation 6 (90 degrees clockwise), which image::open does
    // not apply. Keep the same unrotated coordinate system as stored annotations.
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut oriented = jpeg[..2].to_vec();
    oriented.extend_from_slice(&[0xff, 0xe1]);
    oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    oriented.extend_from_slice(exif);
    oriented.extend_from_slice(&jpeg[2..]);
    let source = fixture.repo.root().join("oriented.jpg");
    fs::write(&source, &oriented).unwrap();
    fixture.record.canonical_path = "oriented.jpg".into();
    fixture.record.blake3 = blake3::hash(&oriented).to_hex().to_string();
    let expected = image::open(&source).unwrap().to_rgba8();
    let encoded = fixture.get(PreviewProfile::StandardV1).await.unwrap();
    assert_eq!((encoded.width, encoded.height), (37, 19));
    assert_eq!(
        image::load_from_memory_with_format(&encoded.webp, image::ImageFormat::WebP)
            .unwrap()
            .to_rgba8(),
        expected
    );
}

#[tokio::test]
async fn failed_publication_cleans_temporary_data_and_can_retry() {
    let fixture = Fixture::new(73, 47, PreviewConfig::default());
    fixture.get(PreviewProfile::StandardV1).await.unwrap();
    let blocked = fixture.cache.inner.root.join(format!(
        "{}.preview",
        cache_key(&fixture.repo, &fixture.record, PreviewProfile::DataSaverV1)
    ));
    fs::create_dir(&blocked).unwrap();
    assert_eq!(
        fixture.get(PreviewProfile::DataSaverV1).await.unwrap_err(),
        PreviewError::Cache
    );
    assert!(
        fs::read_dir(&fixture.cache.inner.root)
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
    );
    fs::remove_dir(blocked).unwrap();
    fixture.get(PreviewProfile::DataSaverV1).await.unwrap();
    assert_eq!(fixture.generations(), 2);
}
