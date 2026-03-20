use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    Err,
    Warning,
    Notice,
    Info,
    Debug,
}

impl Priority {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Err => Self::Warning,
            Self::Warning => Self::Notice,
            Self::Notice => Self::Info,
            Self::Info => Self::Debug,
            Self::Debug => Self::Err,
        }
    }

    pub fn as_journalctl_arg(&self) -> &'static str {
        match self {
            Self::Err => "err",
            Self::Warning => "warning",
            Self::Notice => "notice",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "err" | "error" => Self::Err,
            "warning" | "warn" => Self::Warning,
            "notice" => Self::Notice,
            "debug" => Self::Debug,
            _ => Self::Info,
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_journalctl_arg())
    }
}

/// Check if a log line contains the search query (case-insensitive).
/// Returns the byte ranges of all matches.
pub fn find_matches(line: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return vec![];
    }
    let lower_line = line.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(pos) = lower_line[start..].find(&lower_query) {
        let abs_pos = start + pos;
        matches.push((abs_pos, abs_pos + query.len()));
        start = abs_pos + 1;
    }
    matches
}
