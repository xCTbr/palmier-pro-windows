//! Where provider API keys live.
//!
//! Keys are read from the OS keychain when one is reachable, and from the environment
//! otherwise. They are never written to a file, and never appear in a log, an error, or
//! a tool response — only a count and a masked hint ever leave this module.

use std::fmt;

const SERVICE: &str = "palmier";

/// One provider's keys, in the order they should be tried.
#[derive(Clone, Default)]
pub struct KeyRing {
    keys: Vec<String>,
}

impl fmt::Debug for KeyRing {
    /// Never print the keys themselves, not even in a panic message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyRing({} keys)", self.keys.len())
    }
}

impl KeyRing {
    pub fn from_keys(keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            keys: keys.into_iter().filter(|k| !k.trim().is_empty()).collect(),
        }
    }

    /// Load a provider's keys: the keychain first, then the environment.
    ///
    /// `env_prefix` names the first variable and numbers the rest — `STITCH_API_KEY`,
    /// `STITCH_API_KEY_2`, and so on, matching how these are normally set already.
    pub fn load(provider: &str, env_prefix: &str) -> Self {
        let mut keys = Self::from_keychain(provider);
        if keys.is_empty() {
            keys = Self::from_env(env_prefix);
        }
        Self::from_keys(keys)
    }

    fn from_env(prefix: &str) -> Vec<String> {
        let mut keys = Vec::new();
        if let Ok(first) = std::env::var(prefix) {
            keys.push(first);
        }
        // Numbered from 2, and stops at the first gap so a typo cannot silently hide keys.
        for n in 2..=16 {
            match std::env::var(format!("{prefix}_{n}")) {
                Ok(key) => keys.push(key),
                Err(_) => break,
            }
        }
        keys
    }

    fn from_keychain(provider: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for n in 1..=16 {
            let account = format!("{provider}-{n}");
            match keyring::Entry::new(SERVICE, &account).and_then(|e| e.get_password()) {
                Ok(key) => keys.push(key),
                // A missing entry ends the list; an unreachable keychain (headless Linux,
                // no secret service) simply yields none and the environment is used.
                Err(_) => break,
            }
        }
        keys
    }

    /// Store a key in the OS keychain at `slot`, counting from 1.
    pub fn store(provider: &str, slot: usize, key: &str) -> Result<(), KeyError> {
        if key.trim().is_empty() {
            return Err(KeyError::Empty);
        }
        let account = format!("{provider}-{slot}");
        keyring::Entry::new(SERVICE, &account)
            .and_then(|entry| entry.set_password(key))
            .map_err(|error| KeyError::Keychain(error.to_string()))
    }

    /// Forget the key at `slot`.
    pub fn forget(provider: &str, slot: usize) -> Result<(), KeyError> {
        let account = format!("{provider}-{slot}");
        keyring::Entry::new(SERVICE, &account)
            .and_then(|entry| entry.delete_credential())
            .map_err(|error| KeyError::Keychain(error.to_string()))
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.keys.get(index).map(String::as_str)
    }

    /// Enough of each key to tell them apart in a UI, and not enough to use one.
    pub fn hints(&self) -> Vec<String> {
        self.keys.iter().map(|key| mask(key)).collect()
    }
}

/// `AIzaSyC…9f2K` — first four and last four, whatever the length.
fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 10 {
        return "•".repeat(chars.len().max(4));
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("the key is empty")]
    Empty,
    #[error("the system keychain is unavailable: {0}")]
    Keychain(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_never_appears_in_debug_output() {
        let ring = KeyRing::from_keys(["AIzaSyCsupersecretvalue9f2K".to_string()]);
        let printed = format!("{ring:?}");
        assert!(!printed.contains("supersecret"), "{printed}");
        assert_eq!(printed, "KeyRing(1 keys)");
    }

    #[test]
    fn a_hint_identifies_without_revealing() {
        let ring = KeyRing::from_keys(["AIzaSyCsupersecretvalue9f2K".to_string()]);
        let hint = &ring.hints()[0];
        assert_eq!(hint, "AIza…9f2K");
        assert!(!hint.contains("supersecret"));
    }

    #[test]
    fn a_short_key_is_masked_entirely() {
        let ring = KeyRing::from_keys(["short".to_string()]);
        assert_eq!(ring.hints()[0], "•••••");
    }

    #[test]
    fn blank_entries_are_dropped_rather_than_tried() {
        let ring = KeyRing::from_keys(["a".into(), "  ".into(), "b".into(), String::new()]);
        assert_eq!(ring.len(), 2);
    }
}
