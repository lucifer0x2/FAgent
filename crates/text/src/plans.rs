//! Reusable HarfRust shape plans, one cache per font shared by its clones.
//!
//! `harfrust::shape` without a plan compiles a new one on every call (feature map and
//! lookup lists: script/language system lookup, feature sort, lookup collection). For the
//! short runs a design measures, compiling the plan cost more than shaping: ~23% of the
//! viewer engine worker's time while loading a 229-frame board. A plan is reusable for the
//! same face with the same direction, script, language, user features, feature variations
//! and (when lookups vary) normalized coordinates; `ShapePlanKey::matches` checks exactly
//! those, so a cached plan shapes byte-for-byte like a freshly built one.
use harfrust::{Direction, Feature, Language, Script, ShapePlan, ShapePlanKey};
use std::sync::{Arc, Mutex, PoisonError};

/// Plans kept per font, oldest evicted first. A face typically needs one plan per script and
/// feature set (tracking off/on); variable fonts whose lookups vary need one per instance.
const MAX_PLANS: usize = 32;

#[derive(Clone, Default)]
pub(crate) struct PlanCache(Arc<Mutex<Vec<Arc<ShapePlan>>>>);

impl PlanCache {
    /// The plan for shaping with `font` (the exact instance passed to `harfrust::shape`) and
    /// these segment properties, compiled on first use.
    pub(crate) fn get(
        &self,
        font: &harfrust::Font,
        direction: Direction,
        script: Option<Script>,
        language: Option<&Language>,
        features: &[Feature],
    ) -> Arc<ShapePlan> {
        let key = ShapePlanKey::new(font, script, direction)
            .language(language)
            .features(features);
        if let Some(plan) = self.plans().iter().find(|plan| key.matches(plan)) {
            return plan.clone();
        }
        // Compiled outside the lock: server threads shaping other runs of this font wait only
        // for the lookup. Two threads may compile the same plan once; both are equivalent.
        let plan = Arc::new(ShapePlan::new(font, direction, script, language, features));
        let mut plans = self.plans();
        if !plans.iter().any(|cached| key.matches(cached)) {
            if plans.len() >= MAX_PLANS {
                plans.remove(0);
            }
            plans.push(plan.clone());
        }
        plan
    }

    fn plans(&self) -> std::sync::MutexGuard<'_, Vec<Arc<ShapePlan>>> {
        // A panic while holding the lock leaves a valid list (pushes and removes are whole).
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.plans().len()
    }
}

#[cfg(test)]
mod tests {
    use crate::{Font, PositionedRun};

    const NOTO: &[u8] = include_bytes!("../../../renderer/fonts/NotoSans.ttf");
    const LATO: &[u8] = include_bytes!("../../../fixtures/fonts/Lato-Bold.ttf");

    fn font(bytes: &[u8]) -> Font {
        let hash = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes));
        Font::from_bytes(bytes, &hash).unwrap()
    }

    fn same(a: &PositionedRun, b: &PositionedRun) {
        assert_eq!(a.advance.to_bits(), b.advance.to_bits());
        assert_eq!(a.glyphs.len(), b.glyphs.len());
        for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.cluster, b.cluster);
            assert_eq!(a.x.to_bits(), b.x.to_bits());
            assert_eq!(a.y.to_bits(), b.y.to_bits());
            assert_eq!(a.x_advance.to_bits(), b.x_advance.to_bits());
            assert_eq!(a.y_advance.to_bits(), b.y_advance.to_bits());
        }
    }

    #[test]
    fn a_plan_is_reused_only_for_the_same_script_and_features() {
        let noto = font(NOTO);
        assert_eq!(noto.plans.len(), 0);
        noto.positioned_run("Hello", 16.0, 0.0).unwrap();
        noto.positioned_run("World, again", 31.0, 0.0).unwrap();
        assert_eq!(
            noto.plans.len(),
            1,
            "same script and features at another size"
        );
        noto.positioned_run("Привет", 16.0, 0.0).unwrap();
        assert_eq!(noto.plans.len(), 2, "another script");
        noto.positioned_run("Hello", 16.0, 1.5).unwrap();
        assert_eq!(
            noto.plans.len(),
            3,
            "tracking disables liga/clig: other features"
        );
        noto.positioned_run("Hello", 16.0, -2.0).unwrap();
        assert_eq!(
            noto.plans.len(),
            3,
            "any nonzero tracking has the same features"
        );
        let clone = noto.clone();
        clone.positioned_run("Αλφα Hello", 12.0, 0.0).unwrap();
        assert_eq!(noto.plans.len(), 4, "clones share the cache");
        let face = noto.with_face("Noto", 700, "normal").unwrap();
        assert_eq!(face.plans.len(), 0, "another instance starts empty");
        assert_eq!(clone.plans.len(), 4);
        let lato = font(LATO);
        lato.positioned_run("Hello", 16.0, 0.0).unwrap();
        assert_eq!(
            (lato.plans.len(), clone.plans.len()),
            (1, 4),
            "fonts never share plans"
        );
    }

    #[test]
    fn cached_plans_shape_like_fresh_ones() {
        let texts = [
            "Plan, review and ship design changes.",
            "ffi fl office — “quotes” 1/2 ½",
            "Ж\u{301} α\u{301} i\u{307}j\u{307} u\u{308}\u{301}",
            "Mixed: Hello Привет Γειά σου Hello",
            "",
            " \t\n tabs and breaks ",
        ];
        // Warm caches serve every run below after the first of each kind.
        let warm = [
            font(NOTO),
            font(LATO).with_face("Lato", 700, "normal").unwrap(),
        ];
        for round in 0..2 {
            for (i, bytes) in [NOTO, LATO].into_iter().enumerate() {
                for text in texts {
                    for (size, spacing) in [(16.0, 0.0), (9.5, 0.0), (48.0, 1.25), (16.0, -0.5)] {
                        let fresh = if i == 0 {
                            font(bytes)
                        } else {
                            font(bytes).with_face("Lato", 700, "normal").unwrap()
                        };
                        let expected = fresh.positioned_run(text, size, spacing).unwrap();
                        let actual = warm[i].positioned_run(text, size, spacing).unwrap();
                        same(&actual, &expected);
                        if round == 1 {
                            assert!(warm[i].plans.len() <= super::MAX_PLANS);
                        }
                    }
                }
            }
        }
    }
}
