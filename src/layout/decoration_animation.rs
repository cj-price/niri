use crate::animation::{Animation, Clock};

/// An interruptible decoration transition. The first value is installed without a fade.
#[derive(Debug, Default)]
pub(super) struct DecorationAnimation {
    target: Option<f64>,
    animation: Option<Animation>,
}

impl DecorationAnimation {
    pub fn update(&mut self, target: f64, clock: &Clock, config: niri_config::Animation) -> f64 {
        if config.off || clock.should_complete_instantly() {
            self.animation = None;
        } else if self.target.is_some_and(|previous| previous != target) {
            self.animation = Some(Animation::new(
                clock.clone(),
                self.value(),
                target,
                0.,
                config,
            ));
        }
        self.target = Some(target);
        self.advance();
        self.value()
    }

    pub fn advance(&mut self) {
        if self.animation.as_ref().is_some_and(Animation::is_done) {
            self.animation = None;
        }
    }

    pub fn value(&self) -> f64 {
        self.animation
            .as_ref()
            .map_or(self.target.unwrap_or(0.), Animation::clamped_value)
    }

    pub fn ongoing(&self) -> bool {
        self.animation.as_ref().is_some_and(|anim| !anim.is_done())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn finishes_and_respects_global_off() {
        let mut clock = Clock::with_time(Duration::ZERO);
        let config = niri_config::animations::DecorationChangeAnim::default().0;
        let mut fade = DecorationAnimation::default();
        assert_eq!(fade.update(1., &clock, config), 1.);
        assert!(!fade.ongoing());
        assert_eq!(fade.update(0., &clock, config), 1.);
        clock.set_unadjusted(Duration::from_millis(200));
        assert_eq!(fade.update(0., &clock, config), 0.);
        assert!(!fade.ongoing());
        fade.update(1., &clock, config);
        clock.set_complete_instantly(true);
        assert_eq!(fade.update(1., &clock, config), 1.);
        assert!(!fade.ongoing());
    }

    #[test]
    fn interruption_and_disable() {
        let mut clock = Clock::with_time(Duration::ZERO);
        let mut fade = DecorationAnimation::default();
        let mut config = niri_config::Animation {
            off: false,
            kind: niri_config::animations::Kind::Easing(niri_config::animations::EasingParams {
                duration_ms: 100,
                curve: niri_config::animations::Curve::Linear,
            }),
        };
        assert_eq!(fade.update(0., &clock, config), 0.);
        assert_eq!(fade.update(1., &clock, config), 0.);
        clock.set_unadjusted(Duration::from_millis(50));
        assert!((fade.update(0., &clock, config) - 0.5).abs() < 0.001);
        assert!(fade.ongoing());
        config.off = true;
        assert_eq!(fade.update(1., &clock, config), 1.);
        assert!(!fade.ongoing());
    }
}
