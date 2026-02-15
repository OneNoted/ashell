use std::time::{Duration, Instant};

use crate::{config::NotificationsModuleConfig, services::notifications::Notification};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupPhase {
    SlideIn,
    Display,
    SlideOut,
}

#[derive(Debug, Clone)]
pub struct PopupEntry {
    pub notification: Notification,
    pub phase: PopupPhase,
    pub phase_started: Instant,
    pub display_duration: Duration,
}

pub struct PopupState {
    pub entries: Vec<PopupEntry>,
    pub max_visible: usize,
    pub animation_duration: Duration,
}

impl PopupState {
    pub fn new(config: &NotificationsModuleConfig) -> Self {
        Self {
            entries: Vec::new(),
            max_visible: config.popup_max_visible,
            animation_duration: Duration::from_millis(config.popup_animation_ms),
        }
    }

    pub fn update_config(&mut self, config: &NotificationsModuleConfig) {
        self.max_visible = config.popup_max_visible;
        self.animation_duration = Duration::from_millis(config.popup_animation_ms);
    }

    pub fn enqueue(&mut self, notification: Notification, display_duration: Duration) {
        // If this notification replaces an existing one, remove the old entry
        self.entries
            .retain(|e| e.notification.id != notification.id);

        let now = Instant::now();
        self.entries.push(PopupEntry {
            notification,
            phase: PopupPhase::SlideIn,
            phase_started: now,
            display_duration,
        });

        // If we exceed max_visible, transition oldest to SlideOut
        while self.entries.iter().filter(|e| e.phase != PopupPhase::SlideOut).count()
            > self.max_visible
        {
            if let Some(oldest) = self
                .entries
                .iter_mut()
                .find(|e| e.phase != PopupPhase::SlideOut)
            {
                oldest.phase = PopupPhase::SlideOut;
                oldest.phase_started = now;
            } else {
                break;
            }
        }
    }

    /// Advance phases, remove completed entries. Returns true if entries changed.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let anim = self.animation_duration;
        let mut changed = false;

        for entry in &mut self.entries {
            let elapsed = now.duration_since(entry.phase_started);
            match entry.phase {
                PopupPhase::SlideIn => {
                    if elapsed >= anim {
                        entry.phase = PopupPhase::Display;
                        entry.phase_started = now;
                        changed = true;
                    }
                }
                PopupPhase::Display => {
                    if elapsed >= entry.display_duration {
                        entry.phase = PopupPhase::SlideOut;
                        entry.phase_started = now;
                        changed = true;
                    }
                }
                PopupPhase::SlideOut => {
                    // Will be removed below
                }
            }
        }

        let before = self.entries.len();
        self.entries.retain(|e| {
            if e.phase == PopupPhase::SlideOut {
                let elapsed = now.duration_since(e.phase_started);
                elapsed < anim
            } else {
                true
            }
        });
        if self.entries.len() != before {
            changed = true;
        }

        changed
    }

    pub fn dismiss(&mut self, id: u32) {
        let now = Instant::now();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.notification.id == id) {
            entry.phase = PopupPhase::SlideOut;
            entry.phase_started = now;
        }
    }

    pub fn is_active(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Overall bubble visibility progress (0.0-1.0).
    /// Max of individual entry progresses so bubble stays visible while any entry animates.
    #[cfg(test)]
    pub fn bubble_progress(&self) -> f32 {
        self.bubble_progress_at(Instant::now())
    }

    pub fn bubble_progress_at(&self, now: Instant) -> f32 {
        self.entries
            .iter()
            .map(|e| self.entry_progress_at(e, now))
            .fold(0.0_f32, f32::max)
    }

    #[cfg(test)]
    pub fn entry_progress_staggered(&self, entry: &PopupEntry, index: usize) -> f32 {
        self.entry_progress_staggered_at(entry, index, Instant::now())
    }

    pub fn entry_progress_staggered_at(
        &self,
        entry: &PopupEntry,
        index: usize,
        now: Instant,
    ) -> f32 {
        const STAGGER_DELAY_MS: u64 = 40;

        let elapsed = now.duration_since(entry.phase_started).as_secs_f32();
        let anim = self.animation_duration.as_secs_f32();
        let stagger = index as f32 * (STAGGER_DELAY_MS as f32 / 1000.0);

        match entry.phase {
            PopupPhase::SlideIn => {
                let effective = (elapsed - stagger).max(0.0);
                let t = (effective / anim).min(1.0);
                ease_out_back(t)
            }
            PopupPhase::Display => 1.0,
            PopupPhase::SlideOut => {
                let t = (elapsed / anim).min(1.0);
                1.0 - ease_in_cubic(t)
            }
        }
    }

    #[cfg(test)]
    pub fn entry_progress(&self, entry: &PopupEntry) -> f32 {
        self.entry_progress_at(entry, Instant::now())
    }

    /// Entry progress used for surface-level sizing. Uses ease_out_cubic (no overshoot)
    /// so the Wayland surface never grows past its target size.
    pub fn entry_progress_at(&self, entry: &PopupEntry, now: Instant) -> f32 {
        let elapsed = now.duration_since(entry.phase_started).as_secs_f32();
        let anim = self.animation_duration.as_secs_f32();

        match entry.phase {
            PopupPhase::SlideIn => {
                let t = (elapsed / anim).min(1.0);
                ease_out_cubic(t)
            }
            PopupPhase::Display => 1.0,
            PopupPhase::SlideOut => {
                let t = (elapsed / anim).min(1.0);
                1.0 - ease_in_cubic(t)
            }
        }
    }

    /// Compute stable surface height that avoids per-frame Wayland surface resizing.
    /// - If any entry is active (SlideIn/Display): snap to full target immediately.
    /// - If all entries are SlideOut: animate down monotonically (no overshoot).
    /// - If no entries: 0.
    pub fn target_surface_height_at(&self, now: Instant) -> f32 {
        let active_count = self
            .entries
            .iter()
            .filter(|e| e.phase != PopupPhase::SlideOut)
            .count();

        if active_count > 0 {
            // Snap to full target — surface stays stable during entry animations
            (active_count as f32) * 80.0 + 16.0
        } else if !self.entries.is_empty() {
            // All entries exiting — shrink monotonically using max progress
            let max_progress = self
                .entries
                .iter()
                .map(|e| self.entry_progress_at(e, now))
                .fold(0.0_f32, f32::max);
            let entry_count = self.entries.len();
            ((entry_count as f32) * 80.0 + 16.0) * max_progress
        } else {
            0.0
        }
    }
}

fn ease_out_back(t: f32) -> f32 {
    let c1: f32 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::notifications::{Notification, Urgency};
    use std::thread;

    fn test_config() -> NotificationsModuleConfig {
        NotificationsModuleConfig {
            max_notifications: 50,
            default_timeout: 5000,
            popup_enabled: true,
            popup_max_visible: 3,
            popup_duration_ms: 5000,
            popup_animation_ms: 100, // short for fast tests
        }
    }

    fn make_notification(id: u32) -> Notification {
        Notification {
            id,
            app_name: format!("App{id}"),
            icon: None,
            summary: format!("Title {id}"),
            body: format!("Body {id}"),
            actions: vec![],
            urgency: Urgency::Normal,
            timestamp: chrono::Local::now(),
            transient: false,
        }
    }

    // --- Easing functions ---

    #[test]
    fn ease_out_back_boundaries() {
        assert!((ease_out_back(0.0)).abs() < f32::EPSILON);
        assert!((ease_out_back(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ease_in_cubic_boundaries() {
        assert!((ease_in_cubic(0.0)).abs() < f32::EPSILON);
        assert!((ease_in_cubic(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ease_out_back_is_fast_start_slow_end() {
        // At t=0.5, ease_out_back should be > 0.5 (front-loaded)
        assert!(ease_out_back(0.5) > 0.5);
    }

    #[test]
    fn ease_out_back_overshoots() {
        // ease_out_back should exceed 1.0 at some point mid-animation
        let peak = (0..=100)
            .map(|i| ease_out_back(i as f32 / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.0, "expected overshoot > 1.0, got {peak}");
    }

    #[test]
    fn ease_out_cubic_boundaries() {
        assert!((ease_out_cubic(0.0)).abs() < f32::EPSILON);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ease_out_cubic_no_overshoot() {
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let v = ease_out_cubic(t);
            assert!(
                v <= 1.0 + f32::EPSILON,
                "ease_out_cubic({t}) = {v} overshoots 1.0"
            );
        }
    }

    #[test]
    fn ease_in_is_slow_start_fast_end() {
        // At t=0.5, ease_in_cubic should be < 0.5 (back-loaded)
        assert!(ease_in_cubic(0.5) < 0.5);
    }

    #[test]
    fn entry_progress_staggered_delays_later_entries() {
        let config = test_config(); // 100ms animation
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        state.enqueue(make_notification(2), Duration::from_secs(5));

        // Both entries just enqueued — index 0 should have more progress than index 1
        let p0 = state.entry_progress_staggered(&state.entries[0].clone(), 0);
        let p1 = state.entry_progress_staggered(&state.entries[1].clone(), 1);
        assert!(
            p0 >= p1,
            "index 0 progress ({p0}) should be >= index 1 progress ({p1})"
        );
    }

    // --- PopupState: enqueue ---

    #[test]
    fn enqueue_adds_entry_in_slide_in_phase() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));

        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].phase, PopupPhase::SlideIn);
        assert_eq!(state.entries[0].notification.id, 1);
        assert!(state.is_active());
    }

    #[test]
    fn enqueue_replaces_notification_with_same_id() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        state.enqueue(make_notification(1), Duration::from_secs(5));

        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].notification.id, 1);
    }

    #[test]
    fn enqueue_respects_max_visible() {
        let config = test_config(); // max_visible = 3
        let mut state = PopupState::new(&config);

        for i in 1..=4 {
            state.enqueue(make_notification(i), Duration::from_secs(5));
        }

        // 4 entries total, but only 3 should be non-SlideOut
        let non_slide_out = state
            .entries
            .iter()
            .filter(|e| e.phase != PopupPhase::SlideOut)
            .count();
        assert_eq!(non_slide_out, 3);

        // The oldest (id=1) should be in SlideOut
        let oldest = state.entries.iter().find(|e| e.notification.id == 1).unwrap();
        assert_eq!(oldest.phase, PopupPhase::SlideOut);
    }

    // --- PopupState: tick phase transitions ---

    #[test]
    fn tick_transitions_slide_in_to_display() {
        let config = test_config(); // 100ms animation
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        assert_eq!(state.entries[0].phase, PopupPhase::SlideIn);

        // Wait longer than animation duration
        thread::sleep(Duration::from_millis(150));
        let changed = state.tick();

        assert!(changed);
        assert_eq!(state.entries[0].phase, PopupPhase::Display);
    }

    #[test]
    fn tick_transitions_display_to_slide_out() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_millis(50));

        // Wait past animation to get to Display
        thread::sleep(Duration::from_millis(150));
        state.tick();
        assert_eq!(state.entries[0].phase, PopupPhase::Display);

        // Wait past display_duration to get to SlideOut
        thread::sleep(Duration::from_millis(100));
        let changed = state.tick();

        assert!(changed);
        assert_eq!(state.entries[0].phase, PopupPhase::SlideOut);
    }

    #[test]
    fn tick_removes_completed_slide_out_entries() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_millis(10));

        // tick() only transitions one phase per call (resets phase_started),
        // so we must wait and tick through each phase separately.
        // SlideIn → Display
        thread::sleep(Duration::from_millis(150));
        state.tick();
        // Display → SlideOut
        thread::sleep(Duration::from_millis(50));
        state.tick();
        // SlideOut complete → removed
        thread::sleep(Duration::from_millis(150));
        state.tick();

        assert!(state.entries.is_empty());
        assert!(!state.is_active());
    }

    #[test]
    fn tick_returns_false_when_no_changes() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));

        // Immediately tick — should not change (still in SlideIn, animation not done)
        let changed = state.tick();
        assert!(!changed);
    }

    // --- PopupState: dismiss ---

    #[test]
    fn dismiss_transitions_to_slide_out() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        state.dismiss(1);

        assert_eq!(state.entries[0].phase, PopupPhase::SlideOut);
    }

    #[test]
    fn dismiss_nonexistent_id_is_noop() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        state.dismiss(999);

        assert_eq!(state.entries[0].phase, PopupPhase::SlideIn);
    }

    // --- PopupState: bubble_progress ---

    #[test]
    fn bubble_progress_is_zero_when_empty() {
        let config = test_config();
        let state = PopupState::new(&config);

        assert!((state.bubble_progress()).abs() < f32::EPSILON);
    }

    #[test]
    fn bubble_progress_starts_near_zero_on_slide_in() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        let progress = state.bubble_progress();

        // Just enqueued, progress should be very small (near 0)
        assert!(progress < 0.3, "expected < 0.3, got {progress}");
    }

    #[test]
    fn bubble_progress_is_one_during_display() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));

        // Wait past animation
        thread::sleep(Duration::from_millis(150));
        state.tick();

        assert_eq!(state.entries[0].phase, PopupPhase::Display);
        assert!((state.bubble_progress() - 1.0).abs() < f32::EPSILON);
    }

    // --- Auto-resize clamping logic (mirrors iced program.rs) ---
    //
    // These test the pure math that determines the target surface size
    // from content_size and auto_size_limits, independent of iced internals.

    /// Simulates the auto-resize clamping: given content dimensions and
    /// limits min, returns the clamped target size.
    fn auto_resize_target(content_w: f32, content_h: f32, min_w: f32, min_h: f32) -> (f32, f32) {
        let w = content_w.max(min_w).max(1.0);
        let h = content_h.max(min_h).max(1.0);
        (w, h)
    }

    /// Simulates the size-change check that gates request_surface_size calls.
    fn should_resize(target_w: f32, target_h: f32, current_w: f32, current_h: f32) -> bool {
        (target_w - current_w).abs() > 0.5 || (target_h - current_h).abs() > 0.5
    }

    #[test]
    fn auto_resize_clamps_zero_content_to_min() {
        // Empty popup: content is 0x0, limits min is 1x1
        let (w, h) = auto_resize_target(0.0, 0.0, 1.0, 1.0);
        assert_eq!((w, h), (1.0, 1.0));
    }

    #[test]
    fn auto_resize_clamps_to_absolute_minimum_of_1() {
        // Even with min_w=0, the floor is 1.0
        let (w, h) = auto_resize_target(0.0, 0.0, 0.0, 0.0);
        assert_eq!((w, h), (1.0, 1.0));
    }

    #[test]
    fn auto_resize_passes_through_normal_content() {
        let (w, h) = auto_resize_target(500.0, 96.0, 1.0, 1.0);
        assert_eq!((w, h), (500.0, 96.0));
    }

    #[test]
    fn auto_resize_uses_min_when_content_smaller() {
        let (w, h) = auto_resize_target(50.0, 10.0, 100.0, 50.0);
        assert_eq!((w, h), (100.0, 50.0));
    }

    #[test]
    fn should_resize_detects_significant_change() {
        assert!(should_resize(500.0, 96.0, 1.0, 1.0));
        assert!(should_resize(500.0, 96.0, 499.0, 96.0));
    }

    #[test]
    fn should_resize_ignores_subpixel_jitter() {
        assert!(!should_resize(500.0, 96.0, 500.3, 96.2));
        assert!(!should_resize(500.0, 96.0, 500.0, 96.0));
    }

    #[test]
    fn should_resize_converges_after_request() {
        // Simulates: first call detects change, second call sees matching size
        let (target_w, target_h) = auto_resize_target(500.0, 96.0, 1.0, 1.0);
        let initial_current = (1.0, 1.0);

        // First: should resize
        assert!(should_resize(target_w, target_h, initial_current.0, initial_current.1));

        // After request_surface_size, the window reports the new size
        let updated_current = (target_w, target_h);

        // Second: should NOT resize (converged)
        assert!(!should_resize(target_w, target_h, updated_current.0, updated_current.1));
    }

    // --- Popup height target (mirrors app.rs render_popup_bubble) ---
    //
    // Tests that the max_height calculation used for clip-based animation
    // produces correct values for the auto-resize pipeline.

    fn popup_max_height(entry_count: usize, bubble_progress: f32) -> f32 {
        let target = (entry_count as f32) * 80.0 + 16.0;
        target * bubble_progress
    }

    #[test]
    fn popup_height_is_zero_when_progress_is_zero() {
        assert_eq!(popup_max_height(1, 0.0), 0.0);
        assert_eq!(popup_max_height(3, 0.0), 0.0);
    }

    #[test]
    fn popup_height_is_full_when_progress_is_one() {
        assert_eq!(popup_max_height(1, 1.0), 96.0);  // 1*80 + 16
        assert_eq!(popup_max_height(2, 1.0), 176.0); // 2*80 + 16
        assert_eq!(popup_max_height(3, 1.0), 256.0); // 3*80 + 16
    }

    #[test]
    fn popup_height_scales_with_progress() {
        let full = popup_max_height(1, 1.0);
        let half = popup_max_height(1, 0.5);
        assert!((half - full * 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn auto_resize_with_animated_popup_height() {
        // During animation: content height follows bubble_progress
        // Auto-resize should clamp small heights to min but pass through larger ones
        let min_h = 1.0;

        // Start of animation: progress ~0, height ~0 → clamped to 1.0
        let h = popup_max_height(1, 0.0);
        let (_, clamped_h) = auto_resize_target(500.0, h, 1.0, min_h);
        assert_eq!(clamped_h, 1.0);

        // Mid animation: progress 0.5, height 48 → passes through
        let h = popup_max_height(1, 0.5);
        let (_, clamped_h) = auto_resize_target(500.0, h, 1.0, min_h);
        assert_eq!(clamped_h, 48.0);

        // Full: progress 1.0, height 96 → passes through
        let h = popup_max_height(1, 1.0);
        let (_, clamped_h) = auto_resize_target(500.0, h, 1.0, min_h);
        assert_eq!(clamped_h, 96.0);
    }

    // --- target_surface_height_at ---

    #[test]
    fn target_surface_height_snaps_for_active_entries() {
        let config = test_config();
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));
        state.enqueue(make_notification(2), Duration::from_secs(5));

        // Even at t≈0 (just enqueued), surface height should be full target
        let now = Instant::now();
        let height = state.target_surface_height_at(now);
        let expected = 2.0 * 80.0 + 16.0; // 176.0
        assert!(
            (height - expected).abs() < f32::EPSILON,
            "expected {expected}, got {height}"
        );
    }

    #[test]
    fn target_surface_height_shrinks_during_all_slideout() {
        let config = test_config(); // 100ms animation
        let mut state = PopupState::new(&config);

        state.enqueue(make_notification(1), Duration::from_secs(5));

        // Wait for SlideIn → Display
        thread::sleep(Duration::from_millis(150));
        state.tick();

        // Dismiss to trigger SlideOut
        state.dismiss(1);

        // Height should decrease monotonically during SlideOut
        let mut prev_height = f32::MAX;
        for _ in 0..5 {
            thread::sleep(Duration::from_millis(15));
            let now = Instant::now();
            let height = state.target_surface_height_at(now);
            assert!(
                height <= prev_height + f32::EPSILON,
                "height increased: {prev_height} -> {height}"
            );
            prev_height = height;
        }
    }

    #[test]
    fn target_surface_height_is_zero_when_empty() {
        let config = test_config();
        let state = PopupState::new(&config);
        assert_eq!(state.target_surface_height_at(Instant::now()), 0.0);
    }

    // --- Full lifecycle integration test ---

    #[test]
    fn full_notification_lifecycle() {
        let config = test_config(); // 100ms animation
        let mut state = PopupState::new(&config);

        // 1. Empty state
        assert!(!state.is_active());
        assert_eq!(state.bubble_progress(), 0.0);

        // 2. Enqueue notification
        state.enqueue(make_notification(1), Duration::from_millis(50));
        assert!(state.is_active());
        assert_eq!(state.entries[0].phase, PopupPhase::SlideIn);

        // 3. During SlideIn: height should be small
        let progress = state.bubble_progress();
        let height = popup_max_height(1, progress);
        let (_, target_h) = auto_resize_target(500.0, height, 1.0, 1.0);
        assert!(target_h >= 1.0, "surface should be at least 1px high");
        assert!(should_resize(500.0, target_h, 1.0, 1.0), "should resize from initial 1x1");

        // 4. Wait for SlideIn to complete → Display
        thread::sleep(Duration::from_millis(150));
        state.tick();
        assert_eq!(state.entries[0].phase, PopupPhase::Display);
        assert!((state.bubble_progress() - 1.0).abs() < f32::EPSILON);

        // At Display: full height
        let height = popup_max_height(1, 1.0);
        let (w, h) = auto_resize_target(500.0, height, 1.0, 1.0);
        assert_eq!((w, h), (500.0, 96.0));

        // 5. Wait for Display to expire → SlideOut
        thread::sleep(Duration::from_millis(100));
        state.tick();
        assert_eq!(state.entries[0].phase, PopupPhase::SlideOut);

        // 6. During SlideOut: progress decreasing
        // (check immediately, before the animation completes)
        let progress = state.bubble_progress();
        assert!(progress <= 1.0, "progress should be <= 1.0, got {progress}");

        // 7. Wait for SlideOut to complete → removed
        thread::sleep(Duration::from_millis(150));
        state.tick();
        assert!(state.entries.is_empty());
        assert!(!state.is_active());
    }
}
