//! Parser for mkfile syntax.
//!
//! Converts lexer tokens into an AST: rules and variable assignments.
//! Phase 1a scope: concrete rules only. No metarules, includes, or variable expansion.

use crate::attr::{parse_attributes, Attributes, ParseAttrError};
use crate::error::ParseError;
use crate::include::IncludeContext;
use crate::lex::Token;
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
}

/// Variable assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    pub name: String,
    pub value: String,
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Parse tokens into statements.
///
/// Thin wrapper that delegates to [`parse_with_includes`] with a fresh
/// include context and the current working directory as the base.
pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, ParseError> {
    parse_with_includes(
        tokens,
        &mut IncludeContext::new(),
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
}

/// Parse tokens with include support.
///
/// Handles `< file` include directives by reading and parsing the
/// referenced mkfile. The `base_dir` is used to resolve relative paths.
fn parse_with_includes(
    tokens: &[Token],
    ctx: &mut IncludeContext,
    base_dir: &Path,
) -> Result<Vec<Stmt>, ParseError> {
    let mut stmts = Vec::new();
    let mut pos: usize = 0;
    let mut line: usize = 1;

    while pos < tokens.len() && tokens[pos] != Token::Eof {
        match &tokens[pos] {
            Token::Include => {
                // < file — include directive
                pos += 1;
                let mut path_parts = Vec::new();
                while pos < tokens.len() && matches!(&tokens[pos], Token::Word(_)) {
                    if let Token::Word(ref w) = tokens[pos] {
                        path_parts.push(w.clone());
                    }
                    pos += 1;
                }
                let path = path_parts.join(" ");
                // Read and parse the included file
                match ctx.include_file(&path, base_dir) {
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
                    let assign = parse_assign(tokens, &mut pos, &mut line)?;
                    stmts.push(Stmt::Assign(assign));
                } else {
                    let rule = parse_rule(tokens, &mut pos, &mut line)?;
                    stmts.push(Stmt::Rule(rule));
                }
            }
            _ => {
                // Colon at start or other unexpected token — try parse_rule
                // (produces EmptyTarget for bare colon, ExpectedColon otherwise)
                let rule = parse_rule(tokens, &mut pos, &mut line)?;
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

    // Collect value words
    let mut parts: Vec<&str> = Vec::new();
    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
        if let Token::Word(s) = &tokens[*pos] {
            parts.push(s);
        }
        *pos += 1;
    }
    let value = parts.join(" ");

    // Consume trailing newline
    if *pos < tokens.len() && matches!(&tokens[*pos], Token::Newline) {
        *pos += 1;
        *line += 1;
    }

    Ok(Assign { name, value })
}

// ── rule parsing ───────────────────────────────────────────────────────────

fn parse_rule(
    tokens: &[Token],
    pos: &mut usize,
    line: &mut usize,
) -> Result<Rule, ParseError> {
    let start_line = *line;

    // Collect targets (words left of colon)
    let mut targets: Vec<String> = Vec::new();
    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
        if let Token::Word(s) = &tokens[*pos] {
            targets.push(s.clone());
        }
        *pos += 1;
    }

    if targets.is_empty() {
        return Err(ParseError::EmptyTarget { line: start_line });
    }

    // Expect colon
    if *pos >= tokens.len() || !matches!(&tokens[*pos], Token::Colon) {
        return Err(ParseError::ExpectedColon { line: start_line });
    }
    *pos += 1; // consume `:`

    // Check for attributes between colons: `target:VQ: prereq`
    let mut attrs = Attributes::new();
    if *pos < tokens.len()
        && matches!(&tokens[*pos], Token::Word(_))
        && *pos + 1 < tokens.len()
        && matches!(&tokens[*pos + 1], Token::Colon)
    {
        if let Token::Word(attr_str) = &tokens[*pos] {
            attrs = parse_attributes(attr_str).map_err(|e| match e {
                ParseAttrError::UnknownAttr(c) => ParseError::UnknownAttr {
                    attr: c,
                    line: start_line,
                },
            })?;
        }
        *pos += 2; // skip attribute word + closing colon
    }
    // else: single colon, no attributes

    // Collect prerequisites (words right of colon)
    let mut prereqs: Vec<String> = Vec::new();
    while *pos < tokens.len() && matches!(&tokens[*pos], Token::Word(_)) {
        if let Token::Word(s) = &tokens[*pos] {
            prereqs.push(s.clone());
        }
        *pos += 1;
    }

    // Consume trailing newline after header
    if *pos < tokens.len() && matches!(&tokens[*pos], Token::Newline) {
        *pos += 1;
        *line += 1;
    }

    // Collect recipe (indented lines)
    let recipe = parse_recipe(tokens, pos, line);

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
    })
}

// ── recipe parsing ─────────────────────────────────────────────────────────

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
}
