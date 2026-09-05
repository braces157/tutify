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
    pub fn insert_next(&mut self, id: String) {
        if self.order.is_empty() || self.cursor.is_none() {
            self.enqueue(id);
            return;
        }
        let new_idx = self.ids.len();
        self.ids.push(id);
        let insert_at = self.cursor.unwrap() + 1;
        self.order.insert(insert_at, new_idx);
        if self.selected >= insert_at {
            self.selected += 1;
        }
    }
    pub fn clear(&mut self) -> bool {
        let had_playing = self.cursor.is_some();
        self.ids.clear();
        self.order.clear();
        self.cursor = None;
        self.selected = 0;
        self.position_ms = 0;
        had_playing
    }
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.order.len() || to >= self.order.len() || from == to {
            return false;
        }
        let item = self.order.remove(from);
        self.order.insert(to, item);
        self.cursor = match self.cursor {
            Some(c) if c == from => Some(to),
            Some(c) if from < to && c > from && c <= to => Some(c - 1),
            Some(c) if to < from && c >= to && c < from => Some(c + 1),
            other => other,
        };
        self.selected = to;
        true
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
    #[test]
    fn insert_next_places_directly_after_cursor() {
        let mut q = queue();
        assert_eq!(q.cursor, Some(2));
        let next_id = "9".repeat(22);
        q.insert_next(next_id.clone());
        q.validate().unwrap();
        assert_eq!(q.cursor, Some(2));
        assert_eq!(q.order[3], 5);
        assert_eq!(q.ids[5], next_id);
    }
    #[test]
    fn clear_wipes_queue_and_resets_cursor() {
        let mut q = queue();
        assert!(q.clear());
        assert!(q.ids.is_empty());
        assert!(q.order.is_empty());
        assert_eq!(q.cursor, None);
        q.validate().unwrap();
    }
    #[test]
    fn move_item_reorders_and_tracks_cursor() {
        let mut q = queue();
        // cursor is at 2
        assert!(q.move_item(0, 4));
        q.validate().unwrap();
        // Item at 0 moved to 4, so cursor (2) shifted down to 1
        assert_eq!(q.cursor, Some(1));
        // Move playing item from 1 to 3
        assert!(q.move_item(1, 3));
        q.validate().unwrap();
        assert_eq!(q.cursor, Some(3));
    }
}
