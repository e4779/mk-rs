//! Parser for mkfile syntax.
//!
//! Converts lexer tokens into an AST: rules and variable assignments.
//! F-045: parser carries a `&mut Scope` and expands variables at parse time
//! (read-time semantics), mirroring plan9port `parse.c` / `word.c`.

use crate::attr::{parse_attributes, Attributes, ParseAttrError};
use crate::error::ParseError;
use crate::include::IncludeContext;
use crate::lex::Token;
use crate::var::{builtin_scope, import_env, Precedence, Scope};
use std::path::{Path, PathBuf};

// ── AST types ──────────────────────────────────────────────────────────────

/// Top-level mkfile statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Rule(Rule),
    Assign(Assign),
}

/// A single build rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub targets: Vec<String>,
    pub prereqs: Vec<String>,
    pub attributes: Attributes,
    pub recipe: Option<String>,
    pub is_metarule: bool,
    pub is_regex: bool,
    pub line: usize,
    /// Custom comparison program from P: attribute (e.g., "Pcmp" → Some("cmp")).
    pub prog: Option<String>,
}

/// Variable assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    pub name: String,
    pub value: String,
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Parse tokens into statements with a fresh scope (builtins + env).
///
/// Thin wrapper that builds a default scope and delegates to
/// [`parse_with_scope`]. Suitable for tests and library usage when
/// no pre-seeded scope is needed.
pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, ParseError> {
    let mut scope = builtin_scope();
    import_env(&mut scope);
    parse_with_scope(tokens, &mut scope)
}

/// Parse tokens into statements, expanding variables against the given scope.
///
/// The scope is mutated in place: each `Assign` statement expands its RHS
/// and stores the result in `scope` at parse time (read-time semantics).
/// Rule headers (targets, prereqs) are NOT expanded here — that happens
/// in Phase 3.
///
/// Callers who want CLI-override vars (S10) should pre-populate the scope
/// with `CommandLine`-precedence values before calling this function.
pub fn parse_with_scope(
    tokens: &[Token],
    scope: &mut Scope,
) -> Result<Vec<Stmt>, ParseError> {
    parse_with_includes(
        tokens,
        &mut IncludeContext::new(),
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        scope,
    )
}

/// Parse tokens with include support and variable expansion.
///
/// Handles `< file` include directives by reading and parsing the
/// referenced mkfile. The `base_dir` is used to resolve relative paths.
/// `scope` is threaded through includes so that included files share
/// the same variable namespace.
fn parse_with_includes(
    tokens: &[Token],
    ctx: &mut IncludeContext,
    base_dir: &Path,
    scope: &mut Scope,
) -> Result<Vec<Stmt>, ParseError> {
    let mut stmts = Vec::new();
    let mut pos: usize = 0;
    let mut line: usize = 1;

    while pos < tokens.len() && tokens[pos] != Token::Eof {
        match &tokens[pos] {
            Token::Include => {
                pos += 1;
                if pos < tokens.len() && tokens[pos] == Token::Pipe {
                    // <| command — pipe include
                    pos += 1;
                    let mut cmd_parts = Vec::new();
                    while pos < tokens.len() && matches!(&tokens[pos], Token::Word(_)) {
                        if let Token::Word(ref w) = tokens[pos] {
                            cmd_parts.push(w.clone());
                        }
                        pos += 1;
                    }
                    let command = cmd_parts.join(" ");
                    match ctx.include_command(&command, base_dir, scope) {
                        Ok(included_stmts) => {
                            stmts.extend(included_stmts);
                        }
                        Err(e) => {
                            return Err(ParseError::UnexpectedToken {
                                expected: "valid command".into(),
                                got: e.to_string(),
                                line,
                            });
                        }
                    }
                } else {
                    // < file — include directive
                    let mut path_parts = Vec::new();
                    while pos < tokens.len() && matches!(&tokens[pos], Token::Word(_)) {
                        if let Token::Word(ref w) = tokens[pos] {
                            path_parts.push(w.clone());
                        }
                        pos += 1;
                    }
                    let path = path_parts.join(" ");
                    match ctx.include_file(&path, base_dir, scope) {
                        Ok(included_stmts) => {
                            stmts.extend(included_stmts);
                        }
                        Err(e) => {
                            return Err(ParseError::UnexpectedToken {
                                expected: "valid include path".into(),
                                got: e.to_string(),
                                line,
                            });
                        }
                    }
                }
                // Consume the trailing newline
                if pos < tokens.len() && tokens[pos] == Token::Newline {
                    pos += 1;
                    line += 1;
                }
            }
            Token::Newline => {
                line += 1;
                pos += 1;
            }
            Token::Indent | Token::Pipe => {
                // Skip unrecognized / not-yet-supported top-level tokens
                pos = skip_logical_line(tokens, pos, &mut line);
            }
            Token::Word(_) => {
                if is_assignment(tokens, pos) {
                    let assign = parse_assign(tokens, &mut pos, &mut line, scope)?;
                    stmts.push(Stmt::Assign(assign));
                } else {
                    let rule = parse_rule(tokens, &mut pos, &mut line, scope)?;
                    stmts.push(Stmt::Rule(rule));
                }
            }
            _ => {
                // Colon at start or other unexpected token — try parse_rule
                // (produces EmptyTarget for bare colon, ExpectedColon otherwise)
                let rule = parse_rule(tokens, &mut pos, &mut line, scope)?;
                stmts.push(Stmt::Rule(rule));
            }
        }
    }

    Ok(stmts)
}

// ── line-type detection ────────────────────────────────────────────────────

/// Returns true if the logical line starting at `start` is an assignment
/// (contains `=` before `:` or end-of-line).
fn is_assignment(tokens: &[Token], start: usize) -> bool {
    let mut i = start;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Equals => return true,
            Token::Colon => return false,
            Token::Newline | Token::Eof => return false,
            _ => i += 1,
        }
    }
    false
}

/// Skip tokens until the next Newline or Eof, incrementing the line counter.
/// Returns the new position.
fn skip_logical_line(tokens: &[Token], mut pos: usize, line: &mut usize) -> usize {
    while pos < tokens.len() {
        match &tokens[pos] {
            Token::Newline => {
                *line += 1;
                return pos + 1;
            }
            Token::Eof => return pos,
            _ => pos += 1,
        }
    }
    pos
}

// ── assignment parsing ─────────────────────────────────────────────────────

fn parse_assign(
    tokens: &[Token],
    pos: &mut usize,
    line: &mut usize,
    scope: &mut Scope,
) -> Result<Assign, ParseError> {
    // Variable name
    let name = match &tokens[*pos] {
        Token::Word(s) => s.clone(),
        _ => {
            return Err(ParseError::UnexpectedToken {
                expected: "variable name".into(),
                got: token_name(&tokens[*pos]),
                line: *line,
            });
        }
    };
    *pos += 1;

    // Expect `=`
    if !matches!(&tokens[*pos], Token::Equals) {
        return Err(ParseError::UnexpectedToken {
            expected: "=".into(),
            got: token_name(&tokens[*pos]),
            line: *line,
        });
    }
    *pos += 1;

    // Collect raw value words
    let mut parts: Vec<&str> = Vec::new();
    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
        if let Token::Word(s) = &tokens[*pos] {
            parts.push(s);
        }
        *pos += 1;
    }
    let raw_value = parts.join(" ");

    // Expand the RHS and store in scope (S2: read-time, fully expanded).
    // The precedence gate in scope.set prevents mkfile reassigns from
    // overriding CLI vars (S10).
    scope.set(&name, &raw_value, Precedence::Mkfile);
    // Read back the expanded value for the AST.
    let expanded_value = scope.get(&name).unwrap_or(&raw_value).to_string();

    // Consume trailing newline
    if *pos < tokens.len() && matches!(&tokens[*pos], Token::Newline) {
        *pos += 1;
        *line += 1;
    }

    Ok(Assign {
        name,
        value: expanded_value,
    })
}

// ── rule parsing ───────────────────────────────────────────────────────────

fn parse_rule(
    tokens: &[Token],
    pos: &mut usize,
    line: &mut usize,
    scope: &mut Scope,
) -> Result<Rule, ParseError> {
    let start_line = *line;

    // Collect raw targets (words left of colon)
    let mut raw_targets: Vec<String> = Vec::new();
    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
        if let Token::Word(s) = &tokens[*pos] {
            raw_targets.push(s.clone());
        }
        *pos += 1;
    }

    if raw_targets.is_empty() {
        return Err(ParseError::EmptyTarget { line: start_line });
    }

    // Expect colon
    if *pos >= tokens.len() || !matches!(&tokens[*pos], Token::Colon) {
        return Err(ParseError::ExpectedColon { line: start_line });
    }
    *pos += 1; // consume `:`

    // Check for attributes between colons: `target:VQ: prereq`
    // S9: do NOT expand the attribute token — it is parsed literally.
    // A `$` in the attribute-word is a syntax error in the reference.
    let mut attrs = Attributes::new();
    let mut prog: Option<String> = None;
    if *pos < tokens.len()
        && matches!(&tokens[*pos], Token::Word(_))
        && *pos + 1 < tokens.len()
        && matches!(&tokens[*pos + 1], Token::Colon)
    {
        if let Token::Word(attr_str) = &tokens[*pos] {
            let (clean_attr_str, extracted_prog) = extract_prog_from_attr(attr_str);
            prog = extracted_prog;
            attrs = parse_attributes(&clean_attr_str).map_err(|e| match e {
                ParseAttrError::UnknownAttr(c) => ParseError::UnknownAttr {
                    attr: c,
                    line: start_line,
                },
            })?;
        }
        *pos += 2; // skip attribute word + closing colon
    }
    // else: single colon, no attributes

    // Collect raw prerequisites (words right of colon)
    let mut raw_prereqs: Vec<String> = Vec::new();
    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
        if let Token::Word(s) = &tokens[*pos] {
            raw_prereqs.push(s.clone());
        }
        *pos += 1;
    }

    // Detect GNU Make $(...) syntax on RAW tokens (before expansion).
    // mk has its own glob syntax (*.txt, dir/*.c) — $(wildcard ...) is not supported.
    for prereq in &raw_prereqs {
        if prereq.contains("$(") {
            return Err(ParseError::UnexpectedToken {
                expected: "mk glob pattern (e.g., '*.txt' or 'dir/*.c')".into(),
                got: format!(
                    "GNU Make syntax $(...) in prereq '{}' is not supported",
                    prereq
                ),
                line: start_line,
            });
        }
    }

    // F-045 Phase 3: expand each target/prereq word through scope,
    // then split into possibly-multiple words (S11a: whole-word $VAR).
    // S5: namelist transforms (${VAR:%=%}) work via scope.expand.
    // S7: recipe-time vars ($prereq, $target, $stem, etc.) are NOT
    //     defined at parse time → expand to empty string.
    let targets: Vec<String> = expand_and_split(&raw_targets, scope);
    let prereqs: Vec<String> = expand_and_split(&raw_prereqs, scope);

    // Consume trailing newline after header
    if *pos < tokens.len() && matches!(&tokens[*pos], Token::Newline) {
        *pos += 1;
        *line += 1;
    }

    // Collect recipe (indented lines)
    let recipe = parse_recipe(tokens, pos, line);

    // S13/E-2: is_metarule must run on EXPANDED target text.
    // A variable like PAT=%.o in the scope can make `$PAT` a metarule.
    let is_metarule = targets.iter().any(|t| t.contains('%') || t.contains('&'));
    let is_regex = attrs.is_regex();

    Ok(Rule {
        targets,
        prereqs,
        attributes: attrs,
        recipe,
        is_metarule,
        is_regex,
        line: start_line,
        prog,
    })
}

// ── attribute helpers ─────────────────────────────────────────────────────

/// Valid attribute characters (used for extracting P program name).
const VALID_ATTR_CHARS: &[char] = &['V', 'Q', 'N', 'U', 'D', 'E', 'P', 'R', 'n'];

/// Extract the custom comparison program name from a P attribute.
///
/// In Plan 9 mk, `target:Pprog: prereq` attaches the P attribute with a
/// program name directly following the P. E.g., `"Pcmp"` → P attr + program
/// `"cmp"`. The program name is all non-attribute characters after P until
/// the next attribute character or end of string.
///
/// Returns the cleaned attribute string (program chars removed) and the
/// optional program name.
fn extract_prog_from_attr(attr_str: &str) -> (String, Option<String>) {
    if let Some(p_pos) = attr_str.find('P') {
        let before = &attr_str[..p_pos];
        let after = &attr_str[p_pos + 1..];

        // Program name: non-attr chars after P
        let prog_end = after
            .find(|c: char| VALID_ATTR_CHARS.contains(&c))
            .unwrap_or(after.len());
        let prog = &after[..prog_end];

        // Reconstruct attr string: before + 'P' + remaining attrs after prog
        let clean = format!("{}P{}", before, &after[prog_end..]);
        let prog = if prog.is_empty() {
            None
        } else {
            Some(prog.to_string())
        };
        (clean, prog)
    } else {
        (attr_str.to_string(), None)
    }
}

// ── recipe parsing ─────────────────────────────────────────────────────────

/// Expand each word in a rule-header position through the scope,
/// then split each expanded result on whitespace into possibly-many words.
///
/// This implements S11a (whole-word `$VAR` → multiple targets/prereqs).
/// Uses `split_whitespace` for the split; literal-glue cases (S11b/c)
/// are a known gap tracked by F-003a.
fn expand_and_split(raw_words: &[String], scope: &mut Scope) -> Vec<String> {
    let mut result = Vec::new();
    for word in raw_words {
        let expanded = scope.expand(word);
        // split_whitespace: correct for whole-word $VAR (S11a).
        // For literal-glue (pre.$VAR / $VAR.x) with multi-word values,
        // the split may diverge from the reference — see F-003a.
        for part in expanded.split_whitespace() {
            if !part.is_empty() {
                result.push(part.to_string());
            }
        }
    }
    result
}

/// Collect indented lines after a rule header.
///
/// Reads sequences of `Indent, Word*, Newline` and joins them into
/// a single recipe string. Stops at a blank line, non-indented line, or Eof.
fn parse_recipe(tokens: &[Token], pos: &mut usize, line: &mut usize) -> Option<String> {
    let mut recipe_lines: Vec<String> = Vec::new();

    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Indent) {
        // Skip the Indent token
        *pos += 1;

        // Collect Word tokens until Newline or Eof
        let mut words: Vec<&str> = Vec::new();
        while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
            if let Token::Word(s) = &tokens[*pos] {
                words.push(s);
            }
            *pos += 1;
        }

        if !words.is_empty() {
            recipe_lines.push(words.join(" "));
        }

        // Consume trailing Newline
        if *pos < tokens.len() && matches!(&tokens[*pos], Token::Newline) {
            *pos += 1;
            *line += 1;
        } else {
            // No newline after indented content — stop collecting
            break;
        }
    }

    if recipe_lines.is_empty() {
        None
    } else {
        Some(recipe_lines.join("\n"))
    }
}

// ── debug helpers ──────────────────────────────────────────────────────────

/// Human-readable name for a token (used in error messages).
fn token_name(tok: &Token) -> String {
    match tok {
        Token::Word(s) => format!("word '{s}'"),
        Token::Colon => "colon".into(),
        Token::Equals => "equals".into(),
        Token::Include => "include".into(),
        Token::Pipe => "pipe".into(),
        Token::Indent => "indent".into(),
        Token::Newline => "newline".into(),
        Token::Eof => "end of file".into(),
    }
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::{tokenize, ShellMode};

    fn parse_str(input: &str) -> Result<Vec<Stmt>, ParseError> {
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        parse(&tokens)
    }

    #[test]
    fn simple_rule() {
        let stmts = parse_str("target: prereq\n").unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.targets, vec!["target"]);
                assert_eq!(r.prereqs, vec!["prereq"]);
                assert!(r.recipe.is_none());
                assert!(!r.is_metarule);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_with_recipe() {
        let stmts = parse_str("target: prereq\n\techo hello\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.recipe, Some("echo hello".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_multi_line_recipe() {
        let stmts = parse_str("target: prereq\n\techo one\n\techo two\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.recipe, Some("echo one\necho two".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_without_prereqs() {
        let stmts = parse_str("target:\n\techo hi\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.prereqs, Vec::<String>::new());
                assert_eq!(r.recipe, Some("echo hi".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_with_virtual_attr() {
        let stmts = parse_str("target:V: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.is_virtual());
                assert!(!r.is_metarule);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_with_multiple_attrs() {
        let stmts = parse_str("target:VQ: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.is_virtual());
                assert!(r.attributes.is_quiet());
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn assignment() {
        let stmts = parse_str("CC = gcc\n").unwrap();
        match &stmts[0] {
            Stmt::Assign(a) => {
                assert_eq!(a.name, "CC");
                assert_eq!(a.value, "gcc");
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn assignment_multi_word_value() {
        let stmts = parse_str("CFLAGS = -Wall -O2\n").unwrap();
        match &stmts[0] {
            Stmt::Assign(a) => {
                assert_eq!(a.name, "CFLAGS");
                assert_eq!(a.value, "-Wall -O2");
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn multiple_rules() {
        let stmts = parse_str("a: b\n\techo a\n\nc: d\n\techo c\n").unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn rule_with_multiple_targets() {
        let stmts = parse_str("a b: c d\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.targets, vec!["a", "b"]);
                assert_eq!(r.prereqs, vec!["c", "d"]);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_with_multiple_prereqs() {
        let stmts = parse_str("prog: a.o b.o c.o\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.prereqs, vec!["a.o", "b.o", "c.o"]);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn missing_colon() {
        let result = parse_str("target prereq\n");
        assert!(result.is_err());
    }

    #[test]
    fn empty_target() {
        let result = parse_str(": prereq\n");
        assert!(result.is_err());
    }

    #[test]
    fn empty_input() {
        let stmts = parse_str("").unwrap();
        assert!(stmts.is_empty());
    }

    #[test]
    fn only_comments() {
        let stmts = parse_str("# just a comment\n").unwrap();
        assert!(stmts.is_empty());
    }

    #[test]
    fn blank_lines_terminate_recipe() {
        let input = "target: prereq\n\techo one\n\necho standalone\n";
        // Blank line terminates recipe. Then "echo standalone" is a bare word — invalid.
        assert!(parse_str(input).is_err());
        // But without the bare word, it parses fine:
        let valid = "target: prereq\n\techo one\n";
        let stmts = parse_str(valid).unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.recipe, Some("echo one".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn metarule_detection() {
        let stmts = parse_str("%.o: %.c\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.is_metarule);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn regex_rule_detection() {
        let stmts = parse_str("foo:R: bar\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.is_regex);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_with_line_number() {
        // Input has a comment on line 1, rule on line 2
        let stmts = parse_str("# comment\ntarget: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.line, 2);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn include_command_parses_stdout() {
        // <| echo "target: prereq" should parse as a rule
        let input = "<| echo \"target: prereq\"\n";
        let tokens = tokenize(input, ShellMode::Sh).unwrap();
        let stmts = parse(&tokens).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.targets, vec!["target"]);
                assert_eq!(r.prereqs, vec!["prereq"]);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn p_attribute_with_program() {
        // target:Pcmp: prereq → P attr with program "cmp"
        let stmts = parse_str("target:Pcmp: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.has_comparison());
                assert_eq!(r.prog, Some("cmp".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn p_attribute_no_program() {
        // target:P: prereq → P attr, no program name
        let stmts = parse_str("target:P: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.has_comparison());
                assert_eq!(r.prog, None);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn p_attribute_with_trailing_attrs() {
        // target:PcmpQ: prereq → P attr, program "cmp", Q attr
        let stmts = parse_str("target:PcmpQ: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.has_comparison());
                assert!(r.attributes.is_quiet());
                assert_eq!(r.prog, Some("cmp".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn p_attribute_with_leading_attrs() {
        // target:VPcmp: prereq → V attr, P attr, program "cmp"
        let stmts = parse_str("target:VPcmp: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.is_virtual());
                assert!(r.attributes.has_comparison());
                assert_eq!(r.prog, Some("cmp".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn p_attribute_pv_means_p_and_v_attrs_no_prog() {
        // target:PV: prereq → P attr + V attr, NO program (V is attr char)
        let stmts = parse_str("target:PV: prereq\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert!(r.attributes.has_comparison());
                assert!(r.attributes.is_virtual());
                assert_eq!(r.prog, None);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn rule_no_newline_at_eof() {
        let stmts = parse_str("target: prereq").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.targets, vec!["target"]);
                assert_eq!(r.prereqs, vec!["prereq"]);
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn recipe_with_equals_parsed_correctly() {
        // Regression: = inside recipe text was treated as assignment
        let stmts = parse_str("test:V:\n\tconst x=1\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.recipe, Some("const x=1".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn recipe_with_node_equals_parsed_correctly() {
        let stmts = parse_str("test:V:\n\tlet x=1\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.recipe, Some("let x=1".into()));
            }
            _ => panic!("expected Rule"),
        }
    }

    // ── GNU Make syntax rejection tests ────────────────────────────────

    #[test]
    fn rejects_gnu_make_wildcard_syntax() {
        let tokens = tokenize(
            "target: $(wildcard /tmp/test/*.txt)\n\techo x\n",
            ShellMode::Sh,
        )
        .unwrap();
        let result = parse(&tokens);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("$(") || err.contains("$(wildcard"),
            "error should mention $(...) syntax, got: {err}"
        );
    }

    #[test]
    fn rejects_dollar_paren_in_any_prereq() {
        let tokens = tokenize(
            "target: foo $(bar) baz\n\techo x\n",
            ShellMode::Sh,
        )
        .unwrap();
        let result = parse(&tokens);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("$("),
            "error should mention $(...) syntax, got: {err}"
        );
    }

    #[test]
    fn allows_dollar_without_paren_in_prereq() {
        // F-045 S7: recipe-time vars ($prereq, $target, etc.) are NOT
        // defined at parse time → expand to empty string.
        // Previously this test asserted literal "$prereq"; now it asserts
        // empty — matching plan9port mk reference behavior.
        let stmts = parse_str("target: $prereq\n\techo x\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.prereqs, Vec::<String>::new());
            }
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn allows_parentheses_without_dollar_in_prereq() {
        // Archive member syntax like lib.a(foo.o) should still be valid
        let stmts = parse_str("target: lib.a(foo.o)\n\techo x\n").unwrap();
        match &stmts[0] {
            Stmt::Rule(r) => {
                assert_eq!(r.prereqs, vec!["lib.a(foo.o)"]);
            }
            _ => panic!("expected Rule"),
        }
    }

    // ── F-045 Phase 2/3 contract tests ─────────────────────────────────

    #[test]
    fn f045_s1_read_time_expansion() {
        // S1: rule header uses the variable's value AT the line where
        // the rule is read, not the final value.
        let input = "TARG=early\ntarget: $TARG\nTARG=late\n";
        let stmts = parse_str(input).unwrap();
        let rule_stmts: Vec<_> = stmts.iter().filter_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).collect();
        assert_eq!(rule_stmts.len(), 1);
        assert_eq!(rule_stmts[0].prereqs, vec!["early"]);
    }

    #[test]
    fn f045_s2_assign_rhs_expanded() {
        // S2: Assignment RHS is recursively expanded at assignment time.
        let stmts = parse_str("A=world\nB=hello $A\n").unwrap();
        let assigns: Vec<_> = stmts.iter().filter_map(|s| {
            if let Stmt::Assign(a) = s { Some((a.name.as_str(), a.value.as_str())) } else { None }
        }).collect();
        assert_eq!(assigns, vec![("A", "world"), ("B", "hello world")]);
    }

    #[test]
    fn f045_s4_var_is_word_list() {
        // S4: A variable is a word list. $VAR in a header produces one
        // target/prereq per word.
        let input = "SRCS=a.c b.c\ntarget: $SRCS\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.prereqs, vec!["a.c", "b.c"]);
    }

    #[test]
    fn f045_s4_var_is_word_list_targets() {
        // S4: Multiple targets from a variable.
        let input = "TARGS=x y z\n$TARGS:\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.targets, vec!["x", "y", "z"]);
    }

    #[test]
    fn f045_s5_namelist_in_header() {
        // S5: ${VAR:%.c=%.o} works in target and prereq position.
        let input = "SRC=a.c b.c\ntarget: ${SRC:%.c=%.o}\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.prereqs, vec!["a.o", "b.o"]);
    }

    #[test]
    fn f045_s6_dollar_dollar_in_header() {
        // S6: $$ -> $ in headers, then re-scanned.
        let input = "SRCS=a.c\ntarget: $$SRCS\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.prereqs, vec!["a.c"]);
    }

    #[test]
    fn f045_s7_recipe_var_in_header_empties() {
        // S7: $target, $prereq, $stem, etc. are NOT defined at parse time
        // -> expand to empty string.
        let input = "target: $prereq $stem $target\n\techo hi\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.prereqs, Vec::<String>::new());
    }

    #[test]
    fn f045_s9_var_target_before_attrs_no_attr_expand() {
        // S9: Target word expands, but the attribute word does NOT.
        let input = "objtype=x86\n${objtype}l.h:Q:\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.targets, vec!["x86l.h"]);
        assert!(rule.attributes.is_quiet());
    }

    #[test]
    fn f045_s13_var_to_metarule() {
        // S13/E-2: $PAT where PAT=%.o produces a metarule.
        let input = "PAT=%.o\n$PAT: %.c\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert!(rule.is_metarule, "variable expanding to %-pattern should be metarule");
        assert_eq!(rule.targets, vec!["%.o"]);
    }

    #[test]
    fn f045_s13_var_to_multi_target() {
        // S13/E-3: $NAMES where NAMES=alpha beta produces multiple rules.
        let input = "NAMES=alpha beta\n$NAMES:\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.targets, vec!["alpha", "beta"]);
    }

    #[test]
    fn f045_s11a_whole_word_var_many_targets() {
        // S11a: A variable holding multiple words, used as a whole word,
        // produces multiple targets/prereqs.
        let input = "FILES=a.c b.c c.c\n$FILES: deps\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.targets, vec!["a.c", "b.c", "c.c"]);
    }

    #[test]
    fn f045_s11b_literal_glue_prefix() {
        // S11b: pre.$VAR with multi-word VAR, known gap F-003a.
        let input = "PARTS=one two\ntarget: pre.$PARTS\n";
        let stmts = parse_str(input).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.prereqs, vec!["pre.one", "two"]);
    }

    #[test]
    fn f045_assign_time_order_matters() {
        let input = "GREETING=$FIRST world\nFIRST=hello\n";
        let stmts = parse_str(input).unwrap();
        let assigns: Vec<_> = stmts.iter().filter_map(|s| {
            if let Stmt::Assign(a) = s { Some((a.name.as_str(), a.value.as_str())) } else { None }
        }).collect();
        assert_eq!(assigns, vec![
            ("GREETING", " world"),
            ("FIRST", "hello"),
        ]);
    }

    #[test]
    fn f045_parse_with_scope_preserves_cli_vars() {
        // S10: CLI vars in scope must not be overridden by mkfile assigns.
        use crate::var::builtin_scope;
        let mut scope = builtin_scope();
        scope.set_raw("VAR", "cli_value", super::Precedence::CommandLine);
        let tokens = tokenize("VAR=mkfile_value\ntarget: $VAR\n", ShellMode::Sh).unwrap();
        let stmts = parse_with_scope(&tokens, &mut scope).unwrap();
        let rule = stmts.iter().find_map(|s| {
            if let Stmt::Rule(r) = s { Some(r) } else { None }
        }).unwrap();
        assert_eq!(rule.prereqs, vec!["cli_value"]);
    }
}
