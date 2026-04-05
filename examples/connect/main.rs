use log::info;
use ocpp_client::logger::init_logger;
use ocpp_client::rest_server::start_rest_server;
use reqwest;
use serde_json::{json, Value};
use std::env;
use std::thread::sleep;
use std::time::Duration;

#[derive(Debug, Clone)]
struct CliArgs {
    run_server: bool,
    run_client: bool,
    test_file: Option<String>,
    speed_multiplier: f64,
    test_length: u64,
    data_directory: String,
}

impl CliArgs {
    fn parse() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();
        
        // If no arguments, show help
        if args.len() == 1 {
            Self::print_help();
            std::process::exit(0);
        }

        // Check for --help
        if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
            Self::print_help();
            std::process::exit(0);
        }

        let mut cli_args = CliArgs {
            run_server: args.contains(&"--server".to_string()),
            run_client: args.contains(&"--client".to_string()),
            test_file: None,
            speed_multiplier: 1.0,
            test_length: 1000,
            data_directory: "./data".to_string(),
        };

        // If neither server nor client specified, default to both
        if !cli_args.run_server && !cli_args.run_client {
            cli_args.run_server = true;
            cli_args.run_client = true;
        }

        // Parse --test argument
        if let Some(pos) = args.iter().position(|x| x == "--test") {
            if let Some(file) = args.get(pos + 1) {
                cli_args.test_file = Some(file.clone());
            } else {
                return Err("--test requires a filename argument".to_string());
            }
        }

        // Parse --speed argument
        if let Some(pos) = args.iter().position(|x| x == "--speed") {
            if let Some(speed_str) = args.get(pos + 1) {
                cli_args.speed_multiplier = speed_str
                    .parse()
                    .map_err(|_| format!("Invalid speed value: {}", speed_str))?;
                if cli_args.speed_multiplier <= 0.0 {
                    return Err("Speed multiplier must be positive".to_string());
                }
            } else {
                return Err("--speed requires a numeric argument".to_string());
            }
        }

        // Parse --test_length argument
        if let Some(pos) = args.iter().position(|x| x == "--test_length") {
            if let Some(length_str) = args.get(pos + 1) {
                cli_args.test_length = length_str
                    .parse()
                    .map_err(|_| format!("Invalid test_length value: {}", length_str))?;
                if cli_args.test_length == 0 {
                    return Err("Test length must be greater than 0".to_string());
                }
            } else {
                return Err("--test_length requires a numeric argument".to_string());
            }
        }

        // Parse --data_directory argument
        if let Some(pos) = args.iter().position(|x| x == "--data_directory") {
            if let Some(dir) = args.get(pos + 1) {
                cli_args.data_directory = dir.clone();
            } else {
                return Err("--data_directory requires a path argument".to_string());
            }
        }

        // Validate: test file only with client mode
        if cli_args.test_file.is_some() && !cli_args.run_client {
            return Err("--test can only be used with --client mode".to_string());
        }

            info!(
        "OCPP Client starting with options: run_server={}, run_client={}, test_file={:?}, speed={}, test_length={}, data_directory={}",
        cli_args.run_server, cli_args.run_client, cli_args.test_file, cli_args.speed_multiplier, cli_args.test_length, cli_args.data_directory
    );

        Ok(cli_args)
    }

    fn print_help() {
        let help = r#"
OCPP Client Simulator - CLI Reference
======================================

USAGE:
    ocpp-client [OPTIONS]

OPTIONS:
    --server                Start only the REST API server
    --client                Enable client mode (make requests/load test files)
    --test <FILE>           Load test configuration from JSON file (requires --client)
    --speed <MULTIPLIER>    Simulation speed multiplier (default: 1.0)
                           e.g., --speed 2.0 runs 2x faster
    --test_length <SECONDS> Duration to run the simulation (default: 1000)
    --data_directory <PATH> Directory containing JSON data files (default: ./data)
    --help, -h             Show this help message

MODES:
    1. Server only:
       ocpp-client --server
       Starts REST API server for dynamic interaction

    2. Client with test file (Static):
       ocpp-client --client --test test_0_1.json
       Loads configuration, runs events, exits when complete

    3. Interactive (Server + Client):
       ocpp-client --server --client
       Combines REST API with CLI interaction

EXAMPLES:
    # Start server at http://localhost:3000
    ocpp-client --server

    # Run static test 2x faster
    ocpp-client --server --client --test test_0_1.json --speed 2.0

    # Use custom data directory
    ocpp-client --server --client --data_directory /path/to/data

    # Start interactive mode
    ocpp-client --server --client --speed 0.5

REST API ENDPOINTS:
    POST   /cp/from-file        Create charger from config files
    POST   /cp/from-rest        Create charger with REST parameters
    GET    /cp                  List all chargers
    POST   /cp/ID/events        Add event to charger  
    GET    /summary             Get test summary

For detailed information, see REST_API.md and CLI_REFERENCE.md
"#;
        println!("{}", help);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_logger();
    println!("OCPP Client Simulator starting up...");
    let args = CliArgs::parse()?;
    println!("OCPP Client Simulator Parsed Args");
    println!("logs can be found in ./logs/ with 1 general log and 1 log per charger");


    if args.run_server {
        // Start REST API server in a background task
        let _rest_server = start_rest_server().await;
        info!("REST Server started at http://127.0.0.1:3000");
    }

    if args.run_client {
        if let Some(test_file) = &args.test_file {
            // Static mode: load test file and run it
            info!("Static mode: Loading test configuration from {}", test_file);

            // Give REST server a moment to start if both are running
            if args.run_server {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            rest_client_example(test_file, args.speed_multiplier, args.test_length, args.data_directory).await?;

            // In static mode, exit after test completes
            info!("Test completed, exiting.");
            return Ok(());
        } else {
            // Interactive mode: just keep running and wait for Ctrl-C
            info!("Interactive mode started. Send requests to http://127.0.0.1:3000");
            info!("Press Ctrl-C to exit.");
        }
    }

    // Wait for Ctrl-C in server or interactive mode
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, exiting gracefully.");

    Ok(())
}

async fn rest_client_example(
    test_config: &str,
    _speed_multiplier: f64,
    test_length: u64,
    data_directory: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("=== REST Client - Loading test configuration ===");

    let client = reqwest::Client::new();
    let rest_url = "http://127.0.0.1:3000";

    info!("Fetching test configuration from file: {}", test_config);

    let config_text = std::fs::read_to_string(test_config)?;
    let config_json: Value = serde_json::from_str(&config_text)?;

    let cp_names = config_json
        .get("cps")
        .and_then(|v| v.as_array())
        .ok_or("test config must contain cps array")?;

    info!("Test config contains {} chargers", cp_names.len());

    for cp_item in cp_names {
        let cp_name = cp_item.as_str().ok_or("cp name must be string")?;

        let cp_request = json!({
            "name": cp_name,
            "use_original": false,
            "data_directory": data_directory
        });

        let cp_response = client
            .post(&format!("{}/cp/from-file", rest_url))
            .json(&cp_request)
            .send()
            .await?;

        let cp_data: Value = cp_response.json().await?;
        info!(
            "Created charger '{}' with ID: {}",
            cp_name,
            cp_data
                .get("cp_id")
                .map(|v| v.to_string())
                .unwrap_or("?".to_string())
        );
    }

    info!("Test configuration loaded successfully");
    info!("Running test for {} seconds...", test_length);
    tokio::time::sleep(Duration::from_secs(test_length)).await;
    
    // Generate summary JSON
    generate_test_summary(&client, rest_url).await?;
    
    Ok(())
}

async fn generate_test_summary(
    client: &reqwest::Client,
    rest_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Generating test summary...");

    let summary_url = format!("{}/summary", rest_url);
    let response = client.get(&summary_url).send().await?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch summary from {}: {}",
            summary_url,
            response.status()
        )
        .into());
    }

    let summary_json: serde_json::Value = response.json().await?;
    let summary_path = "./logs/summary.json";
    let summary_text = serde_json::to_string_pretty(&summary_json)?;
    std::fs::write(summary_path, summary_text)?;

    info!("Test summary written to {}", summary_path);
    Ok(())
}
