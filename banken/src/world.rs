use std::path::{Path, PathBuf};

use blue_lang_host::Capability;
use blue_lang_host::world::{self, Load, Spec};
use serde_json::Value;

use crate::estate::RefusedRows;

const SPEC: Spec<'static> = Spec {
    call: "banken",
    kinds: "{**host_kinds}",
    allow: &[Capability::Environment, Capability::Clock],
    fuel: world::DEFAULT_FUEL,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub binds: Vec<(String, String)>,
    pub offs: Vec<String>,
    pub refused_rows: Option<RefusedRows>,
    pub editor: Option<crate::edit::EditorChoice>,
    pub targets_dir: Option<PathBuf>,
    pub refusals: Vec<String>,
}

#[must_use]
pub fn world_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BANKEN_WORLD").map(PathBuf::from) {
        return Some(path);
    }
    let absolute = |var: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    absolute("XDG_CONFIG_HOME")
        .or_else(|| absolute("HOME").map(|h| h.join(".config")))
        .map(|dir| dir.join("banken").join("banken.b"))
}

#[must_use]
pub fn read(file: &Path) -> Load {
    if !file.exists() {
        return Load::Absent;
    }
    match world::judged::<()>(file, &SPEC) {
        Ok(judged) => Load::Judged(judged),
        Err(refusal) => Load::Refused(refusal),
    }
}

fn word(v: &Value) -> Option<String> {
    world::word(v).map(str::to_owned)
}

fn read_config(name: &str, value: &Value, overlay: &mut Overlay) {
    let Some(fields) = value.as_object() else {
        overlay
            .refusals
            .push(format!("config {name}: the value is not a record"));
        return;
    };
    for (key, field) in fields {
        match (name, key.as_str()) {
            ("picker", "refused") => match word(field).as_deref() {
                Some("hide") => overlay.refused_rows = Some(RefusedRows::Hide),
                Some("dim") => overlay.refused_rows = Some(RefusedRows::Dim),
                _ => overlay
                    .refusals
                    .push(format!("config picker.refused: {field} is not hide or dim")),
            },
            ("picker", "targets") => match field.as_str() {
                Some(dir) if Path::new(dir).is_absolute() => {
                    overlay.targets_dir = Some(PathBuf::from(dir));
                }
                _ => overlay.refusals.push(format!(
                    "config picker.targets: {field} is not an absolute path"
                )),
            },
            ("edit", "editor") => match word(field).as_deref() {
                Some("embedded") => overlay.editor = Some(crate::edit::EditorChoice::Embedded),
                Some("external") => overlay.editor = Some(crate::edit::EditorChoice::External),
                _ => overlay.refusals.push(format!(
                    "config edit.editor: {field} is not embedded or external"
                )),
            },
            _ => overlay
                .refusals
                .push(format!("config {name}.{key}: banken reads no such field")),
        }
    }
}

fn read_entry(entry: &Value, overlay: &mut Overlay) {
    let kind = entry.get("entry").and_then(word).unwrap_or_default();
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let value = entry.get("value").cloned().unwrap_or(Value::Null);
    match kind.as_str() {
        "bind" => match value.get("action").and_then(word) {
            Some(action) if action == "off" => overlay.offs.push(name),
            Some(action) => overlay.binds.push((name, action)),
            None => overlay
                .refusals
                .push(format!("bind {name}: names no action")),
        },
        "config" => read_config(&name, &value, overlay),
        other => overlay
            .refusals
            .push(format!("{other} {name}: banken takes no such entry")),
    }
}

#[must_use]
pub fn overlay_of(load: &Load) -> Overlay {
    let mut overlay = Overlay::default();
    match load {
        Load::Absent => {}
        Load::Refused(refusal) => overlay.refusals.push(refusal.to_string()),
        Load::Judged(judged) => {
            overlay.refusals = world::refusals_of(judged)
                .into_iter()
                .map(|r| r.to_string())
                .collect();
            for entry in world::listed(judged, "off") {
                if entry.get("entry").and_then(word).as_deref() == Some("bind")
                    && let Some(name) = entry.get("name").and_then(Value::as_str)
                {
                    overlay.offs.push(name.to_owned());
                }
            }
            for entry in world::listed(judged, "accepted") {
                read_entry(&entry, &mut overlay);
            }
        }
    }
    overlay
}

#[must_use]
pub fn current() -> Overlay {
    world_file().map_or_else(Overlay::default, |f| overlay_of(&read(&f)))
}

impl Overlay {
    #[must_use]
    pub fn catalog(&self, base: &banken_spec::Catalog) -> (banken_spec::Catalog, Vec<String>) {
        let (catalog, mut refused) = base.with_nav_overlay(&self.binds, &self.offs);
        let mut all = self.refusals.clone();
        all.append(&mut refused);
        let marked = all
            .into_iter()
            .map(|r| {
                let mut s = String::from("banken.b: ");
                s.push_str(&r);
                s
            })
            .collect();
        (catalog, marked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn judged(accepted: &Value, off: &Value) -> Load {
        Load::Judged(json!({ "accepted": accepted, "off": off, "refused": [] }))
    }

    #[test]
    fn binds_offs_and_config_reach_the_overlay() {
        let o = overlay_of(&judged(
            &json!([
                {"entry": "bind", "name": "ctrl+n", "value": {"action": "select_next"}},
                {"entry": "config", "name": "picker", "value": {"refused": "dim"}}
            ]),
            &json!([{"entry": "bind", "name": "k"}]),
        ));
        assert_eq!(
            o.binds,
            vec![("ctrl+n".to_owned(), "select_next".to_owned())]
        );
        assert_eq!(o.offs, vec!["k".to_owned()]);
        assert_eq!(o.refused_rows, Some(RefusedRows::Dim));
        assert!(o.refusals.is_empty(), "{:?}", o.refusals);
    }

    #[test]
    fn a_bad_entry_is_refused_alone_and_its_siblings_stand() {
        let o = overlay_of(&judged(
            &json!([
                {"entry": "config", "name": "picker", "value": {"refused": "loud", "targets": "/k/targets"}},
                {"entry": "look", "name": "appearance", "value": {}},
                {"entry": "bind", "name": "ctrl+n", "value": {"action": "select_next"}}
            ]),
            &json!([]),
        ));
        assert_eq!(o.binds.len(), 1);
        assert_eq!(o.targets_dir, Some(PathBuf::from("/k/targets")));
        assert_eq!(o.refused_rows, None);
        assert_eq!(o.refusals.len(), 2, "{:?}", o.refusals);
    }

    #[test]
    fn the_catalog_carries_world_bindings_and_names_each_refusal() {
        let base = banken_spec::load_catalog().expect("shipped");
        let o = Overlay {
            binds: vec![
                ("ctrl+n".into(), "select_next".into()),
                ("ctrl+y".into(), "fly".into()),
            ],
            ..Overlay::default()
        };
        let (c, refusals) = o.catalog(&base);
        assert!(c.nav_keys().iter().any(|n| n.keys.canonical() == "ctrl+n"));
        assert_eq!(refusals.len(), 1);
        assert!(refusals[0].starts_with("banken.b: bind ctrl+y"));
    }

    #[test]
    fn a_real_blue_world_is_judged_entry_by_entry() {
        let dir = std::env::temp_dir().join(format!("banken-world-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let file = dir.join("banken.b");
        std::fs::write(
            &file,
            "use(\"motenashi\", %i[bind config])\n\ndef banken()\n  [\n    bind(\"ctrl+n\", :select_next),\n    bind(\"k\", :off),\n    config(:picker, {refused: :dim})\n  ]\nend\n",
        )
        .expect("write world");
        let judged = world::judged::<()>(&file, &SPEC).expect("the world loads");
        let o = overlay_of(&Load::Judged(judged));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            o.binds,
            vec![("ctrl+n".to_owned(), "select_next".to_owned())]
        );
        assert_eq!(o.offs, vec!["k".to_owned()]);
        assert_eq!(o.refused_rows, Some(RefusedRows::Dim));
        assert!(o.refusals.is_empty(), "{:?}", o.refusals);
    }

    #[test]
    fn an_absent_world_changes_nothing() {
        assert_eq!(overlay_of(&Load::Absent), Overlay::default());
    }
}
