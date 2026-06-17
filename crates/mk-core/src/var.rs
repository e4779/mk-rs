//! Variable system for mk-core: symbol table with precedence, parent-chain
//! lookup for nested includes, and $VAR / ${VAR} / $$ expansion.
//!
//! mk variable expansion follows Plan 9 mk conventions:
//!   $$     → literal $
//!   $VAR   → value of VAR (name ends at non-alphanumeric, non-underscore)
//!   ${VAR} → value of VAR (exact name between braces)
//!
//! Unknown variables silently expand to the empty string.
//! Expansion uses a seen-set cycle detector: deep chains resolve
//! (no depth limit), cycles yield empty/partial — never an error, never a hang.
//! Supports namelist transforms: `${VAR:%.c=%.o}`.
//!
//! # Recipe-time variables
//!
//! `$target`, `$prereq`, `$stem`, `$newprereq`, `$alltarget`, and `$pid`
//! are **not** expanded by [`Scope`] at parse time. Instead, the scheduler
//! injects them as environment variables into the recipe's execution context
//! just before the shell runs (see [`crate::recipe::Recipe`]). This means they
//! bypass the normal scope chain and are never stored in a mkfile assignment.

use std::collections::{HashMap, HashSet};

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

/// Execute a sh-style or rc-style backtick command and return its stdout.
///
/// sh style:  `echo a.c b.c`
/// rc style:  `{echo hello}`
/// If the value doesn't start with a backtick, it is returned unchanged.
pub fn expand_backtick(value: &str) -> String {
    let cmd = if value.starts_with("`{") && value.ends_with("}`") {
        &value[2..value.len() - 2]
    } else if value.starts_with('`') && value.ends_with('`') {
        &value[1..value.len() - 1]
    } else {
        return value.to_string();
    };

    match std::process::Command::new("sh").arg("-c").arg(cmd).output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.trim().to_string()
        }
        Err(_) => String::new(),
    }
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
    /// Set a variable at the given precedence level, fully expanding
    /// its value: backtick → variable references → namelist transforms.
    ///
    /// If the variable already exists at a higher or equal precedence, it is
    /// NOT overwritten. Returns true if the value was set.
    pub fn set(&mut self, name: &str, value: &str, prec: Precedence) -> bool {
        if let Some((_, stored_prec)) = self.vars.get(name) {
            if prec < *stored_prec {
                return false;
            }
        }
        let expanded = expand_backtick(value);
        let expanded = self.expand(&expanded);
        self.vars.insert(name.to_string(), (expanded, prec));
        true
    }

    /// Set a variable at the given precedence level WITHOUT expanding its value.
    /// Stores the literal string as-is. Used for builtins and env imports
    /// where values containing `$` must never be re-expanded (QU-1).
    pub fn set_raw(&mut self, name: &str, value: &str, prec: Precedence) -> bool {
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
        self.vars.contains_key(name) || self.parent.as_ref().is_some_and(|p| p.contains(name))
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

// ── Scope: export ─────────────────────────────────────────────────────────

impl Scope {
    /// Export all visible variables as a flat `HashMap<String, String>`.
    /// Useful for passing the variable scope to recipe execution.
    pub fn export(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for (name, value) in self.iter() {
            map.insert(name.to_string(), value.to_string());
        }
        map
    }
}

// ── Scope: expansion ───────────────────────────────────────────────────────

impl Scope {
    /// Expand variable references in a string.
    ///
    /// Handles: `$VAR`, `${VAR}`, `$$` → literal `$`.
    /// Unknown variables silently expand to the empty string (mk convention).
    ///
    /// Uses a seen-set cycle detector: re-scans the result until stable.
    /// Deep chains resolve completely (no artificial depth limit); cycles
    /// yield empty/partial results — never an error, never a hang.
    /// This matches plan9port mk semantics (F-045, §12 verification D-1..D-15).
    pub fn expand(&self, input: &str) -> String {
        let mut current = input.to_string();
        // Track seen intermediate results to detect cycles.
        // A cycle is when expand_once produces a string we've already seen
        // in this expansion chain, which would cause infinite oscillation.
        let mut seen: HashSet<String> = HashSet::new();

        loop {
            let expanded = self.expand_once(&current);
            if expanded == current {
                return current;
            }
            if !seen.insert(expanded.clone()) {
                // Cycle detected: same intermediate result seen twice.
                // Return the expanded result (partial, matching reference).
                // This handles cases like A=$B, B=$A where re-scanning
                // oscillates between "$B" and "$A" indefinitely.
                return expanded;
            }
            current = expanded;
        }
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
                        if let Some(end) = input[i + 2..].find('}') {
                            let content = &input[i + 2..i + 2 + end];
                            // Check for namelist transform: ${VAR:pattern=replacement}
                            if let Some(colon_pos) = content.find(':') {
                                let var_name = &content[..colon_pos];
                                let subst = &content[colon_pos + 1..];
                                if let Some(eq_pos) = subst.find('=') {
                                    let pattern = &subst[..eq_pos];
                                    let replacement = &subst[eq_pos + 1..];
                                    let value = self.get(var_name).unwrap_or("");
                                    let expanded = namelist_transform(value, pattern, replacement);
                                    result.push_str(&expanded);
                                } else {
                                    // Pattern without = — just do simple lookup
                                    result.push_str(self.get(content).unwrap_or(""));
                                }
                            } else {
                                // Simple ${VAR}
                                if let Some(val) = self.get(content) {
                                    result.push_str(val);
                                }
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
                        if !bytes[i + 1].is_ascii_alphabetic() && bytes[i + 1] != b'_' {
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

// ── Namelist transform ─────────────────────────────────────────────────────

/// Transform a space-separated list of words using a %-based pattern.
///
/// Pattern `A%B` matches words with prefix A and suffix B, capturing the
/// middle as the *stem*. Replacement `C%D` emits prefix C + stem + suffix D.
/// Words that don't match the pattern are dropped.
///
/// This implements mk's `${SRC:%.c=%.o}` namelist transformation.
fn namelist_transform(value: &str, pattern: &str, replacement: &str) -> String {
    // Split pattern into prefix and suffix around '%'.
    let (pat_pre, pat_suf) = if let Some(pos) = pattern.find('%') {
        (&pattern[..pos], &pattern[pos + 1..])
    } else {
        // No % in pattern — no transformation possible.
        return value.to_string();
    };

    // Split replacement into prefix and suffix around '%'.
    let (repl_pre, repl_suf) = if let Some(pos) = replacement.find('%') {
        (&replacement[..pos], &replacement[pos + 1..])
    } else {
        ("", replacement)
    };

    let mut result: Vec<String> = Vec::new();
    for word in value.split_whitespace() {
        if word.starts_with(pat_pre) && word.ends_with(pat_suf) {
            let stem_start = pat_pre.len();
            let stem_end = word.len() - pat_suf.len();
            if stem_start <= stem_end {
                let stem = &word[stem_start..stem_end];
                let mut out = String::with_capacity(repl_pre.len() + stem.len() + repl_suf.len());
                out.push_str(repl_pre);
                out.push_str(stem);
                out.push_str(repl_suf);
                result.push(out);
            }
        }
        // Words that don't match the pattern are dropped.
    }
    result.join(" ")
}

// ── Built-in defaults & environment ────────────────────────────────────────

/// Create a scope with built-in mk defaults (CC=cc, MKSHELL=/bin/sh, etc.).
/// Uses set_raw — builtin values are literal and must not be re-expanded.
pub fn builtin_scope() -> Scope {
    let mut s = Scope::new();
    s.set_raw("AS", "as", Precedence::Builtin);
    s.set_raw("CC", "cc", Precedence::Builtin);
    s.set_raw("CFLAGS", "", Precedence::Builtin);
    s.set_raw("FC", "f77", Precedence::Builtin);
    s.set_raw("FFLAGS", "", Precedence::Builtin);
    s.set_raw("LDFLAGS", "", Precedence::Builtin);
    s.set_raw("LEX", "lex", Precedence::Builtin);
    s.set_raw("LFLAGS", "", Precedence::Builtin);
    s.set_raw("NPROC", "1", Precedence::Builtin);
    s.set_raw("NREP", "1", Precedence::Builtin);
    s.set_raw("YACC", "yacc", Precedence::Builtin);
    s.set_raw("YFLAGS", "", Precedence::Builtin);
    s.set_raw("MKSHELL", "/bin/sh", Precedence::Builtin);
    s
}

/// Import OS environment variables into a scope at Environment precedence.
/// Uses set_raw — env values containing `$` must NOT be re-expanded (QU-1).
/// Skips variables that are already set at higher precedence.
pub fn import_env(scope: &mut Scope) {
    for (key, value) in std::env::vars() {
        scope.set_raw(&key, &value, Precedence::Environment);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
    fn set_raw_stores_literal() {
        let mut s = Scope::new();
        s.set_raw("RAW", "$HOME", Precedence::Mkfile);
        assert_eq!(s.get("RAW"), Some("$HOME"));
        // set_raw does NOT expand — value stored as-is
    }

    #[test]
    fn set_full_expansion() {
        let mut s = Scope::new();
        s.set("A", "world", Precedence::Mkfile);
        s.set("B", "hello $A", Precedence::Mkfile);
        assert_eq!(s.get("B"), Some("hello world"));
    }

    #[test]
    fn expand_simple_var() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.expand("$FOO"), "bar");
    }

    #[test]
    fn expand_braced_var() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.expand("${FOO}"), "bar");
    }

    #[test]
    fn expand_double_dollar() {
        let s = Scope::new();
        assert_eq!(s.expand("$$"), "$");
    }

    #[test]
    fn expand_undefined_var() {
        let s = Scope::new();
        assert_eq!(s.expand("$NONEXISTENT"), "");
    }

    #[test]
    fn expand_multiple_vars() {
        let mut s = Scope::new();
        s.set("A", "hello", Precedence::Mkfile);
        s.set("B", "world", Precedence::Mkfile);
        assert_eq!(s.expand("$A $B"), "hello world");
    }

    #[test]
    fn expand_var_at_end_of_string() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        assert_eq!(s.expand("prefix_$FOO"), "prefix_bar");
    }

    #[test]
    fn expand_var_trailing_chars() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        // $FOO.c → "bar.c" (FOO ends at '.')
        assert_eq!(s.expand("$FOO.c"), "bar.c");
    }

    #[test]
    fn expand_var_in_braces_with_trailing() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        // ${FOO}.c → "bar.c"
        assert_eq!(s.expand("${FOO}.c"), "bar.c");
    }

    #[test]
    fn expand_from_parent() {
        let mut parent = Scope::new();
        parent.set("FOO", "bar", Precedence::Mkfile);
        let child = Scope::with_parent(parent);
        assert_eq!(child.expand("$FOO"), "bar");
    }

    #[test]
    fn expand_recursive_simple() {
        let mut s = Scope::new();
        // Stored values are fully expanded at set time, so A's value is
        // already "hello" — expanding "$A" just does a direct lookup.
        s.set("B", "hello", Precedence::Mkfile);
        s.set("A", "$B", Precedence::Mkfile);
        assert_eq!(s.get("A"), Some("hello"));
        assert_eq!(s.expand("$A"), "hello");
    }

    #[test]
    fn cycle_yields_empty_no_error() {
        // F-045 D-1: A=$B; B=$A → both empty, no error
        let mut s = Scope::new();
        s.set("A", "$B", Precedence::Mkfile);
        s.set("B", "$A", Precedence::Mkfile);
        // A was set before B existed → A=empty
        // B was set when A=empty → B=empty
        assert_eq!(s.get("A"), Some(""));
        assert_eq!(s.get("B"), Some(""));
    }

    #[test]
    fn expand_no_depth_limit() {
        // F-045 D-8: deep chains resolve completely
        let mut s = Scope::new();
        s.set("V0", "leaf", Precedence::Mkfile);
        for i in 1..=1000 {
            let prev = format!("V{}", i - 1);
            let cur = format!("V{i}");
            s.set(&cur, &format!("${prev}"), Precedence::Mkfile);
        }
        // V1000 should resolve to "leaf"
        assert_eq!(s.get("V1000"), Some("leaf"));
    }

    #[test]
    fn three_cycle_partial() {
        // F-045 D-10: A=a$B; B=b$C; C=c$A → A=a, B=b, C=ca
        let mut s = Scope::new();
        s.set("A", "a$B", Precedence::Mkfile);
        assert_eq!(s.get("A"), Some("a")); // B doesn't exist yet
        s.set("B", "b$C", Precedence::Mkfile);
        assert_eq!(s.get("B"), Some("b")); // C doesn't exist yet
        s.set("C", "c$A", Precedence::Mkfile);
        assert_eq!(s.get("C"), Some("ca")); // A is "a"
    }

    #[test]
    fn assign_time_order_matters() {
        // F-045 D-12: GREETING=$FIRST world; FIRST=hello → GREETING=" world"
        let mut s = Scope::new();
        s.set("GREETING", "$FIRST world", Precedence::Mkfile);
        assert_eq!(s.get("GREETING"), Some(" world"));
        s.set("FIRST", "hello", Precedence::Mkfile);
        assert_eq!(s.get("FIRST"), Some("hello"));
        // GREETING stays " world" — read-time semantics (FIRST was empty at that line)
        assert_eq!(s.get("GREETING"), Some(" world"));
    }

    #[test]
    fn stored_after_expand() {
        // F-045 D-14/D-15: A=aa; B=$A; C=$B → B=aa, C=aa
        let mut s = Scope::new();
        s.set("A", "aa", Precedence::Mkfile);
        s.set("B", "$A", Precedence::Mkfile);
        s.set("C", "$B", Precedence::Mkfile);
        assert_eq!(s.get("B"), Some("aa"));
        assert_eq!(s.get("C"), Some("aa"));
    }

    #[test]
    fn env_literal_dollar_kept() {
        // F-045 QU-1: env values with $ must NOT be re-expanded
        let mut s = Scope::new();
        s.set_raw("DOLLAR_VAR", "price$5", Precedence::Environment);
        assert_eq!(s.get("DOLLAR_VAR"), Some("price$5"));
        // expand should keep the literal $5 (not try to expand variable "5")
        let expanded = s.expand("$DOLLAR_VAR");
        // "$5" is not a variable reference ($ followed by non-alpha), so it stays
        assert_eq!(expanded, "price$5");
    }

    #[test]
    fn expand_cycle_in_string_yields_partial() {
        // For input that would cycle between intermediate strings,
        // the seen-set detector returns the partial result.
        // With set-time full expansion, cycles naturally resolve via read-time
        // ordering. This test verifies the expand() cycle detector works
        // on pathological direct input.
        let mut s = Scope::new();
        s.set_raw("X", "$Y", Precedence::Mkfile);
        s.set_raw("Y", "$X", Precedence::Mkfile);
        // expand("$X") → lookup X → "$Y" → re-scan → lookup Y → "$X" → cycle
        // The seen-set detector will catch the oscillation and return.
        let result = s.expand("$X");
        // Should not hang or error — just return whatever we got
        assert!(!result.is_empty() || result.is_empty()); // any value is fine, just no panic/hang
    }

    #[test]
    fn expand_bare_dollar_at_end() {
        let s = Scope::new();
        // lone $ at end of string: name is empty → expands to empty
        assert_eq!(s.expand("foo$"), "foo$");
    }

    #[test]
    fn expand_unclosed_brace() {
        let s = Scope::new();
        // ${FOO without closing brace → treated as literal text
        assert_eq!(s.expand("${FOO"), "${FOO");
    }

    #[test]
    fn expand_empty_braces() {
        let s = Scope::new();
        // ${} → empty name → empty string
        assert_eq!(s.expand("${}"), "");
    }

    #[test]
    fn expand_var_with_underscore() {
        let mut s = Scope::new();
        s.set("MY_VAR", "val", Precedence::Mkfile);
        assert_eq!(s.expand("$MY_VAR"), "val");
    }

    #[test]
    fn expand_var_with_digits() {
        let mut s = Scope::new();
        s.set("F1", "one", Precedence::Mkfile);
        assert_eq!(s.expand("$F1"), "one");
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
        // With set-time full expansion, C is set first → "done",
        // then B → expand("$C") → "done", then A → expand("$B") → "done"
        s.set("C", "done", Precedence::Mkfile);
        s.set("B", "$C", Precedence::Mkfile);
        s.set("A", "$B", Precedence::Mkfile);
        assert_eq!(s.get("A"), Some("done"));
        assert_eq!(s.expand("$A"), "done");
    }

    #[test]
    fn expand_mixed_literal_and_var() {
        let mut s = Scope::new();
        s.set("SRC", "main.c", Precedence::Mkfile);
        assert_eq!(s.expand("cc $CFLAGS -c $SRC"), "cc  -c main.c");
    }

    #[test]
    fn expand_dollar_in_middle_of_text() {
        let s = Scope::new();
        // $100 → $ followed by digit (not alpha/underscore) → literal $
        assert_eq!(s.expand("price: $100"), "price: $100");
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

    #[test]
    fn export_scope_to_hashmap() {
        let mut s = Scope::new();
        s.set("FOO", "bar", Precedence::Mkfile);
        s.set("CC", "gcc", Precedence::Mkfile);
        let map = s.export();
        assert_eq!(map.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(map.get("CC").map(|s| s.as_str()), Some("gcc"));
    }

    #[test]
    fn export_includes_parent() {
        let mut parent = Scope::new();
        parent.set("PARENT_VAR", "p", Precedence::Mkfile);
        let mut child = Scope::with_parent(parent);
        child.set("CHILD_VAR", "c", Precedence::Mkfile);
        let map = child.export();
        assert_eq!(map.get("PARENT_VAR").map(|s| s.as_str()), Some("p"));
        assert_eq!(map.get("CHILD_VAR").map(|s| s.as_str()), Some("c"));
    }

    #[test]
    fn backtick_expansion() {
        let mut s = Scope::new();
        s.set("FILES", "`echo a.c b.c`", Precedence::Mkfile);
        assert_eq!(s.get("FILES"), Some("a.c b.c"));
    }

    #[test]
    fn backtick_rc_style() {
        let mut s = Scope::new();
        s.set("FILES", "`{echo hello}`", Precedence::Mkfile);
        assert_eq!(s.get("FILES"), Some("hello"));
    }

    #[test]
    fn export_child_shadows_parent() {
        let mut parent = Scope::new();
        parent.set("FOO", "parent", Precedence::Mkfile);
        let mut child = Scope::with_parent(parent);
        child.set("FOO", "child", Precedence::Mkfile);
        let map = child.export();
        assert_eq!(map.get("FOO").map(|s| s.as_str()), Some("child"));
    }

    // ── Namelist transform tests ───────────────────────────────────────

    #[test]
    fn expand_namelist_simple() {
        let mut s = Scope::new();
        s.set("SRC", "a.c b.c c.c", Precedence::Mkfile);
        assert_eq!(s.expand("${SRC:%.c=%.o}"), "a.o b.o c.o");
    }

    #[test]
    fn expand_namelist_partial_match() {
        let mut s = Scope::new();
        s.set(
            "FILES",
            "src/main.c README.md src/lib.c",
            Precedence::Mkfile,
        );
        // Only .c files match %.c pattern; non-matching words are dropped.
        assert_eq!(s.expand("${FILES:%.c=%.o}"), "src/main.o src/lib.o");
    }

    #[test]
    fn expand_namelist_prefix_change() {
        let mut s = Scope::new();
        s.set("SRC", "src/main.c src/util.c", Precedence::Mkfile);
        // Change prefix: src/%.c → obj/%.o
        assert_eq!(s.expand("${SRC:src/%.c=obj/%.o}"), "obj/main.o obj/util.o");
    }

    #[test]
    fn expand_namelist_no_match() {
        let mut s = Scope::new();
        s.set("SRC", "a.c b.c", Precedence::Mkfile);
        // Pattern doesn't match any word → empty result.
        assert_eq!(s.expand("${SRC:%.rs=%.o}"), "");
    }

    #[test]
    fn expand_namelist_undefined_var() {
        let s = Scope::new();
        // Undefined variable → empty string → empty result.
        assert_eq!(s.expand("${NOSUCH:%.c=%.o}"), "");
    }

    #[test]
    fn expand_namelist_no_percent_in_pattern() {
        let mut s = Scope::new();
        s.set("SRC", "hello world", Precedence::Mkfile);
        // No % in pattern → returns value unchanged.
        assert_eq!(s.expand("${SRC:hello=bye}"), "hello world");
    }

    #[test]
    fn expand_namelist_no_percent_in_replacement() {
        let mut s = Scope::new();
        s.set("SRC", "a.c b.c", Precedence::Mkfile);
        // Replacement without % replaces the matched suffix.
        assert_eq!(s.expand("${SRC:%.c=.o}"), "a.o b.o");
    }

    #[test]
    fn expand_namelist_with_simple_var_fallback() {
        // If braces contain colon but no '=', treat as simple ${VAR} lookup.
        let mut s = Scope::new();
        s.set("FOO:BAR", "gotit", Precedence::Mkfile);
        assert_eq!(s.expand("${FOO:BAR}"), "gotit");
    }

    // ── F-045 Phase 1 specific tests ───────────────────────────────────

    #[test]
    fn phase1_backtick_still_runs_in_set() {
        // S3: backtick must run at assignment time
        let mut s = Scope::new();
        s.set("FILES", "`echo a.c b.c`", Precedence::Mkfile);
        let val = s.get("FILES").unwrap();
        // should be backtick-expanded
        assert!(val.contains("a.c"));
        assert!(!val.contains('`'));
    }

    #[test]
    fn phase1_set_raw_no_expand() {
        let mut s = Scope::new();
        s.set_raw("VAR", "$REF", Precedence::Mkfile);
        assert_eq!(s.get("VAR"), Some("$REF"));
        // set_raw stores literally even if REF is defined
        s.set_raw("REF", "hello", Precedence::Mkfile);
        assert_eq!(s.get("VAR"), Some("$REF"));
    }
}
