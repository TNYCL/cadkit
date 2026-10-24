//! Bounded XML parser and serializer that forbids DTDs and external entity resolution.

use crate::native::{CORE, CityGmlDocument, Element, Name, Node, XmlAttribute};
use cadkit_core::LimitKind;
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
        return Err(Error::limit(
            LimitKind::InputBytes,
            "XML input bytes".into(),
        ));
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
        // Check the raw slice against the string and document budgets before unescaping allocates.
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
            return Err(Error::limit(
                LimitKind::DecompressedBytes,
                "XML text bytes".into(),
            ));
        }
        if !matches!(event, Event::End(_) | Event::Eof) {
            nodes = nodes
                .checked_add(1)
                .ok_or_else(|| Error::limit(LimitKind::Objects, "XML nodes".into()))?;
            if nodes > limits.max_objects {
                return Err(Error::limit(LimitKind::Objects, "XML nodes".into()));
            }
        }
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if stack.len() as u64 >= u64::from(limits.max_depth.min(256)) {
                    return Err(Error::limit(LimitKind::Depth, "XML depth".into()));
                }
                if e.name().as_ref().len() as u64 > u64::from(limits.max_string_bytes) {
                    return Err(Error::limit(
                        LimitKind::StringBytes,
                        "XML name bytes".into(),
                    ));
                }
                let ns = namespace(reader.resolver().resolve_element(e.name()).0, offset)?;
                text_bytes =
                    text_bytes.saturating_add(e.name().as_ref().len() as u64 + ns.len() as u64);
                let mut element = Element::new(&ns, &string(e.name().as_ref(), offset)?);
                element.offset = offset;
                for attr in e.attributes() {
                    if element.attributes.len() >= 256 {
                        return Err(Error::limit(
                            LimitKind::Objects,
                            "XML attributes per element".into(),
                        ));
                    }
                    let attr = attr.map_err(|e| Error::invalid(offset, e.to_string()))?;
                    if attr.value.len() as u64 > u64::from(limits.max_string_bytes)
                        || attr.key.as_ref().len() as u64 > u64::from(limits.max_string_bytes)
                    {
                        return Err(Error::limit(
                            LimitKind::StringBytes,
                            "XML attribute bytes".into(),
                        ));
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
                    return Err(Error::limit(
                        LimitKind::StringBytes,
                        "XML comment bytes".into(),
                    ));
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
            return Err(Error::limit(
                LimitKind::DecompressedBytes,
                "XML decoded bytes".into(),
            ));
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
        return Err(Error::limit(
            LimitKind::StringBytes,
            "XML text bytes".into(),
        ));
    }
    *total = total.saturating_add(value.len() as u64);
    if let Some(parent) = stack.last_mut() {
        if let Some(Node::Text(previous)) = parent.children.last_mut() {
            if previous.len().saturating_add(value.len()) as u64 > u64::from(max) {
                return Err(Error::limit(
                    LimitKind::StringBytes,
                    "XML text bytes".into(),
                ));
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
    exceeded: bool,
}
impl std::io::Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if (self.bytes.len() as u64).saturating_add(bytes.len() as u64) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("XML output byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn write_xml(writer: &mut Writer<Output>, event: Event<'_>) -> Result<()> {
    writer.write_event(event).map_err(|e| {
        if writer.get_ref().exceeded {
            Error::limit(LimitKind::Output, "XML output byte limit".into())
        } else {
            Error::Io(e)
        }
    })
}

const XML_PREFIX_NS: &str = "http://www.w3.org/XML/1998/namespace";

fn lookup_ns<'a>(scopes: &[(&'a str, &'a str)], prefix: &str) -> Option<&'a str> {
    if prefix == "xml" {
        return Some(XML_PREFIX_NS);
    }
    scopes
        .iter()
        .rev()
        .find(|&&(p, _)| p == prefix)
        .map(|&(_, uri)| uri)
}

fn validate_element_namespaces<'a>(
    e: &'a Element,
    scopes: &mut Vec<(&'a str, &'a str)>,
) -> Result<()> {
    for (idx, a) in e.attributes.iter().enumerate() {
        let local = a
            .name
            .qualified
            .rsplit(':')
            .next()
            .unwrap_or(&a.name.qualified);
        for prev in e.attributes.get(..idx).unwrap_or(&[]) {
            if prev.name.is(&a.name.namespace, local) {
                return Err(Error::invalid(e.offset, "duplicate expanded XML attribute"));
            }
        }
        if a.name.qualified == "xmlns" {
            if a.name.namespace != XMLNS || a.value == XMLNS || a.value == XML_PREFIX_NS {
                return Err(Error::invalid(
                    0,
                    "XML namespace declarations disagree with model names",
                ));
            }
            scopes.push(("", a.value.as_str()));
        } else if let Some(prefix) = a.name.qualified.strip_prefix("xmlns:") {
            if a.name.namespace != XMLNS
                || prefix.is_empty()
                || prefix == "xmlns"
                || a.value.is_empty()
                || a.value == XMLNS
                || (prefix == "xml") != (a.value == XML_PREFIX_NS)
            {
                return Err(Error::invalid(
                    0,
                    "XML namespace declarations disagree with model names",
                ));
            }
            scopes.push((prefix, a.value.as_str()));
        }
    }

    if let Some((prefix, _)) = e.name.qualified.split_once(':') {
        let Some(bound) = lookup_ns(scopes, prefix) else {
            return Err(Error::invalid(e.offset, "undeclared XML namespace prefix"));
        };
        if bound != e.name.namespace || bound.is_empty() {
            return Err(Error::invalid(
                0,
                "XML namespace declarations disagree with model names",
            ));
        }
    } else {
        let bound = lookup_ns(scopes, "").unwrap_or("");
        if bound != e.name.namespace {
            return Err(Error::invalid(
                0,
                "XML namespace declarations disagree with model names",
            ));
        }
    }

    for a in &e.attributes {
        if a.name.qualified == "xmlns" || a.name.qualified.starts_with("xmlns:") {
            continue;
        }
        if a.name.namespace == XMLNS {
            return Err(Error::invalid(
                0,
                "XML namespace declarations disagree with model names",
            ));
        }
        if let Some((prefix, _)) = a.name.qualified.split_once(':') {
            let Some(bound) = lookup_ns(scopes, prefix) else {
                return Err(Error::invalid(e.offset, "undeclared XML namespace prefix"));
            };
            if bound != a.name.namespace || bound.is_empty() {
                return Err(Error::invalid(
                    0,
                    "XML namespace declarations disagree with model names",
                ));
            }
        } else if !a.name.namespace.is_empty() {
            return Err(Error::invalid(
                0,
                "XML namespace declarations disagree with model names",
            ));
        }
    }
    Ok(())
}

pub(crate) fn serialize(doc: &CityGmlDocument, options: &ReadOptions) -> Result<Vec<u8>> {
    // Validate tree depth and string budgets before serializing.
    crate::validate::elements(doc, &options.limits)?;
    if !doc.root.name.is(CORE, "CityModel") {
        return Err(Error::UnknownFormat);
    }
    let mut writer = Writer::new(Output {
        bytes: vec![],
        limit: options.limits.max_input_bytes,
        exceeded: false,
    });
    write_xml(
        &mut writer,
        Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)),
    )?;
    enum Action<'a> {
        Element(&'a Element),
        Node(&'a Node),
        End(&'a str, usize),
    }
    let mut scopes: Vec<(&str, &str)> = Vec::new();
    let mut todo = vec![Action::Element(&doc.root)];
    while let Some(action) = todo.pop() {
        match action {
            Action::End(name, saved_scope_len) => {
                scopes.truncate(saved_scope_len);
                write_xml(&mut writer, Event::End(BytesEnd::new(name)))?;
            }
            Action::Element(e) | Action::Node(Node::Element(e)) => {
                let saved_scope_len = scopes.len();
                validate_element_namespaces(e, &mut scopes)?;
                let mut start = BytesStart::new(e.name.qualified.as_str());
                for a in &e.attributes {
                    // Escape whitespace control characters as numeric references so XML attribute normalization preserves them.
                    let escaped = quick_xml::escape::escape(&a.value)
                        .replace('\t', "&#9;")
                        .replace('\n', "&#10;")
                        .replace('\r', "&#13;");
                    start.push_attribute((a.name.qualified.as_bytes(), escaped.as_bytes()));
                }
                write_xml(&mut writer, Event::Start(start))?;
                todo.push(Action::End(&e.name.qualified, saved_scope_len));
                for child in e.children.iter().rev() {
                    todo.push(Action::Node(child));
                }
            }
            Action::Node(Node::Text(t)) => {
                let escaped = quick_xml::escape::escape(t).replace('\r', "&#13;");
                write_xml(&mut writer, Event::Text(BytesText::from_escaped(&escaped)))?;
            }
            Action::Node(Node::Comment(t)) => {
                if t.contains("--") || t.ends_with('-') {
                    return Err(Error::invalid(0, "invalid XML comment"));
                }
                write_xml(&mut writer, Event::Comment(BytesText::from_escaped(t)))?;
            }
        }
    }
    Ok(writer.into_inner().bytes)
}
