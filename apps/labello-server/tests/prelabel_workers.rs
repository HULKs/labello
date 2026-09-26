#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
};

// Minimal self-contained ONNX: a static input and a constant detection output.
fn integer(field: u8, mut value: u64) -> Vec<u8> {
    let mut bytes = vec![field << 3];
    while value >= 128 {
        bytes.push(value as u8 | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}
fn message(field: u8, value: &[u8]) -> Vec<u8> {
    let mut prefix = integer(field, value.len() as u64);
    prefix[0] |= 2;
    prefix.extend(value);
    prefix
}
fn value(name: &str, shape: &[u64]) -> Vec<u8> {
    let shape = shape
        .iter()
        .flat_map(|&dimension| message(1, &integer(1, dimension)))
        .collect::<Vec<_>>();
    let tensor = [integer(1, 1), message(2, &shape)].concat();
    [
        message(1, name.as_bytes()),
        message(2, &message(1, &tensor)),
    ]
    .concat()
}
fn model(score: f32) -> Vec<u8> {
    let data = [16.0_f32, 16.0, 8.0, 8.0, score]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let tensor = [
        integer(1, 1),
        integer(1, 5),
        integer(1, 1),
        integer(2, 1),
        message(8, b"predictions"),
        message(9, &data),
    ]
    .concat();
    let graph = [
        message(2, b"worker-test"),
        message(5, &tensor),
        message(11, &value("images", &[1, 3, 32, 32])),
        message(12, &value("predictions", &[1, 5, 1])),
    ]
    .concat();
    [
        integer(1, 8),
        message(7, &graph),
        message(8, &integer(2, 17)),
    ]
    .concat()
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Worker {
    fn new() -> Self {
        Self(
            Command::new(
                std::env::var_os("LABELLO_TEST_SERVER")
                    .unwrap_or_else(|| env!("CARGO_BIN_EXE_labello-server").into()),
            )
            .arg("--prelabel-worker")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        )
    }
    fn request(&mut self, model: &[u8], digest: &str, class: &str) -> Value {
        let config = json!({"configId":"model","name":"Model","model":{"modelId":"model","displayName":"Model","version":"1","location":"model.onnx"},
            "execution":{"mode":"server_side","command":[]},"outputProcessing":{"confidenceThreshold":0.25,"suppressOverlapsIou":0.5},"availableToAnnotators":true,
            "yolo":{"inputSize":32,"outputName":"predictions","classCount":1,"classMappings":[{"modelClassId":0,"classId":class}],"keypoints":[],"modelDigest":digest}});
        let task = json!({"taskId":"boxes","name":"Boxes","annotationType":"bounding_box","classIds":[class],"instructions":{"title":"","exampleText":"","exampleImages":[]},
            "skeleton":null,"review":{"workflow":"approval","allowReviewerCorrections":true},"prelabelConfigIds":["model"],"manualBoxGuideMigration":null,"enabled":true});
        let header = serde_json::to_vec(&json!([{"onnxLibrary":"/nonexistent-labello-test-runtime.so"},
            {"operation":"infer","provider":"cpu","digest":digest,"threads":1,"config":config,"task":task}])).unwrap();
        let input = self.0.stdin.as_mut().unwrap();
        for bytes in [&header[..], model, IMAGE] {
            input
                .write_all(&(bytes.len() as u64).to_le_bytes())
                .unwrap();
            input.write_all(bytes).unwrap();
        }
        input.flush().unwrap();
        let output = self.0.stdout.as_mut().unwrap();
        let mut length = [0; 8];
        output.read_exact(&mut length).unwrap();
        let length = u64::from_le_bytes(length);
        assert!(
            length <= 32 * 1024 * 1024,
            "worker must return a framed reply, not close after one image"
        );
        let mut bytes = vec![0; length as usize];
        output.read_exact(&mut bytes).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
}

#[test]
fn process_reuses_sessions_without_model_bytes_and_applies_each_requests_mapping() {
    let model = model(0.9);
    let digest = labello_inference::model_digest(&model);
    let mut worker = Worker::new();
    let first = worker.request(&model, &digest, "person");
    assert_eq!(first["loaded"], true);
    assert!(first["result"]["Ok"].is_object());
    let next = worker.request(&[], &digest, "ball");
    assert_eq!(next["loaded"], false);
    assert_eq!(next["result"]["Ok"]["execution"], "server_cpu");
    assert_eq!(next["result"]["Ok"]["suggestions"][0]["classId"], "ball");
    worker.0.stdin.take();
    assert!(worker.0.wait().unwrap().success());
}

#[test]
fn model_cache_is_bounded_and_missing_evicted_models_cannot_reuse_another_session() {
    let mut worker = Worker::new();
    let first = model(0.9);
    let digest = labello_inference::model_digest(&first);
    assert_eq!(worker.request(&first, &digest, "person")["loaded"], true);
    for score in [0.8, 0.7] {
        let model = model(score);
        let digest = labello_inference::model_digest(&model);
        assert_eq!(worker.request(&model, &digest, "person")["loaded"], true);
    }
    assert_eq!(
        worker.request(&[], &digest, "person")["result"]["Err"],
        "model"
    );
    assert_eq!(worker.request(&first, &digest, "person")["loaded"], true);
}

#[test]
fn oversized_protocol_frames_are_rejected_before_allocation() {
    let mut worker = Worker::new();
    worker
        .0
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&u64::MAX.to_le_bytes())
        .unwrap();
    worker.0.stdin.take();
    assert!(!worker.0.wait().unwrap().success());
}

// One gray pixel encoded as PNG; no external test files or models.
const IMAGE: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 104, 104, 104, 0, 0, 3, 4,
    1, 129, 75, 211, 210, 16, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
