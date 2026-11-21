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
