use clap::Parser;
use mithka_tdlib::{inspect_database, run, RunOptions, SessionConfig, TdJson};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "mithka-tdlib-spike",
    version,
    about = "Load a pinned Mithka libtdjson.so and list chats from a copied TDLib database"
)]
struct Cli {
    /// Path to Mithka's libtdjson.so (1.8.67 patched build).
    #[arg(long)]
    tdjson: PathBuf,

    /// Copied TDLib database directory (contains td.binlog and files/).
    #[arg(long)]
    database: PathBuf,

    /// Telegram api_id. Same value Mithka used for this database.
    #[arg(long, env = "TDLIB_API_ID")]
    api_id: i32,

    /// Telegram api_hash. Same value Mithka used for this database.
    #[arg(long, env = "TDLIB_API_HASH")]
    api_hash: String,

    /// TDLib device_model. Mithka on Linux sends Android.
    #[arg(long, default_value = "Android")]
    device_model: String,

    /// TDLib system_language_code.
    #[arg(long, default_value = "en")]
    system_language_code: String,

    /// TDLib system_version. Empty lets TDLib detect the OS version.
    #[arg(long, default_value = "Linux")]
    system_version: String,

    /// TDLib application_version.
    #[arg(long, default_value = "mithka-tdlib-spike/0.1.0")]
    application_version: String,

    /// How many chat titles to print from the main chat list.
    #[arg(long, default_value_t = 20)]
    chat_limit: i32,

    /// TDLib log verbosity forwarded to stderr (0 = fatal, 1 = errors).
    #[arg(long, default_value_t = 1)]
    verbosity: i32,

    /// Print each incoming TDLib @type. Request bodies are never printed.
    #[arg(long)]
    debug: bool,

    /// Seconds to wait for authorizationStateReady.
    #[arg(long, default_value_t = 90)]
    auth_timeout: u64,

    /// Seconds after Ready to wait for chat titles.
    #[arg(long, default_value_t = 20)]
    chat_timeout: u64,
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(code) => ExitCode::from(code),
        Err(failure) => {
            eprintln!("error: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

struct CliFailure {
    code: u8,
    message: String,
}

fn fail(code: u8, message: impl Into<String>) -> Result<u8, CliFailure> {
    Err(CliFailure {
        code,
        message: message.into(),
    })
}

fn run_cli() -> Result<u8, CliFailure> {
    let cli = Cli::parse();
    if cli.api_id <= 0 {
        return fail(2, "--api-id must be a positive integer");
    }
    let api_hash = cli.api_hash.trim().to_string();
    if api_hash.is_empty() {
        return fail(2, "--api-hash / TDLIB_API_HASH is empty");
    }
    if cli.device_model.trim().is_empty()
        || cli.system_language_code.trim().is_empty()
        || cli.application_version.trim().is_empty()
    {
        return fail(
            2,
            "--device-model, --system-language-code, and --application-version must be non-empty",
        );
    }
    if !(1..=100).contains(&cli.chat_limit) {
        return fail(2, "--chat-limit must be between 1 and 100");
    }
    if !cli.tdjson.is_file() {
        return fail(
            2,
            format!("--tdjson is not a file: {}", cli.tdjson.display()),
        );
    }

    let database = cli.database.canonicalize().map_err(|err| CliFailure {
        code: 2,
        message: format!(
            "cannot resolve --database {}: {err}",
            cli.database.display()
        ),
    })?;
    for warning in inspect_database(&database).map_err(|message| CliFailure { code: 2, message })? {
        eprintln!("warning: {warning}");
    }

    let database_directory = utf8_path(&database)?;
    let files = database.join("files");
    let files_directory = utf8_path(&files)?;
    let tdjson = cli.tdjson.canonicalize().unwrap_or(cli.tdjson);

    println!("libtdjson: {}", tdjson.display());
    println!("database: {database_directory}");
    println!("files: {files_directory}");
    println!("api_id: {}", cli.api_id);
    println!("api_hash: set ({} characters)", api_hash.chars().count());
    println!("device_model: {}", cli.device_model);
    println!("system_language_code: {}", cli.system_language_code);
    println!("system_version: {}", cli.system_version);
    println!("application_version: {}", cli.application_version);

    let td = TdJson::open(&tdjson).map_err(|err| CliFailure {
        code: 1,
        message: err.to_string(),
    })?;

    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    if let Err(err) = ctrlc::set_handler(move || {
        if !flag.swap(true, Ordering::SeqCst) {
            eprintln!("interrupt: closing TDLib");
        }
    }) {
        eprintln!("warning: Ctrl-C handler was not installed: {err}");
    }

    let cfg = SessionConfig {
        database_directory,
        files_directory,
        api_id: cli.api_id,
        api_hash,
        device_model: cli.device_model,
        system_language_code: cli.system_language_code,
        system_version: cli.system_version,
        application_version: cli.application_version,
        chat_limit: cli.chat_limit,
    };
    let opts = RunOptions {
        verbosity: cli.verbosity,
        debug: cli.debug,
        auth_timeout: Duration::from_secs(cli.auth_timeout),
        chat_timeout: Duration::from_secs(cli.chat_timeout),
        close_timeout: Duration::from_secs(8),
    };

    run(&td, cfg, stop.as_ref(), &opts)
        .map(|code| code as u8)
        .map_err(|message| CliFailure { code: 1, message })
}

fn utf8_path(path: &std::path::Path) -> Result<String, CliFailure> {
    path.to_str().map(str::to_string).ok_or_else(|| CliFailure {
        code: 2,
        message: format!("path is not valid UTF-8: {}", path.display()),
    })
}
