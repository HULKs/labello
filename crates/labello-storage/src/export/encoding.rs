use std::{collections::BTreeSet, fmt::Write};

use labello_domain::{
    AnnotationGeometry, AnnotationId, AnnotationVersion, BoundingBox, ExportClassMapping,
    ExportProfile, ExportSplit, KeypointState,
};
use serde::Serialize;

use super::ExportFailure;

/// Coverage and current box identity are established by the capture policy.
pub(super) struct Row<'a> {
    pub annotation: &'a AnnotationVersion,
    pub class_index: u32,
    pub bbox: BoundingBox,
    pub derived_box: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RowTrace {
    pub row: usize,
    pub annotation_id: AnnotationId,
    pub annotation_version: u32,
    pub class_index: u32,
    pub derived_box: bool,
}

pub(super) fn labels(
    profile: ExportProfile,
    rows: &mut [Row<'_>],
) -> Result<(Vec<u8>, Vec<RowTrace>), ExportFailure> {
    rows.sort_by(|a, b| {
        (a.class_index, &a.annotation.annotation_id)
            .cmp(&(b.class_index, &b.annotation.annotation_id))
    });
    let mut identities = BTreeSet::new();
    let mut reader_rows = BTreeSet::new();
    let mut text = String::new();
    let mut traces = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let row_start = text.len();
        if row.annotation.deleted
            || !identities.insert(&row.annotation.annotation_id)
            || row.annotation.annotation_type != profile.annotation_type()
        {
            return Err(ExportFailure::InvalidInput);
        }
        row.bbox
            .validate()
            .map_err(|_| ExportFailure::InvalidInput)?;
        write!(text, "{}", row.class_index).expect("string writes cannot fail");
        for value in [
            f64::from(row.bbox.x) + f64::from(row.bbox.width) / 2.0,
            f64::from(row.bbox.y) + f64::from(row.bbox.height) / 2.0,
            f64::from(row.bbox.width),
            f64::from(row.bbox.height),
        ] {
            number(&mut text, value);
        }
        match (&profile, &row.annotation.geometry) {
            (ExportProfile::UltralyticsYoloDetectV1, AnnotationGeometry::BoundingBox(bbox))
                if *bbox == row.bbox && !row.derived_box => {}
            (ExportProfile::UltralyticsYoloPoseV1, AnnotationGeometry::Skeleton(pose)) => {
                pose.validate().map_err(|_| ExportFailure::InvalidInput)?;
                for point in &pose.keypoints {
                    if let Some(position) = point.point {
                        number(&mut text, f64::from(position.x));
                        number(&mut text, f64::from(position.y));
                    } else {
                        // Visibility zero, not a placed point at the image origin.
                        text.push_str(" 0.000000000 0.000000000");
                    }
                    text.push_str(match point.state {
                        KeypointState::Visible => " 2",
                        KeypointState::Hidden => " 1",
                        KeypointState::Absent => " 0",
                    });
                }
            }
            _ => return Err(ExportFailure::InvalidInput),
        }
        // Ultralytics deduplicates equal float32 rows. Refuse a lossy object-count
        // mapping, including distinct source values rounded to the same reader row.
        let reader_row = text[row_start..]
            .split_whitespace()
            .map(|value| value.parse::<f32>().map(f32::to_bits))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ExportFailure::InvalidInput)?;
        if !reader_rows.insert(reader_row) {
            return Err(ExportFailure::AmbiguousObjects);
        }
        text.push('\n');
        traces.push(RowTrace {
            row: index + 1,
            annotation_id: row.annotation.annotation_id.clone(),
            annotation_version: row.annotation.version,
            class_index: row.class_index,
            derived_box: row.derived_box,
        });
    }
    // A newline encodes zero rows while remaining transferable by the existing
    // browser importer, whose upload protocol requires a nonempty final chunk.
    if text.is_empty() {
        text.push('\n');
    }
    Ok((text.into_bytes(), traces))
}

fn number(text: &mut String, value: f64) {
    // Nine decimals exceed the 1e-6 normalized interoperability contract. Avoid -0.
    let value = if value == 0.0 { 0.0 } else { value };
    write!(text, " {value:.9}").expect("string writes cannot fail");
}

pub(super) fn descriptor(
    profile: ExportProfile,
    mapping: &[ExportClassMapping],
) -> Result<Vec<u8>, ExportFailure> {
    if mapping.is_empty()
        || mapping
            .iter()
            .enumerate()
            .any(|(i, class)| class.index as usize != i)
    {
        return Err(ExportFailure::InvalidInput);
    }
    // Empty split lists are explicit and do not duplicate an image into validation.
    // Omit `path`: both readers then anchor paths at the descriptor's directory.
    // Ultralytics interprets an explicit `path: .` relative to the process cwd.
    let mut text = String::from("train: train.txt\nval: val.txt\ntest: test.txt\nnames:\n");
    for class in mapping {
        let name = serde_json::to_string(&class.name).map_err(|_| ExportFailure::InvalidInput)?;
        writeln!(text, "  {}: {name}", class.index).expect("string writes cannot fail");
    }
    if profile == ExportProfile::UltralyticsYoloPoseV1 {
        let count = mapping[0]
            .skeleton
            .as_ref()
            .ok_or(ExportFailure::InvalidInput)?
            .keypoints
            .len();
        if count == 0 {
            return Err(ExportFailure::InvalidInput);
        }
        writeln!(text, "kpt_shape: [{count}, 3]\nkpt_names:").expect("string writes cannot fail");
        for class in mapping {
            let spec = class.skeleton.as_ref().ok_or(ExportFailure::InvalidInput)?;
            if spec.keypoints.len() != count {
                return Err(ExportFailure::InvalidInput);
            }
            let names = spec
                .keypoints
                .iter()
                .map(|point| &point.name)
                .collect::<Vec<_>>();
            let names = serde_json::to_string(&names).map_err(|_| ExportFailure::InvalidInput)?;
            writeln!(text, "  {}: {names}", class.index).expect("string writes cannot fail");
        }
    }
    Ok(text.into_bytes())
}

pub(super) fn image_paths(
    split: ExportSplit,
    digest: &str,
    extension: &str,
) -> Result<(String, String), ExportFailure> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || !matches!(extension, "png" | "jpg" | "webp" | "bmp")
    {
        return Err(ExportFailure::InvalidInput);
    }
    Ok((
        format!("images/{}/{digest}.{extension}", split.as_str()),
        format!("labels/{}/{digest}.txt", split.as_str()),
    ))
}

#[cfg(test)]
mod tests;
