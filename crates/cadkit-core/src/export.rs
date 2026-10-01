//! Yazıcılardan önce bellek içi modelin kaynak bütçesini denetler.

use crate::{Document, EntityKind, Error, Limits, Result, Value};

/// JSON girdisini boyut sınırından sonra ayrıştırır ve belge bütçelerini denetler.
pub fn document_from_json(text: &str, limits: &Limits) -> Result<Document> {
    if text.len() as u64 > limits.max_input_bytes {
        return Err(Error::LimitExceeded("document JSON input bytes".into()));
    }
    let doc = serde_json::from_str(text).map_err(|e| Error::invalid(0, e.to_string()))?;
    validate_limits(&doc, limits)?;
    Ok(doc)
}

/// Nötr belgeyi kopyalamadan derinlik, nesne, geometri ve JSON boyutunu sınırlar.
pub fn validate_limits(doc: &Document, limits: &Limits) -> Result<()> {
    let mut objects = doc.models.len().saturating_add(doc.blocks.len()) as u64;
    let mut vertices = 0u64;
    let mut todo = vec![];
    for entities in doc
        .models
        .iter()
        .map(|m| &m.entities)
        .chain(doc.blocks.iter().map(|b| &b.entities))
    {
        objects = objects.saturating_add(entities.len() as u64);
        if objects > limits.max_objects {
            return Err(Error::LimitExceeded("export objects".into()));
        }
        todo.extend(entities.iter().map(|e| (e, 0u32)));
    }
    while let Some((e, depth)) = todo.pop() {
        if depth >= limits.max_depth.min(256) {
            return Err(Error::LimitExceeded("export depth".into()));
        }
        let n = match &e.kind {
            EntityKind::Group { children, .. } => {
                objects = objects.saturating_add(children.len() as u64);
                if objects > limits.max_objects {
                    return Err(Error::LimitExceeded("export objects".into()));
                }
                todo.extend(children.iter().map(|e| (e, depth + 1)));
                0
            }
            EntityKind::Line { .. } => 2,
            EntityKind::Polyline { vertices, .. } => vertices.len() as u64,
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                if interiors.len() as u64 > limits.max_objects {
                    return Err(Error::LimitExceeded("export rings".into()));
                }
                interiors.iter().fold(exterior.len() as u64, |n, r| {
                    n.saturating_add(r.len() as u64)
                })
            }
            EntityKind::Mesh { vertices, faces } => {
                if faces.len() as u64 > limits.max_objects {
                    return Err(Error::LimitExceeded("export faces".into()));
                }
                faces.iter().fold(vertices.len() as u64, |n, f| {
                    n.saturating_add(f.len() as u64)
                })
            }
            EntityKind::Face { points, .. } => points.len() as u64,
            _ => 1,
        };
        if n > u64::from(limits.max_vertices) {
            return Err(Error::LimitExceeded("export entity vertices".into()));
        }
        vertices = vertices
            .saturating_add(n)
            .saturating_add(e.attributes.len() as u64);
        if vertices > limits.max_total_vertices {
            return Err(Error::LimitExceeded("export total vertices".into()));
        }
        for value in e
            .props
            .values()
            .chain(e.attributes.iter().map(|a| &a.value))
        {
            value_limit(value, limits)?;
        }
    }
    for props in std::iter::once(&doc.props)
        .chain(doc.models.iter().map(|v| &v.props))
        .chain(doc.blocks.iter().map(|v| &v.props))
        .chain(doc.layers.iter().map(|v| &v.props))
        .chain(doc.linetypes.iter().map(|v| &v.props))
        .chain(doc.text_styles.iter().map(|v| &v.props))
    {
        for value in props.values() {
            value_limit(value, limits)?;
        }
    }
    // Serileştirici akışa yazar; ikinci bir belge veya JSON ağacı ayırmaz.
    let mut sink = Counter {
        remaining: limits.max_decompressed_bytes,
    };
    serde_json::to_writer(&mut sink, doc).map_err(|e| Error::invalid(0, e.to_string()))?;
    Ok(())
}

fn value_limit(root: &Value, limits: &Limits) -> Result<()> {
    let mut todo = vec![(root, 0u32)];
    let mut nodes = 1u64;
    while let Some((v, d)) = todo.pop() {
        if d >= limits.max_depth.min(256) {
            return Err(Error::LimitExceeded("export property depth".into()));
        }
        match v {
            Value::Float(value) if !value.is_finite() => {
                return Err(Error::invalid(0, "non-finite metadata value"));
            }
            Value::List(values) => {
                nodes = nodes.saturating_add(values.len() as u64);
                if nodes > limits.max_objects {
                    return Err(Error::LimitExceeded("export property objects".into()));
                }
                todo.extend(values.iter().map(|v| (v, d + 1)));
            }
            Value::Text(s) if s.len() as u64 > u64::from(limits.max_string_bytes) => {
                return Err(Error::LimitExceeded("export string".into()));
            }
            _ => {}
        }
    }
    Ok(())
}

struct Counter {
    remaining: u64,
}
impl std::io::Write for Counter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(b.len() as u64)
            .ok_or_else(|| std::io::Error::other("export byte limit"))?;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
