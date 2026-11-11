//! Benchmark profiles and test depth.
//!
//! Profiles are a named set of benchmark values. The selected profile is stored in settings and
//! copied into each run so history and reports show the values that actually produced a result.

use serde::{Deserialize, Serialize};

/// A named benchmark preset. `Custom` keeps whatever values the user last edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    Quick,
    #[default]
    Balanced,
    Thorough,
    Custom,
}

/// How hard the full test works once a resolver has been selected.
///
/// `cached_rounds` replaces the old fixed inner round count and `random_lookups` replaces the
/// fixed random-name count. Both are persisted with the other settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestDepth {
    pub cached_rounds: usize,
    pub random_lookups: usize,
}

impl TestDepth {
    /// The default depth used by 0.7.0 and by the Balanced profile.
    pub const BALANCED: TestDepth = TestDepth {
        cached_rounds: 3,
        random_lookups: 5,
    };

    /// Keep user input inside a range that cannot hang or flood the benchmark.
    pub fn clamped(self) -> TestDepth {
        TestDepth {
            cached_rounds: self.cached_rounds.clamp(1, 10),
            random_lookups: self.random_lookups.clamp(0, 50),
        }
    }
}

impl Default for TestDepth {
    fn default() -> Self {
        Self::BALANCED
    }
}

/// All values a preset writes into settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileValues {
    pub timeout_ms: u64,
    pub jobs: usize,
    pub repeat: usize,
    pub cached_rounds: usize,
    pub random_lookups: usize,
}

impl Profile {
    pub const ALL: [Profile; 4] = [
        Profile::Quick,
        Profile::Balanced,
        Profile::Thorough,
        Profile::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Profile::Quick => "Quick",
            Profile::Balanced => "Balanced",
            Profile::Thorough => "Thorough",
            Profile::Custom => "Custom",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Profile::Quick => "A fast pass with fewer rounds. Good for a first look.",
            Profile::Balanced => "The default balance of speed and confidence.",
            Profile::Thorough => "More rounds and random names for a steadier ranking.",
            Profile::Custom => "Your own timeout, concurrency and depth values.",
        }
    }

    pub const fn values(self) -> Option<ProfileValues> {
        match self {
            Profile::Quick => Some(ProfileValues {
                timeout_ms: 700,
                jobs: 24,
                repeat: 1,
                cached_rounds: 2,
                random_lookups: 3,
            }),
            Profile::Balanced => Some(ProfileValues {
                timeout_ms: 1000,
                jobs: 16,
                repeat: 1,
                cached_rounds: 3,
                random_lookups: 5,
            }),
            Profile::Thorough => Some(ProfileValues {
                timeout_ms: 1500,
                jobs: 16,
                repeat: 2,
                cached_rounds: 4,
                random_lookups: 8,
            }),
            Profile::Custom => None,
        }
    }

    /// Name the preset that exactly matches these values, or `Custom`.
    pub fn infer(timeout_ms: u64, jobs: usize, repeat: usize, depth: TestDepth) -> Profile {
        Profile::ALL
            .into_iter()
            .filter(|profile| *profile != Profile::Custom)
            .find(|profile| {
                profile.values().is_some_and(|values| {
                    values.timeout_ms == timeout_ms
                        && values.jobs == jobs
                        && values.repeat == repeat
                        && values.cached_rounds == depth.cached_rounds
                        && values.random_lookups == depth.random_lookups
                })
            })
            .unwrap_or(Profile::Custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    #[test]
    fn quick_values_match_spec() {
        assert_eq!(
            Profile::Quick.values(),
            Some(ProfileValues {
                timeout_ms: 700,
                jobs: 24,
                repeat: 1,
                cached_rounds: 2,
                random_lookups: 3,
            })
        );
    }

    #[test]
    fn balanced_values_match_spec() {
        assert_eq!(
            Profile::Balanced.values(),
            Some(ProfileValues {
                timeout_ms: 1000,
                jobs: 16,
                repeat: 1,
                cached_rounds: 3,
                random_lookups: 5,
            })
        );
    }

    #[test]
    fn thorough_values_match_spec() {
        assert_eq!(
            Profile::Thorough.values(),
            Some(ProfileValues {
                timeout_ms: 1500,
                jobs: 16,
                repeat: 2,
                cached_rounds: 4,
                random_lookups: 8,
            })
        );
    }

    #[test]
    fn custom_has_no_preset() {
        assert_eq!(Profile::Custom.values(), None);
    }

    #[test]
    fn infer_recognizes_presets_and_custom() {
        for profile in [Profile::Quick, Profile::Balanced, Profile::Thorough] {
            let values = profile.values().unwrap();
            assert_eq!(
                Profile::infer(
                    values.timeout_ms,
                    values.jobs,
                    values.repeat,
                    TestDepth {
                        cached_rounds: values.cached_rounds,
                        random_lookups: values.random_lookups,
                    },
                ),
                profile
            );
        }
        assert_eq!(
            Profile::infer(1234, 16, 1, TestDepth::BALANCED),
            Profile::Custom
        );
    }

    #[test]
    fn old_settings_with_non_default_values_become_custom() {
        let settings = Settings {
            timeout_ms: 2500,
            jobs: 7,
            repeat: 3,
            ..Settings::default()
        };
        assert_eq!(
            Profile::infer(
                settings.timeout_ms,
                settings.jobs,
                settings.repeat,
                settings.depth()
            ),
            Profile::Custom
        );
    }
}
