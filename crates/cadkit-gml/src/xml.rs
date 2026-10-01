//! Dış kaynak çözümlemeyen ve ağaç boyutunu sınırlayan XML katmanı.

use crate::native::{CORE, CityGmlDocument, Element, Name, Node, XmlAttribute};
use cadkit_core::{Error, ReadOptions, Result};
use quick_xml::{
    NsReader, Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
    name::ResolveResult,
};

const XMLNS: &str = "http://www.w3.org/2000/xmlns/";

fn string(bytes: &[u8], offset: u64) -> Result<String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::invalid(offset, "XML must be UTF-8"))
}
fn namespace(result: ResolveResult<'_>, offset: u64) -> Result<String> {
    match result {
        ResolveResult::Bound(ns) => string(ns.as_ref(), offset),
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Unknown(_) => Err(Error::invalid(offset, "undeclared XML namespace prefix")),
    }
}

pub(crate) fn parse(bytes: &[u8], options: &ReadOptions) -> Result<CityGmlDocument> {
    let limits = &options.limits;
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(Error::LimitExceeded("XML input bytes".into()));
    }
    std::str::from_utf8(bytes).map_err(|_| Error::invalid(0, "XML must be UTF-8"))?;
    check_chars(
        std::str::from_utf8(bytes).map_err(|_| Error::invalid(0, "XML must be UTF-8"))?,
        0,
    )?;
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    let mut stack: Vec<Element> = vec![];
    let mut root = None;
    let mut nodes = 0u64;
    let mut text_bytes = 0u64;
    let mut declaration = false;
    loop {
        let offset = reader.buffer_position();
        let event = reader
            .read_event()
            .map_err(|e| Error::invalid(offset, e.to_string()))?;
        // Metin normalizasyonu kopya ayırmadan önce ham parça bütçeye sığmalıdır.
        let raw_text_len = match &event {
            Event::Text(e) | Event::Comment(e) => Some(e.len()),
            Event::CData(e) => Some(e.len()),
            Event::GeneralRef(e) => Some(e.len()),
            _ => None,
        };
        if raw_text_len.is_some_and(|n| {
            n as u64 > u64::from(limits.max_string_bytes)
                || text_bytes.saturating_add(n as u64) > limits.max_decompressed_bytes
        }) {
            return Err(Error::LimitExceeded("XML text bytes".into()));
        }
        if !matches!(event, Event::End(_) | Event::Eof) {
            nodes = nodes
                .checked_add(1)
                .ok_or_else(|| Error::LimitExceeded("XML nodes".into()))?;
            if nodes > limits.max_objects {
                return Err(Error::LimitExceeded("XML nodes".into()));
            }
        }
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if stack.len() as u64 >= u64::from(limits.max_depth.min(256)) {
                    return Err(Error::LimitExceeded("XML depth".into()));
                }
                if e.name().as_ref().len() as u64 > u64::from(limits.max_string_bytes) {
                    return Err(Error::LimitExceeded("XML name bytes".into()));
                }
                let ns = namespace(reader.resolver().resolve_element(e.name()).0, offset)?;
                text_bytes =
                    text_bytes.saturating_add(e.name().as_ref().len() as u64 + ns.len() as u64);
                let mut element = Element::new(&ns, &string(e.name().as_ref(), offset)?);
                element.offset = offset;
                for attr in e.attributes() {
                    if element.attributes.len() >= 256 {
                        return Err(Error::LimitExceeded("XML attributes per element".into()));
                    }
                    let attr = attr.map_err(|e| Error::invalid(offset, e.to_string()))?;
                    if attr.value.len() as u64 > u64::from(limits.max_string_bytes)
                        || attr.key.as_ref().len() as u64 > u64::from(limits.max_string_bytes)
                    {
                        return Err(Error::LimitExceeded("XML attribute bytes".into()));
                    }
                    let qualified = string(attr.key.as_ref(), offset)?;
                    let ns = if qualified == "xmlns" || qualified.starts_with("xmlns:") {
                        XMLNS.into()
                    } else {
                        namespace(reader.resolver().resolve_attribute(attr.key).0, offset)?
                    };
                    if element.attributes.iter().any(|a| {
                        a.name
                            .is(&ns, qualified.rsplit(':').next().unwrap_or(&qualified))
                    }) {
                        return Err(Error::invalid(offset, "duplicate expanded XML attribute"));
                    }
                    let value = attr
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|e| Error::invalid(offset, e.to_string()))?
                        .into_owned();
                    check_chars(&value, offset)?;
                    text_bytes = text_bytes.saturating_add(
                        value.len() as u64 + qualified.len() as u64 + ns.len() as u64,
                    );
                    element.attributes.push(XmlAttribute {
                        name: Name {
                            qualified,
                            namespace: ns,
                        },
                        value,
                    });
                }
                if empty {
                    attach(element, &mut stack, &mut root)?;
                } else {
                    stack.push(element);
                }
            }
            Event::End(_) => {
                let e = stack
                    .pop()
                    .ok_or_else(|| Error::invalid(offset, "unexpected XML end"))?;
                attach(e, &mut stack, &mut root)?;
            }
            Event::Text(e) => {
                let value = e
                    .xml10_content()
                    .map_err(|e| Error::invalid(offset, e.to_string()))?
                    .into_owned();
                push_text(
                    value,
                    &mut stack,
                    offset,
                    &mut text_bytes,
                    limits.max_string_bytes,
                )?;
            }
            Event::CData(e) => {
                let value = e
                    .xml10_content()
                    .map_err(|e| Error::invalid(offset, e.to_string()))?
                    .into_owned();
                push_text(
                    value,
                    &mut stack,
                    offset,
                    &mut text_bytes,
                    limits.max_string_bytes,
                )?;
            }
            Event::GeneralRef(e) => {
                let raw = e
                    .decode()
                    .map_err(|e| Error::invalid(offset, e.to_string()))?;
                let value = quick_xml::escape::unescape(&format!("&{raw};"))
                    .map_err(|e| Error::invalid(offset, e.to_string()))?
                    .into_owned();
                push_text(
                    value,
                    &mut stack,
                    offset,
                    &mut text_bytes,
                    limits.max_string_bytes,
                )?;
            }
            Event::Comment(e) => {
                let value = e
                    .decode()
                    .map_err(|e| Error::invalid(offset, e.to_string()))?
                    .into_owned();
                if value.len() as u64 > u64::from(limits.max_string_bytes) {
                    return Err(Error::LimitExceeded("XML comment bytes".into()));
                }
                text_bytes = text_bytes.saturating_add(value.len() as u64);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::Comment(value));
                }
            }
            Event::Decl(e) => {
                if declaration || root.is_some() || !stack.is_empty() || offset > 3 {
                    return Err(Error::invalid(offset, "misplaced XML declaration"));
                }
                declaration = true;
                if e.version()
                    .map_err(|err| Error::invalid(offset, err.to_string()))?
                    .as_ref()
                    != b"1.0"
                {
                    return Err(Error::Unsupported("XML version must be 1.0".into()));
                }
                if let Some(enc) = e.encoding() {
                    let enc = enc.map_err(|e| Error::invalid(offset, e.to_string()))?;
                    if !enc.eq_ignore_ascii_case(b"UTF-8") {
                        return Err(Error::Unsupported(
                            "CityGML XML encoding must be UTF-8".into(),
                        ));
                    }
                }
            }
            Event::DocType(_) => {
                return Err(Error::invalid(offset, "DTD declarations are forbidden"));
            }
            Event::PI(_) => return Err(Error::Unsupported("XML processing instructions".into())),
            Event::Eof => break,
        }
        if text_bytes > limits.max_decompressed_bytes {
            return Err(Error::LimitExceeded("XML decoded bytes".into()));
        }
    }
    if !stack.is_empty() {
        return Err(Error::invalid(
            reader.buffer_position(),
            "unclosed XML element",
        ));
    }
    let root = root.ok_or(Error::UnknownFormat)?;
    if !root.name.is(CORE, "CityModel") {
        return Err(Error::UnknownFormat);
    }
    Ok(CityGmlDocument { root })
}

fn attach(e: Element, stack: &mut [Element], root: &mut Option<Element>) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.push(e);
    } else if root.is_none() {
        *root = Some(e);
    } else {
        return Err(Error::invalid(e.offset, "multiple XML roots"));
    }
    Ok(())
}
fn push_text(
    value: String,
    stack: &mut [Element],
    offset: u64,
    total: &mut u64,
    max: u32,
) -> Result<()> {
    check_chars(&value, offset)?;
    if value.len() as u64 > u64::from(max) {
        return Err(Error::LimitExceeded("XML text bytes".into()));
    }
    *total = total.saturating_add(value.len() as u64);
    if let Some(parent) = stack.last_mut() {
        if let Some(Node::Text(previous)) = parent.children.last_mut() {
            if previous.len().saturating_add(value.len()) as u64 > u64::from(max) {
                return Err(Error::LimitExceeded("XML text bytes".into()));
            }
            previous.push_str(&value);
        } else {
            parent.children.push(Node::Text(value));
        }
    } else if !value.trim().is_empty() {
        return Err(Error::invalid(offset, "text outside XML root"));
    }
    Ok(())
}

pub(crate) fn check_chars(s: &str, offset: u64) -> Result<()> {
    if s.chars().any(|c| !matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')) {
        return Err(Error::invalid(offset, "forbidden XML 1.0 character"));
    }
    Ok(())
}

struct Output {
    bytes: Vec<u8>,
    limit: u64,
}
impl std::io::Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if (self.bytes.len() as u64).saturating_add(bytes.len() as u64) > self.limit {
            return Err(std::io::Error::other("XML output byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn serialize(doc: &CityGmlDocument, options: &ReadOptions) -> Result<Vec<u8>> {
    // Ağacı klonlamadan önce derinlik ve bütün metin bütçeleri denetlenir.
    crate::validate::elements(doc, &options.limits)?;
    let mut writer = Writer::new(Output {
        bytes: vec![],
        limit: options.limits.max_input_bytes,
    });
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    enum Action<'a> {
        Element(&'a Element),
        Node(&'a Node),
        End(&'a str),
    }
    let mut todo = vec![Action::Element(&doc.root)];
    while let Some(action) = todo.pop() {
        match action {
            Action::End(name) => writer.write_event(Event::End(BytesEnd::new(name)))?,
            Action::Element(e) | Action::Node(Node::Element(e)) => {
                let mut start = BytesStart::new(e.name.qualified.as_str());
                for a in &e.attributes {
                    // XML öznitelik normalizasyonunun sayısal referansları değiştirmesi önlenir.
                    let escaped = quick_xml::escape::escape(&a.value)
                        .replace('\t', "&#9;")
                        .replace('\n', "&#10;")
                        .replace('\r', "&#13;");
                    start.push_attribute((a.name.qualified.as_bytes(), escaped.as_bytes()));
                }
                writer.write_event(Event::Start(start))?;
                todo.push(Action::End(&e.name.qualified));
                for child in e.children.iter().rev() {
                    todo.push(Action::Node(child));
                }
            }
            Action::Node(Node::Text(t)) => {
                let escaped = quick_xml::escape::escape(t).replace('\r', "&#13;");
                writer.write_event(Event::Text(BytesText::from_escaped(&escaped)))?;
            }
            Action::Node(Node::Comment(t)) => {
                writer.write_event(Event::Comment(BytesText::from_escaped(t)))?
            }
        }
    }
    let bytes = writer.into_inner().bytes;
    let parsed = parse(&bytes, options)?;
    if !same_names(&doc.root, &parsed.root) {
        return Err(Error::invalid(
            0,
            "XML namespace declarations disagree with model names",
        ));
    }
    Ok(bytes)
}

fn same_names(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && a.attributes == b.attributes
        && a.text() == b.text()
        && a.elements().count() == b.elements().count()
        && a.elements()
            .zip(b.elements())
            .all(|(a, b)| same_names(a, b))
}
