use std::process::Command;
#[test]
fn dedicated_binary_ticks_reopens_world_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("dedicated.pvx");
    for _ in 0..2 {
        let run = Command::new(env!("CARGO_BIN_EXE_primal-server"))
            .args([
                "--world",
                world.to_str().unwrap(),
                "--ticks",
                "3",
                "--bind",
                "127.0.0.1:0",
            ])
            .output()
            .unwrap();
        assert!(run.status.success(), "{:?}", run.stderr);
        let log = String::from_utf8(run.stdout).unwrap();
        assert!(log.contains("\"ticks\":3") && log.contains("\"event\":\"stopped\""));
    }
}

#[test]
fn dedicated_binary_reopens_the_configured_forest_and_refuses_a_preset_change() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("forest.pvx");
    let config = dir.path().join("forest.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "world": world, "preset": "forest-modern", "ticks": 1, "bind": "127.0.0.1:0"
        }))
        .unwrap(),
    )
    .unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_primal-server"))
        .arg("--config")
        .arg(config)
        .output()
        .unwrap();
    assert!(first.status.success(), "{:?}", first.stderr);
    for (preset, success) in [(None, true), (Some("historical"), false)] {
        let mut run = Command::new(env!("CARGO_BIN_EXE_primal-server"));
        run.arg("--world")
            .arg(&world)
            .args(["--bind", "127.0.0.1:0", "--ticks", "1"]);
        if let Some(preset) = preset {
            run.args(["--preset", preset]);
        }
        let run = run.output().unwrap();
        assert_eq!(run.status.success(), success, "{:?}", run);
        if success {
            let started = String::from_utf8(run.stdout).unwrap();
            assert!(started.contains("\"preset\":\"forest-modern\""));
        } else {
            assert!(String::from_utf8(run.stderr).unwrap().contains("Preset"));
        }
    }
}
