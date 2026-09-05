use crate::{model::Repeat, storage::validate_ids};
use anyhow::{Result, bail};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Queue {
    pub version: u32,
    pub ids: Vec<String>,
    pub order: Vec<usize>,
    pub cursor: Option<usize>,
    pub selected: usize,
    pub position_ms: u32,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            version: 1,
            ids: vec![],
            order: vec![],
            cursor: None,
            selected: 0,
            position_ms: 0,
        }
    }
}

impl Queue {
    pub fn validate(&self) -> Result<()> {
        validate_ids(&self.ids)?;
        let mut order = self.order.clone();
        order.sort_unstable();
        if self.version != 1
            || order != (0..self.ids.len()).collect::<Vec<_>>()
            || self.cursor.is_some_and(|i| i >= self.order.len())
            || (!self.ids.is_empty() && self.selected >= self.ids.len())
        {
            bail!(
                "Queue snapshot has an unsupported version or invalid order; preserve queue.json and move it aside to reset"
            );
        }
        Ok(())
    }
    pub fn current(&self) -> Option<&str> {
        self.cursor
            .and_then(|c| self.order.get(c))
            .and_then(|i| self.ids.get(*i))
            .map(String::as_str)
    }
    pub fn replace(&mut self, ids: Vec<String>, index: usize, shuffle: bool) {
        self.ids = ids;
        self.order = (0..self.ids.len()).collect();
        self.cursor = (!self.ids.is_empty()).then_some(index.min(self.ids.len().saturating_sub(1)));
        self.selected = self.cursor.unwrap_or(0);
        self.position_ms = 0;
        if shuffle {
            self.set_shuffle(true);
        }
    }
    pub fn set_shuffle(&mut self, enabled: bool) {
        let current = self.cursor.map(|c| self.order[c]);
        self.order = (0..self.ids.len()).collect();
        if enabled {
            self.order.shuffle(&mut rand::thread_rng());
            if let Some(i) = current {
                let at = self.order.iter().position(|x| *x == i).unwrap();
                self.order.swap(0, at);
            }
        }
        self.cursor = current.and_then(|i| self.order.iter().position(|x| *x == i));
        self.selected = self.cursor.unwrap_or(0);
    }
    pub fn enqueue(&mut self, id: String) {
        self.order.push(self.ids.len());
        self.ids.push(id);
    }
    pub fn select(&mut self, at: usize) {
        if at < self.order.len() {
            self.cursor = Some(at);
            self.selected = at;
            self.position_ms = 0;
        }
    }
    pub fn advance(&mut self, repeat: Repeat, completed: bool) -> bool {
        if self.order.is_empty() {
            return false;
        }
        let next = match self.cursor {
            None => 0,
            Some(c) if completed && repeat == Repeat::Track => c,
            Some(c) if c + 1 < self.order.len() => c + 1,
            Some(_) if repeat == Repeat::Queue => 0,
            _ => return false,
        };
        self.select(next);
        true
    }
    pub fn previous(&mut self) -> bool {
        if self.cursor.is_none() {
            return false;
        }
        if self.position_ms > 3000 {
            self.position_ms = 0;
            return true;
        }
        self.select(self.cursor.unwrap().saturating_sub(1));
        true
    }
    /// Returns true when the playing item was removed. Caller stops; never auto-skips.
    pub fn remove(&mut self, at: usize) -> bool {
        if at >= self.order.len() {
            return false;
        }
        let removed_current = self.cursor == Some(at);
        let original = self.order.remove(at);
        self.ids.remove(original);
        for i in &mut self.order {
            if *i > original {
                *i -= 1;
            }
        }
        self.cursor = match self.cursor {
            Some(_) if removed_current => None,
            Some(c) if c > at => Some(c - 1),
            c => c,
        };
        if removed_current {
            self.position_ms = 0;
        }
        self.selected = self.selected.min(self.order.len().saturating_sub(1));
        removed_current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queue() -> Queue {
        let mut q = Queue::default();
        q.replace((0..5).map(|i| format!("{i:022}")).collect(), 2, false);
        q
    }
    #[test]
    fn shuffle_preserves_current_and_unshuffle_restores_order() {
        let mut q = queue();
        let current = q.current().unwrap().to_owned();
        q.set_shuffle(true);
        assert_eq!(q.current(), Some(current.as_str()));
        q.validate().unwrap();
        q.set_shuffle(false);
        assert_eq!(q.cursor, Some(2));
        assert_eq!(q.order, vec![0, 1, 2, 3, 4]);
    }
    #[test]
    fn completion_repeat_and_manual_next() {
        let mut q = queue();
        assert!(q.advance(Repeat::Track, true));
        assert_eq!(q.cursor, Some(2));
        assert!(q.advance(Repeat::Track, false));
        assert_eq!(q.cursor, Some(3));
        q.select(4);
        assert!(!q.advance(Repeat::Off, true));
        assert!(q.advance(Repeat::Queue, true));
        assert_eq!(q.cursor, Some(0));
    }
    #[test]
    fn duplicates_remove_and_enqueue() {
        let mut q = queue();
        let current = q.current().unwrap().to_owned();
        q.enqueue(current.clone());
        assert!(!q.remove(0));
        assert_eq!(q.current(), Some(current.as_str()));
        assert!(q.remove(1));
        assert_eq!(q.current(), None);
        q.validate().unwrap();
    }
    #[test]
    fn previous_restarts_then_goes_back() {
        let mut q = queue();
        q.position_ms = 9000;
        q.previous();
        assert_eq!(q.cursor, Some(2));
        q.previous();
        assert_eq!(q.cursor, Some(1));
    }
    #[test]
    fn reject_invalid_snapshot() {
        let mut q = queue();
        q.order[0] = 2;
        assert!(q.validate().is_err());
    }
}
