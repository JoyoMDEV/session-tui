//! The start preset: what `sessions start` and the browser hand to the hook of the session they
//! start, in `$SESSIONS_PRESET`. A new session has no ID yet, so its tickets and title cannot be
//! written to `sessions.json` before it exists. It knows nothing about the hook and does not read
//! or write the sessions file.

use serde::{Deserialize, Serialize};

/// The environment variable that carries the preset to the hook.
pub const ENV: &str = "SESSIONS_PRESET";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
pub struct Preset {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tickets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Preset {
    /// A preset from what a person typed: blanks are dropped, so an empty title is no title.
    pub fn new(tickets: Vec<String>, title: Option<String>) -> Preset {
        Preset {
            tickets: tickets
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            title: title
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty() && self.title.is_none()
    }

    /// The value for `$SESSIONS_PRESET`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a preset is always valid JSON")
    }

    /// The preset in `text`, or `None` if it is empty or not a preset. A broken value must never
    /// stop a session from starting, so it means no preset.
    pub fn parse(text: &str) -> Option<Preset> {
        let preset: Preset = serde_json::from_str(text.trim()).ok()?;
        let preset = Preset::new(preset.tickets, preset.title);
        (!preset.is_empty()).then_some(preset)
    }

    /// The preset in `$SESSIONS_PRESET`, if there is a usable one.
    pub fn from_env() -> Option<Preset> {
        let text = std::env::var(ENV).ok()?;
        if text.trim().is_empty() {
            return None;
        }
        let preset = Preset::parse(&text);
        if preset.is_none() {
            eprintln!("sessions: ignoring {ENV}, which is not a valid preset");
        }
        preset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_survives_the_round_trip_through_json() {
        let preset = Preset::new(
            vec!["ABC-1".into(), "ABC-2".into()],
            Some("Fix login".into()),
        );
        assert_eq!(Preset::parse(&preset.to_json()), Some(preset));
    }

    #[test]
    fn empty_parts_are_not_written() {
        assert_eq!(Preset::new(vec![], None).to_json(), "{}");
        let only_title = Preset::new(vec![], Some("T".into()));
        assert_eq!(only_title.to_json(), r#"{"title":"T"}"#);
        let only_tickets = Preset::new(vec!["ABC-1".into()], None);
        assert_eq!(only_tickets.to_json(), r#"{"tickets":["ABC-1"]}"#);
    }

    #[test]
    fn blanks_are_dropped() {
        let preset = Preset::new(vec!["  ".into(), " ABC-1 ".into()], Some("   ".into()));
        assert_eq!(preset.tickets, ["ABC-1"]);
        assert_eq!(preset.title, None);
        assert!(Preset::new(vec![" ".into()], Some(String::new())).is_empty());
    }

    #[test]
    fn parse_is_tolerant_of_anything_that_is_not_a_usable_preset() {
        for bad in [
            "",
            "   ",
            "not json",
            "[]",
            "42",
            "null",
            "{}",
            r#"{"tickets":"ABC-1"}"#,
            r#"{"tickets":[1,2]}"#,
            r#"{"title":["x"]}"#,
            r#"{"tickets":[" "],"title":""}"#,
        ] {
            assert_eq!(Preset::parse(bad), None, "{bad:?}");
        }
        // Keys it does not know are ignored, so a newer browser can add one.
        let preset = Preset::parse(r#"{"title":"T","future":true}"#).unwrap();
        assert_eq!(preset.title.as_deref(), Some("T"));
    }
}
