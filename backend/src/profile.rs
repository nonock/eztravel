//! What the phone sets through the API: which devices to notify, and until
//! when the user is away. Shared by the API and the poll loop, saved in the
//! data folder.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::service::{load, save};

/// Tokens change when the app is reinstalled: keep the latest few only.
pub const MAX_DEVICES: usize = 5;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Firebase tokens, oldest first.
    pub devices: Vec<String>,
    /// Last day away from the office, included.
    pub away_through: Option<Date>,
}

impl Profile {
    /// Adds a device, or makes it the most recent if already known.
    pub fn register(&mut self, token: &str) {
        self.forget(token);
        self.devices.push(token.to_string());
        let extra = self.devices.len().saturating_sub(MAX_DEVICES);
        self.devices.drain(..extra);
    }

    pub fn forget(&mut self, token: &str) {
        self.devices.retain(|known| known != token);
    }
}

/// The profile behind a lock, saved after each change.
pub struct Store {
    path: PathBuf,
    profile: Mutex<Profile>,
}

impl Store {
    /// Picks up the saved profile; without one, starts with `seed_devices`.
    pub fn open(path: &Path, seed_devices: &[String]) -> Self {
        let profile = load(path).unwrap_or_else(|| Profile {
            devices: seed_devices.to_vec(),
            away_through: None,
        });
        Self {
            path: path.to_path_buf(),
            profile: Mutex::new(profile),
        }
    }

    /// A copy, so that no lock is held while the caller works.
    pub fn profile(&self) -> Profile {
        self.profile
            .lock()
            .expect("no thread panics while holding the profile")
            .clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Profile)) {
        let mut profile = self
            .profile
            .lock()
            .expect("no thread panics while holding the profile");
        change(&mut profile);
        save(&self.path, &*profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn tokens(profile: &Profile) -> Vec<&str> {
        profile.devices.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_device_is_registered_once_and_the_latest_comes_last() {
        let mut profile = Profile::default();
        profile.register("a");
        profile.register("b");
        profile.register("a");
        assert_eq!(tokens(&profile), ["b", "a"]);
    }

    #[test]
    fn only_the_latest_devices_are_kept() {
        let mut profile = Profile::default();
        for token in ["1", "2", "3", "4", "5", "6", "7"] {
            profile.register(token);
        }
        assert_eq!(tokens(&profile), ["3", "4", "5", "6", "7"]);
    }

    #[test]
    fn a_device_can_be_forgotten() {
        let mut profile = Profile {
            devices: vec!["a".into(), "b".into()],
            away_through: None,
        };
        profile.forget("a");
        profile.forget("unknown");
        assert_eq!(tokens(&profile), ["b"]);
    }

    #[test]
    fn the_store_saves_each_change_and_finds_it_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let store = Store::open(&path, &["seed".into()]);
        assert_eq!(tokens(&store.profile()), ["seed"]);
        store.update(|profile| {
            profile.register("phone");
            profile.away_through = Some(date(2026, 12, 24));
        });
        // The seed only applies when nothing was saved.
        let reopened = Store::open(&path, &["other seed".into()]);
        assert_eq!(
            reopened.profile(),
            Profile {
                devices: vec!["seed".into(), "phone".into()],
                away_through: Some(date(2026, 12, 24))
            }
        );
    }
}
