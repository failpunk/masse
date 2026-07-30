use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Decides which panes stay loaded. Two independent pressures:
///
///   * a cap (`max`), so opening a fourth pane closes the stalest one;
///   * an idle timeout, so a pane you opened this morning and walked away from
///     does not hold a WebKit content process all day.
///
/// Deliberately free of any webview types, so the policy governing the app's
/// memory can be tested without a window.
#[derive(Debug)]
pub struct Lru {
    order: Vec<String>,
    seen: HashMap<String, Instant>,
    max: usize,
}

impl Lru {
    pub fn new(max: usize) -> Self {
        Lru {
            order: Vec::new(),
            seen: HashMap::new(),
            max: max.max(1),
        }
    }

    /// Mark `key` most recently used; return keys that must now be destroyed.
    /// The key just touched is never returned, however small `max` is.
    pub fn touch(&mut self, key: &str) -> Vec<String> {
        self.touch_at(key, Instant::now())
    }

    /// Keys untouched for longer than `idle`. `keep` is never returned: whatever
    /// is on screen stays, no matter how long you have been staring at it.
    pub fn stale(&mut self, idle: Duration, keep: &str) -> Vec<String> {
        self.stale_at(Instant::now(), idle, keep)
    }

    fn touch_at(&mut self, key: &str, now: Instant) -> Vec<String> {
        self.order.retain(|k| k != key);
        self.order.push(key.to_string());
        self.seen.insert(key.to_string(), now);

        let mut evicted = Vec::new();
        while self.order.len() > self.max {
            let dropped = self.order.remove(0);
            self.seen.remove(&dropped);
            evicted.push(dropped);
        }
        evicted
    }

    fn stale_at(&mut self, now: Instant, idle: Duration, keep: &str) -> Vec<String> {
        let expired: Vec<String> = self
            .order
            .iter()
            .filter(|k| k.as_str() != keep)
            .filter(|k| {
                self.seen
                    .get(k.as_str())
                    .is_some_and(|last| now.duration_since(*last) >= idle)
            })
            .cloned()
            .collect();
        for key in &expired {
            self.order.retain(|k| k != key);
            self.seen.remove(key);
        }
        expired
    }

    #[cfg(test)]
    pub fn live(&self) -> &[String] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn holds_up_to_max_then_drops_the_oldest() {
        let mut lru = Lru::new(2);
        assert!(lru.touch("a").is_empty());
        assert!(lru.touch("b").is_empty());
        assert_eq!(lru.touch("c"), vec!["a"]);
        assert_eq!(lru.live(), ["b", "c"]);
    }

    #[test]
    fn revisiting_refreshes_rather_than_duplicating() {
        let mut lru = Lru::new(2);
        lru.touch("a");
        lru.touch("b");
        assert!(lru.touch("a").is_empty(), "'a' is already loaded");
        assert_eq!(lru.live(), ["b", "a"]);
        assert_eq!(lru.touch("c"), vec!["b"], "'b' is now the stale one");
    }

    #[test]
    fn the_pane_being_shown_is_never_evicted() {
        let mut lru = Lru::new(1);
        lru.touch("a");
        assert_eq!(lru.touch("b"), vec!["a"]);
        assert_eq!(lru.live(), ["b"]);
    }

    #[test]
    fn a_zero_budget_is_treated_as_one() {
        let mut lru = Lru::new(0);
        assert!(lru.touch("a").is_empty());
        assert_eq!(lru.live(), ["a"]);
    }

    #[test]
    fn idle_panes_are_reclaimed_but_the_visible_one_survives() {
        let start = Instant::now();
        let mut lru = Lru::new(4);
        lru.touch_at("mail", start);
        lru.touch_at("calendar", start);

        // Five minutes later nothing has been touched, and mail is on screen.
        let later = start + 5 * MIN;
        assert_eq!(lru.stale_at(later, 4 * MIN, "mail"), vec!["calendar"]);
        assert_eq!(lru.live(), ["mail"], "the visible pane is kept");
    }

    #[test]
    fn a_pane_used_recently_is_not_reclaimed() {
        let start = Instant::now();
        let mut lru = Lru::new(4);
        lru.touch_at("mail", start);
        lru.touch_at("calendar", start + 4 * MIN);
        let now = start + 5 * MIN;
        // calendar was touched a minute ago, so only mail has gone cold.
        assert_eq!(lru.stale_at(now, 4 * MIN, "calendar"), vec!["mail"]);
    }

    #[test]
    fn nothing_is_reclaimed_before_the_timeout() {
        let start = Instant::now();
        let mut lru = Lru::new(4);
        lru.touch_at("a", start);
        lru.touch_at("b", start);
        assert!(lru.stale_at(start + MIN, 10 * MIN, "a").is_empty());
        assert_eq!(lru.live().len(), 2);
    }

    #[test]
    fn a_reclaimed_pane_can_come_back() {
        let start = Instant::now();
        let mut lru = Lru::new(2);
        lru.touch_at("a", start);
        lru.touch_at("b", start);
        lru.stale_at(start + 9 * MIN, 5 * MIN, "b");
        assert_eq!(lru.live(), ["b"]);
        assert!(lru.touch_at("a", start + 10 * MIN).is_empty());
        assert_eq!(lru.live(), ["b", "a"]);
    }
}
