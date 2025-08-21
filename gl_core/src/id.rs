use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier backed by ULID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(ulid::Ulid);

impl Id {
    /// Generate a new ID
    pub fn new() -> Self {
        Self(ulid::Ulid::new())
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for Id {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_roundtrip() {
        let id = Id::new();
        let id_str = id.to_string();
        let parsed: Id = id_str.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_id_serde() {
        let id = Id::new();
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: Id = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }
}
