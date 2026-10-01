//! CFB akışlarını, dizin metadatasını ve kaynak sınırlarını koruyan yazma katmanı.

use cadkit_core::{Error, ReadOptions, Result};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Seek, SeekFrom, Write},
};

pub(super) struct BoundedCursor {
    inner: Cursor<Vec<u8>>,
    limit: u64,
}
impl BoundedCursor {
    fn new(limit: u64) -> Self {
        Self {
            inner: Cursor::new(vec![]),
            limit,
        }
    }
    pub(super) fn bytes(self) -> Vec<u8> {
        self.inner.into_inner()
    }
}
impl Read for BoundedCursor {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(b)
    }
}
impl Seek for BoundedCursor {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        let old = self.inner.position();
        let result = self.inner.seek(p)?;
        if result > self.limit {
            self.inner.set_position(old);
            return Err(std::io::Error::other("CFB output limit exceeded"));
        }
        Ok(result)
    }
}
impl Write for BoundedCursor {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self
            .inner
            .position()
            .checked_add(b.len() as u64)
            .is_none_or(|n| n > self.limit)
        {
            return Err(std::io::Error::other("CFB output limit exceeded"));
        }
        self.inner.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn rewrite(
    seed: &[u8],
    replacements: &BTreeMap<String, Option<Vec<u8>>>,
    options: &ReadOptions,
) -> Result<Vec<u8>> {
    if seed.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::LimitExceeded("DGN seed bytes".into()));
    }
    let mut source = ::cfb::CompoundFile::open(Cursor::new(seed))?;
    let mut entries = vec![];
    let mut budget = 0u64;
    for entry in source.walk() {
        if entries.len() as u64 >= options.limits.max_objects {
            return Err(Error::LimitExceeded("CFB entries".into()));
        }
        budget = budget
            .checked_add(entry.len())
            .ok_or_else(|| Error::LimitExceeded("CFB streams".into()))?;
        if budget > options.limits.max_decompressed_bytes {
            return Err(Error::LimitExceeded("CFB streams".into()));
        }
        entries.push(entry);
    }
    let mut output = ::cfb::CompoundFile::create_with_version(
        source.version(),
        BoundedCursor::new(options.limits.max_input_bytes),
    )?;
    for entry in &entries {
        if entry.is_storage() && !entry.is_root() {
            output.create_storage_all(entry.path())?;
        }
    }
    for entry in &entries {
        if !entry.is_stream() {
            continue;
        }
        let key = entry
            .path()
            .to_string_lossy()
            .trim_start_matches('/')
            .to_owned();
        match replacements.get(&key) {
            Some(None) => {}
            Some(Some(bytes)) => {
                output.create_stream(entry.path())?.write_all(bytes)?;
            }
            None => {
                let mut src = source.open_stream(entry.path())?;
                let mut dst = output.create_stream(entry.path())?;
                std::io::copy(&mut src.by_ref().take(entry.len()), &mut dst)?;
            }
        }
    }
    for (name, data) in replacements {
        if let Some(data) = data {
            let path = format!("/{name}");
            if !source.is_stream(&path) {
                if let Some((parent, _)) = path.rsplit_once('/') {
                    if !parent.is_empty() {
                        output.create_storage_all(parent)?;
                    }
                }
                output.create_stream(&path)?.write_all(data)?;
            }
        }
    }
    let created: Vec<_> = output
        .walk()
        .filter(|e| !source.exists(e.path()))
        .map(|e| e.path().to_owned())
        .collect();
    let seed_time = source.entry("/")?.created();
    for path in created {
        output.set_created_time(&path, seed_time)?;
        output.set_modified_time(&path, seed_time)?;
    }
    for entry in &entries {
        if !output.exists(entry.path()) {
            continue;
        }
        if entry.is_storage() {
            output.set_storage_clsid(entry.path(), *entry.clsid())?;
        }
        output.set_state_bits(entry.path(), entry.state_bits())?;
        output.set_created_time(entry.path(), entry.created())?;
        output.set_modified_time(entry.path(), entry.modified())?;
    }
    output.flush()?;
    Ok(output.into_inner().bytes())
}

pub(super) fn stream(seed: &[u8], name: &str, max: u64) -> Result<Vec<u8>> {
    let mut cf = ::cfb::CompoundFile::open(Cursor::new(seed))?;
    let n = cf.entry(name)?.len();
    if n > max {
        return Err(Error::LimitExceeded("DGN seed stream".into()));
    }
    let mut data = vec![];
    cf.open_stream(name)?.take(n).read_to_end(&mut data)?;
    Ok(data)
}
