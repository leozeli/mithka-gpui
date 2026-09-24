//! Local RSS, Atom, and JSON Feed subscriptions.
//!
//! This crate does not talk to TDLib. The GPUI shell stores sources and cached
//! items in `$XDG_DATA_HOME/ad.neko.mithka.gpui/subscriptions.json` and fetches
//! documents on a background thread.

mod fetch;
mod parse;
mod store;
mod text;

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

pub use parse::{parse_document, ParsedFeed, ParsedItem};
pub use store::{store_path, store_path_in, Item, Source, SubscriptionStore};

#[derive(Clone, Debug)]
pub struct RefreshRequest {
    pub id: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub enum FeedEvent {
    Fetched {
        id: String,
        title: String,
        items: Vec<ParsedItem>,
    },
    Failed {
        id: String,
        message: String,
    },
}

pub struct FeedClient {
    jobs: Sender<RefreshRequest>,
    events: Mutex<Receiver<FeedEvent>>,
}

impl FeedClient {
    pub fn spawn() -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        thread::Builder::new()
            .name("mithka-rss".into())
            .spawn(move || worker(job_rx, event_tx))
            .expect("spawn rss worker");
        Self {
            jobs,
            events: Mutex::new(events),
        }
    }

    pub fn refresh(&self, id: String, url: String) {
        let _ = self.jobs.send(RefreshRequest { id, url });
    }

    pub fn drain(&self) -> Vec<FeedEvent> {
        let events = self.events.lock().unwrap_or_else(|err| err.into_inner());
        let mut out = Vec::new();
        while let Ok(event) = events.try_recv() {
            out.push(event);
        }
        out
    }
}

fn worker(jobs: Receiver<RefreshRequest>, events: Sender<FeedEvent>) {
    while let Ok(job) = jobs.recv() {
        let event = match fetch::fetch_feed(&job.url, &job.id) {
            Ok(parsed) => FeedEvent::Fetched {
                id: job.id,
                title: parsed.title,
                items: parsed.items,
            },
            Err(message) => FeedEvent::Failed {
                id: job.id,
                message,
            },
        };
        if events.send(event).is_err() {
            break;
        }
    }
}
