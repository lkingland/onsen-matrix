//! Whether the panel is there: in for a while after something happens,
//! then out, with a fade either way. Plain state with a clock handed in,
//! so it can be reasoned about, and tested, without a frame in sight.

/// The panel's presence. `alpha` is what is drawn; `visible` is where it
/// is heading, which is what "the panel is in" means to the keys.
#[derive(Default, Debug, PartialEq)]
pub struct Presence {
    wanted: bool,
    alpha: f32,
    /// When to start fading out, on the clock `tick` is given.
    hide_at: Option<f32>,
}

impl Presence {
    /// Brings the panel in, or keeps it, until `hold` seconds from `now`.
    pub fn show(&mut self, now: f32, hold: f32) {
        self.wanted = true;
        self.hide_at = Some(now + hold);
    }

    /// Sends the panel out now.
    pub fn hide(&mut self) {
        self.wanted = false;
        self.hide_at = None;
    }

    /// Whether the panel is in, or on its way in.
    pub fn visible(&self) -> bool {
        self.wanted
    }

    /// How much of the panel to draw, 0 to 1.
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// A frame: the hold expiring, and the alpha moving toward what is
    /// wanted over `fade` seconds.
    pub fn tick(&mut self, now: f32, dt: f32, fade: f32) {
        if self.hide_at.is_some_and(|at| now >= at) {
            self.hide();
        }
        let target = if self.wanted { 1.0 } else { 0.0 };
        let step = if fade > 0.0 { dt / fade } else { 1.0 };
        self.alpha = if target > self.alpha { (self.alpha + step).min(target) } else { (self.alpha - step).max(target) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comes_in_holds_and_goes() {
        let mut p = Presence::default();
        assert!(!p.visible());
        p.show(0.0, 10.0);
        assert!(p.visible());
        // In over 0.5 s, by frames of a tenth.
        p.tick(0.1, 0.1, 0.5);
        assert!((p.alpha() - 0.2).abs() < 1e-6);
        for i in 2..=5 {
            p.tick(0.1 * i as f32, 0.1, 0.5);
        }
        assert_eq!(p.alpha(), 1.0);
        p.tick(1.0, 0.1, 0.5);
        assert_eq!(p.alpha(), 1.0, "steady while held");
        // The hold expires: out again, to nothing.
        p.tick(10.0, 0.1, 0.5);
        assert!(!p.visible());
        assert!(p.alpha() < 1.0);
        for i in 1..=10 {
            p.tick(10.0 + 0.1 * i as f32, 0.1, 0.5);
        }
        assert_eq!(p.alpha(), 0.0);
    }

    #[test]
    fn a_change_mid_fade_turns_it_round_and_hide_is_immediate() {
        let mut p = Presence::default();
        p.show(0.0, 1.0);
        p.tick(0.1, 0.1, 0.5);
        p.tick(1.0, 0.1, 0.5); // the hold is up: heading out
        assert!(!p.visible());
        p.show(1.1, 1.0);
        assert!(p.visible());
        p.tick(1.2, 0.1, 0.5);
        assert!(p.alpha() > 0.0);
        p.hide();
        assert!(!p.visible());
        p.tick(1.3, 0.1, 0.0); // no fade: straight to nothing
        assert_eq!(p.alpha(), 0.0);
    }
}
