//! Lexer — tokenizes a workflow's compact source into a stream of [`Token`]s
//! for the parser. Handles the language's symbols (`§`, `!!`, `→`, `~`, `^`,
//! `+`, `$`) and structural keywords.
//!
//! Behavior-preserving port of the reference tokenizer. The scan is total: an
//! unrecognized character is skipped (matching the reference) rather than
//! aborting, so the only hard lex error is an unterminated string literal —
//! a case the parity corpus never exercises, so equivalence is preserved while
//! a real error path stays covered.

use crate::error::{Error, Result, Span};
use crate::vocab;

/// The category of a [`Token`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    // Structure
    /// `§` followed by an identifier (and optional `:name`).
    Section,
    /// `;; ...` line comment.
    Comment,
    /// End of line.
    Newline,
    /// End of input.
    Eof,

    // `@` keywords
    /// `@ON:`
    AtOn,
    /// `@in:`
    AtIn,
    /// `@out:`
    AtOut,
    /// `@ctx:`
    AtCtx,
    /// `@err:`
    AtErr,
    /// `@steps:`
    AtSteps,
    /// `@test:name` (value = the name).
    AtTest,
    /// `@macro:name` (value = the name).
    AtMacro,

    // `!` keywords
    /// `!!`
    DoubleBang,
    /// `!PREF:`
    BangPref,
    /// `!BREAK`
    BangBreak,
    /// `!SKIP`
    BangSkip,
    /// `!OVERRIDE`
    BangOverride,
    /// `!HALT`
    BangHalt,
    /// `!PAUSE`
    BangPause,

    // `?` keywords
    /// `?IF`
    QIf,
    /// `?ELIF`
    QElif,
    /// `?ELSE`
    QElse,
    /// `?SWITCH`
    QSwitch,

    // `~` keywords
    /// `~FOR`
    TildeFor,
    /// `~UNTIL`
    TildeUntil,
    /// `~PARALLEL`
    TildeParallel,
    /// `~MAP`
    TildeMap,
    /// `~RETRY` (followed by `:n`).
    TildeRetry,
    /// `~COMPENSATE` (followed by `: CMD(args)`).
    TildeCompensate,
    /// `~JOIN`
    TildeJoin,
    /// `~END`
    TildeEnd,
    /// `~identifier` analysis flag (value = the identifier).
    Flag,

    // Operators
    /// `→`
    Arrow,
    /// `|`
    Pipe,
    /// `=`
    Equals,
    /// `!=`
    NotEquals,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Lte,
    /// `>=`
    Gte,
    /// `::`
    DoubleColon,
    /// `*` (the `?SWITCH` default-arm marker).
    Star,

    // Keywords
    /// `NEVER`
    Never,
    /// `ALWAYS`
    Always,
    /// `CONTAINS`
    Contains,
    /// `NOT`
    Not,
    /// `IN`
    In,
    /// `AND`
    And,
    /// `OR`
    Or,
    /// `OVER`
    Over,
    /// `WITHOUT`
    Without,
    /// `BEFORE`
    Before,
    /// `IF`
    If,
    /// `MAX:n` (value = the number).
    Max,
    /// `RUN`
    Run,

    // Literals
    /// Numeric literal.
    Number,
    /// Double-quoted string literal (value = unescaped contents).
    StringLit,
    /// `true`
    True,
    /// `false`
    False,
    /// `null`
    Null,

    // Identifiers & references
    /// `$name(.field)*`
    Variable,
    /// `+identifier`
    Modifier,
    /// `^identifier(:param)*`
    Validator,
    /// Generic word.
    Identifier,
    /// ALL_CAPS known command name.
    CommandName,
    /// `wf:name`
    WfRef,
    /// `N.` step number at the start of an indented context.
    StepNumber,

    // Delimiters
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `:`
    Colon,
    /// `,`
    Comma,
    /// `.`
    Dot,
    /// `?` optional marker (e.g. in `@in`).
    Question,
}

/// A single terminal symbol with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The token category.
    pub ty: TokenType,
    /// The raw string captured from source.
    pub value: String,
    /// 1-based line where the token starts.
    pub line: u32,
    /// 1-based column where the token starts.
    pub column: u32,
}

impl Token {
    /// Construct a token.
    pub fn new(ty: TokenType, value: impl Into<String>, line: u32, column: u32) -> Self {
        Token {
            ty,
            value: value.into(),
            line,
            column,
        }
    }
}

/// Look up a `~`-prefixed control keyword.
fn tilde_keyword(word: &str) -> Option<TokenType> {
    match word {
        "FOR" => Some(TokenType::TildeFor),
        "UNTIL" => Some(TokenType::TildeUntil),
        "PARALLEL" => Some(TokenType::TildeParallel),
        "MAP" => Some(TokenType::TildeMap),
        "RETRY" => Some(TokenType::TildeRetry),
        "COMPENSATE" => Some(TokenType::TildeCompensate),
        "JOIN" => Some(TokenType::TildeJoin),
        "END" => Some(TokenType::TildeEnd),
        _ => None,
    }
}

/// Look up a bare keyword (`NEVER`, `IN`, `OVER`, …).
fn keyword(word: &str) -> Option<TokenType> {
    match word {
        "NEVER" => Some(TokenType::Never),
        "ALWAYS" => Some(TokenType::Always),
        "CONTAINS" => Some(TokenType::Contains),
        "NOT" => Some(TokenType::Not),
        "IN" => Some(TokenType::In),
        "AND" => Some(TokenType::And),
        "OR" => Some(TokenType::Or),
        "OVER" => Some(TokenType::Over),
        "WITHOUT" => Some(TokenType::Without),
        "BEFORE" => Some(TokenType::Before),
        "IF" => Some(TokenType::If),
        "RUN" => Some(TokenType::Run),
        _ => None,
    }
}

/// Tokenizes a `.nodus` source string into a stream of [`Token`]s in a single
/// pass with one character of lookahead.
pub struct Lexer {
    source: Vec<char>,
    pos: usize,
    line: u32,
    column: u32,
    tokens: Vec<Token>,
    extra_commands: Vec<String>,
}

impl Lexer {
    /// Initialize the lexer over `source` with the builtin vocabulary only.
    pub fn new(source: &str) -> Self {
        Lexer {
            source: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            extra_commands: Vec::new(),
        }
    }

    /// Initialize the lexer with an extended vocabulary from `schema`.
    ///
    /// Host commands in `schema` are recognized as `CommandName` tokens
    /// in addition to the builtin `KNOWN_COMMANDS`. Use this variant inside
    /// `run_with_schema` to enable host-declared command parsing.
    pub fn new_with_schema(source: &str, schema: &crate::vocab::Schema) -> Self {
        Lexer {
            source: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            extra_commands: schema.host_commands().to_vec(),
        }
    }

    /// Convenience: tokenize `source` in one call with the builtin vocabulary.
    pub fn tokenize_str(source: &str) -> Result<Vec<Token>> {
        Lexer::new(source).tokenize()
    }

    /// Convenience: tokenize `source` with an extended vocabulary from `schema`.
    pub fn tokenize_str_with_schema(
        source: &str,
        schema: &crate::vocab::Schema,
    ) -> Result<Vec<Token>> {
        Lexer::new_with_schema(source, schema).tokenize()
    }

    /// Convert the entire source into a list of tokens, terminated by `Eof`.
    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        while self.pos < self.source.len() {
            self.skip_spaces();
            if self.at_end() {
                break;
            }

            let ch = self.ch();

            // Newlines
            if ch == '\r' || ch == '\n' {
                self.read_newline();
                continue;
            }

            // Comment `;;`
            if ch == ';' && self.peek() == Some(';') {
                self.read_comment();
                continue;
            }

            match ch {
                '\u{00a7}' => {
                    self.read_section();
                    continue;
                }
                '@' => {
                    self.read_at();
                    continue;
                }
                '!' => {
                    self.read_bang();
                    continue;
                }
                '?' => {
                    self.read_question();
                    continue;
                }
                '~' => {
                    self.read_tilde();
                    continue;
                }
                '$' => {
                    self.read_variable();
                    continue;
                }
                '+' => {
                    self.read_modifier();
                    continue;
                }
                '^' => {
                    self.read_validator();
                    continue;
                }
                '\u{2192}' => {
                    self.emit(TokenType::Arrow, "\u{2192}");
                    self.advance();
                    continue;
                }
                '=' => {
                    self.emit(TokenType::Equals, "=");
                    self.advance();
                    continue;
                }
                '<' => {
                    if self.peek() == Some('=') {
                        self.emit(TokenType::Lte, "<=");
                        self.advance();
                        self.advance();
                    } else {
                        self.emit(TokenType::Lt, "<");
                        self.advance();
                    }
                    continue;
                }
                '>' => {
                    if self.peek() == Some('=') {
                        self.emit(TokenType::Gte, ">=");
                        self.advance();
                        self.advance();
                    } else {
                        self.emit(TokenType::Gt, ">");
                        self.advance();
                    }
                    continue;
                }
                '|' => {
                    self.emit(TokenType::Pipe, "|");
                    self.advance();
                    continue;
                }
                '(' => {
                    self.emit(TokenType::LParen, "(");
                    self.advance();
                    continue;
                }
                ')' => {
                    self.emit(TokenType::RParen, ")");
                    self.advance();
                    continue;
                }
                '{' => {
                    self.emit(TokenType::LBrace, "{");
                    self.advance();
                    continue;
                }
                '}' => {
                    self.emit(TokenType::RBrace, "}");
                    self.advance();
                    continue;
                }
                '[' => {
                    self.emit(TokenType::LBracket, "[");
                    self.advance();
                    continue;
                }
                ']' => {
                    self.emit(TokenType::RBracket, "]");
                    self.advance();
                    continue;
                }
                ',' => {
                    self.emit(TokenType::Comma, ",");
                    self.advance();
                    continue;
                }
                '"' => {
                    self.read_string()?;
                    continue;
                }
                ':' => {
                    if self.peek() == Some(':') {
                        self.emit(TokenType::DoubleColon, "::");
                        self.advance();
                        self.advance();
                    } else {
                        self.emit(TokenType::Colon, ":");
                        self.advance();
                    }
                    continue;
                }
                '.' => {
                    self.emit(TokenType::Dot, ".");
                    self.advance();
                    continue;
                }
                '*' => {
                    self.emit(TokenType::Star, "*");
                    self.advance();
                    continue;
                }
                _ => {}
            }

            // Number (or negative number)
            if ch.is_ascii_digit() || (ch == '-' && self.peek_is_digit()) {
                self.read_number();
                continue;
            }

            // Identifier / keyword / command
            if ch.is_alphabetic() || ch == '_' {
                self.read_identifier();
                continue;
            }

            // Unknown character — skip (reference parity).
            self.advance();
        }

        self.emit(TokenType::Eof, "");
        Ok(self.tokens)
    }

    // ── Character helpers ───────────────────────────────────────────────────

    fn at_end(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn ch(&self) -> char {
        self.source[self.pos]
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }

    fn peek_is_digit(&self) -> bool {
        matches!(self.source.get(self.pos + 1), Some(c) if c.is_ascii_digit())
    }

    fn advance(&mut self) {
        if self.pos < self.source.len() {
            if self.source[self.pos] == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
    }

    fn emit(&mut self, ty: TokenType, value: impl Into<String>) {
        self.tokens
            .push(Token::new(ty, value, self.line, self.column));
    }

    fn skip_spaces(&mut self) {
        while self.pos < self.source.len() && matches!(self.source[self.pos], ' ' | '\t') {
            self.advance();
        }
    }

    /// Skip horizontal whitespace (spaces and tabs) without consuming newlines.
    fn skip_inline_whitespace(&mut self) {
        while self.pos < self.source.len()
            && (self.source[self.pos] == ' ' || self.source[self.pos] == '\t')
        {
            self.pos += 1;
            self.column += 1;
        }
    }

    /// Accumulate alphanumeric / underscore characters into a word, advancing
    /// the cursor and column (words never span newlines).
    fn read_word(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.source.len()
            && (self.source[self.pos].is_alphanumeric() || self.source[self.pos] == '_')
        {
            self.pos += 1;
            self.column += 1;
        }
        self.source[start..self.pos].iter().collect()
    }

    // ── Token readers ───────────────────────────────────────────────────────

    fn read_newline(&mut self) {
        self.emit(TokenType::Newline, "\\n");
        if self.ch() == '\r' {
            self.advance();
            if !self.at_end() && self.ch() == '\n' {
                self.advance();
            }
        } else {
            self.advance();
        }
    }

    fn read_comment(&mut self) {
        let col = self.column;
        let start = self.pos;
        while self.pos < self.source.len() && self.source[self.pos] != '\n' {
            self.pos += 1;
            self.column += 1;
        }
        let text: String = self.source[start..self.pos].iter().collect();
        self.tokens
            .push(Token::new(TokenType::Comment, text.trim(), self.line, col));
    }

    fn read_string(&mut self) -> Result<()> {
        let col = self.column;
        let start_line = self.line;
        self.advance(); // skip opening "
        let mut chars = String::new();
        while !self.at_end() && self.ch() != '"' {
            if self.ch() == '\\' && self.peek() == Some('"') {
                chars.push('"');
                self.advance();
                self.advance();
            } else {
                chars.push(self.ch());
                self.advance();
            }
        }
        if self.at_end() {
            return Err(Error::Lex {
                span: Span::new(start_line, col),
                message: "unterminated string literal".to_string(),
            });
        }
        self.advance(); // skip closing "
        self.tokens
            .push(Token::new(TokenType::StringLit, chars, start_line, col));
        Ok(())
    }

    fn read_number(&mut self) {
        let col = self.column;
        let start = self.pos;
        if self.source[self.pos] == '-' {
            self.pos += 1;
            self.column += 1;
        }
        while self.pos < self.source.len() && self.source[self.pos].is_ascii_digit() {
            self.pos += 1;
            self.column += 1;
        }

        if self.pos < self.source.len() && self.source[self.pos] == '.' {
            let after_dot = self.pos + 1;
            // A trailing `.` before whitespace/EOF is a step number (`3.`).
            if after_dot >= self.source.len()
                || matches!(self.source[after_dot], ' ' | '\t' | '\n' | '\r')
            {
                let num: String = self.source[start..self.pos].iter().collect();
                self.pos += 1; // skip dot
                self.column += 1;
                self.tokens
                    .push(Token::new(TokenType::StepNumber, num, self.line, col));
                return;
            }
            self.pos += 1;
            self.column += 1;
            while self.pos < self.source.len() && self.source[self.pos].is_ascii_digit() {
                self.pos += 1;
                self.column += 1;
            }
        }

        let num: String = self.source[start..self.pos].iter().collect();
        self.tokens
            .push(Token::new(TokenType::Number, num, self.line, col));
    }

    fn read_section(&mut self) {
        let col = self.column;
        self.advance(); // skip §
        let word = self.read_word();
        let mut value = format!("\u{00a7}{word}");

        if !self.at_end() && self.ch() == ':' {
            value.push(':');
            self.pos += 1;
            self.column += 1;
            let name = self.read_word();
            value.push_str(&name);
        }

        self.tokens
            .push(Token::new(TokenType::Section, value, self.line, col));
    }

    fn read_at(&mut self) {
        let col = self.column;
        self.advance(); // skip @
        let word = self.read_word();

        let has_colon = !self.at_end() && self.ch() == ':';
        if has_colon {
            self.pos += 1;
            self.column += 1;
        }

        let key = if has_colon {
            format!("{word}:")
        } else {
            word.clone()
        };

        let mapped = match key.as_str() {
            "ON:" => Some(TokenType::AtOn),
            "in:" => Some(TokenType::AtIn),
            "out:" => Some(TokenType::AtOut),
            "ctx:" => Some(TokenType::AtCtx),
            "err:" => Some(TokenType::AtErr),
            "steps:" => Some(TokenType::AtSteps),
            _ => None,
        };

        if let Some(ty) = mapped {
            self.tokens
                .push(Token::new(ty, format!("@{key}"), self.line, col));
        } else if word == "test" && has_colon {
            self.skip_inline_whitespace();
            let name = self.read_word();
            self.tokens
                .push(Token::new(TokenType::AtTest, name, self.line, col));
        } else if word == "macro" && has_colon {
            self.skip_inline_whitespace();
            let name = self.read_word();
            self.tokens
                .push(Token::new(TokenType::AtMacro, name, self.line, col));
        } else {
            self.tokens.push(Token::new(
                TokenType::Identifier,
                format!("@{key}"),
                self.line,
                col,
            ));
        }
    }

    fn read_bang(&mut self) {
        let col = self.column;
        self.advance();

        if !self.at_end() && self.ch() == '!' {
            self.advance();
            self.tokens
                .push(Token::new(TokenType::DoubleBang, "!!", self.line, col));
            return;
        }

        if !self.at_end() && self.ch() == '=' {
            self.advance();
            self.tokens
                .push(Token::new(TokenType::NotEquals, "!=", self.line, col));
            return;
        }

        let word = self.read_word();
        let has_colon = !self.at_end() && self.ch() == ':';
        if has_colon {
            self.pos += 1;
            self.column += 1;
        }

        let key = if has_colon { format!("{word}:") } else { word };

        let mapped = match key.as_str() {
            "PREF:" => Some(TokenType::BangPref),
            "BREAK" => Some(TokenType::BangBreak),
            "SKIP" => Some(TokenType::BangSkip),
            "OVERRIDE" => Some(TokenType::BangOverride),
            "HALT" => Some(TokenType::BangHalt),
            "PAUSE" => Some(TokenType::BangPause),
            _ => None,
        };

        if let Some(ty) = mapped {
            self.tokens
                .push(Token::new(ty, format!("!{key}"), self.line, col));
        } else {
            self.tokens.push(Token::new(
                TokenType::Identifier,
                format!("!{key}"),
                self.line,
                col,
            ));
        }
    }

    fn read_question(&mut self) {
        let col = self.column;
        self.advance();

        let word = self.read_word();
        let mapped = match word.as_str() {
            "IF" => Some(TokenType::QIf),
            "ELIF" => Some(TokenType::QElif),
            "ELSE" => Some(TokenType::QElse),
            "SWITCH" => Some(TokenType::QSwitch),
            _ => None,
        };

        if let Some(ty) = mapped {
            self.tokens
                .push(Token::new(ty, format!("?{word}"), self.line, col));
        } else {
            self.tokens
                .push(Token::new(TokenType::Question, "?", self.line, col));
            // Not a `?`-keyword: rewind the word so it lexes on its own.
            if !word.is_empty() {
                let n = word.chars().count();
                self.pos -= n;
                self.column -= n as u32;
            }
        }
    }

    fn read_tilde(&mut self) {
        let col = self.column;
        self.advance();
        let word = self.read_word();

        if let Some(ty) = tilde_keyword(&word) {
            self.tokens
                .push(Token::new(ty, format!("~{word}"), self.line, col));
        } else {
            self.tokens
                .push(Token::new(TokenType::Flag, word, self.line, col));
        }
    }

    fn read_variable(&mut self) {
        let col = self.column;
        self.advance();
        let mut name = self.read_word();

        while !self.at_end() && self.ch() == '.' {
            let next = self.pos + 1;
            match self.source.get(next) {
                Some(c) if c.is_alphabetic() || *c == '_' => {
                    name.push('.');
                    self.pos += 1;
                    self.column += 1;
                    name.push_str(&self.read_word());
                }
                _ => break,
            }
        }

        self.tokens.push(Token::new(
            TokenType::Variable,
            format!("${name}"),
            self.line,
            col,
        ));
    }

    fn read_modifier(&mut self) {
        let col = self.column;
        self.advance();
        let name = self.read_word();
        self.tokens.push(Token::new(
            TokenType::Modifier,
            format!("+{name}"),
            self.line,
            col,
        ));
    }

    fn read_validator(&mut self) {
        let col = self.column;
        self.advance();
        let mut full = self.read_word();

        while !self.at_end() && self.ch() == ':' {
            full.push(':');
            self.pos += 1;
            self.column += 1;
            while !self.at_end()
                && !matches!(
                    self.ch(),
                    ' ' | '\t' | '\n' | '\r' | ',' | ')' | '^' | '~' | '+'
                )
            {
                full.push(self.ch());
                self.pos += 1;
                self.column += 1;
            }
        }

        self.tokens.push(Token::new(
            TokenType::Validator,
            format!("^{full}"),
            self.line,
            col,
        ));
    }

    fn read_identifier(&mut self) {
        let col = self.column;
        let word = self.read_word();

        if word == "wf" && !self.at_end() && self.ch() == ':' {
            self.pos += 1;
            self.column += 1;
            let name = self.read_word();
            self.tokens.push(Token::new(
                TokenType::WfRef,
                format!("wf:{name}"),
                self.line,
                col,
            ));
            return;
        }

        if word == "MAX" && !self.at_end() && self.ch() == ':' {
            self.pos += 1;
            self.column += 1;
            let start = self.pos;
            while !self.at_end() && self.ch().is_ascii_digit() {
                self.pos += 1;
                self.column += 1;
            }
            let num: String = self.source[start..self.pos].iter().collect();
            self.tokens
                .push(Token::new(TokenType::Max, num, self.line, col));
            return;
        }

        if let Some(ty) = keyword(&word) {
            self.tokens.push(Token::new(ty, word, self.line, col));
            return;
        }

        match word.as_str() {
            "true" => {
                self.tokens
                    .push(Token::new(TokenType::True, word, self.line, col));
                return;
            }
            "false" => {
                self.tokens
                    .push(Token::new(TokenType::False, word, self.line, col));
                return;
            }
            "null" => {
                self.tokens
                    .push(Token::new(TokenType::Null, word, self.line, col));
                return;
            }
            _ => {}
        }

        // An ALL_CAPS word is a command when the builtin schema knows it, or
        // when it appears in the host-supplied extra_commands list from a
        // schema extension. The builtin check is first for performance.
        if vocab::is_known_command(&word) || self.extra_commands.iter().any(|c| c == &word) {
            self.tokens
                .push(Token::new(TokenType::CommandName, word, self.line, col));
            return;
        }

        self.tokens
            .push(Token::new(TokenType::Identifier, word, self.line, col));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(tokens: &[Token]) -> Vec<TokenType> {
        tokens.iter().map(|t| t.ty).collect()
    }

    #[test]
    fn tokenizes_header_and_runtime() {
        let toks = Lexer::tokenize_str("§wf:beautiful_mention v1.0\n").unwrap();
        assert_eq!(toks[0].ty, TokenType::Section);
        assert_eq!(toks[0].value, "§wf:beautiful_mention");
        // `v1` is an identifier, `.0` a dot + number; ends with Newline then Eof.
        assert_eq!(toks.last().unwrap().ty, TokenType::Eof);
    }

    #[test]
    fn tokenizes_core_symbols() {
        let toks = Lexer::tokenize_str("FETCH($post_url) → $raw").unwrap();
        let got = types(&toks);
        assert_eq!(
            got,
            vec![
                TokenType::CommandName, // FETCH
                TokenType::LParen,
                TokenType::Variable, // $post_url
                TokenType::RParen,
                TokenType::Arrow,
                TokenType::Variable, // $raw
                TokenType::Eof,
            ]
        );
        assert_eq!(toks[0].value, "FETCH");
        assert_eq!(toks[2].value, "$post_url");
    }

    #[test]
    fn tokenizes_constraints_and_flags() {
        let toks =
            Lexer::tokenize_str("!!NEVER: publish WITHOUT validate\nANALYZE($raw) ~sentiment")
                .unwrap();
        let got = types(&toks);
        assert!(got.contains(&TokenType::DoubleBang));
        assert!(got.contains(&TokenType::Never));
        assert!(got.contains(&TokenType::Without));
        assert!(got.contains(&TokenType::Flag));
        // The flag carries the bare identifier.
        let flag = toks.iter().find(|t| t.ty == TokenType::Flag).unwrap();
        assert_eq!(flag.value, "sentiment");
    }

    #[test]
    fn tokenizes_modifiers_validators_and_until() {
        let toks = Lexer::tokenize_str(
            "GEN(reply) +tone=warm\nVALIDATE($d) ^len:280\n~UNTIL $q > 0.85 | MAX:3:",
        )
        .unwrap();
        let got = types(&toks);
        assert!(got.contains(&TokenType::Modifier));
        assert!(got.contains(&TokenType::Validator));
        assert!(got.contains(&TokenType::TildeUntil));
        assert!(got.contains(&TokenType::Max));
        let modifier = toks.iter().find(|t| t.ty == TokenType::Modifier).unwrap();
        assert_eq!(modifier.value, "+tone");
        let validator = toks.iter().find(|t| t.ty == TokenType::Validator).unwrap();
        assert_eq!(validator.value, "^len:280");
        let max = toks.iter().find(|t| t.ty == TokenType::Max).unwrap();
        assert_eq!(max.value, "3");
    }

    #[test]
    fn conditional_keywords_lex_distinctly() {
        let toks = Lexer::tokenize_str("?IF $x < 0.2 → ROUTE(wf:crisis) !BREAK").unwrap();
        let got = types(&toks);
        assert!(got.contains(&TokenType::QIf));
        assert!(got.contains(&TokenType::Lt));
        assert!(got.contains(&TokenType::WfRef));
        assert!(got.contains(&TokenType::BangBreak));
        let wf = toks.iter().find(|t| t.ty == TokenType::WfRef).unwrap();
        assert_eq!(wf.value, "wf:crisis");
    }

    #[test]
    fn halt_and_pause_lex_as_distinct_bang_keywords() {
        let toks = Lexer::tokenize_str("?IF $r > 0.9 → ESCALATE(human) !HALT").unwrap();
        assert!(types(&toks).contains(&TokenType::BangHalt));

        let toks = Lexer::tokenize_str("?IF $r > 0.9 → ASK(human) !PAUSE").unwrap();
        assert!(types(&toks).contains(&TokenType::BangPause));
    }

    #[test]
    fn retry_keyword_lexes_with_bound() {
        let toks = Lexer::tokenize_str("~RETRY:3 FETCH($u) → $d").unwrap();
        let got = types(&toks);
        assert!(
            got.contains(&TokenType::TildeRetry),
            "~RETRY must lex as TildeRetry"
        );
        // The `:3` bound follows as Colon + Number.
        assert!(got.contains(&TokenType::Colon));
        assert!(got.contains(&TokenType::Number));
    }

    #[test]
    fn map_keyword_lexes_before_generic_flag() {
        let toks = Lexer::tokenize_str("~MAP $items: GEN($it) → $out").unwrap();
        let got = types(&toks);
        assert!(
            got.contains(&TokenType::TildeMap),
            "~MAP must lex as TildeMap, not a generic Flag"
        );
        assert!(
            !got.contains(&TokenType::Flag),
            "~MAP must not fall through to Flag"
        );
    }

    #[test]
    fn switch_keyword_and_default_marker_lex() {
        let toks = Lexer::tokenize_str(
            "?SWITCH $category:\n  urgent → ROUTE(wf:crisis)\n  * → LOG($m)\n~END",
        )
        .unwrap();
        let got = types(&toks);
        assert!(
            got.contains(&TokenType::QSwitch),
            "?SWITCH must lex as QSwitch"
        );
        assert!(got.contains(&TokenType::Star), "* must lex as Star");
    }

    #[test]
    fn step_number_distinguished_from_float() {
        let toks = Lexer::tokenize_str("1. FETCH($u)\n?IF $x < 0.2 → LOG($x)").unwrap();
        let step = toks.iter().find(|t| t.ty == TokenType::StepNumber).unwrap();
        assert_eq!(step.value, "1");
        let num = toks.iter().find(|t| t.ty == TokenType::Number).unwrap();
        assert_eq!(num.value, "0.2");
    }

    #[test]
    fn tracks_line_and_column() {
        let toks = Lexer::tokenize_str("LOG($a)\nLOG($b)").unwrap();
        let second_log = toks
            .iter()
            .filter(|t| t.ty == TokenType::CommandName)
            .nth(1)
            .unwrap();
        assert_eq!(second_log.line, 2);
        assert_eq!(second_log.column, 1);
    }

    #[test]
    fn unknown_command_is_identifier() {
        // ALL_CAPS not in the schema lexes as a generic identifier.
        let toks = Lexer::tokenize_str("NOTACOMMAND($x)").unwrap();
        assert_eq!(toks[0].ty, TokenType::Identifier);
        assert_eq!(toks[0].value, "NOTACOMMAND");
    }

    #[test]
    fn unknown_character_is_skipped() {
        // `%` is not in the alphabet — the reference skips it; we match.
        let toks = Lexer::tokenize_str("LOG % ($x)").unwrap();
        let got = types(&toks);
        assert_eq!(
            got,
            vec![
                TokenType::CommandName,
                TokenType::LParen,
                TokenType::Variable,
                TokenType::RParen,
                TokenType::Eof,
            ]
        );
    }

    #[test]
    fn unterminated_string_is_a_lex_error() {
        let err = Lexer::tokenize_str("LOG(\"oops)").unwrap_err();
        match err {
            Error::Lex { span, .. } => {
                assert_eq!(span.line, 1);
            }
            other => panic!("expected lex error, got {other:?}"),
        }
    }

    #[test]
    fn comment_is_captured_and_trimmed() {
        let toks = Lexer::tokenize_str(";;  a full-line note  \nLOG($x)").unwrap();
        assert_eq!(toks[0].ty, TokenType::Comment);
        assert_eq!(toks[0].value, ";;  a full-line note");
    }
}
