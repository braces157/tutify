use super::*;
use std::collections::HashSet;

impl Queue {
    /// Slots after three original entries, never before or on the playing entry.
    /// Existing suggestions occupy a slot; manual additions remain originals.
    pub fn smart_slots(&self) -> Vec<usize> {
        if !self.smart_shuffle {
            return Vec::new();
        }
        let mut originals = 0;
        let mut slots = Vec::new();
        for (at, index) in self.order.iter().enumerate() {
            if self.suggestions.contains(index) {
                originals = 0;
            } else {
                if originals >= 3 {
                    if self.cursor.is_none_or(|c| at > c + 1) {
                        slots.push(at);
                    }
                    originals = 0;
                }
                originals += 1;
            }
        }
        if originals >= 3 {
            slots.push(self.order.len());
        }
        slots
    }

    /// Merge in one pass: bounded at MAX_TRACKS, preserving current/selected
    /// occurrences and all existing order. Returns only the IDs actually added.
    pub fn add_smart_suggestions(&mut self, ids: Vec<String>) -> Vec<String> {
        let slots = self.smart_slots();
        let capacity = MAX_TRACKS.saturating_sub(self.ids.len()).min(slots.len());
        let mut seen: HashSet<_> = self.ids.iter().cloned().collect();
        let added: Vec<_> = ids
            .into_iter()
            .filter(|id| valid_id(id) && seen.insert(id.clone()))
            .take(capacity)
            .collect();
        if added.is_empty() {
            return added;
        }
        let current = self.cursor.map(|c| self.order[c]);
        let selected = self.order.get(self.selected).copied();
        let mut order = Vec::with_capacity(self.order.len() + added.len());
        let mut slots = slots.into_iter().take(added.len()).peekable();
        let mut next_index = self.ids.len();
        for at in 0..=self.order.len() {
            if slots.peek() == Some(&at) {
                slots.next();
                order.push(next_index);
                next_index += 1;
            }
            if let Some(&index) = self.order.get(at) {
                order.push(index);
            }
        }
        self.suggestions
            .extend(self.ids.len()..self.ids.len() + added.len());
        self.ids.extend(added.iter().cloned());
        self.order = order;
        self.cursor = current.and_then(|i| self.order.iter().position(|&x| x == i));
        self.selected = selected
            .and_then(|i| self.order.iter().position(|&x| x == i))
            .unwrap_or(0);
        self.revision += 1;
        added
    }

    /// Keep a playing recommendation as an ordinary entry so disabling never
    /// interrupts audio. Remove other injected occurrences, including history.
    pub fn disable_smart_shuffle(&mut self) {
        if !self.smart_shuffle && self.suggestions.is_empty() {
            return;
        }
        let current = self.cursor.map(|c| self.order[c]);
        let selected = self.order.get(self.selected).copied();
        let mut remap = vec![None; self.ids.len()];
        let mut next = 0;
        for (i, entry) in remap.iter_mut().enumerate() {
            if !self.suggestions.contains(&i) || current == Some(i) {
                *entry = Some(next);
                next += 1;
            }
        }
        let mut index = 0;
        self.ids.retain(|_| {
            let keep = remap[index].is_some();
            index += 1;
            keep
        });
        self.order = self.order.iter().filter_map(|&i| remap[i]).collect();
        self.cursor = current
            .and_then(|i| remap[i])
            .and_then(|i| self.order.iter().position(|&x| x == i));
        self.selected = selected
            .and_then(|i| remap[i])
            .and_then(|i| self.order.iter().position(|&x| x == i))
            .unwrap_or_else(|| self.selected.min(self.order.len().saturating_sub(1)));
        self.smart_shuffle = false;
        self.suggestions.clear();
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(i: usize) -> String {
        format!("{i:022}")
    }
    fn smart_queue(n: usize) -> Queue {
        let mut queue = Queue::default();
        queue.replace((0..n).map(id).collect(), 0, false);
        queue.smart_shuffle = true;
        queue
    }

    #[test]
    fn inserts_one_per_three_without_moving_current_or_selection() {
        let mut q = smart_queue(9);
        q.position_ms = 12_345;
        q.selected = 7;
        let added = q.add_smart_suggestions((20..30).map(id).collect());
        assert_eq!(added, (20..23).map(id).collect::<Vec<_>>());
        assert_eq!(q.order, vec![0, 1, 2, 9, 3, 4, 5, 10, 6, 7, 8, 11]);
        assert_eq!(q.current(), Some(id(0).as_str()));
        assert_eq!(q.order[q.selected], 7);
        assert_eq!(q.position_ms, 12_345);
        assert!(q.smart_slots().is_empty());
        assert!(q.add_smart_suggestions(vec![id(40)]).is_empty());
        q.validate().unwrap();
    }

    #[test]
    fn disabling_preserves_playing_suggestion_and_manual_duplicate() {
        let mut q = smart_queue(9);
        q.add_smart_suggestions(vec![id(20), id(21), id(22)]);
        q.select(7); // playing suggestion 21
        q.position_ms = 4500;
        q.enqueue(id(20)); // same ID, explicitly added occurrence
        q.disable_smart_shuffle();
        assert_eq!(q.current(), Some(id(21).as_str()));
        assert_eq!(q.position_ms, 4500);
        assert_eq!(q.ids.iter().filter(|i| **i == id(20)).count(), 1);
        assert!(!q.ids.contains(&id(22)));
        assert!(q.suggestions.is_empty());
        assert_eq!(q.ids.len(), 11);
        q.validate().unwrap();
    }

    #[test]
    fn filters_duplicates_and_respects_manual_next_and_played_history() {
        let mut q = smart_queue(9);
        q.select(2);
        q.insert_next(id(50));
        let history = q.order[..4].to_vec();
        q.add_smart_suggestions(vec![id(0), id(50), "bad".into(), id(20), id(20), id(21)]);
        assert_eq!(&q.order[..4], history);
        assert_eq!(q.ids.iter().filter(|i| **i == id(20)).count(), 1);
        assert!(q.advance(crate::model::Repeat::Off, false));
        assert_eq!(q.current(), Some(id(50).as_str()));
        q.previous();
        assert_eq!(q.current(), Some(id(2).as_str()));
        q.validate().unwrap();
    }

    #[test]
    fn markers_follow_remove_move_and_snapshot_restore() {
        let mut q = smart_queue(6);
        q.add_smart_suggestions(vec![id(20), id(21)]);
        q.remove(1);
        q.move_item(2, 4);
        q.validate().unwrap();
        let restored: Queue = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        restored.validate().unwrap();
        assert!(restored.smart_shuffle);
        assert_eq!(restored.suggestions, q.suggestions);
        assert_eq!(restored.order, q.order);
        q.disable_smart_shuffle();
        assert!(!q.ids.contains(&id(20)));
        assert!(!q.ids.contains(&id(21)));
        q.validate().unwrap();
        let legacy: Queue = serde_json::from_str(r#"{"ids":[],"order":[]}"#).unwrap();
        legacy.validate().unwrap();
        assert!(!legacy.smart_shuffle);
    }

    #[test]
    fn bounded_for_empty_short_and_full_queues_and_invalid_snapshots() {
        for n in [0, 1, 2, MAX_TRACKS] {
            let mut q = smart_queue(n);
            assert!(q.add_smart_suggestions(vec![id(MAX_TRACKS + 1)]).is_empty());
            q.validate().unwrap();
        }
        let mut q = smart_queue(MAX_TRACKS - 1);
        assert_eq!(
            q.add_smart_suggestions(vec![id(MAX_TRACKS), id(MAX_TRACKS + 1)])
                .len(),
            1
        );
        q.validate().unwrap();
        q.suggestions.insert(MAX_TRACKS);
        assert!(q.validate().is_err());
    }
}
