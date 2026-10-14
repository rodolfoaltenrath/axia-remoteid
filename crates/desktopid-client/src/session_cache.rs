//! Temporary capabilities only. Passwords and OTPs never enter this cache.
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};
use zeroize::Zeroizing;

pub const AUTHORIZATION_TTL: Duration = Duration::from_secs(10 * 60);
pub(crate) type CacheKey = (String, String, String, String, String);

#[derive(Clone, Copy)]
pub(crate) struct Clock {
    pub monotonic: Instant,
    pub wall: SystemTime,
}
impl Clock {
    pub fn now() -> Self {
        Self {
            monotonic: Instant::now(),
            wall: SystemTime::now(),
        }
    }
}
struct Entry {
    token: Zeroizing<String>,
    created: Clock,
}
impl Entry {
    fn remaining(&self, now: Clock) -> Duration {
        let Some(elapsed) = now.monotonic.checked_duration_since(self.created.monotonic) else {
            return Duration::ZERO;
        };
        let Ok(wall_elapsed) = now.wall.duration_since(self.created.wall) else {
            return Duration::ZERO;
        };
        // Wall time covers system suspend; monotonic time prevents clock
        // adjustments from extending an authorization.
        AUTHORIZATION_TTL.saturating_sub(elapsed.max(wall_elapsed))
    }
}
#[derive(Default)]
pub(crate) struct SessionCache {
    entries: HashMap<CacheKey, Entry>,
}
impl SessionCache {
    pub fn purge(&mut self, now: Clock) {
        self.entries
            .retain(|_, entry| !entry.remaining(now).is_zero());
    }
    pub fn get(&mut self, key: &CacheKey, now: Clock) -> Option<Zeroizing<String>> {
        self.purge(now);
        self.entries.get(key).map(|entry| entry.token.clone())
    }
    pub fn put(&mut self, key: CacheKey, token: Zeroizing<String>, created: Clock) {
        self.purge(Clock::now());
        if self.entries.len() >= 16 {
            self.entries.clear();
        }
        self.entries.insert(key, Entry { token, created });
    }
    pub fn remove(&mut self, key: &CacheKey) {
        self.entries.remove(key);
    }
    pub fn retain_context(&mut self, key: &CacheKey) {
        self.entries
            .retain(|other, _| other.0 == key.0 && other.1 == key.1 && other.2 == key.2);
    }
    pub fn next_expiration(&mut self, now: Clock) -> Option<Duration> {
        self.purge(now);
        self.entries
            .values()
            .map(|entry| entry.remaining(now))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(certificate: &str) -> CacheKey {
        (
            "https://example.invalid".into(),
            "desktop".into(),
            "identity".into(),
            certificate.into(),
            "reference".into(),
        )
    }
    fn later(clock: Clock, duration: Duration) -> Clock {
        Clock {
            monotonic: clock.monotonic + duration,
            wall: clock.wall + duration,
        }
    }
    #[test]
    fn token_expires_at_ten_minutes_without_sliding_on_use() {
        let mut cache = SessionCache::default();
        let start = Clock::now();
        cache.put(key("a"), Zeroizing::new("lab-token".into()), start);
        assert!(
            cache
                .get(&key("a"), later(start, Duration::from_secs(599)))
                .is_some()
        );
        assert!(
            cache
                .get(&key("a"), later(start, AUTHORIZATION_TTL))
                .is_none()
        );
    }
    #[test]
    fn certificates_and_identity_context_do_not_share_tokens() {
        let mut cache = SessionCache::default();
        let now = Clock::now();
        cache.put(key("a"), Zeroizing::new("lab-token".into()), now);
        assert!(cache.get(&key("b"), now).is_none());
        let mut changed = key("a");
        changed.2 = "different-identity".into();
        cache.retain_context(&changed);
        assert!(cache.get(&key("a"), now).is_none());
    }
    #[test]
    fn suspend_and_backwards_wall_clock_discard_tokens() {
        let start = Clock::now();
        for wall in [
            start.wall + AUTHORIZATION_TTL,
            start.wall - Duration::from_secs(1),
        ] {
            let mut cache = SessionCache::default();
            cache.put(key("a"), Zeroizing::new("lab-token".into()), start);
            assert!(
                cache
                    .get(
                        &key("a"),
                        Clock {
                            monotonic: start.monotonic,
                            wall
                        }
                    )
                    .is_none()
            );
        }
    }
}
