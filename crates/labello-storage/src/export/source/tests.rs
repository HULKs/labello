use super::*;
use crate::DatasetRepository;
use labello_domain::{DatasetId, ImageId, now};

async fn fixture() -> (tempfile::TempDir, DatasetRepository, ImageRecord) {
    let dir = tempfile::tempdir().unwrap();
    let repository = DatasetRepository::new(dir.path());
    repository
        .initialize(DatasetMetadata::new(
            DatasetId::from("export"),
            "Export",
            now(),
        ))
        .await
        .unwrap();
    let bytes = b"immutable synthetic original";
    std::fs::write(dir.path().join("images/source.png"), bytes).unwrap();
    let record = ImageRecord {
        image_id: ImageId::from("image"),
        blake3: blake3::hash(bytes).to_hex().to_string(),
        canonical_path: "images/source.png".into(),
        known_paths: vec!["images/source.png".into()],
        duplicate_paths: vec![],
        file_name: "source.png".into(),
        byte_size: bytes.len() as u64,
        width: 10,
        height: 10,
        media_type: "image/png".into(),
        source_memberships: None,
    };
    repository
        .save_images_index(&ImagesIndex {
            images_by_hash: BTreeMap::from([(record.blake3.clone(), record.clone())]),
            ..ImagesIndex::default()
        })
        .await
        .unwrap();
    (dir, repository, record)
}

#[tokio::test]
async fn captured_original_is_immutable_and_changes_abort_final_validation() {
    let (dir, repository, image) = fixture().await;
    let limits = ExportLimits::default();
    let source = Source::open(dir.path(), &limits).unwrap();
    let cancel = AtomicBool::new(false);
    let mut captured = tempfile::tempfile().unwrap();
    source
        .copy_original(&image, &mut captured, &limits, &cancel)
        .unwrap();
    source.verify_original(&image, &limits, &cancel).unwrap();
    source.verify_configuration(&limits).unwrap();
    // Same-length replacements must fail the digest check too.
    std::fs::write(
        repository.root().join(&image.canonical_path),
        vec![b'x'; image.byte_size as usize],
    )
    .unwrap();
    assert_eq!(
        source.verify_original(&image, &limits, &cancel),
        Err(ExportFailure::SourceChanged)
    );
    use std::io::{Seek, SeekFrom};
    captured.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    captured.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"immutable synthetic original");
    let mut changed = source.metadata.clone();
    changed.name = "Changed".into();
    repository.save_dataset(&changed).await.unwrap();
    assert_eq!(
        source.verify_configuration(&limits),
        Err(ExportFailure::SourceChanged)
    );
}

#[tokio::test]
async fn capture_reads_fresh_index_and_refuses_bounds_cancel_and_symlinks() {
    let (dir, repository, image) = fixture().await;
    let limits = ExportLimits::default();
    let source = Source::open(dir.path(), &limits).unwrap();
    let mut output = tempfile::tempfile().unwrap();
    assert_eq!(
        source.copy_original(&image, &mut output, &limits, &AtomicBool::new(true)),
        Err(ExportFailure::Cancelled)
    );
    assert_eq!(
        source.copy_original(
            &image,
            &mut output,
            &ExportLimits {
                max_file_bytes: 1,
                ..limits.clone()
            },
            &AtomicBool::new(false)
        ),
        Err(ExportFailure::Limit)
    );
    assert!(matches!(
        Source::open(
            dir.path(),
            &ExportLimits {
                max_metadata_bytes: 1,
                ..limits.clone()
            }
        ),
        Err(ExportFailure::Limit)
    ));
    // The repository's warmed index cache cannot hide an external index replacement.
    repository.load_images_index().await.unwrap();
    std::fs::write(
        repository.images_index_path(),
        serde_json::to_vec(&ImagesIndex::default()).unwrap(),
    )
    .unwrap();
    assert!(
        Source::open(dir.path(), &limits)
            .unwrap()
            .metadata
            .images
            .is_empty()
    );
    assert_eq!(
        source.verify_configuration(&limits),
        Err(ExportFailure::SourceChanged)
    );
    #[cfg(target_os = "linux")]
    {
        std::fs::remove_file(repository.root().join(&image.canonical_path)).unwrap();
        std::os::unix::fs::symlink("/dev/zero", repository.root().join(&image.canonical_path))
            .unwrap();
        assert_eq!(
            source.verify_original(&image, &limits, &AtomicBool::new(false)),
            Err(ExportFailure::InvalidInput)
        );
    }
}
