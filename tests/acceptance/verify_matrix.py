import pathlib
import re


def main():
    root = pathlib.Path(__file__).resolve().parent
    matrix = (root / "MATRIX.md").read_text(encoding="utf-8")
    registry = (root / "TEST_COVERAGE.md").read_text(encoding="utf-8")
    automated_rows = [
        line for line in matrix.splitlines() if line.startswith("|") and "automated" in line.lower()
    ]
    assert automated_rows, "acceptance matrix contains no automated rows"

    missing_ids = []
    unknown_ids = []
    for row in automated_rows:
        ids = re.findall(r"`([A-Z][A-Z0-9-]+)`", row)
        if not ids:
            missing_ids.append(row)
        unknown_ids.extend(test_id for test_id in ids if f"`{test_id}`" not in registry)

    assert not missing_ids, "automated matrix rows without test IDs:\n" + "\n".join(missing_ids)
    assert not unknown_ids, "matrix IDs absent from TEST_COVERAGE.md: " + ", ".join(sorted(set(unknown_ids)))
    print(f"acceptance matrix traceability: PASS ({len(automated_rows)} automated rows)")


if __name__ == "__main__":
    main()
