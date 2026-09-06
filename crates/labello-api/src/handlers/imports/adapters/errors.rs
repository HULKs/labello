fn map_storage(error: storage::StorageError) -> ApiError {
    let resource_limit = matches!(&error, storage::StorageError::Import { code, .. } if matches!(code.as_str(),
        "annotations_per_image_limit" | "category_limit" | "coverage_limit"
        | "descriptor_byte_limit" | "generated_event_limit" | "generated_state_limit"
        | "image_decoded_bytes_limit" | "image_decoded_memory_limit" | "image_encoded_bytes_limit"
        | "image_pixel_limit" | "json_nesting_limit" | "staging_quota_exceeded"
        | "structured_data_node_limit" | "structured_data_value_limit" | "task_limit"
        | "yolo_column_limit" | "yolo_line_limit" | "yolo_yaml_alias_limit"));
    let rejection = map_storage_rejection(error);
    if resource_limit {
        ApiError::ResourceLimit(Box::new(rejection))
    } else {
        rejection
    }
}

fn map_storage_rejection(error: storage::StorageError) -> ApiError {
    match error {
        storage::StorageError::NotFound(_) => ApiError::NotFound("import job".to_string()),
        storage::StorageError::Import { code, message } => match code.as_str() {
            "import_owner_mismatch" => {
                ApiError::HiddenDenial(Box::new(ApiError::NotFound("import job".to_string())))
            }
            "import_root_forbidden" => ApiError::Forbidden("import root access denied".to_string()),
            "import_id_invalid" | "destination_id_invalid" | "destination_id_reserved" => {
                ApiError::BadRequest(message)
            }
            "source_file_limit"
            | "source_byte_limit"
            | "source_file_too_large"
            | "server_source_browse_limit"
            | "upload_chunk_limit"
            | "selected_image_limit"
            | "annotation_limit"
            | "keypoint_limit" => ApiError::PayloadTooLarge(message),
            "destination_exists"
            | "destination_reserved"
            | "source_path_collision"
            | "job_phase_invalid"
            | "source_sealed"
            | "upload_chunk_not_sequential"
            | "upload_chunk_retry_mismatch"
            | "source_changed"
            | "plan_stale"
            | "job_not_cancellable"
            | "import_unavailable" => ApiError::Conflict(message),
            "reservation_limit"
            | "upload_concurrency_limit"
            | "build_concurrency_limit"
            | "descriptor_inspection_busy" => {
                ApiError::ResourceLimit(Box::new(ApiError::Conflict(message)))
            }
            "parser_time_limit" => {
                ApiError::ResourceLimit(Box::new(ApiError::Unprocessable(message)))
            }
            "profile_disabled"
            | "destination_name_invalid"
            | "source_incomplete"
            | "ground_truth_attestation_required"
            | "plan_not_committable"
            | "source_file_missing"
            | "import_root_missing"
            | "upload_chunk_digest_mismatch"
            | "source_file_digest_mismatch" => ApiError::Unprocessable(message),
            _ if code.starts_with("yolo_")
                || code.starts_with("coco_")
                || code.starts_with("image_")
                || code.starts_with("descriptor_")
                || code.starts_with("server_source_")
                || code.starts_with("source_path_")
                || code.starts_with("source_file_")
                || code.starts_with("geometry_")
                || code.starts_with("category_") =>
            {
                ApiError::Unprocessable(message)
            }
            _ => ApiError::Storage(storage::StorageError::Import { code, message }),
        },
        error => ApiError::Storage(error),
    }
}
