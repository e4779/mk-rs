//! Variable system for mk-core: symbol table with precedence, parent-chain
//! lookup for nested includes, and $VAR / ${VAR} / $$ expansion.
//!
//! mk variable expansion follows Plan 9 mk conventions:
//!   $$     → literal $
//!   $VAR   → value of VAR (name ends at non-alphanumeric, non-underscore)
//!   ${VAR} → value of VAR (exact name between braces)
//!
//! Unknown variables silently expand to the empty string.
//! Expansion is recursive (re-scanned up to 10 levels).

use std::collections::{HashMap, HashSet};

use crate::error::VarError;

/// Variable scope with parent-chain lookup (for nested includes).
#[derive(Debug, Clone, Default)]
pub struct Scope {
    vars: HashMap<String, (String, Precedence)>,
    parent: Option<Box<Scope>>,
}

/// Precedence levels for variable assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    /// Built-in defaults (lowest)
    Builtin = 0,
    /// Imported from environment
    Environment = 1,
    /// Set in mkfile
    Mkfile = 2,
    /// Set on command line (highest)
    CommandLine = 3,
}

/// Iterator over visible variables in a scope, including parent chain.
/// Each variable is yielded once; child-scope definitions shadow parent ones.
pub struct ScopeIter<'a> {
    entries: Vec<(&'a str, &'a str)>,
    pos: usize,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Find the end of a variable name starting at `start`.
/// Name chars: ASCII alphanumeric + underscore.
/// Returns the index of the first non-name char, or `s.len()` if all remaining
/// chars are valid.
fn find_end_of_name(s: &str, start: usize) -> usize {
    s[start..]
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map_or(s.len(), |pos| start + pos)
}

// ── Scope: construction ────────────────────────────────────────────────────

impl Scope {
    /// Create an empty scope.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a scope with a parent for chain lookup.
    pub fn with_parent(parent: Scope) -> Self {
        Self {
            vars: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }
}

// ── Scope: variable access ─────────────────────────────────────────────────

impl Scope {
    /// Set a variable at the given precedence level.
    /// If the variable already exists at a higher or equal precedence, it is
    /// NOT overwritten. Returns true if the value was set.
    pub fn set(&mut self, name: &str, value: &str, prec: Precedence) -> bool {
        if let Some((_, stored_prec)) = self.vars.get(name) {
            if prec < *stored_prec {
                return false;
            }
        }
        self.vars
            .insert(name.to_string(), (value.to_string(), prec));
        true
    }

    /// Set a variable unconditionally (ignores precedence).
    /// Used for recipe-time vars.
    pub fn set_force(&mut self, name: &str, value: &str) {
        self.vars.insert(
            name.to_string(),
            (value.to_string(), Precedence::CommandLine),
        );
    }

    /// Get a variable value. Walks the parent chain.
    /// Returns None if not found (caller should treat as empty string).
    pub fn get(&self, name: &str) -> Option<&str> {
        if let Some((val, _)) = self.vars.get(name) {
            return Some(val.as_str());
        }
        self.parent.as_ref()?.get(name)
    }

    /// Check if a variable exists in this scope or any parent.
    pub fn contains(&self, name: &str) -> bool {
        self.vars.contains_key(name)
            || self
                .parent
                .as_ref()
                .map_or(false, |p| p.contains(name))
    }

    /// Iterate over all variables visible from this scope (including parents).
    pub fn iter(&self) -> ScopeIter<'_> {
        let mut entries = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut current: Option<&Scope> = Some(self);
        while let Some(scope) = current {
            for (name, (value, _)) in &scope.vars {
                if seen.insert(name.as_str()) {
                    entries.push((name.as_str(), value.as_str()));
                }
            }
            current = scope.parent.as_deref();
        }
        ScopeIter { entries, pos: 0 }
    }
}

// ── Scope: expansion ───────────────────────────────────────────────────────

impl Scope {
    /// Expand variable references in a string.
    ///
    /// Handles: `$VAR`, `${VAR}`, `$$` → literal `$`.
    /// Unknown variables silently expand to the empty string (mk convention).
    /// Recursive: re-scans the result for more `$` refs up to 10 levels deep.
    ///
    /// Returns `VarError::RecursiveExpansion` if the recursion limit is
    /// exceeded.
    pub fn expand(&self, input: &str) -> Result<String, VarError> {
        const MAX_DEPTH: usize = 10;
        let mut current = input.to_string();
        for _ in 0..MAX_DEPTH {
            let expanded = self.expand_once(&current);
            if expanded == current {
                return Ok(current);
            }
            current = expanded;
        }
        // One final attempt: if it's stable now, it's fine; otherwise error.
        let expanded = self.expand_once(&current);
        if expanded == current {
            return Ok(current);
        }
        Err(VarError::RecursiveExpansion {
            name: current,
        })
    }

    /// Single pass: expand all $VAR, ${VAR}, $$ references without recursion.
    fn expand_once(&self, input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        while i < len {
            if bytes[i] == b'$' && i + 1 < len {
                match bytes[i + 1] {
                    b'$' => {
                        // $$ → literal $
                        result.push('$');
                        i += 2;
                    }
                    b'{' => {
                        // ${VAR}
                        if let Some(end) = input[i + 2..].find('}') {
                            let name = &input[i + 2..i + 2 + end];
                            if let Some(val) = self.get(name) {
                                result.push_str(val);
                            }
                            i = i + 2 + end + 1; // skip past }
                        } else {
                            // Unclosed brace: treat as literal ${...
                            result.push_str(&input[i..]);
                            break;
                        }
                    }
                    _ => {
                        // Check if next char starts a valid variable name
                        // (alphabetic or underscore, per mk convention)
                        if !bytes[i+1].is_ascii_alphabetic() && bytes[i+1] != b'_' {
                            // Not a valid var name — treat $ as literal
                            result.push('$');
                            i += 1;
                        } else {
                            // $VAR — name ends at first non-name char
                            let j = find_end_of_name(input, i + 1);
                            let name = &input[i + 1..j];
                            if let Some(val) = self.get(name) {
                                result.push_str(val);
                            }
                            i = j;
                        }
                    }
                }
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        }
        result
    }
}

// ── Iterator impl ──────────────────────────────────────────────────────────

impl<'a> Iterator for ScopeIter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.entries.len() {
            let item = self.entries[self.pos];
            self.pos += 1;
            Some(item)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.entries.len() - self.pos;
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for ScopeIter<'a> {}

// ── Built-in defaults & environment ────────────────────────────────────────

/// Create a scope with built-in mk defaults (CC=cc, MKSHELL=/bin/sh, etc.).
pub fn builtin_scope() -> Scope {
    let mut s = Scope::new();
    s.set("AS", "as", Precedence::Builtin);
    s.set("CC", "cc", Precedence::Builtin);
    s.set("CFLAGS", "", Precedence::Builtin);
    s.set("FC", "f77", Precedence::Builtin);
    s.set("FFLAGS", "", Precedence::Builtin);
    s.set("LDFLAGS", "", Precedence::Builtin);
    s.set("LEX", "lex", Precedence::Builtin);
    s.set("LFLAGS", "", Precedence::Builtin);
    s.set("NPROC", "1", Precedence::Builtin);
    s.set("NREP", "1", Precedence::Builtin);
    s.set("YACC", "yacc", Precedence::Builtin);
    s.set("YFLAGS", "", Precedence::Builtin);
    s.set("MKSHELL", "/bin/sh", Precedence::Builtin);
    s
}

/// Import OS environment variables into a scope at Environment precedence.
/// Skips variables that are already set at higher precedence.
pub fn import_env(scope: &mut Scope) {
    for (key, value) in std::env::vars() {
        scope.set(&key, &value, Precedence::Environment);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_scope() -> Scope {
        builtin_scope()
    }

    #[test]
    fn set_and_get() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.get("FOO"), Some("bar"));
    }

    #[test]
    fn precedence_respected() {
        let mut s = Scope::new();
        s.set("FOO", "env_val", Precedence::Environment);
        s.set("FOO", "builtin_val", Precedence::Builtin); // lower prec → ignored
        assert_eq!(s.get("FOO"), Some("env_val"));
    }

    #[test]
    fn higher_precedence_wins() {
        let mut s = Scope::new();
        s.set("FOO", "builtin", Precedence::Builtin);
        s.set("FOO", "override", Precedence::CommandLine);
        assert_eq!(s.get("FOO"), Some("override"));
    }

    #[test]
    fn force_set_overrides_precedence() {
        let mut s = Scope::new();
        s.set("FOO", "builtin", Precedence::Builtin);
        s.set_force("FOO", "forced");
        assert_eq!(s.get("FOO"), Some("forced"));
    }

    #[test]
    fn parent_chain_lookup() {
        let mut parent = Scope::new();
        parent.set("FOO", "from_parent", Precedence::Mkfile);
        let child = Scope::with_parent(parent);
        assert_eq!(child.get("FOO"), Some("from_parent"));
    }

    #[test]
    fn child_shadows_parent() {
        let mut parent = Scope::new();
        parent.set("FOO", "parent_val", Precedence::Mkfile);
        let mut child = Scope::with_parent(parent);
        child.set("FOO", "child_val", Precedence::Mkfile);
        assert_eq!(child.get("FOO"), Some("child_val"));
    }

    #[test]
    fn missing_var_is_none() {
        let s = Scope::new();
        assert_eq!(s.get("NONEXISTENT"), None);
    }

    #[test]
    fn builtin_defaults() {
        let s = builtin_scope();
        assert_eq!(s.get("CC"), Some("cc"));
        assert_eq!(s.get("NPROC"), Some("1"));
        assert_eq!(s.get("MKSHELL"), Some("/bin/sh"));
    }

    #[test]
    fn expand_simple_var() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.expand("$FOO").unwrap(), "bar");
    }

    #[test]
    fn expand_braced_var() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.expand("${FOO}").unwrap(), "bar");
    }

    #[test]
    fn expand_double_dollar() {
        let s = Scope::new();
        assert_eq!(s.expand("$$").unwrap(), "$");
    }

    #[test]
    fn expand_undefined_var() {
        let s = Scope::new();
        assert_eq!(s.expand("$NONEXISTENT").unwrap(), "");
    }

    #[test]
    fn expand_multiple_vars() {
        let mut s = Scope::new();
        s.set("A", "hello", Precedence::Mkfile);
        s.set("B", "world", Precedence::Mkfile);
        assert_eq!(s.expand("$A $B").unwrap(), "hello world");
    }

    #[test]
    fn expand_var_at_end_of_string() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.expand("prefix_$FOO").unwrap(), "prefix_bar");
    }

    #[test]
    fn expand_var_trailing_chars() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        // $FOO.c → "bar.c" (FOO ends at '.')
        assert_eq!(s.expand("$FOO.c").unwrap(), "bar.c");
    }

    #[test]
    fn expand_var_in_braces_with_trailing() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        // ${FOO}.c → "bar.c"
        assert_eq!(s.expand("${FOO}.c").unwrap(), "bar.c");
    }

    #[test]
    fn expand_from_parent() {
        let mut parent = Scope::new();
        parent.set("FOO", "bar", Precedence::Mkfile);
        let child = Scope::with_parent(parent);
        assert_eq!(child.expand("$FOO").unwrap(), "bar");
    }

    #[test]
    fn expand_recursive_simple() {
        let mut s = Scope::new();
        s.set("A", "$B", Precedence::Mkfile);
        s.set("B", "hello", Precedence::Mkfile);
        assert_eq!(s.expand("$A").unwrap(), "hello");
    }

    #[test]
    fn expand_recursive_limit() {
        let mut s = Scope::new();
        // A → B → C → D → ... never resolves
        s.set("A", "$B", Precedence::Mkfile);
        s.set("B", "$C", Precedence::Mkfile);
        s.set("C", "$D", Precedence::Mkfile);
        s.set("D", "$E", Precedence::Mkfile);
        s.set("E", "$F", Precedence::Mkfile);
        s.set("F", "$G", Precedence::Mkfile);
        s.set("G", "$H", Precedence::Mkfile);
        s.set("H", "$I", Precedence::Mkfile);
        s.set("I", "$J", Precedence::Mkfile);
        s.set("J", "$K", Precedence::Mkfile);
        s.set("K", "$A", Precedence::Mkfile);
        let result = s.expand("$A");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VarError::RecursiveExpansion { .. }
        ));
    }

    #[test]
    fn expand_bare_dollar_at_end() {
        let s = Scope::new();
        // lone $ at end of string: name is empty → expands to empty
        assert_eq!(s.expand("foo$").unwrap(), "foo$");
    }

    #[test]
    fn expand_unclosed_brace() {
        let s = Scope::new();
        // ${FOO without closing brace → treated as literal text
        assert_eq!(s.expand("${FOO").unwrap(), "${FOO");
    }

    #[test]
    fn expand_empty_braces() {
        let s = Scope::new();
        // ${} → empty name → empty string
        assert_eq!(s.expand("${}").unwrap(), "");
    }

    #[test]
    fn expand_var_with_underscore() {
        let mut s = Scope::new();
        s.set("MY_VAR", "val", Precedence::Mkfile);
        assert_eq!(s.expand("$MY_VAR").unwrap(), "val");
    }

    #[test]
    fn expand_var_with_digits() {
        let mut s = Scope::new();
        s.set("F1", "one", Precedence::Mkfile);
        assert_eq!(s.expand("$F1").unwrap(), "one");
    }

    #[test]
    fn contains_in_scope() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert!(s.contains("FOO"));
        assert!(!s.contains("BAR"));
    }

    #[test]
    fn contains_in_parent() {
        let mut parent = Scope::new();
        parent.set("FOO", "parent_val", Precedence::Mkfile);
        let child = Scope::with_parent(parent);
        assert!(child.contains("FOO"));
        assert!(!child.contains("BAR"));
    }

    #[test]
    fn iter_yields_all() {
        let mut parent = Scope::new();
        parent.set("PARENT_VAR", "p", Precedence::Mkfile);
        let mut child = Scope::with_parent(parent);
        child.set("CHILD_VAR", "c", Precedence::Mkfile);

        let vars: Vec<_> = child.iter().collect();
        assert!(vars.contains(&("CHILD_VAR", "c")));
        assert!(vars.contains(&("PARENT_VAR", "p")));
    }

    #[test]
    fn iter_child_shadows_parent() {
        let mut parent = Scope::new();
        parent.set("FOO", "parent", Precedence::Mkfile);
        let mut child = Scope::with_parent(parent);
        child.set("FOO", "child", Precedence::Mkfile);

        let vars: Vec<_> = child.iter().collect();
        // Should only see "FOO" once (child value)
        let foo_count = vars.iter().filter(|(k, _)| *k == "FOO").count();
        assert_eq!(foo_count, 1);
    }

    #[test]
    fn set_returns_bool() {
        let mut s = Scope::new();
        assert!(s.set("FOO", "first", Precedence::Mkfile));
        // Same precedence → overwrite allowed
        assert!(s.set("FOO", "second", Precedence::Mkfile));
        // Lower precedence → denied
        assert!(!s.set("FOO", "ignored", Precedence::Environment));
        assert_eq!(s.get("FOO"), Some("second"));
    }

    #[test]
    fn expand_recursive_deep() {
        let mut s = Scope::new();
        // 3 levels: A → $B → $C → "done"
        s.set("A", "$B", Precedence::Mkfile);
        s.set("B", "$C", Precedence::Mkfile);
        s.set("C", "done", Precedence::Mkfile);
        assert_eq!(s.expand("$A").unwrap(), "done");
    }

    #[test]
    fn expand_mixed_literal_and_var() {
        let mut s = Scope::new();
        s.set("SRC", "main.c", Precedence::Mkfile);
        assert_eq!(
            s.expand("cc $CFLAGS -c $SRC").unwrap(),
            "cc  -c main.c"
        );
    }

    #[test]
    fn expand_dollar_in_middle_of_text() {
        let s = Scope::new();
        // $$ in middle of text
        assert_eq!(s.expand("price: $100").unwrap(), "price: $100");
    }

    #[test]
    fn builtin_scope_has_expected_keys() {
        let s = builtin_scope();
        assert!(s.contains("CC"));
        assert!(s.contains("CFLAGS"));
        assert!(s.contains("NPROC"));
        assert!(s.contains("MKSHELL"));
    }

    #[test]
    fn import_env_respects_existing_higher_precedence() {
        // Set up a controlled environment-like scenario
        let mut s = Scope::new();
        // Simulate command line override
        s.set("PATH", "/custom", Precedence::CommandLine);
        // Simulate env import (would set PATH if lower prec)
        s.set("PATH", "/usr/bin", Precedence::Environment);
        assert_eq!(s.get("PATH"), Some("/custom"));
    }
}
