//! Rule attribute system.
//!
//! Attributes are single-character flags that appear between colons in rule headers:
//! `target:VQ: prereq`. They control execution behaviour (quiet, virtual, regex, etc).

use std::error::Error;
use std::fmt;

/// Bitflags for rule attributes.
/// These appear between colons in rule headers: `target:VQ: prereq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attributes(u16);

/// Attribute constants — each is a single bit position.
impl Attributes {
    pub const VIRTUAL: u16 = 1 << 0; // V — target is virtual, not a real file
    pub const QUIET: u16 = 1 << 1; // Q — don't echo recipe before executing
    pub const NO_EXEC: u16 = 1 << 2; // N — no recipe to execute; treat target as updated
    pub const UNEXPORTED: u16 = 1 << 3; // U — target considered updated even if recipe didn't touch it
    pub const DELETE_ON_ERROR: u16 = 1 << 4; // D — delete target file if recipe fails
    pub const EXCLUSIVE: u16 = 1 << 5; // E — run recipe exclusively (no parallel jobs during this recipe)
    pub const COMPARISON: u16 = 1 << 6; // P — custom comparison program for out-of-date check
    pub const REGEX: u16 = 1 << 7; // R — regex metarule (target is a regex pattern)
    pub const NO_VIRTUAL: u16 = 1 << 8; // n — metarule: only match real files, not virtual targets
}

impl Attributes {
    /// Create an empty attribute set.
    pub fn new() -> Self {
        Self(0)
    }

    /// Returns the underlying u16 value.
    pub fn bits(&self) -> u16 {
        self.0
    }

    /// Returns true if no attributes are set.
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Set an attribute and return modified self (builder-style).
    pub fn with(&self, attr: u16) -> Self {
        Self(self.0 | attr)
    }

    pub fn is_virtual(&self) -> bool {
        self.0 & Self::VIRTUAL != 0
    }

    pub fn is_quiet(&self) -> bool {
        self.0 & Self::QUIET != 0
    }

    pub fn is_no_exec(&self) -> bool {
        self.0 & Self::NO_EXEC != 0
    }

    pub fn is_unexported(&self) -> bool {
        self.0 & Self::UNEXPORTED != 0
    }

    pub fn is_delete_on_error(&self) -> bool {
        self.0 & Self::DELETE_ON_ERROR != 0
    }

    pub fn is_exclusive(&self) -> bool {
        self.0 & Self::EXCLUSIVE != 0
    }

    pub fn has_comparison(&self) -> bool {
        self.0 & Self::COMPARISON != 0
    }

    pub fn is_regex(&self) -> bool {
        self.0 & Self::REGEX != 0
    }

    pub fn is_no_virtual(&self) -> bool {
        self.0 & Self::NO_VIRTUAL != 0
    }
}

impl fmt::Display for Attributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_virtual() {
            write!(f, "V")?;
        }
        if self.is_quiet() {
            write!(f, "Q")?;
        }
        if self.is_no_exec() {
            write!(f, "N")?;
        }
        if self.is_unexported() {
            write!(f, "U")?;
        }
        if self.is_delete_on_error() {
            write!(f, "D")?;
        }
        if self.is_exclusive() {
            write!(f, "E")?;
        }
        if self.has_comparison() {
            write!(f, "P")?;
        }
        if self.is_regex() {
            write!(f, "R")?;
        }
        if self.is_no_virtual() {
            write!(f, "n")?;
        }
        Ok(())
    }
}

/// Parse attribute characters from a string like "VQ" or "VQn".
/// Each character maps to one flag. Unknown characters return an error.
/// The string should contain only attribute characters (no whitespace, no colons).
pub fn parse_attributes(s: &str) -> Result<Attributes, ParseAttrError> {
    let mut attrs = Attributes::new();
    for ch in s.chars() {
        let bit = match ch {
            'V' => Attributes::VIRTUAL,
            'Q' => Attributes::QUIET,
            'N' => Attributes::NO_EXEC,
            'U' => Attributes::UNEXPORTED,
            'D' => Attributes::DELETE_ON_ERROR,
            'E' => Attributes::EXCLUSIVE,
            'P' => Attributes::COMPARISON,
            'R' => Attributes::REGEX,
            'n' => Attributes::NO_VIRTUAL,
            _ => return Err(ParseAttrError::UnknownAttr(ch)),
        };
        attrs = attrs.with(bit);
    }
    Ok(attrs)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseAttrError {
    UnknownAttr(char),
}

impl fmt::Display for ParseAttrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseAttrError::UnknownAttr(c) => write!(f, "unknown attribute character: '{c}'"),
        }
    }
}

impl Error for ParseAttrError {}

/// Human-readable descriptions for CLI help / `-e` flag.
pub const ATTR_HELP: &[(&str, &str)] = &[
    ("V", "Virtual target — not a real file, always considered stale"),
    ("Q", "Quiet — don't echo recipe before executing"),
    ("N", "No-exec — treat target as updated without running recipe"),
    ("U", "Unexported — target is updated even if recipe didn't change it"),
    ("D", "Delete on error — delete target if recipe fails"),
    ("E", "Exclusive — run recipe without parallel jobs"),
    ("P", "Custom comparison — use program to determine if target is stale"),
    ("R", "Regex metarule — target pattern is a regular expression"),
    ("n", "No-virtual — metarule matches only real files, not virtual targets"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert!(Attributes::new().is_empty());
    }

    #[test]
    fn parse_single_v() {
        let a = parse_attributes("V").unwrap();
        assert!(a.is_virtual());
        assert!(!a.is_quiet());
        assert!(!a.is_no_exec());
        assert!(!a.is_unexported());
        assert!(!a.is_delete_on_error());
        assert!(!a.is_exclusive());
        assert!(!a.has_comparison());
        assert!(!a.is_regex());
        assert!(!a.is_no_virtual());
        assert_eq!(a.bits(), Attributes::VIRTUAL);
    }

    #[test]
    fn parse_quiet() {
        let a = parse_attributes("Q").unwrap();
        assert!(a.is_quiet());
        assert!(!a.is_virtual());
        assert_eq!(a.bits(), Attributes::QUIET);
    }

    #[test]
    fn parse_virtual_quiet() {
        let a = parse_attributes("VQ").unwrap();
        assert!(a.is_virtual());
        assert!(a.is_quiet());
        assert!(!a.is_no_exec());
        assert_eq!(a.bits(), Attributes::VIRTUAL | Attributes::QUIET);
    }

    #[test]
    fn parse_all_attrs() {
        let a = parse_attributes("VQNUDPERn").unwrap();
        assert!(a.is_virtual());
        assert!(a.is_quiet());
        assert!(a.is_no_exec());
        assert!(a.is_unexported());
        assert!(a.is_delete_on_error());
        assert!(a.is_exclusive());
        assert!(a.has_comparison());
        assert!(a.is_regex());
        assert!(a.is_no_virtual());
        assert_eq!(
            a.bits(),
            Attributes::VIRTUAL
                | Attributes::QUIET
                | Attributes::NO_EXEC
                | Attributes::UNEXPORTED
                | Attributes::DELETE_ON_ERROR
                | Attributes::EXCLUSIVE
                | Attributes::COMPARISON
                | Attributes::REGEX
                | Attributes::NO_VIRTUAL
        );
    }

    #[test]
    fn parse_unknown_attr() {
        let result = parse_attributes("X");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ParseAttrError::UnknownAttr('X'));
    }

    #[test]
    fn parse_unknown_in_middle() {
        let result = parse_attributes("VXQ");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ParseAttrError::UnknownAttr('X'));
    }

    #[test]
    fn parse_empty_string() {
        let a = parse_attributes("").unwrap();
        assert!(a.is_empty());
        assert_eq!(a.bits(), 0);
    }

    #[test]
    fn default_is_empty() {
        assert!(Attributes::default().is_empty());
    }

    #[test]
    fn builder_with_virtual() {
        let a = Attributes::new().with(Attributes::VIRTUAL);
        assert!(a.is_virtual());
        assert!(!a.is_quiet());
    }

    #[test]
    fn builder_chain() {
        let a = Attributes::new()
            .with(Attributes::VIRTUAL)
            .with(Attributes::QUIET);
        assert!(a.is_virtual());
        assert!(a.is_quiet());
    }

    #[test]
    fn bits_roundtrip() {
        let a = parse_attributes("VQN").unwrap();
        let expected = Attributes::VIRTUAL | Attributes::QUIET | Attributes::NO_EXEC;
        assert_eq!(a.bits() & expected, expected);
    }

    #[test]
    fn attr_help_has_all_9() {
        assert_eq!(ATTR_HELP.len(), 9);
    }
}
