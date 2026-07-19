use assert_cmd::Command;

#[test]
fn generated_demo_can_be_inspected_and_verified() {
    let directory = tempfile::tempdir().unwrap();
    let tape = directory.path().join("demo.ddt");

    Command::cargo_bin("depthdeck")
        .unwrap()
        .args([
            "generate-demo",
            tape.to_str().unwrap(),
            "--duration",
            "2",
            "--rate",
            "20",
        ])
        .assert()
        .success();

    Command::cargo_bin("depthdeck")
        .unwrap()
        .args(["inspect", tape.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("BTC/USD"));

    Command::cargo_bin("depthdeck")
        .unwrap()
        .args(["verify", tape.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"checksum_valid\":true"));
}
