//! Snapshot-based undo/redo. Generic and UI-free so it can be unit tested.

const MAX_DEPTH: usize = 200;

#[derive(Debug)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new() }
    }
}

impl<T> History<T> {
    /// Record `before` (the state prior to an edit). Clears the redo stack.
    pub fn record(&mut self, before: T) {
        self.undo.push(before);
        if self.undo.len() > MAX_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Returns the state to restore; `current` is stored for redo.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_roundtrip() {
        let mut h = History::default();
        h.record(1);
        assert_eq!(h.undo(2), Some(1));
        assert!(h.can_redo());
        assert_eq!(h.redo(1), Some(2));
        assert!(!h.can_redo());
        assert_eq!(h.undo(2), Some(1));
    }

    #[test]
    fn record_clears_redo() {
        let mut h = History::default();
        h.record(1);
        h.undo(2);
        h.record(3);
        assert!(!h.can_redo());
    }

    #[test]
    fn empty_is_none() {
        let mut h: History<i32> = History::default();
        assert_eq!(h.undo(0), None);
        assert_eq!(h.redo(0), None);
    }
}
