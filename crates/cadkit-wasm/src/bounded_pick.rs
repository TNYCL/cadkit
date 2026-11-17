//! Small, owned inspection summaries. The selected entity stays borrowed until
//! its capped fields are copied; unrelated props never cross the WASM boundary.

use crate::access::{kind_tag, own_layer};
use crate::pick::{PickCandidate, block_of, hex_color};
use cadkit::{Document, Entity, EntityKind, Value};
use serde::Serialize;
use std::io::{self, Write};

#[derive(Serialize)]
struct Attribute {
    tag: String,
    value: String,
}

#[derive(Serialize)]
struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
}

#[derive(Serialize)]
struct Summary {
    index: usize,
    id: Option<u64>,
    layer: String,
    kind: &'static str,
    color: String,
    bbox: Bounds,
    distance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<String>,
    attributes: Vec<Attribute>,
    truncated: bool,
}

struct Preview {
    text: String,
    chars: usize,
    max_chars: usize,
    remaining: usize,
    nodes: usize,
    full: bool,
    truncated: bool,
}

impl Preview {
    fn append(&mut self, text: &str, quoted: bool) {
        for ch in text.chars() {
            let encoded = ch.to_string();
            let escaped;
            let part = if quoted {
                escaped = serde_json::to_string(&encoded).unwrap_or_default();
                escaped
                    .get(1..escaped.len().saturating_sub(1))
                    .unwrap_or("")
            } else {
                &encoded
            };
            for output in part.chars() {
                if self.chars >= self.max_chars || output.len_utf8() > self.remaining {
                    self.full = true;
                    self.truncated = true;
                    return;
                }
                self.text.push(output);
                self.chars += 1;
                self.remaining -= output.len_utf8();
            }
        }
    }

    fn value(&mut self, value: &Value, depth: usize, quoted: bool) {
        if self.full {
            return;
        }
        if self.nodes >= 64 {
            self.truncated = true;
            self.append("…", false);
            self.full = true;
            return;
        }
        self.nodes += 1;
        match value {
            Value::Null => self.append(if quoted { "null" } else { "" }, false),
            Value::Bool(value) => self.append(if *value { "true" } else { "false" }, false),
            Value::Int(value) => self.append(&value.to_string(), false),
            Value::Float(value) if value.is_finite() => self.append(&value.to_string(), false),
            Value::Text(text) => {
                if quoted {
                    self.append("\"", false);
                }
                if !self.full {
                    self.append(text, quoted);
                }
                if quoted && !self.full {
                    self.append("\"", false);
                }
            }
            Value::List(items) => {
                if depth >= 3 {
                    self.truncated = true;
                    self.append("[…]", false);
                    return;
                }
                self.append("[", false);
                for (index, item) in items.iter().take(16).enumerate() {
                    if self.full {
                        break;
                    }
                    if index > 0 {
                        self.append(", ", false);
                    }
                    self.value(item, depth + 1, true);
                }
                if items.len() > 16 {
                    self.truncated = true;
                    if !self.full {
                        self.append(", …", false);
                    }
                }
                if !self.full {
                    self.append("]", false);
                }
            }
            Value::Bytes(bytes) => {
                if depth >= 3 {
                    self.truncated = true;
                    self.append("[…]", false);
                    return;
                }
                self.append("[", false);
                for (index, byte) in bytes.iter().take(16).enumerate() {
                    if self.full {
                        break;
                    }
                    if index > 0 {
                        self.append(", ", false);
                    }
                    self.value(&Value::Int(i64::from(*byte)), depth + 1, true);
                }
                if bytes.len() > 16 {
                    self.truncated = true;
                    if !self.full {
                        self.append(", …", false);
                    }
                }
                if !self.full {
                    self.append("]", false);
                }
            }
            _ => {
                self.truncated = true;
                self.append("[unsupported value]", false);
            }
        }
    }
}

struct Budget {
    remaining: usize,
    truncated: bool,
}
impl Budget {
    fn preview(&mut self, max_chars: usize, run: impl FnOnce(&mut Preview)) -> String {
        let mut preview = Preview {
            text: String::new(),
            chars: 0,
            max_chars,
            remaining: self.remaining,
            nodes: 0,
            full: false,
            truncated: false,
        };
        run(&mut preview);
        self.remaining = preview.remaining;
        self.truncated |= preview.truncated;
        preview.text
    }
    fn text(&mut self, text: &str, max: usize) -> String {
        self.preview(max, |p| p.append(text, false))
    }
}

struct CappedJson {
    text: Vec<u8>,
    limit: usize,
}
impl Write for CappedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.text.len()) {
            return Err(io::Error::other("pick output budget exceeded"));
        }
        self.text.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn summary(
    doc: &Document,
    hit: &PickCandidate<'_>,
    max_output: usize,
) -> Result<String, ()> {
    let entity = hit.entity;
    let mut budget = Budget {
        remaining: 8192.min(max_output),
        truncated: false,
    };
    let layer = budget.text(hit.context.layer_name(entity), 200);
    let text = entity_text(entity).map(|text| budget.text(text, 500));
    let block = block_of(entity).map(|text| budget.text(text, 200));
    let attributes = capped_attributes(entity, &mut budget);
    let rgb = entity.resolved_color_in(doc, &hit.context);
    let summary = Summary {
        index: hit.index,
        id: entity.id,
        layer,
        kind: entity.kind.type_name(),
        color: hex_color(rgb),
        bbox: Bounds {
            min: [hit.bbox.min.x, hit.bbox.min.y, hit.bbox.min.z],
            max: [hit.bbox.max.x, hit.bbox.max.y, hit.bbox.max.z],
        },
        distance: hit.distance,
        text,
        block,
        attributes,
        truncated: budget.truncated,
    };
    let mut output = CappedJson {
        text: Vec::new(),
        limit: max_output,
    };
    serde_json::to_writer(&mut output, &summary).map_err(|_| ())?;
    String::from_utf8(output.text).map_err(|_| ())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HitSummary {
    path: Vec<usize>,
    model_path: Vec<usize>,
    kind: &'static str,
    layer: String,
    id: Option<u64>,
    model_kind: &'static str,
    model_layer: String,
    model_id: Option<u64>,
    block: Option<String>,
    attributes: Vec<Attribute>,
    color: String,
    bbox: Bounds,
    distance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    truncated: bool,
}

fn capped_attributes(entity: &Entity, budget: &mut Budget) -> Vec<Attribute> {
    let mut attributes = Vec::new();
    for attribute in entity.attributes.iter().take(20) {
        if budget.remaining == 0 {
            budget.truncated = true;
            break;
        }
        let tag = budget.text(&attribute.tag, 100);
        let value = budget.preview(200, |p| p.value(&attribute.value, 0, false));
        attributes.push(Attribute { tag, value });
    }
    budget.truncated |= entity.attributes.len() > 20;
    attributes
}

fn entity_text(entity: &Entity) -> Option<&str> {
    match &entity.kind {
        EntityKind::Text { value, .. } => Some(value.as_str()),
        EntityKind::MText { plain, value, .. } => {
            Some(if plain.is_empty() { value } else { plain }.as_str())
        }
        EntityKind::Dimension { text, .. } => text.as_deref(),
        _ => None,
    }
}

/// The `pick()` result: paths of the hit and of the model entity that owns it, both
/// entities' kind / layer / id, and the owner's block and attributes, with the same caps as
/// [`summary`] and the complete JSON within `max_output` bytes.
pub(crate) fn hit_summary(
    doc: &Document,
    model: usize,
    hit: &PickCandidate<'_>,
    max_output: usize,
) -> Result<String, ()> {
    let entity = hit.entity;
    let path = hit.context.path();
    let model_path = path
        .get(..hit.context.model_path_len().min(path.len()))
        .unwrap_or(&path)
        .to_vec();
    let owner = doc.entity_at(model, &model_path).unwrap_or(entity);
    let mut budget = Budget {
        remaining: 8192.min(max_output),
        truncated: false,
    };
    let layer = budget.text(hit.context.layer_name(entity), 200);
    let model_layer = budget.text(own_layer(owner), 200);
    let text = entity_text(entity).map(|t| budget.text(t, 500));
    let block = block_of(owner)
        .or_else(|| block_of(entity))
        .map(|b| budget.text(b, 200));
    let attributes = capped_attributes(owner, &mut budget);
    let summary = HitSummary {
        path,
        model_path,
        kind: kind_tag(&entity.kind),
        layer,
        id: entity.id,
        model_kind: kind_tag(&owner.kind),
        model_layer,
        model_id: owner.id,
        block,
        attributes,
        color: hex_color(entity.resolved_color_in(doc, &hit.context)),
        bbox: Bounds {
            min: [hit.bbox.min.x, hit.bbox.min.y, hit.bbox.min.z],
            max: [hit.bbox.max.x, hit.bbox.max.y, hit.bbox.max.z],
        },
        distance: hit.distance,
        text,
        truncated: budget.truncated,
    };
    let mut output = CappedJson {
        text: Vec::new(),
        limit: max_output,
    };
    serde_json::to_writer(&mut output, &summary).map_err(|_| ())?;
    String::from_utf8(output.text).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pick::{PickOptions, pick_candidate_in_doc};
    use cadkit::{Attribute as ModelAttribute, Entity, Point3};

    fn fixture() -> Document {
        let mut doc = Document::new();
        let mut entity = Entity::line(Point3::xy(0.0, 0.0), Point3::xy(10.0, 0.0));
        let mut attribute = ModelAttribute::default();
        attribute.tag = "tag".into();
        attribute.value = Value::Text("🙂".repeat(1_000_000));
        entity.attributes.push(attribute);
        entity
            .props
            .insert("test.large".into(), Value::Text("x".repeat(1_000_000)));
        doc.model_space_mut().entities.push(entity);
        doc
    }

    #[test]
    fn candidate_borrows_metadata_and_preview_stops_at_limits() {
        let doc = fixture();
        let (hit, exhausted) = pick_candidate_in_doc(
            &doc,
            0,
            (5.0, 0.0),
            1.0,
            false,
            &PickOptions::default(),
            100,
        );
        assert!(!exhausted);
        let hit = hit.unwrap();
        assert!(std::ptr::eq(hit.entity, &doc.models[0].entities[0]));
        let json = summary(&doc, &hit, 4096).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["attributes"][0]["value"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            200
        );
        assert_eq!(value["truncated"], true);
        assert!(value.get("props").is_none());
        assert!(json.len() < 4096);
        let exact = summary(&doc, &hit, json.len()).unwrap();
        assert_eq!(exact, json);
        assert!(summary(&doc, &hit, json.len() - 1).is_err());
    }

    #[test]
    fn work_exhaustion_is_reported_even_after_a_hit() {
        let mut doc = fixture();
        doc.model_space_mut()
            .entities
            .push(Entity::line(Point3::xy(0.0, 0.0), Point3::xy(10.0, 0.0)));
        let (hit, exhausted) =
            pick_candidate_in_doc(&doc, 0, (5.0, 0.0), 1.0, false, &PickOptions::default(), 1);
        assert!(hit.is_some());
        assert!(exhausted);
    }

    #[test]
    fn structured_values_stop_at_depth_count_and_node_budgets() {
        let mut budget = Budget {
            remaining: 8192,
            truncated: false,
        };
        let list = Value::List(vec![Value::Int(7); 100_000]);
        let text = budget.preview(200, |p| p.value(&list, 0, false));
        assert_eq!(text.matches('7').count(), 16);
        assert!(text.contains('…'));
        assert!(budget.truncated);
        let nested = Value::List(vec![Value::List(vec![Value::List(vec![Value::List(
            vec![Value::Int(1)],
        )])])]);
        assert!(
            budget
                .preview(200, |p| p.value(&nested, 0, false))
                .contains('…')
        );
        let mut preview = Preview {
            text: String::new(),
            chars: 0,
            max_chars: 4096,
            remaining: 8192,
            nodes: 0,
            full: false,
            truncated: false,
        };
        let wide = Value::List(vec![Value::List(vec![Value::Int(1); 16]); 16]);
        preview.value(&wide, 0, false);
        assert_eq!(preview.nodes, 64);
        assert!(preview.full && preview.truncated);
    }

    #[test]
    fn aggregate_unicode_and_escaping_stay_bounded() {
        let mut doc = fixture();
        let entity = &mut doc.model_space_mut().entities[0];
        entity.attributes[0].tag = "🙂".repeat(100);
        entity.attributes.resize(25, entity.attributes[0].clone());
        let (hit, _) = pick_candidate_in_doc(
            &doc,
            0,
            (5.0, 0.0),
            1.0,
            false,
            &PickOptions::default(),
            100,
        );
        let json = summary(&doc, &hit.unwrap(), 65_536).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["attributes"].as_array().unwrap().len() <= 20);
        assert_eq!(value["truncated"], true);
        let text_bytes: usize = value["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["tag"].as_str().unwrap().len() + a["value"].as_str().unwrap().len())
            .sum();
        assert!(text_bytes <= 8192);
        let mut budget = Budget {
            remaining: 8192,
            truncated: false,
        };
        let value = Value::List(vec![
            Value::Text("\"\n🙂".into()),
            Value::Bool(true),
            Value::Null,
        ]);
        assert_eq!(
            budget.preview(200, |p| p.value(&value, 0, false)),
            "[\"\\\"\\n🙂\", true, null]"
        );
        assert!(!budget.truncated);
    }
}
