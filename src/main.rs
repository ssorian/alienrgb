use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let set_all_json = arguments
        .first()
        .is_some_and(|command| command == "set-all")
        && arguments.iter().any(|argument| argument == "--json");
    let cli = match alienrgb::cli::parse(arguments) {
        Ok(cli) => cli,
        Err(message) => {
            if set_all_json {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "schema_version": 1,
                        "command": "set_all",
                        "overall_status": "preflight_failure",
                        "failure_code": "invalid_arguments",
                        "transport_attempted": false,
                        "transport_performed": false,
                        "message": message,
                    })
                );
            } else {
                eprintln!("{message}");
            }
            return ExitCode::from(2);
        }
    };
    let json = cli.json;
    match alienrgb::cli::run(cli) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if json {
                if let Some(error) = error.downcast_ref::<alienrgb::cli::CliRunError>() {
                    let value = error.report.as_ref().map_or_else(
                        || serde_json::to_string_pretty(error),
                        serde_json::to_string_pretty,
                    );
                    match value {
                        Ok(value) => eprintln!("{value}"),
                        Err(_) => eprintln!("{}", serde_json::json!({"code":"render_failed"})),
                    }
                } else {
                    eprintln!(
                        "{}",
                        serde_json::json!({"code":"runtime_failed","message":error.to_string(),"transport_performed":false})
                    );
                }
            } else {
                eprintln!("alienrgb: {error}");
            }
            ExitCode::from(1)
        }
    }
}
