//! Synthetic production-router benchmark; opt-in, never pointed at existing data.
use super::*;
use std::time::Instant;

async fn request(
    app: &axum::Router,
    cookie: &str,
    csrf: &str,
    method: &str,
    uri: &str,
    body: Value,
) -> (Value, u128) {
    let started = Instant::now();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::COOKIE, cookie)
                .header(crate::csrf::HEADER, csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let result = serde_json::from_slice(&bytes).unwrap();
    (result, started.elapsed().as_micros())
}

fn counters() -> (u64, u64, u64) {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    let ticks = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
    let io = std::fs::read_to_string("/proc/self/io").unwrap();
    let reads = io
        .lines()
        .find_map(|l| l.strip_prefix("rchar: "))
        .unwrap()
        .parse()
        .unwrap();
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let peak = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (ticks, reads, peak)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
#[ignore = "opt-in Linux performance measurement; creates disposable synthetic data"]
async fn nine_client_polling_performance() {
    let image_count = std::env::var("LABELLO_BENCH_IMAGES")
        .unwrap_or_else(|_| "6928".into())
        .parse::<usize>()
        .unwrap();
    let rounds = std::env::var("LABELLO_BENCH_ROUNDS")
        .unwrap_or_else(|_| "5".into())
        .parse::<usize>()
        .unwrap();
    assert!(image_count >= 9 * rounds);
    let temp = tempfile::tempdir().unwrap();
    let state = ApiState::new(temp.path());
    let repo = state.repo(&DatasetId::from("ds")).unwrap();
    let timestamp = now();
    let task = TaskId::from("boxes");
    let mut metadata = DatasetMetadata::new(
        DatasetId::from("ds"),
        "Synthetic polling benchmark",
        timestamp,
    );
    metadata.tasks.push(TaskDefinition {
        task_id: task.clone(),
        name: "Boxes".into(),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![ClassId::from("person")],
        instructions: TutorialContent {
            title: "Boxes".into(),
            example_text: String::new(),
            example_images: Vec::new(),
        },
        skeleton: None,
        review: ReviewConfig {
            workflow: ReviewWorkflow::None,
            ..ReviewConfig::default()
        },
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    metadata.label_classes.push(LabelClass {
        class_id: ClassId::from("person"),
        name: "Person".into(),
        color: "#ffffff".into(),
        description: None,
    });
    let mut sessions = Vec::new();
    for i in 0..9 {
        let user = UserId::from(format!("user_{i}"));
        metadata.role_assignments.push(DatasetRoleAssignment {
            dataset_id: metadata.dataset_id.clone(),
            user_id: user.clone(),
            roles: BTreeSet::from([DatasetRole::Annotator]),
            assigned_at: timestamp,
            assigned_by: None,
        });
        state
            .server_store
            .upsert_user(UserAccount {
                user_id: user.clone(),
                display_name: user.to_string(),
                github_user_id: None,
                github_login: None,
                created_at: timestamp,
                updated_at: timestamp,
            })
            .unwrap();
        let session = state.create_session(user).unwrap();
        sessions.push((format!("labello_session={}", session.cookie), session.csrf));
    }
    repo.initialize(metadata).await.unwrap();
    let mut index = ImagesIndex::default();
    let mut fixture_bytes = 0;
    for i in 0..image_count {
        let image = ImageId::from(format!("img_{i:05}"));
        let mut events = Vec::new();
        for version in 1..=8 {
            for label in 0..8 {
                let mut annotation = AnnotationVersion::native(
                    AnnotationId::from(format!("label_{label}")),
                    task.clone(),
                    ClassId::from("person"),
                    AnnotationType::BoundingBox,
                    AnnotationGeometry::BoundingBox(BoundingBox {
                        x: 0.1,
                        y: 0.1,
                        width: 0.2,
                        height: 0.2,
                    }),
                    UserId::from("user_0"),
                    timestamp,
                );
                annotation.version = version;
                events.push(labello_domain::EventLogEntry::new(
                    events.len() as u64 + 1,
                    image.clone(),
                    UserId::from("user_0"),
                    DatasetRole::Annotator,
                    timestamp,
                    EventPayload::AnnotationVersionCreated {
                        annotation,
                        previous_version: (version > 1).then_some(version - 1),
                        reason: None,
                    },
                ));
            }
        }
        let image_state = labello_domain::rebuild_state(image.clone(), &events).unwrap();
        let directory = repo.root().join("annotations").join(image.as_str());
        std::fs::create_dir_all(&directory).unwrap();
        let event_bytes = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect::<String>();
        let state_bytes = serde_json::to_vec_pretty(&image_state).unwrap();
        fixture_bytes += event_bytes.len() + state_bytes.len();
        std::fs::write(directory.join("events.jsonl"), event_bytes).unwrap();
        std::fs::write(directory.join("state.json"), state_bytes).unwrap();
        index.images_by_hash.insert(
            format!("hash_{i}"),
            ImageRecord {
                image_id: image,
                blake3: format!("hash_{i}"),
                canonical_path: format!("images/{i}.png"),
                known_paths: Vec::new(),
                duplicate_paths: Vec::new(),
                file_name: format!("{i}.png"),
                byte_size: 0,
                width: 2,
                height: 2,
                media_type: "image/png".into(),
                source_memberships: None,
            },
        );
    }
    repo.save_images_index(&index).await.unwrap();
    let app = production_router(state);
    let (cookie, csrf) = &sessions[0];
    request(&app, cookie, csrf, "GET", "/datasets/ds/stats", Value::Null).await;
    request(&app, cookie, csrf, "GET", "/presence", Value::Null).await;
    let before = counters();
    let clock_ticks = std::process::Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .unwrap();
    let clock_ticks = String::from_utf8(clock_ticks.stdout)
        .unwrap()
        .trim()
        .parse::<f64>()
        .unwrap();
    let started = Instant::now();
    let mut clients = tokio::task::JoinSet::new();
    for (cookie, csrf) in sessions {
        let app = app.clone();
        clients.spawn(async move {
            let mut times = BTreeMap::<&str, Vec<u128>>::new();
            for _ in 0..rounds {
                let (assignment, elapsed) = request(&app, &cookie, &csrf, "POST", "/datasets/ds/images/next", json!({"taskId":"boxes","kind":"annotation"})).await;
                times.entry("claim").or_default().push(elapsed);
                let assignment: Assignment = serde_json::from_value(assignment).unwrap();
                let uri = format!("/datasets/ds/images/{}/annotation-batch?assignmentId={}&imageId={}&taskId=boxes&kind=annotation", assignment.image_id, assignment.assignment_id, assignment.image_id);
                let (_, elapsed) = request(&app, &cookie, &csrf, "POST", &uri, json!({"payloads":[],"complete":true})).await;
                times.entry("submit").or_default().push(elapsed);
                for (name, uri) in [("stats", "/datasets/ds/stats"), ("presence", "/presence"), ("availability", "/datasets/ds/assignments/availability?kind=annotation")] {
                    let (_, elapsed) = request(&app, &cookie, &csrf, "GET", uri, Value::Null).await;
                    times.entry(name).or_default().push(elapsed);
                }
            }
            times
        });
    }
    let mut times = BTreeMap::<&str, Vec<u128>>::new();
    while let Some(client) = clients.join_next().await {
        for (name, samples) in client.unwrap() {
            times.entry(name).or_default().extend(samples);
        }
    }
    let after = counters();
    let timings = times.into_iter().map(|(name, mut values)| {
        values.sort();
        (name, json!({"count":values.len(), "mean_us":values.iter().sum::<u128>() / values.len() as u128, "p95_us":values[(values.len()-1)*95/100]}))
    }).collect::<BTreeMap<_,_>>();
    println!(
        "{}",
        json!({"images":image_count,"clients":9,"rounds":rounds,"fixture_bytes":fixture_bytes,"wall_seconds":started.elapsed().as_secs_f64(),"cpu_seconds":(after.0-before.0) as f64 / clock_ticks,"logical_read_bytes":after.1-before.1,"peak_rss_kib":after.2,"endpoints":timings})
    );
}
