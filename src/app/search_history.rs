use super::*;

/// Bounded session-only queries, kept separately from listening statistics.
#[derive(Default)]
pub struct SearchHistory {
    entries: VecDeque<String>,
    cursor: Option<usize>,
    draft: String,
}

impl SearchHistory {
    pub(super) fn record(&mut self, query: &str) {
        let query = query.trim();
        self.cursor = None;
        self.draft.clear();
        if query.is_empty() {
            return;
        }
        self.entries.retain(|entry| entry != query);
        self.entries.push_front(query.to_owned());
        self.entries.truncate(20);
    }

    pub(super) fn recall(&mut self, current: &str, older: bool) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        if older {
            let next = match self.cursor {
                None => {
                    self.draft = current.to_owned();
                    0
                }
                Some(at) => (at + 1).min(self.entries.len() - 1),
            };
            self.cursor = Some(next);
            Some(self.entries[next].clone())
        } else if let Some(at) = self.cursor {
            if at == 0 {
                self.cursor = None;
                Some(self.draft.clone())
            } else {
                self.cursor = Some(at - 1);
                Some(self.entries[at - 1].clone())
            }
        } else {
            None
        }
    }

    pub(super) fn detach(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }
}
