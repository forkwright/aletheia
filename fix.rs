pub use crate::meta::RecordMeta;
pub use crate::meta::Provenance;

mod meta {
    use serde::{Deserialize, Serialize};
    use std::time::{SystemTime, Instant};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct RecordMeta {
        pub formula: Option<String>,
        pub event_time: Instant,
        pub version: u64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct Provenance {
        pub source: String,
        pub path: String,
        pub offset: u32,
    }

    impl RecordMeta {
        pub const VERSION_INIT: u64 = 1;
        pub const PATH_INIT: &str = "root";
    }

    impl RecordMeta {
        pub fn new(formula: impl Into<String>, time: Option<Instant>) -> Self {
            RecordMeta {
                formula: formula.into(),
                event_time: time.unwrap_or_else(Instant::now),
                version: RecordMeta::VERSION_INIT,
            }
        }

        pub fn preserved(&self) -> bool {
            self.formula.is_some()
        }

        pub fn timestamp(&self) -> Option<SystemTime> {
            self.event_time.map(|i| i.into())
        }
    }
}

mod reward {
    use super::meta::{RecordMeta, Provenance};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Reward {
        pub id: Uuid,
        pub meta: RecordMeta,
        pub value: i64,
    }

    impl RecordMeta for Reward {} // Placeholder for trait impls if generic is needed
}

mod dpo {
    use super::meta::{RecordMeta, Provenance};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Dpo {
        pub id: Uuid,
        pub meta: RecordMeta,
        pub score: f64,
        pub variant: String,
    }

    impl RecordMeta for Dpo {}
}

mod after_action {
    use super::meta::{RecordMeta, Provenance};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AfterAction {
        pub id: Uuid,
        pub meta: RecordMeta,
        pub summary: String,
        pub feedback: Option<i32>,
    }

    impl RecordMeta for AfterAction {}
}

mod lesson {
    use super::meta::{RecordMeta, Provenance};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Lesson {
        pub id: Uuid,
        pub meta: RecordMeta,
        pub duration: u64,
        pub context: String,
    }

    impl RecordMeta for Lesson {}
}

pub use meta::RecordMeta;
pub use reward::Reward;
pub use dpo::Dpo;
pub use after_action::AfterAction;
pub use lesson::Lesson;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_provenance_preserved() {
        let formula = "x * 2";
        let reward = Reward {
            id: Uuid::nil(),
            meta: RecordMeta {
                formula: Some(formula.to_string()),
                event_time: Instant::now(),
                version: 1,
            },
            value: 100,
        };
        assert!(reward.meta.formula.is_some());
        assert_eq!(reward.meta.formula, Some(formula.to_string()));
    }

    #[test]
    fn test_causality_ordering() {
        let reward1 = Reward::new(Uuid::new_v4(), 10, "base");
        let reward2 = Reward::new(Uuid::new_v4(), 20, "updated");
        assert!(reward1.meta.event_time < reward2.meta.event_time);
    }
}