// Tokenizer for Plan 9 mk mkfile syntax.
//
// Character-by-character state machine. Handles:
//   - Comment stripping (# outside quotes/backticks)
//   - Line continuation (\ + \n → space in sh, elided in rc)
//   - Backtick regions (opaque — stored literally)
//   - Quoted strings (single + double, rc doubles '' for literal ')
//   - Special chars (: = < |) with ${…} nesting awareness
//   - Indentation detection (first non-newline char is space/tab → Indent)
//
// See kb/mk-spec.md features F-001 through F-016.

use std::fmt;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single mkfile token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A whitespace-delimited word (may contain quoted strings, variable refs).
    Word(String),
    /// `:` — separates targets from prerequisites/attributes.
    Colon,
    /// `=` — variable assignment.
    Equals,
    /// `<` — file include directive.
    Include,
    /// `|` — pipe include directive (`<|`).
    Pipe,
    /// Leading whitespace — marks a recipe line.
    Indent,
    /// End of a logical line (blank line or end of input line).
    Newline,
    /// End of input.
    Eof,
}

/// Which shell's quoting rules to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMode {
    /// sh mode: backslash-newline becomes a space; `"` is a quote.
    Sh,
    /// rc mode: backslash-newline is elided; `"` is literal text.
    Rc,
}

impl fmt::Display for ShellMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellMode::Sh => write!(f, "sh"),
            ShellMode::Rc => write!(f, "rc"),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LexError {
    #[error("unterminated quote at position {pos}")]
    UnterminatedQuote { pos: usize },

    #[error("unterminated backtick at position {pos}")]
    UnterminatedBacktick { pos: usize },
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Character-by-character lexer for mkfile syntax.
pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    mode: ShellMode,
    /// True when lexing a recipe line (after Indent, before non-indented Newline).
    in_recipe: bool,
}

impl Lexer {
    /// Create a new lexer for mkfile text. Default mode is Sh.
    pub fn new(input: &str, mode: ShellMode) -> Self {
        Lexer {
            chars: input.chars().collect(),
            pos: 0,
            mode,
            in_recipe: false,
        }
    }

    /// Tokenize the entire input. The last token is always `Eof`.
    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        let mut at_line_start = true;
        let mut word = String::new();
        let mut brace_depth: u32 = 0; // ${…} nesting — suppress special chars

        while self.pos < self.chars.len() {
            // ---- start-of-line indentation detection ----
            if at_line_start {
                at_line_start = false;
                if let Some(c) = self.peek_char() {
                    if c == ' ' || c == '\t' {
                        tokens.push(Token::Indent);
                        self.in_recipe = true;
                        // consume remaining leading whitespace
                        self.skip_whitespace();
                        continue;
                    }
                }
                // Non-indented line → exit recipe mode
                self.in_recipe = false;
            }

            // Guard: record the position before we call next_char in case we
            // need it for an error that occurs inside a read_* helper.
            let save_pos = self.pos;

            let c = match self.next_char() {
                Some(c) => c,
                None => break,
            };

            match c {
                // ---- single-quoted string ----
                '\'' => {
                    word.push('\'');
                    self.read_single_quoted(&mut word, save_pos)?;
                }

                // ---- double-quoted string (sh mode only) ----
                '"' if self.mode == ShellMode::Sh => {
                    word.push('"');
                    self.read_double_quoted(&mut word, save_pos)?;
                }

                // ---- backtick region (opaque text) ----
                '`' => {
                    if self.in_recipe {
                        // Recipes pass through verbatim — backticks are shell syntax
                        word.push('`');
                    } else {
                        word.push('`');
                        self.read_backtick(&mut word, save_pos)?;
                    }
                }

                // ---- comment (# outside quotes / backticks) ----
                '#' => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                    // skip to the \n (but don't consume it — we handle it below)
                    while self.pos < self.chars.len()
                        && self.chars[self.pos] != '\n'
                    {
                        self.pos += 1;
                    }
                    // If we stopped at \n, consume it and emit Newline
                    if self.pos < self.chars.len() && self.chars[self.pos] == '\n'
                    {
                        self.pos += 1;
                        tokens.push(Token::Newline);
                        at_line_start = true;
                    }
                }

                // ---- special chars (suppressed inside ${…}) ----
                ':' if brace_depth == 0 && !self.in_recipe => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                    tokens.push(Token::Colon);
                }

                '=' if brace_depth == 0 && !self.in_recipe => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                    tokens.push(Token::Equals);
                }

                '<' if brace_depth == 0 && word.is_empty() => {
                    tokens.push(Token::Include);
                }

                '|' if brace_depth == 0 && word.is_empty() => {
                    tokens.push(Token::Pipe);
                }

                // ---- whitespace (word delimiter) ----
                ' ' | '\t' => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                }

                // ---- newline ----
                '\n' => {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                    tokens.push(Token::Newline);
                    at_line_start = true;
                }

                // ---- dollar sign — track ${…} nesting ----
                '$' => {
                    word.push('$');
                    // If followed by {, consume it and enter a brace context
                    if let Some('{') = self.peek_char() {
                        self.pos += 1; // skip {
                        word.push('{');
                        brace_depth += 1;
                    }
                }

                // ---- close brace — exit ${…} nesting ----
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    word.push('}');
                }

                // ---- everything else — accumulate into current word ----
                _ => {
                    word.push(c);
                }
            }
        }

        // Flush any remaining word at EOF
        if !word.is_empty() {
            tokens.push(Token::Word(std::mem::take(&mut word)));
        }

        tokens.push(Token::Eof);
        Ok(tokens)
    }

    // ------------------------------------------------------------------
    // Character I/O helpers
    // ------------------------------------------------------------------

    /// Return the next character, handling line continuation (`\` + `\n`).
    /// Skips `\r` for Windows compatibility.
    fn next_char(&mut self) -> Option<char> {
        if self.pos >= self.chars.len() {
            return None;
        }
        let c = self.chars[self.pos];
        self.pos += 1;

        // Skip carriage returns
        if c == '\r' {
            return self.next_char();
        }

        // Line continuation: backslash immediately followed by newline
        if c == '\\' && self.pos < self.chars.len() {
            let next = self.chars[self.pos];
            if next == '\n' {
                self.pos += 1; // skip \n
                match self.mode {
                    ShellMode::Sh => Some(' '),   // replace with space
                    ShellMode::Rc => self.next_char(), // elide both
                }
            } else if next == '\r'
                && self.pos + 1 < self.chars.len()
                && self.chars[self.pos + 1] == '\n'
            {
                // Windows: \<cr><lf>
                self.pos += 2; // skip \r\n
                match self.mode {
                    ShellMode::Sh => Some(' '),
                    ShellMode::Rc => self.next_char(),
                }
            } else {
                Some(c) // literal backslash
            }
        } else {
            Some(c)
        }
    }

    /// Peek at the current character without advancing.
    fn peek_char(&self) -> Option<char> {
        if self.pos >= self.chars.len() {
            None
        } else {
            Some(self.chars[self.pos])
        }
    }

    /// Consume whitespace characters (space, tab).
    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c == ' ' || c == '\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    // ------------------------------------------------------------------
    // Quoted-string & backtick readers
    // ------------------------------------------------------------------

    /// Read until the closing `'`. In rc mode, `''` represents a literal `'`.
    fn read_single_quoted(
        &mut self,
        word: &mut String,
        start_pos: usize,
    ) -> Result<(), LexError> {
        loop {
            let c = match self.next_char() {
                Some(c) => c,
                None => return Err(LexError::UnterminatedQuote { pos: start_pos }),
            };

            if c == '\'' {
                if self.mode == ShellMode::Rc {
                    // In rc, '' is an escaped literal '
                    if let Some('\'') = self.peek_char() {
                        self.pos += 1; // skip the doubled quote
                        word.push('\'');
                        continue;
                    }
                }
                // Closing quote
                word.push('\'');
                return Ok(());
            }

            word.push(c);
        }
    }

    /// Read until an unescaped `"` (sh mode only).  `\"` is an escaped quote
    /// that does NOT close the string.
    fn read_double_quoted(
        &mut self,
        word: &mut String,
        start_pos: usize,
    ) -> Result<(), LexError> {
        loop {
            let c = match self.next_char() {
                Some(c) => c,
                None => return Err(LexError::UnterminatedQuote { pos: start_pos }),
            };

            match c {
                '"' => {
                    word.push('"');
                    return Ok(());
                }
                '\\' => {
                    // Backslash escapes the next character — pull it through
                    // literally so \" does not close the string.
                    word.push('\\');
                    if let Some(next) = self.next_char() {
                        word.push(next);
                    } else {
                        return Err(LexError::UnterminatedQuote {
                            pos: start_pos,
                        });
                    }
                }
                _ => {
                    word.push(c);
                }
            }
        }
    }

    /// Read a backtick region.
    ///
    /// Two styles:
    ///   - rc style: `` `{…} `` — starts with `` `{ ``, ends at `}`
    ///   - sh style: `` `…` `` — starts with `` ` ``, ends at `` ` ``
    ///
    /// Contents are stored literally; `\`+`\n` line continuation is still
    /// applied because every character flows through `next_char()`.
    fn read_backtick(
        &mut self,
        word: &mut String,
        start_pos: usize,
    ) -> Result<(), LexError> {
        // Detect rc-style: `{ ... }
        let rc_style = self.peek_char() == Some('{');

        if rc_style {
            // consume the {
            self.pos += 1;
            word.push('{');

            loop {
                let c = match self.next_char() {
                    Some(c) => c,
                    None => {
                        return Err(LexError::UnterminatedBacktick {
                            pos: start_pos,
                        });
                    }
                };
                word.push(c);
                if c == '}' {
                    return Ok(());
                }
            }
        } else {
            // sh-style: read until closing `
            loop {
                let c = match self.next_char() {
                    Some(c) => c,
                    None => {
                        return Err(LexError::UnterminatedBacktick {
                            pos: start_pos,
                        });
                    }
                };
                word.push(c);
                if c == '`' {
                    return Ok(());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience function
// ---------------------------------------------------------------------------

/// Tokenize an mkfile input string.
pub fn tokenize(input: &str, mode: ShellMode) -> Result<Vec<Token>, LexError> {
    Lexer::new(input, mode).tokenize()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to build tokens without Eof for comparison
    fn tks(tokens: Vec<Token>) -> Vec<Token> {
        let mut v = tokens;
        v.push(Token::Eof);
        v
    }

    fn w(s: &str) -> Token {
        Token::Word(s.to_string())
    }

    // ---- basic ----

    #[test]
    fn empty_input() {
        assert_eq!(tokenize("", ShellMode::Sh).unwrap(), vec![Token::Eof]);
    }

    #[test]
    fn single_word() {
        assert_eq!(
            tokenize("hello", ShellMode::Sh).unwrap(),
            tks(vec![w("hello")])
        );
    }

    #[test]
    fn rule_header() {
        assert_eq!(
            tokenize("target: prereq\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("target"),
                Token::Colon,
                w("prereq"),
                Token::Newline,
            ])
        );
    }

    // ---- comments ----

    #[test]
    fn comment_line() {
        assert_eq!(
            tokenize("# this is a comment\nword\n", ShellMode::Sh).unwrap(),
            tks(vec![Token::Newline, w("word"), Token::Newline,])
        );
    }

    #[test]
    fn trailing_comment() {
        assert_eq!(
            tokenize("word # comment\n", ShellMode::Sh).unwrap(),
            tks(vec![w("word"), Token::Newline,])
        );
    }

    // ---- assignment ----

    #[test]
    fn assignment() {
        assert_eq!(
            tokenize("CC = gcc\n", ShellMode::Sh).unwrap(),
            tks(vec![w("CC"), Token::Equals, w("gcc"), Token::Newline,])
        );
    }

    // ---- includes ----

    #[test]
    fn include_file() {
        assert_eq!(
            tokenize("< mkfile\n", ShellMode::Sh).unwrap(),
            tks(vec![Token::Include, w("mkfile"), Token::Newline,])
        );
    }

    #[test]
    fn include_command() {
        assert_eq!(
            tokenize("<| gcc -M *.c\n", ShellMode::Sh).unwrap(),
            tks(vec![
                Token::Include,
                Token::Pipe,
                w("gcc"),
                w("-M"),
                w("*.c"),
                Token::Newline,
            ])
        );
    }

    // ---- indentation / recipes ----

    #[test]
    fn recipe_line() {
        assert_eq!(
            tokenize("\tcc -c a.c\n", ShellMode::Sh).unwrap(),
            tks(vec![
                Token::Indent,
                w("cc"),
                w("-c"),
                w("a.c"),
                Token::Newline,
            ])
        );
    }

    #[test]
    fn recipe_block() {
        assert_eq!(
            tokenize("target:\n\tcmd1\n\tcmd2\n\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("target"),
                Token::Colon,
                Token::Newline,
                Token::Indent,
                w("cmd1"),
                Token::Newline,
                Token::Indent,
                w("cmd2"),
                Token::Newline,
                Token::Newline,
            ])
        );
    }

    // ---- line continuation ----

    #[test]
    fn escaped_newline_sh() {
        // "foo\\\nbar\n" → foo and bar are separate words (separated by the space
        // that replaced the continuation)
        assert_eq!(
            tokenize("foo\\\nbar\n", ShellMode::Sh).unwrap(),
            tks(vec![w("foo"), w("bar"), Token::Newline,])
        );
    }

    #[test]
    fn escaped_newline_rc() {
        // rc mode: backslash-newline is elided → "foobar"
        assert_eq!(
            tokenize("foo\\\nbar\n", ShellMode::Rc).unwrap(),
            tks(vec![w("foobar"), Token::Newline,])
        );
    }

    // ---- quoted strings ----

    #[test]
    fn single_quoted() {
        assert_eq!(
            tokenize("'hello world'", ShellMode::Sh).unwrap(),
            tks(vec![w("'hello world'")])
        );
    }

    #[test]
    fn double_quoted_sh() {
        assert_eq!(
            tokenize("\"hello world\"", ShellMode::Sh).unwrap(),
            tks(vec![w("\"hello world\"")])
        );
    }

    #[test]
    fn double_quoted_rc_not_special() {
        // In rc, " is not a quote character
        assert_eq!(
            tokenize("\"hello world\"", ShellMode::Rc).unwrap(),
            tks(vec![w("\"hello"), w("world\"")])
        );
    }

    // ---- backtick ----

    #[test]
    fn backtick_command() {
        assert_eq!(
            tokenize("`echo hello`", ShellMode::Sh).unwrap(),
            tks(vec![w("`echo hello`")])
        );
    }

    #[test]
    fn backtick_command_rc_style() {
        assert_eq!(
            tokenize("`{echo hello}", ShellMode::Sh).unwrap(),
            tks(vec![w("`{echo hello}")])
        );
    }

    // ---- attributes ----

    #[test]
    fn attribute_rule() {
        assert_eq!(
            tokenize("target:VQ: prereq\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("target"),
                Token::Colon,
                w("VQ"),
                Token::Colon,
                w("prereq"),
                Token::Newline,
            ])
        );
    }

    // ---- multiple targets ----

    #[test]
    fn multiple_targets() {
        assert_eq!(
            tokenize("a b c: d e\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("a"),
                w("b"),
                w("c"),
                Token::Colon,
                w("d"),
                w("e"),
                Token::Newline,
            ])
        );
    }

    // ---- blank line ----

    #[test]
    fn blank_line() {
        assert_eq!(
            tokenize("a\n\nb\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("a"),
                Token::Newline,
                Token::Newline, // blank line
                w("b"),
                Token::Newline,
            ])
        );
    }

    // ---- errors ----

    #[test]
    fn unterminated_quote() {
        let result = tokenize("'hello", ShellMode::Sh);
        assert!(matches!(result, Err(LexError::UnterminatedQuote { .. })));
    }

    #[test]
    fn unterminated_backtick() {
        let result = tokenize("`hello", ShellMode::Sh);
        assert!(matches!(
            result,
            Err(LexError::UnterminatedBacktick { .. })
        ));
    }

    #[test]
    fn unterminated_rc_backtick() {
        let result = tokenize("`{hello", ShellMode::Sh);
        assert!(matches!(
            result,
            Err(LexError::UnterminatedBacktick { .. })
        ));
    }

    // ---- dollar sign in words ----

    #[test]
    fn dollar_in_word() {
        assert_eq!(
            tokenize("$CC -o $target", ShellMode::Sh).unwrap(),
            tks(vec![w("$CC"), w("-o"), w("$target")])
        );
    }

    #[test]
    fn dollar_brace_in_word() {
        assert_eq!(
            tokenize("${CFLAGS}", ShellMode::Sh).unwrap(),
            tks(vec![w("${CFLAGS}")])
        );
    }

    #[test]
    fn dollar_brace_with_special_chars() {
        // : and = inside ${…} should be part of the word, not separate tokens
        assert_eq!(
            tokenize("${VAR:a=b}", ShellMode::Sh).unwrap(),
            tks(vec![w("${VAR:a=b}")])
        );
    }

    #[test]
    fn nested_dollar_brace() {
        assert_eq!(
            tokenize("${VAR:${OTHER}}", ShellMode::Sh).unwrap(),
            tks(vec![w("${VAR:${OTHER}}")])
        );
    }

    // ---- ShellMode Display ----

    #[test]
    fn shell_mode_display() {
        assert_eq!(format!("{}", ShellMode::Sh), "sh");
        assert_eq!(format!("{}", ShellMode::Rc), "rc");
    }

    // ---- rc single-quote doubling ----

    #[test]
    fn rc_single_quote_escape() {
        // In rc, '' inside a single-quoted string is a literal '
        assert_eq!(
            tokenize("'hello''world'", ShellMode::Rc).unwrap(),
            tks(vec![w("'hello'world'")])
        );
    }

    // ---- escaped double quote inside double-quoted string ----

    #[test]
    fn escaped_double_quote() {
        assert_eq!(
            tokenize(r#""hello\"world""#, ShellMode::Sh).unwrap(),
            tks(vec![w(r#""hello\"world""#)])
        );
    }

    // ---- backslash inside double-quoted string ----

    #[test]
    fn backslash_escape_in_double_quote() {
        // \n (backslash-n, not newline) inside double quotes
        assert_eq!(
            tokenize(r#""hello\nworld""#, ShellMode::Sh).unwrap(),
            tks(vec![w(r#""hello\nworld""#)])
        );
    }

    // ---- multi-word line ----

    #[test]
    fn multi_word_line() {
        assert_eq!(
            tokenize("cc -O2 -c file.c\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("cc"),
                w("-O2"),
                w("-c"),
                w("file.c"),
                Token::Newline,
            ])
        );
    }

    // ---- no trailing newline ----

    #[test]
    fn no_trailing_newline() {
        assert_eq!(
            tokenize("hello world", ShellMode::Sh).unwrap(),
            tks(vec![w("hello"), w("world")])
        );
    }

    // ---- consecutive blank lines ----

    #[test]
    fn consecutive_blank_lines() {
        assert_eq!(
            tokenize("a\n\n\nb\n", ShellMode::Sh).unwrap(),
            tks(vec![
                w("a"),
                Token::Newline,
                Token::Newline,
                Token::Newline,
                w("b"),
                Token::Newline,
            ])
        );
    }

    // ---- comment with no trailing newline (EOF after # comment) ----

    #[test]
    fn comment_at_eof() {
        assert_eq!(
            tokenize("word # no newline", ShellMode::Sh).unwrap(),
            tks(vec![w("word")])
        );
    }

    // ---- only-whitespace line ----

    #[test]
    fn whitespace_only_line() {
        // spaces followed by newline → Indent, Newline
        assert_eq!(
            tokenize("   \n", ShellMode::Sh).unwrap(),
            tks(vec![Token::Indent, Token::Newline])
        );
    }

    // ---- < and | inside a word (not at start) ----

    #[test]
    fn angle_in_middle_of_word() {
        // < in the middle of a word is part of the word
        assert_eq!(
            tokenize("a<b", ShellMode::Sh).unwrap(),
            tks(vec![w("a<b")])
        );
    }

    #[test]
    fn pipe_in_middle_of_word() {
        assert_eq!(
            tokenize("a|b", ShellMode::Sh).unwrap(),
            tks(vec![w("a|b")])
        );
    }

    // ---- : and = inside a regular word (no ${...}) ----
    // These are always special tokens when brace_depth == 0

    #[test]
    fn colon_splits_word() {
        // a:b → a, :, b
        assert_eq!(
            tokenize("a:b", ShellMode::Sh).unwrap(),
            tks(vec![w("a"), Token::Colon, w("b")])
        );
    }

    #[test]
    fn equals_splits_word() {
        assert_eq!(
            tokenize("a=b", ShellMode::Sh).unwrap(),
            tks(vec![w("a"), Token::Equals, w("b")])
        );
    }

    // ---- tab-only indented line ----

    #[test]
    fn tab_indented_recipe() {
        assert_eq!(
            tokenize("\tcmd\n", ShellMode::Sh).unwrap(),
            tks(vec![Token::Indent, w("cmd"), Token::Newline])
        );
    }

    // ---- multiple spaces at line start ----

    #[test]
    fn spaces_indent() {
        assert_eq!(
            tokenize("    cmd\n", ShellMode::Sh).unwrap(),
            tks(vec![Token::Indent, w("cmd"), Token::Newline])
        );
    }

    #[test]
    fn backtick_in_recipe_passed_verbatim() {
        // Recipes should pass backticks through verbatim — they're shell syntax.
        // Regression: lexer was trying to match backticks in recipes.
        let result = tokenize(
            "target:\n\tcmd `backtick` arg\n",
            ShellMode::Sh,
        ).unwrap();
        // Recipe line should be treated as raw tokens, not backtick-processed
        let tokens = tks(vec![
            w("target"),
            Token::Colon,
            Token::Newline,
            Token::Indent,
            w("cmd"),
            w("`backtick`"),  // backtick text preserved as-is
            w("arg"),
            Token::Newline,
        ]);
        assert_eq!(result, tokens);
    }

    #[test]
    fn backtick_brace_in_recipe_passed_verbatim() {
        // es-style `{cmd}` in recipes should pass through verbatim
        let result = tokenize(
            "target:\n\techo `{uptime}`\n",
            ShellMode::Sh,
        ).unwrap();
        let tokens = tks(vec![
            w("target"),
            Token::Colon,
            Token::Newline,
            Token::Indent,
            w("echo"),
            w("`{uptime}`"),  // rc-style backtick preserved verbatim
            Token::Newline,
        ]);
        assert_eq!(result, tokens);
    }
}
