"""Validate production export fixtures with the pinned, offline Ultralytics reader.

Generate inputs with LABELLO_EXPORT_ROUND_TRIP_ARTIFACTS set while running the
both_profiles_round_trip Rust test. This script never constructs a model.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("artifacts", type=Path)
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    artifacts = args.artifacts.resolve()
    original_hashes = {
        path.relative_to(artifacts).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in artifacts.rglob("*") if path.is_file()
    }
    attempts = []

    def refuse(*_args, **_kwargs):
        attempts.append("refused")
        raise RuntimeError("network access is disabled for export reader verification")

    socket.socket.connect = refuse
    socket.socket.connect_ex = refuse
    socket.getaddrinfo = refuse
    with tempfile.TemporaryDirectory(prefix="labello-export-reader-") as temporary:
        runtime = Path(temporary)
        config = runtime / "config"
        (config / "Ultralytics").mkdir(parents=True)
        os.environ.update(
            YOLO_OFFLINE="true", YOLO_AUTOINSTALL="false", YOLO_CONFIG_DIR=str(config),
            MPLCONFIGDIR=str(runtime / "matplotlib"),
        )
        # The dataset helper checks for a plotting font even without training.
        # Supply a repository font locally so that path cannot fetch one remotely.
        font = Path(__file__).resolve().parents[3] / "assets/fonts/inter/Inter-Regular.ttf"
        for name in ("Arial.ttf", "Arial.Unicode.ttf"):
            shutil.copy2(font, config / "Ultralytics" / name)

        import ultralytics
        import numpy as np
        from ultralytics.data.dataset import YOLODataset
        from ultralytics.data.utils import check_det_dataset

        assert ultralytics.__version__ == "8.4.125", "the compatibility reader version is pinned"
        results = []
        for profile in ("detect", "pose"):
            source = artifacts / profile
            evidence = json.loads((source / "round-trip-evidence.json").read_text())
            assert evidence["productionExport"] and evidence["productionReimport"]
            manifest = json.loads((source / "labello-export.json").read_text())
            assert manifest["options"]["profile"] == f"ultralytics_yolo_{profile}_v1"
            data_root = runtime / profile
            shutil.copytree(source, data_root)
            data = check_det_dataset(str(data_root / "data.yaml"), autodownload=False)
            classes = manifest["summary"]["classes"]
            assert data["names"] == {item["index"]: item["name"] for item in classes}
            if profile == "pose":
                assert data["kpt_shape"] == [3, 3]
                assert data["kpt_names"] == {
                    item["index"]: [point["name"] for point in item["skeleton"]["keypoints"]]
                    for item in classes
                }
                assert "flip_idx" not in data
            expected_images = {item["imagePath"]: item for item in manifest["images"]}
            seen = set()
            objects = 0
            empty = 0
            for split in ("train", "val", "test"):
                dataset = YOLODataset(
                    img_path=data[split], data=data, task=profile,
                    augment=False, imgsz=128, cache=False,
                )
                for label in dataset.labels:
                    relative = Path(label["im_file"]).relative_to(data_root).as_posix()
                    assert relative not in seen, "an image cannot occur in two splits"
                    seen.add(relative)
                    expected = expected_images[relative]
                    assert expected["split"] == split
                    rows = [
                        [float(value) for value in line.split()]
                        for line in (data_root / expected["labelPath"]).read_text().splitlines() if line.strip()
                    ]
                    assert len(label["cls"]) == len(rows), "reader must retain every object"
                    assert len(expected["rows"]) == len(rows)
                    objects += len(rows)
                    empty += not rows
                    if not rows:
                        continue
                    values = np.asarray(rows, dtype=np.float64)
                    np.testing.assert_array_equal(label["cls"].reshape(-1), values[:, 0])
                    np.testing.assert_allclose(label["bboxes"], values[:, 1:5], atol=1e-6, rtol=0)
                    if profile == "pose":
                        points = values[:, 5:].reshape(len(rows), 3, 3)
                        np.testing.assert_array_equal(label["keypoints"][:, :, 2], points[:, :, 2])
                        np.testing.assert_allclose(label["keypoints"][:, :, :2], points[:, :, :2], atol=1e-6, rtol=0)
            assert seen == set(expected_images)
            assert (len(seen), objects, empty) == (3, 3, 2)
            results.append({
                "profile": evidence["profile"], "productionExport": True,
                "productionReimport": True, "images": len(seen), "objects": objects,
                "verifiedEmptyImages": empty, "splits": ["train", "val", "test"],
                "archiveBlake3": evidence["archiveBlake3"],
                "normalizedTolerance": 1e-6,
            })
        assert not attempts, "reader attempted a network operation"
    for relative, expected in original_hashes.items():
        assert hashlib.sha256((artifacts / relative).read_bytes()).hexdigest() == expected
    args.report.write_text(json.dumps({
        "reader": "ultralytics", "version": "8.4.125", "networkAttempts": 0,
        "modelsConstructed": 0, "originalArtifactsUnchanged": True, "results": results,
    }, indent=2) + "\n")
    print("Both production export profiles passed the pinned offline dataset reader.")


if __name__ == "__main__":
    main()
