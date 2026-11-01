//! Bounded SVG buffers share a failure flag, including temporary paths and escaped text.
//! Once a write would exceed the cap, no buffer in that render grows again.

use std::cell::Cell;
use std::fmt::{self, Write};
use std::ops::Deref;
use std::rc::Rc;

#[derive(Clone, Default)]
pub(super) struct Budget(Option<Rc<State>>);

struct State {
    cap: usize,
    exceeded: Cell<bool>,
}

impl Budget {
    pub(super) fn is_unlimited(&self) -> bool {
        self.0.is_none()
    }
    pub(super) fn new(cap: usize) -> Self {
        Self(Some(Rc::new(State {
            cap,
            exceeded: Cell::new(false),
        })))
    }

    pub(super) fn exceeded(&self) -> bool {
        self.0.as_ref().is_some_and(|state| state.exceeded.get())
    }

    pub(super) fn allows(&self, bytes: usize) -> bool {
        let Some(state) = &self.0 else { return true };
        if bytes > state.cap {
            state.exceeded.set(true);
        }
        !state.exceeded.get()
    }

    pub(super) fn reject(&self) {
        if let Some(state) = &self.0 {
            state.exceeded.set(true);
        }
    }

    pub(super) fn buffer(&self) -> Buffer {
        Buffer {
            text: String::new(),
            budget: self.clone(),
        }
    }

    pub(super) fn format(&self, args: fmt::Arguments<'_>) -> Buffer {
        let mut buffer = self.buffer();
        // Failure is remembered by the shared budget and reported by the public renderer.
        let _ = buffer.write_fmt(args);
        buffer
    }
}

pub(super) struct Buffer {
    text: String,
    budget: Budget,
}

impl Buffer {
    pub(super) fn exceeded(&self) -> bool {
        self.budget.exceeded()
    }

    pub(super) fn push_str(&mut self, value: &str) {
        let _ = self.write_str(value);
    }

    pub(super) fn push(&mut self, value: char) {
        let _ = self.write_char(value);
    }

    pub(super) fn into_string(self) -> String {
        self.text
    }
}

impl Write for Buffer {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let Some(bytes) = self.text.len().checked_add(value.len()) else {
            self.budget.reject();
            return Err(fmt::Error);
        };
        if !self.budget.allows(bytes) {
            return Err(fmt::Error);
        }
        // Avoid geometric String growth beyond the requested output budget.
        if let Some(state) = &self.budget.0
            && bytes > self.text.capacity()
        {
            let capacity = self
                .text
                .capacity()
                .max(32)
                .saturating_mul(2)
                .min(state.cap)
                .max(bytes);
            self.text.reserve_exact(capacity - self.text.len());
        }
        self.text.push_str(value);
        Ok(())
    }
}

impl Deref for Buffer {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_happens_before_growth_and_stops_other_buffers() {
        let budget = Budget::new(4);
        let mut a = budget.buffer();
        a.push_str("éé");
        assert_eq!(&*a, "éé");
        assert_eq!(a.text.capacity(), 4);
        a.push('x');
        assert!(budget.exceeded());
        assert_eq!(&*a, "éé");
        let b = budget.format(format_args!("{}", "must not be allocated"));
        assert!(b.is_empty());
        assert_eq!(b.text.capacity(), 0);
    }
}
