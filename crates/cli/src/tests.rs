use std::io::Cursor;

use nanobot_app::NanobotApp;
use nanobot_provider::DemoToolCallingProvider;

use super::*;

#[test]
fn interactive_session_stops_on_exit_command() {
    let dir = std::env::temp_dir().join(format!("nanobot-cli-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir should exist");
    let mut app = NanobotApp::new(Config::default(), Box::new(DemoToolCallingProvider), &dir)
        .expect("app should build");
    let input = Cursor::new("write a file\nexit\n");
    let mut output = Vec::new();

    run_interactive_session(&mut app, input, &mut output).expect("interactive session should run");

    let rendered = String::from_utf8(output).expect("utf8");
    assert!(rendered.contains("demo complete"));
    assert!(rendered.contains("Bye"));
}

#[test]
fn logger_init_is_safe_to_call() {
    init_logger();
    init_logger();
}

#[test]
fn serve_mode_runs_with_finite_iterations() {
    let dir = std::env::temp_dir().join(format!("nanobot-cli-serve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir should exist");

    run_service_mode(Config::default(), dir, Some(2), 0).expect("serve mode should run");
}

#[test]
fn gateway_mode_starts_without_seeding_a_gateway_session() {
    let dir = std::env::temp_dir().join(format!("nanobot-cli-gateway-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir should exist");

    run_gateway_mode(Config::default(), dir.clone(), 18790, true, Some(2), 0)
        .expect("gateway mode should run");

    assert!(
        !dir.join("sessions").join("system_gateway.jsonl").exists(),
        "gateway startup should not enqueue a synthetic agent turn"
    );
}

#[test]
fn cli_defaults_gateway_and_serve_to_infinite_with_one_second_interval() {
    let serve = Cli::parse_from(["nanobot", "serve"]);
    let gateway = Cli::parse_from(["nanobot", "gateway"]);

    match serve.command {
        Command::Serve {
            max_iterations,
            interval_ms,
            ..
        } => {
            assert_eq!(max_iterations, None);
            assert_eq!(interval_ms, 1000);
        }
        _ => panic!("expected serve command"),
    }

    match gateway.command {
        Command::Gateway {
            max_iterations,
            interval_ms,
            ..
        } => {
            assert_eq!(max_iterations, None);
            assert_eq!(interval_ms, 1000);
        }
        _ => panic!("expected gateway command"),
    }
}
