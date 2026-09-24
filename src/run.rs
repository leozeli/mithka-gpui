//! Receive loop. One thread calls `td_receive`; Ctrl-C only flips an atomic.

use crate::driver::{
    bootstrap_request, parse_version, set_verbosity_request, version_request, Driver, Effect,
    SessionConfig, TimeoutKind,
};
use crate::tdjson::TdJson;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct RunOptions {
    pub verbosity: i32,
    pub debug: bool,
    pub auth_timeout: Duration,
    pub chat_timeout: Duration,
    pub close_timeout: Duration,
}

pub fn run(
    td: &TdJson,
    cfg: SessionConfig,
    stop: &AtomicBool,
    opts: &RunOptions,
) -> Result<i32, String> {
    emit("database_encryption_key: empty");
    emit("use_test_dc: false");
    emit(td.mithka().line().as_str());

    let verbosity = serde_json::to_string(&set_verbosity_request(opts.verbosity))
        .map_err(|err| err.to_string())?;
    match td.execute(&verbosity)? {
        Some(response) => {
            if response.contains("\"@type\":\"error\"") {
                emit(&format!("TDLib log verbosity was rejected: {response}"));
            }
        }
        None => emit("TDLib setLogVerbosityLevel returned no response"),
    }

    let version = serde_json::to_string(&version_request()).map_err(|err| err.to_string())?;
    match td.execute(&version)? {
        Some(response) => match parse_version(&response) {
            Some(version) => emit(&format!("TDLib version: {version}")),
            None => emit(&format!("TDLib version: (unparsed) {response}")),
        },
        None => emit("TDLib version: (no response)"),
    }

    let client_id = td.create_client_id();
    if client_id <= 0 {
        return Err(format!(
            "td_create_client_id returned {client_id}; the loaded library did not create a client"
        ));
    }
    emit(&format!("client_id: {client_id}"));

    let mut driver = Driver::new(cfg);
    dispatch(
        td,
        client_id,
        opts.debug,
        &Effect {
            send: vec![bootstrap_request()],
            ..Effect::default()
        },
    )?;

    let started = Instant::now();
    let mut collecting_since: Option<Instant> = None;
    let mut close_deadline: Option<Instant> = None;

    loop {
        if stop.load(Ordering::SeqCst) {
            if let Some(code) = dispatch(td, client_id, opts.debug, &driver.interrupt())? {
                return Ok(code);
            }
        }

        if driver.is_closing() && close_deadline.is_none() {
            close_deadline = Some(Instant::now() + opts.close_timeout);
        }
        if let Some(deadline) = close_deadline {
            if Instant::now() >= deadline {
                emit("close timed out; exiting");
                return Ok(driver.pending_code());
            }
        }

        if !driver.saw_ready() && !driver.is_closing() && started.elapsed() >= opts.auth_timeout {
            if let Some(code) = dispatch(
                td,
                client_id,
                opts.debug,
                &driver.on_timeout(TimeoutKind::Auth),
            )? {
                return Ok(code);
            }
        }

        if driver.saw_ready() && collecting_since.is_none() {
            collecting_since = Some(Instant::now());
        }
        if driver.is_collecting() {
            if let Some(since) = collecting_since {
                if since.elapsed() >= opts.chat_timeout {
                    if let Some(code) = dispatch(
                        td,
                        client_id,
                        opts.debug,
                        &driver.on_timeout(TimeoutKind::Chats),
                    )? {
                        return Ok(code);
                    }
                }
            }
        }

        let text = match td.receive(0.5) {
            Some(text) => text,
            None => continue,
        };
        let event: Value = match serde_json::from_str::<Value>(&text) {
            Ok(value) if value.is_object() => value,
            Ok(_) => {
                emit(&format!(
                    "ignored non-object TDLib event ({} bytes)",
                    text.len()
                ));
                continue;
            }
            Err(_) => {
                emit(&format!(
                    "ignored non-json TDLib event ({} bytes)",
                    text.len()
                ));
                continue;
            }
        };
        if opts.debug {
            let typ = event["@type"].as_str().unwrap_or("?");
            emit(&format!("event: {typ}"));
        }
        if let Some(code) = dispatch(td, client_id, opts.debug, &driver.on_event(&event))? {
            return Ok(code);
        }
    }
}

fn dispatch(
    td: &TdJson,
    client_id: i32,
    debug: bool,
    effect: &Effect,
) -> Result<Option<i32>, String> {
    for line in &effect.lines {
        emit(line);
    }
    for request in &effect.send {
        if debug {
            let typ = request["@type"].as_str().unwrap_or("?");
            emit(&format!("send: {typ}"));
        }
        let body = serde_json::to_string(request).map_err(|err| err.to_string())?;
        td.send(client_id, &body)?;
    }
    Ok(effect.exit)
}

fn emit(line: &str) {
    println!("{line}");
    let _ = io::stdout().flush();
}
