//! Starter apps for a fresh install.
//!
//! Hand-written rather than generated: these are the first thing a new user sees,
//! and one that fails the gate on first run is unrecoverable from inside the app.
//! `npm run test:templates` compiles every one through the real loader and asserts
//! it declares a schema.
//!
//! Held as compiled-in constants rather than a `templates` table (which
//! `plans/02-milestones.md` originally sketched). A table would have to be
//! refreshed on every engine update and could go stale against the binary that
//! reads it; constants cannot. Revisit if templates ever become user-authored.

use serde::Serialize;

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub source: &'static str,
    /// Seeded ONLY into an empty store, so a fresh install looks alive without
    /// ever mixing sample rows into real work.
    pub sample: &'static str,
}

#[derive(Serialize)]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bytes: usize,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "todo",
        name: "Todo",
        description: "A checklist. The smallest thing worth evolving.",
        source: include_str!("../../src/canvas/seed-modules/todo.seed.tsx"),
        sample: r#"{"todos":[
            {"id":1,"title":"Ask the chat to add due dates","done":false},
            {"id":2,"title":"Switch back a version to see the history work","done":false},
            {"id":3,"title":"Read the module source in the drawer","done":true}
        ]}"#,
    },
    Template {
        id: "notes",
        name: "Notes",
        description: "A two-pane notebook. Good for trying layout changes.",
        source: include_str!("../../src/canvas/seed-modules/notes.seed.tsx"),
        sample: r#"{"notes":[
            {"id":1,"title":"How this works","body":"Describe a change in the drawer and the app rewrites itself. A version that will not compile is rolled back automatically.","updated":1789000000000},
            {"id":2,"title":"Scratch","body":"","updated":1789000000000}
        ]}"#,
    },
    Template {
        id: "tracker",
        name: "Tracker",
        description: "Habits across the last seven days. Dense grid to reshape.",
        source: include_str!("../../src/canvas/seed-modules/tracker.seed.tsx"),
        sample: r#"{"habits":[
            {"id":1,"name":"Read","days":[]},
            {"id":2,"name":"Walk","days":[]},
            {"id":3,"name":"Write","days":[]}
        ]}"#,
    },
    Template {
        id: "dashboard",
        name: "Board",
        description: "Counters, switches, goals and notes over one store.",
        source: include_str!("../../src/canvas/seed-modules/dashboard.seed.tsx"),
        sample: r#"{"tiles":[
            {"id":1,"kind":"counter","label":"Cups of tea","count":2,"target":5,"done":false,"text":""},
            {"id":2,"kind":"check","label":"Ship something","count":0,"target":1,"done":false,"text":""},
            {"id":3,"kind":"goal","label":"Pages read","count":3,"target":10,"done":false,"text":""},
            {"id":4,"kind":"note","label":"Today","count":0,"target":1,"done":false,"text":"Try asking for a new tile kind."}
        ]}"#,
    },
];

pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

pub fn list() -> Vec<TemplateInfo> {
    TEMPLATES
        .iter()
        .map(|t| TemplateInfo {
            id: t.id.to_string(),
            name: t.name.to_string(),
            description: t.description.to_string(),
            bytes: t.source.len(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_is_findable_and_non_empty() {
        assert!(!TEMPLATES.is_empty());
        for t in TEMPLATES {
            assert!(find(t.id).is_some(), "{} not findable", t.id);
            assert!(t.source.len() > 200, "{} source looks truncated", t.id);
            assert!(t.source.contains("export const schema"), "{} declares no schema", t.id);
            assert!(t.source.contains("export default"), "{} has no default export", t.id);
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = TEMPLATES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate template id");
    }

    /// Sample data has to parse, and every key it seeds must be one the module
    /// actually declares — otherwise the seed lands as unprotected data.
    #[test]
    fn samples_parse_and_match_declared_keys() {
        for t in TEMPLATES {
            let v: serde_json::Value =
                serde_json::from_str(t.sample).unwrap_or_else(|e| panic!("{}: {e}", t.id));
            let obj = v.as_object().unwrap_or_else(|| panic!("{}: not an object", t.id));
            assert!(!obj.is_empty(), "{}: empty sample", t.id);
            for key in obj.keys() {
                assert!(
                    t.source.contains(&format!("{key}:")),
                    "{}: sample key \"{key}\" is not in the module schema",
                    t.id
                );
            }
        }
    }
}
