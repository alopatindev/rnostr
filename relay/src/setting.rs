use crate::Result;
use config::{Config, File};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Information {
    pub name: Option<String>,
    pub description: Option<String>,
    pub pubkey: Option<String>,
    pub contact: Option<String>,
    // supported_nips, software, version
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Db {
    pub path: PathBuf,
}

impl Default for Db {
    fn default() -> Self {
        Self {
            path: PathBuf::from("./data"),
        }
    }
}

/// number of threads config
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Thread {
    /// number of http server threads
    pub http: usize,
    /// number of read event threads
    pub reader: usize,
}

impl Default for Thread {
    fn default() -> Self {
        Self { reader: 0, http: 0 }
    }
}

/// network config
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Network {
    /// server bind host
    pub host: String,
    /// server bind port
    pub port: u16,
    /// heartbeat timeout (default 120 seconds, must bigger than heartbeat interval)
    /// How long before lack of client response causes a timeout
    pub heartbeat_timeout: u64,

    /// heartbeat interval
    /// How often heartbeat pings are sent
    pub heartbeat_interval: u64,

    pub real_ip_header: Option<Vec<String>>,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7707,
            heartbeat_interval: 60,
            heartbeat_timeout: 120,
            real_ip_header: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Limitation {
    /// this is the maximum number of bytes for incoming JSON. default 64K
    pub max_message_length: u64,
    /// total number of subscriptions that may be active on a single websocket connection to this relay. default 20
    pub max_subscriptions: u64,
    /// maximum number of filter values in each subscription. default 10
    pub max_filters: u64,
    /// the relay server will clamp each filter's limit value to this number. This means the client won't be able to get more than this number of events from a single subscription filter. default 300
    pub max_limit: u64,
    /// maximum length of subscription id as a string. default 100
    pub max_subid_length: u64,
    /// for authors and ids filters which are to match against a hex prefix, you must provide at least this many hex digits in the prefix. default 10
    pub min_prefix: u64,
    /// in any event, this is the maximum number of elements in the tags list. default 5000
    pub max_event_tags: u64,
    /// Events older than this will be rejected. default 3 years
    pub max_event_time_older_than_now: u64,
    /// Events newer than this will be rejected. default 15 minutes
    pub max_event_time_newer_than_now: u64,
}

impl Default for Limitation {
    fn default() -> Self {
        Self {
            max_message_length: 65536,
            max_subscriptions: 20,
            max_filters: 10,
            max_limit: 300,
            max_subid_length: 100,
            min_prefix: 10,
            max_event_tags: 5000,
            max_event_time_older_than_now: 94608000,
            max_event_time_newer_than_now: 900,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Setting {
    pub information: Information,
    pub db: Db,
    pub thread: Thread,
    pub network: Network,
    pub limitation: Limitation,

    /// flatten extensions setting
    #[serde(flatten)]
    pub extensions: HashMap<String, HashMap<String, Value>>,
}

pub type SettingWrapper = Arc<RwLock<Setting>>;

impl Setting {
    pub fn default_wrapper() -> SettingWrapper {
        Arc::new(RwLock::new(Self::default()))
    }

    /// information json
    pub fn render_information(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.information)?)
    }

    pub fn read_wrapper<P: AsRef<Path>>(file: P) -> Result<SettingWrapper> {
        Ok(Arc::new(RwLock::new(Self::read(file)?)))
    }

    pub fn read<P: AsRef<Path>>(file: P) -> Result<Self> {
        let def = Self::default();

        let builder = Config::builder();
        let config = builder
            // use defaults
            .add_source(Config::try_from(&def)?)
            // override with file contents
            .add_source(File::with_name(file.as_ref().to_str().unwrap()))
            .build()?;

        let setting: Setting = config.try_deserialize()?;
        Ok(setting)
    }

    pub fn watch<P: AsRef<Path>>(file: P) -> Result<(SettingWrapper, RecommendedWatcher)> {
        let setting = Self::read(&file)?;
        let setting = Arc::new(RwLock::new(setting));
        let c_file = file.as_ref().to_path_buf();
        let c_setting = Arc::clone(&setting);

        let mut watcher =
        // To make sure that the config lives as long as the function
        // we need to move the ownership of the config inside the function
        // To learn more about move please read [Using move Closures with Threads](https://doc.rust-lang.org/book/ch16-01-threads.html?highlight=move#using-move-closures-with-threads)
        RecommendedWatcher::new(move |result: Result<Event, notify::Error>| {
            match result {
                Ok(event) => {
                    if event.kind.is_modify() {
                        match Self::read(&c_file) {
                            Ok(new_setting) => {
                                info!("Reload config success {:?}", c_file);
                                let mut w = c_setting.write();
                                *w = new_setting;
                            }
                            Err(e) => {
                                error!(error = e.to_string(), "failed to reload config {:?}", c_file);
                            }
                        }
                    }
                },
                Err(e) => {
                    error!(error = e.to_string(), "failed to watch file {:?}", c_file);
                },
            }
        }, notify::Config::default())?;

        watcher.watch(file.as_ref(), RecursiveMode::NonRecursive)?;

        Ok((setting, watcher))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::{fs, thread::sleep, time::Duration};
    use tempfile::Builder;

    #[test]
    fn read() -> Result<()> {
        let file = Builder::new()
            .prefix("nostr-relay-config-test-read")
            .suffix(".toml")
            .rand_bytes(0)
            .tempfile()?;

        let setting = Setting::read(&file)?;
        assert_eq!(setting.information.name, None);
        fs::write(
            &file,
            r#"[information]
        name = "nostr"
        "#,
        )?;
        let setting = Setting::read(&file)?;
        assert_eq!(setting.information.name, Some("nostr".to_string()));
        Ok(())
    }

    #[test]
    fn watch() -> Result<()> {
        let file = Builder::new()
            .prefix("nostr-relay-config-test-watch")
            .suffix(".toml")
            .tempfile()?;

        let (setting, _watcher) = Setting::watch(&file)?;
        assert_eq!(setting.read().information.name, None);
        fs::write(
            &file,
            r#"[information]
    name = "nostr"
    "#,
        )?;
        sleep(Duration::from_millis(100));
        // println!("read {:?} {:?}", setting.read(), file);
        assert_eq!(setting.read().information.name, Some("nostr".to_string()));
        Ok(())
    }
}
