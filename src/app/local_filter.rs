pub(super) struct FilterTerms(Vec<String>);

impl FilterTerms {
    pub(super) fn new(query: &str) -> Self {
        Self(
            query
                .to_lowercase()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        )
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn matches<const N: usize>(&self, fields: [&str; N]) -> bool {
        if self.is_empty() {
            return true;
        }
        let fields = fields.map(str::to_lowercase);
        self.0
            .iter()
            .all(|term| fields.iter().any(|field| field.contains(term.as_str())))
    }
}
