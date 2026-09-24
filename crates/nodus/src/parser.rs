//! Parser — recursive-descent construction of the AST from the lexer's token
//! stream.
//!
//! Behavior-preserving port of the reference parser's **workflow** path: header,
//! runtime block, triggers, hard rules, preferences, `@in`/`@out`/`@ctx`/`@err`
//! declarations, and the `@steps` sequence (command calls, conditionals, loops,
//! and parallel blocks). Conditions and rule bodies are kept as raw strings, as
//! the reference does.
//!
//! Schema (`§schema:`) file parsing is deferred — the parity corpus is
//! workflows; that entry point returns a parse error for now rather than being
//! silently mishandled. Config (`§config:`) files parse through the dedicated
//! [`Parser::parse_config`] entry point; `parse`/`parse_with_schema`
//! stay typed to [`WorkflowFile`] and reject a `§config` header with a redirect
//! naming `parse_config`, since a config file is not a workflow.

use crate::ast::{
    AbsoluteRule, CommandCall, Comment, Conditional, ConfigDecl, ConfigField, ContextDecl,
    ErrorDecl, FieldConstraint, FileHeader, FileType, ForLoop, InputDecl, InputField, MacroBlock,
    MapBlock, OutputDecl, ParallelBlock, Preference, RuleKind, RuntimeBlock, Step, Stmt,
    SwitchBlock, TestBlock, Trigger, UntilLoop, Variable, WorkflowFile,
};
use crate::error::{Error, Result, Span};
use crate::lexer::{Lexer, Token, TokenType};

/// Recursive-descent parser over a token stream.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// A `~COMPENSATE: CMD(args)` clause trailing a command's pipeline target,
    /// stashed here by `parse_command_call` and drained by `parse_step` right
    /// after parsing the step's own body. `Step.compensation` has no
    /// natural slot inside `parse_command_call`'s `CommandCall`-only return
    /// type, so this is the escape hatch — mirrors how `~RETRY:n` is read as
    /// a preceding modifier in `parse_step` itself rather than threaded
    /// through every command-parsing call.
    pending_compensation: Option<CommandCall>,
    /// Depth inside a `~FOR`/`~UNTIL`/`~PARALLEL` body. `~COMPENSATE` only
    /// attaches to a step's own top-level action (the restart rule's reasoning
    /// applies here too — attaching it to a per-item/per-branch command would
    /// be ambiguous about which iteration's compensation actually ran), so
    /// `parse_command_call` only stashes one when this is `0`.
    nested_body_depth: u32,
}

impl Parser {
    /// Parse `source` into a [`WorkflowFile`].
    ///
    /// Errors if the source is empty, does not open with a `§` declaration, or
    /// declares a non-workflow file type (schema/config parsing is deferred).
    pub fn parse(source: &str) -> Result<WorkflowFile> {
        let tokens = Lexer::tokenize_str(source)?;
        let mut p = Parser {
            tokens,
            pos: 0,
            pending_compensation: None,
            nested_body_depth: 0,
        };
        p.skip_noise();
        if p.at_end() {
            return Err(crate::error::Error::Parse {
                span: crate::error::Span::new(0, 0),
                message: "empty source — expected a § declaration".to_string(),
            });
        }
        if p.cur_ty() != TokenType::Section {
            return Err(crate::error::Error::Parse {
                span: p.span(),
                message: "expected a § declaration at the start of the file".to_string(),
            });
        }
        let val = p.cur_val();
        if val.starts_with("\u{00a7}wf:") {
            Ok(p.parse_workflow())
        } else if val.starts_with("\u{00a7}schema:") {
            Err(crate::error::Error::Parse {
                span: p.span(),
                message: "schema file parsing is not yet implemented".to_string(),
            })
        } else if val.starts_with("\u{00a7}config:") {
            Err(crate::error::Error::Parse {
                span: p.span(),
                message: "a §config file is not a workflow — use Parser::parse_config() instead"
                    .to_string(),
            })
        } else {
            Err(crate::error::Error::Parse {
                span: p.span(),
                message: format!("unknown file type: {val}"),
            })
        }
    }

    /// Parse `source` into a [`WorkflowFile`] using a schema-extended vocabulary.
    ///
    /// Host commands registered in `schema` (via [`crate::vocab::Schema::with_provider`])
    /// are recognized as valid command tokens during lexing, allowing workflows that
    /// use host-declared commands to parse successfully.
    pub fn parse_with_schema(source: &str, schema: &crate::vocab::Schema) -> Result<WorkflowFile> {
        let tokens = Lexer::tokenize_str_with_schema(source, schema)?;
        let mut p = Parser {
            tokens,
            pos: 0,
            pending_compensation: None,
            nested_body_depth: 0,
        };
        p.skip_noise();
        if p.at_end() {
            return Err(Error::Parse {
                span: Span::new(0, 0),
                message: "empty source — expected a § declaration".to_string(),
            });
        }
        if p.cur_ty() != TokenType::Section {
            return Err(Error::Parse {
                span: p.span(),
                message: "expected a § declaration at the start of the file".to_string(),
            });
        }
        let val = p.cur_val();
        if val.starts_with("\u{00a7}wf:") {
            Ok(p.parse_workflow())
        } else if val.starts_with("\u{00a7}schema:") {
            Err(Error::Parse {
                span: p.span(),
                message: "schema file parsing is not yet implemented".to_string(),
            })
        } else if val.starts_with("\u{00a7}config:") {
            Err(Error::Parse {
                span: p.span(),
                message: "a §config file is not a workflow — use Parser::parse_config() instead"
                    .to_string(),
            })
        } else {
            Err(Error::Parse {
                span: p.span(),
                message: format!("unknown file type: {val}"),
            })
        }
    }

    // ── Config file (§config:) ──────────────────────────────────────────────

    /// Parse `source` into a [`ConfigDecl`].
    ///
    /// Errors if the source is empty, does not open with a `§` declaration, or
    /// declares a file type other than `§config:`.
    pub fn parse_config(source: &str) -> Result<ConfigDecl> {
        let tokens = Lexer::tokenize_str(source)?;
        let mut p = Parser {
            tokens,
            pos: 0,
            pending_compensation: None,
            nested_body_depth: 0,
        };
        p.skip_noise();
        if p.at_end() {
            return Err(Error::Parse {
                span: Span::new(0, 0),
                message: "empty source — expected a § declaration".to_string(),
            });
        }
        if p.cur_ty() != TokenType::Section {
            return Err(Error::Parse {
                span: p.span(),
                message: "expected a § declaration at the start of the file".to_string(),
            });
        }
        if !p.cur_val().starts_with("\u{00a7}config:") {
            return Err(Error::Parse {
                span: p.span(),
                message: "expected a §config: declaration".to_string(),
            });
        }
        let header = p.parse_header(FileType::Config);
        let fields = p.parse_config_fields();
        Ok(ConfigDecl {
            header: Some(header),
            fields,
        })
    }

    /// Reserved config-field clause keyword (introduced by a `:` clause) versus
    /// the start of a new field (`name : type`). `-` is not a valid identifier
    /// character in this lexer, so the enumeration clause lexes as `one_of`
    /// (underscore) rather than the spec prose's `one-of`.
    fn is_config_clause_keyword(word: &str) -> bool {
        matches!(word, "default" | "range" | "one_of" | "describe")
    }

    /// Parse the field-declaration body of a `§config:` file. Field boundaries
    /// are disambiguated structurally (a clause keyword continues the current
    /// field; anything else starts a new one) rather than by indentation —
    /// this lexer does not emit indent/dedent tokens.
    fn parse_config_fields(&mut self) -> Vec<ConfigField> {
        let mut fields: Vec<ConfigField> = Vec::new();
        loop {
            self.skip_noise();
            if self.at_end() || self.check(TokenType::Section) {
                break;
            }
            if self.check(TokenType::Comment) {
                self.advance();
                continue;
            }
            if !matches!(
                self.cur_ty(),
                TokenType::Identifier | TokenType::CommandName
            ) {
                self.advance();
                continue;
            }

            let word = self.cur_val();
            let mut continued_field = false;

            if let Some(field) = fields.last_mut() {
                if word == "required" {
                    field.required = true;
                    self.advance();
                    continued_field = true;
                } else if word == "secret" {
                    field.secret = true;
                    self.advance();
                    continued_field = true;
                } else if Self::is_config_clause_keyword(&word)
                    && self.peek_ty(1) == Some(TokenType::Colon)
                {
                    self.advance(); // clause keyword
                    self.advance(); // colon
                    self.skip_noise();
                    if word == "default" {
                        field.default = self.take_config_value();
                    } else if word == "describe" {
                        if self.check(TokenType::StringLit) {
                            field.describe = Some(self.cur_val());
                            self.advance();
                        }
                    } else if word == "range" {
                        let lo = self.take_config_value().unwrap_or_default();
                        self.skip_noise();
                        if self.check(TokenType::Comma) {
                            self.advance();
                            self.skip_noise();
                        }
                        let hi = self.take_config_value().unwrap_or_default();
                        field.constraint = Some(FieldConstraint::Range { lo, hi });
                    } else if word == "one_of" {
                        let mut values = Vec::new();
                        if let Some(v) = self.take_config_value() {
                            values.push(v);
                        }
                        while self.check(TokenType::Pipe) {
                            self.advance();
                            self.skip_noise();
                            if let Some(v) = self.take_config_value() {
                                values.push(v);
                            }
                        }
                        field.constraint = Some(FieldConstraint::OneOf(values));
                    }
                    continued_field = true;
                }
            }

            if !continued_field {
                fields.push(self.parse_config_field_start());
            }
        }
        fields
    }

    /// Parse a new field's `name : type` head.
    fn parse_config_field_start(&mut self) -> ConfigField {
        let name = self.cur_val();
        self.advance();
        let mut field = ConfigField {
            name,
            ..Default::default()
        };
        self.skip_noise();
        if self.check(TokenType::Colon) {
            self.advance();
            self.skip_noise();
            if !self.at_end()
                && matches!(
                    self.cur_ty(),
                    TokenType::Identifier | TokenType::CommandName
                )
            {
                field.type_name = self.cur_val();
                self.advance();
            }
        }
        field
    }

    /// Consume a single literal token (`default:`/`range:`/`one_of:` value) as
    /// its raw text — mirroring `parse_field_list`'s `@in:` default capture.
    /// Type-directed coercion to a runtime `Value` happens at shape-check time.
    fn take_config_value(&mut self) -> Option<String> {
        if self.at_end() {
            return None;
        }
        match self.cur_ty() {
            TokenType::Number
            | TokenType::StringLit
            | TokenType::True
            | TokenType::False
            | TokenType::Null
            | TokenType::Identifier
            | TokenType::CommandName => {
                let v = self.cur_val();
                self.advance();
                Some(v)
            }
            _ => None,
        }
    }

    // ── Workflow file ────────────────────────────────────────────────────────

    fn parse_workflow(&mut self) -> WorkflowFile {
        let mut wf = WorkflowFile {
            header: Some(self.parse_header(FileType::Workflow)),
            ..Default::default()
        };

        while !self.at_end() {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            match self.cur_ty() {
                TokenType::Section => {
                    if self.cur_val().contains("runtime") {
                        wf.runtime = Some(self.parse_runtime());
                    } else {
                        self.advance();
                        self.consume_rest_of_line();
                    }
                }
                TokenType::AtOn => wf.triggers.push(self.parse_trigger()),
                TokenType::DoubleBang => wf.rules.push(self.parse_absolute_rule()),
                TokenType::BangPref => wf.preferences.push(self.parse_preference()),
                TokenType::AtIn => wf.input_decl = Some(self.parse_input_decl()),
                TokenType::AtOut => wf.output_decl = Some(self.parse_output_decl()),
                TokenType::AtCtx => wf.context_decl = Some(self.parse_context_decl()),
                TokenType::AtErr => wf.error_decl = Some(self.parse_error_decl()),
                TokenType::AtSteps => wf.steps = self.parse_steps(),
                TokenType::AtTest => wf.tests.push(self.parse_test_block()),
                TokenType::AtMacro => wf.macros.push(self.parse_macro_block()),
                TokenType::Comment => {
                    let text = self.cur_val();
                    if text.contains("HUMAN MODE") {
                        wf.human_mode = Some(self.collect_comment_block());
                    } else {
                        wf.comments.push(Comment { text });
                        self.advance();
                    }
                }
                _ => self.advance(),
            }
        }

        wf
    }

    // ── Header ───────────────────────────────────────────────────────────────

    fn parse_header(&mut self, file_type: FileType) -> FileHeader {
        let raw = self.cur_val();
        self.advance();

        // Extract name from §type:name.
        let val = raw.replace('\u{00a7}', "");
        let name = match val.split_once(':') {
            Some((_, n)) => n.to_string(),
            None => val,
        };

        // Version: an identifier starting with `v`, optionally with dotted patch.
        self.skip_noise();
        let mut version = String::new();
        if self.check(TokenType::Identifier) && self.cur_val().starts_with('v') {
            version = self.cur_val();
            self.advance();
            while self.check(TokenType::Dot) {
                let dot_pos = self.pos;
                self.advance();
                if self.check(TokenType::Number) {
                    version.push('.');
                    version.push_str(&self.cur_val());
                    self.advance();
                } else {
                    self.pos = dot_pos; // backtrack
                    break;
                }
            }
        }

        self.skip_to_newline();
        FileHeader {
            file_type,
            name,
            version,
        }
    }

    // ── Runtime block ────────────────────────────────────────────────────────

    fn parse_runtime(&mut self) -> RuntimeBlock {
        self.advance(); // skip §runtime token
        self.skip_noise();
        if self.check(TokenType::Colon) {
            self.advance();
        }
        self.skip_noise();

        let mut rt = RuntimeBlock {
            mode: "production".to_string(),
            ..Default::default()
        };
        if self.check(TokenType::LBrace) {
            self.parse_runtime_braces(&mut rt);
        }
        rt
    }

    /// Scan a `{ key: value, … }` runtime block, populating known keys. A value
    /// is a single token (matching the reference, which does not re-join `/`
    /// path fragments), a `[…]` list, or a `{…}` map (for `agents`).
    fn parse_runtime_braces(&mut self, rt: &mut RuntimeBlock) {
        self.advance(); // skip {
        while !self.at_end() && !self.check(TokenType::RBrace) {
            self.skip_noise();
            if self.check(TokenType::RBrace) {
                break;
            }
            if self.check(TokenType::Comment) {
                self.advance();
                continue;
            }
            if matches!(
                self.cur_ty(),
                TokenType::Identifier | TokenType::CommandName | TokenType::StringLit
            ) {
                let key = self.cur_val();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                }
                self.skip_noise();

                if self.check(TokenType::LBrace) {
                    let pairs = self.parse_brace_pairs();
                    if key == "agents" {
                        rt.agents = pairs;
                    }
                } else if self.check(TokenType::LBracket) {
                    let list = self.parse_bracket_list();
                    if key == "extends" {
                        rt.extends = list;
                    }
                } else if !self.at_end()
                    && !matches!(
                        self.cur_ty(),
                        TokenType::RBrace | TokenType::Newline | TokenType::Eof
                    )
                {
                    let value = self.cur_val();
                    self.advance();
                    match key.as_str() {
                        "core" => rt.core = value,
                        "mode" => rt.mode = value,
                        "extends" => rt.extends = vec![value],
                        "restart_max" => rt.restart_max = value.parse::<u32>().ok(),
                        _ => {}
                    }
                }

                if self.check(TokenType::Comma) {
                    self.advance();
                }
            } else {
                self.advance();
            }
        }
        if self.check(TokenType::RBrace) {
            self.advance();
        }
        self.skip_to_newline();
    }

    fn parse_brace_pairs(&mut self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.advance(); // skip {
        while !self.at_end() && !self.check(TokenType::RBrace) {
            self.skip_noise();
            if self.check(TokenType::RBrace) {
                break;
            }
            if self.check(TokenType::Comment) {
                self.advance();
                continue;
            }
            if matches!(
                self.cur_ty(),
                TokenType::Identifier | TokenType::CommandName | TokenType::StringLit
            ) {
                let key = self.cur_val();
                self.advance();
                if self.check(TokenType::Colon) {
                    self.advance();
                }
                self.skip_noise();
                let mut value = String::new();
                if !self.at_end()
                    && !matches!(
                        self.cur_ty(),
                        TokenType::RBrace | TokenType::Newline | TokenType::Eof
                    )
                {
                    value = self.cur_val();
                    self.advance();
                }
                out.push((key, value));
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            } else {
                self.advance();
            }
        }
        if self.check(TokenType::RBrace) {
            self.advance();
        }
        out
    }

    fn parse_bracket_list(&mut self) -> Vec<String> {
        self.advance(); // skip [
        let mut items = Vec::new();
        while !self.at_end() && !self.check(TokenType::RBracket) {
            self.skip_noise();
            if self.check(TokenType::RBracket) {
                break;
            }
            if self.check(TokenType::Comma) {
                self.advance();
                continue;
            }
            items.push(self.cur_val());
            self.advance();
        }
        if self.check(TokenType::RBracket) {
            self.advance();
        }
        items
    }

    // ── Declarations ─────────────────────────────────────────────────────────

    fn parse_trigger(&mut self) -> Trigger {
        self.advance(); // skip @ON:
        let cond = self.consume_until_arrow();
        let action = if self.check(TokenType::Arrow) {
            self.advance();
            self.consume_rest_of_line()
        } else {
            self.skip_to_newline();
            String::new()
        };
        Trigger {
            condition: cond.trim().to_string(),
            action: action.trim().to_string(),
        }
    }

    fn parse_absolute_rule(&mut self) -> AbsoluteRule {
        self.advance(); // skip !!
        let kind = if self.check(TokenType::Never) {
            self.advance();
            Some(RuleKind::Never)
        } else if self.check(TokenType::Always) {
            self.advance();
            Some(RuleKind::Always)
        } else {
            None
        };
        if self.check(TokenType::Colon) {
            self.advance();
        }
        let content = self.consume_rest_of_line();
        AbsoluteRule {
            kind,
            content: content.trim().to_string(),
        }
    }

    fn parse_preference(&mut self) -> Preference {
        self.advance(); // skip !PREF:
        let line = self.consume_rest_of_line();
        let (preferred, over, condition) = if let Some((pref, rest)) = line.split_once(" OVER ") {
            let rest = rest.trim();
            if let Some((o, cond)) = rest.split_once(" IF ") {
                (
                    pref.trim().to_string(),
                    o.trim().to_string(),
                    Some(cond.trim().to_string()),
                )
            } else {
                (pref.trim().to_string(), rest.to_string(), None)
            }
        } else {
            (line.trim().to_string(), String::new(), None)
        };
        Preference {
            preferred,
            over,
            condition,
        }
    }

    fn parse_input_decl(&mut self) -> InputDecl {
        self.advance(); // skip @in:
        self.skip_noise();
        let mut fields = Vec::new();
        if self.check(TokenType::LBrace) {
            self.advance();
            fields = self.parse_field_list();
            if self.check(TokenType::RBrace) {
                self.advance();
            }
        }
        self.skip_to_newline();
        InputDecl { fields }
    }

    fn parse_field_list(&mut self) -> Vec<InputField> {
        let mut fields = Vec::new();
        while !self.at_end() && !self.check(TokenType::RBrace) {
            self.skip_noise();
            if self.check(TokenType::RBrace) {
                break;
            }
            if self.check(TokenType::Comment) {
                self.advance();
                continue;
            }
            if self.check(TokenType::Identifier) || self.check(TokenType::Variable) {
                let mut f = InputField {
                    name: self.cur_val(),
                    ..Default::default()
                };
                self.advance();

                if self.check(TokenType::Question) {
                    f.optional = true;
                    self.advance();
                }
                if self.check(TokenType::Colon) {
                    self.advance();
                    self.skip_noise();
                    if !self.at_end()
                        && matches!(
                            self.cur_ty(),
                            TokenType::Identifier | TokenType::CommandName
                        )
                    {
                        f.type_name = self.cur_val();
                        self.advance();
                    }
                }
                if self.check(TokenType::Equals) {
                    self.advance();
                    self.skip_noise();
                    if !self.at_end() && self.cur_ty() != TokenType::RBrace {
                        f.default = Some(self.cur_val());
                        f.optional = true;
                        self.advance();
                    }
                }
                if self.check(TokenType::Comment) {
                    f.comment = self.cur_val();
                    self.advance();
                }
                fields.push(f);
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            } else {
                self.advance();
            }
        }
        fields
    }

    fn parse_output_decl(&mut self) -> OutputDecl {
        self.advance(); // skip @out:
        self.skip_noise();
        let variable = if self.check(TokenType::Variable) {
            let v = self.cur_val();
            self.advance();
            v
        } else {
            self.consume_rest_of_line().trim().to_string()
        };
        self.skip_to_newline();
        OutputDecl { variable }
    }

    fn parse_context_decl(&mut self) -> ContextDecl {
        self.advance(); // skip @ctx:
        self.skip_noise();
        let mut contexts = Vec::new();
        if self.check(TokenType::LBracket) {
            self.advance();
            while !self.at_end() && !self.check(TokenType::RBracket) {
                self.skip_noise();
                if self.check(TokenType::RBracket) {
                    break;
                }
                if matches!(self.cur_ty(), TokenType::Identifier | TokenType::StringLit) {
                    contexts.push(self.cur_val());
                }
                // Commas and any stray tokens are simply skipped.
                self.advance();
            }
            if self.check(TokenType::RBracket) {
                self.advance();
            }
        }
        self.skip_to_newline();
        ContextDecl { contexts }
    }

    fn parse_error_decl(&mut self) -> ErrorDecl {
        self.advance(); // skip @err:
        let raw = self.consume_rest_of_line().trim().to_string();
        let handler = self.try_parse_command_from_string(&raw);
        ErrorDecl { handler, raw }
    }

    // ── Steps ────────────────────────────────────────────────────────────────

    fn parse_steps(&mut self) -> Vec<Step> {
        self.advance(); // skip @steps:
        self.skip_noise();
        let mut steps = Vec::new();

        while !self.at_end() {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            match self.cur_ty() {
                TokenType::AtTest | TokenType::AtMacro | TokenType::Section | TokenType::Eof => {
                    break;
                }
                TokenType::Comment => {
                    if self.cur_val().contains("HUMAN MODE") {
                        break;
                    }
                    self.advance();
                }
                TokenType::StepNumber => steps.push(self.parse_step()),
                TokenType::TildeFor
                | TokenType::TildeUntil
                | TokenType::TildeParallel
                | TokenType::TildeMap
                | TokenType::TildeEnd
                | TokenType::TildeJoin => {
                    if let Some(node) = self.parse_control_flow() {
                        steps.push(Step {
                            number: 0,
                            body: Some(node),
                            ..Default::default()
                        });
                    }
                }
                TokenType::QIf | TokenType::QElif | TokenType::QElse => {
                    if let Some(c) = self.parse_conditional() {
                        steps.push(Step {
                            number: 0,
                            body: Some(Stmt::Conditional(c)),
                            ..Default::default()
                        });
                    }
                }
                TokenType::QSwitch => {
                    let sw = self.parse_switch();
                    steps.push(Step {
                        number: 0,
                        body: Some(Stmt::Switch(sw)),
                        ..Default::default()
                    });
                }
                _ => self.advance(),
            }
        }
        steps
    }

    fn parse_step(&mut self) -> Step {
        let number = self.cur_val().parse::<u32>().unwrap_or(0);
        self.advance(); // skip step number
        self.skip_noise();

        let mut step = Step {
            number,
            ..Default::default()
        };

        if self.check(TokenType::Comment) {
            step.comment = self.cur_val();
            self.advance();
            self.skip_noise();
            return step;
        }

        // Optional `~RETRY:n` step modifier preceding the step's action.
        if self.check(TokenType::TildeRetry) {
            step.retry = Some(self.parse_retry_bound());
            self.skip_noise();
        }

        step.body = self.parse_step_body();
        step.compensation = self.pending_compensation.take();

        // Collect indented sub-steps that are not new step numbers.
        // `~COMPENSATE` only attaches to the step's own top-level action
        // (parsed just above); a trailing clause on a sub-step line is
        // treated as nested (silently not captured, same as a ~FOR/~PARALLEL
        // body) rather than risking a leak into the *next* top-level step's
        // `pending_compensation.take()`.
        self.nested_body_depth += 1;
        while !self.at_end() {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            match self.cur_ty() {
                TokenType::StepNumber
                | TokenType::AtTest
                | TokenType::AtMacro
                | TokenType::Section
                | TokenType::Eof => break,
                TokenType::Comment => {
                    if self.cur_val().contains("HUMAN MODE") {
                        break;
                    }
                    self.advance();
                }
                TokenType::TildeEnd => {
                    self.advance();
                    self.skip_to_newline();
                    break;
                }
                TokenType::QIf | TokenType::QElif | TokenType::QElse => {
                    if let Some(c) = self.parse_conditional() {
                        step.sub_steps.push(Stmt::Conditional(c));
                    }
                }
                TokenType::QSwitch => {
                    let sw = self.parse_switch();
                    step.sub_steps.push(Stmt::Switch(sw));
                }
                TokenType::TildeFor
                | TokenType::TildeUntil
                | TokenType::TildeParallel
                | TokenType::TildeMap => {
                    if let Some(node) = self.parse_control_flow() {
                        step.sub_steps.push(node);
                    }
                }
                TokenType::CommandName | TokenType::Run => {
                    step.sub_steps
                        .push(Stmt::Command(self.parse_command_call()));
                }
                _ => self.advance(),
            }
        }
        self.nested_body_depth -= 1;
        self.pending_compensation = None;

        step
    }

    fn parse_step_body(&mut self) -> Option<Stmt> {
        if self.at_end() {
            return None;
        }
        match self.cur_ty() {
            TokenType::QIf | TokenType::QElif | TokenType::QElse => {
                self.parse_conditional().map(Stmt::Conditional)
            }
            TokenType::QSwitch => Some(Stmt::Switch(self.parse_switch())),
            TokenType::TildeFor
            | TokenType::TildeUntil
            | TokenType::TildeParallel
            | TokenType::TildeMap => self.parse_control_flow(),
            TokenType::CommandName | TokenType::Run => {
                Some(Stmt::Command(self.parse_command_call()))
            }
            TokenType::Variable => Some(self.parse_assignment_or_expr()),
            _ => {
                let raw = self.consume_rest_of_line();
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(Stmt::Comment(Comment {
                        text: trimmed.to_string(),
                    }))
                }
            }
        }
    }

    // ── Command calls ────────────────────────────────────────────────────────

    fn parse_command_call(&mut self) -> CommandCall {
        let name = self.cur_val();
        self.advance();

        let mut args = Vec::new();
        if self.check(TokenType::LParen) {
            self.advance();
            args = self.parse_arg_list();
            if self.check(TokenType::RParen) {
                self.advance();
            }
        }

        let mut modifiers = Vec::new();
        let mut validators = Vec::new();
        let mut flags = Vec::new();
        let mut target = None;
        // Set when a trailing ~COMPENSATE clause is parsed: that recursive
        // parse_command_call call already consumed through its own newline,
        // so the unconditional skip_to_newline() below must not run again —
        // it would otherwise find itself sitting at the START of the next
        // line (not at a newline) and swallow that entire next line/step.
        let mut compensation_consumed_line = false;

        while !self.at_end() {
            match self.cur_ty() {
                TokenType::Modifier => {
                    let mod_name = self.cur_val();
                    self.advance();
                    let mut mod_val = String::new();
                    if self.check(TokenType::Equals) {
                        self.advance();
                        mod_val = self.parse_modifier_value();
                    }
                    modifiers.push((mod_name, mod_val));
                }
                TokenType::Validator => {
                    validators.push(self.cur_val());
                    self.advance();
                }
                TokenType::Flag => {
                    flags.push(self.cur_val());
                    self.advance();
                }
                TokenType::Arrow => {
                    self.advance();
                    self.skip_noise();
                    if self.check(TokenType::Variable) {
                        target = Some(self.cur_val());
                        self.advance();
                        // ~COMPENSATE trails the pipeline target on the same
                        // line; only a step's own top-level action may
                        // carry one, never a nested loop/parallel body command.
                        // No skip_noise() here — that would cross a newline
                        // into the next line/step looking for a clause that,
                        // by definition, must be on this same line.
                        if self.nested_body_depth == 0 && self.check(TokenType::TildeCompensate) {
                            self.advance();
                            if self.check(TokenType::Colon) {
                                self.advance();
                            }
                            if matches!(self.cur_ty(), TokenType::CommandName | TokenType::Run) {
                                self.pending_compensation = Some(self.parse_command_call());
                                compensation_consumed_line = true;
                            }
                        }
                    } else {
                        target = Some(self.consume_rest_of_line().trim().to_string());
                    }
                    break;
                }
                _ => break,
            }
        }

        if !compensation_consumed_line {
            self.skip_to_newline();
        }
        CommandCall {
            name,
            args,
            modifiers,
            validators,
            flags,
            pipeline_target: target,
        }
    }

    fn parse_arg_list(&mut self) -> Vec<String> {
        let mut args = Vec::new();
        let mut depth = 0i32;
        let mut current = String::new();

        while !self.at_end() {
            let ty = self.cur_ty();
            if ty == TokenType::RParen && depth == 0 {
                break;
            }
            match ty {
                TokenType::LParen => {
                    depth += 1;
                    current.push('(');
                    self.advance();
                }
                TokenType::RParen => {
                    depth -= 1;
                    current.push(')');
                    self.advance();
                }
                TokenType::Comma if depth == 0 => {
                    if !current.is_empty() {
                        args.push(current.trim().to_string());
                        current.clear();
                    }
                    self.advance();
                }
                TokenType::Newline => self.advance(),
                _ => {
                    current.push_str(&self.cur_val());
                    self.advance();
                }
            }
        }

        if !current.is_empty() {
            args.push(current.trim().to_string());
        }
        args
    }

    fn parse_modifier_value(&mut self) -> String {
        self.skip_noise();
        if self.at_end() {
            return String::new();
        }
        match self.cur_ty() {
            TokenType::LBrace => self.collect_brace_raw(),
            TokenType::LBracket => self.collect_bracket_raw(),
            TokenType::Variable
            | TokenType::StringLit
            | TokenType::Number
            | TokenType::Identifier
            | TokenType::True
            | TokenType::False
            | TokenType::Null
            | TokenType::WfRef
            | TokenType::CommandName => {
                let mut val = self.cur_val();
                self.advance();
                while self.check(TokenType::Dot) {
                    self.advance();
                    if !self.at_end()
                        && matches!(
                            self.cur_ty(),
                            TokenType::Identifier | TokenType::CommandName
                        )
                    {
                        val.push('.');
                        val.push_str(&self.cur_val());
                        self.advance();
                    }
                }
                if self.check(TokenType::LBracket) {
                    val.push_str(&self.collect_bracket_raw());
                    if self.check(TokenType::Dot) {
                        self.advance();
                        if !self.at_end() {
                            val.push('.');
                            val.push_str(&self.cur_val());
                            self.advance();
                        }
                    }
                }
                val
            }
            _ => String::new(),
        }
    }

    // ── Conditionals ─────────────────────────────────────────────────────────

    fn parse_conditional(&mut self) -> Option<Conditional> {
        if self.at_end() {
            return None;
        }
        match self.cur_ty() {
            TokenType::QIf => Some(self.parse_if_chain()),
            TokenType::QElif => Some(self.parse_elif()),
            TokenType::QElse => Some(self.parse_else()),
            _ => None,
        }
    }

    fn parse_if_chain(&mut self) -> Conditional {
        self.advance(); // skip ?IF
        let cond = self.consume_until_action_sep();
        let (action, flags) = self.parse_branch_tail();

        let mut node = Conditional {
            condition: cond.trim().to_string(),
            action,
            break_flag: flags.break_flag,
            skip_flag: flags.skip_flag,
            override_flag: flags.override_flag,
            halt_flag: flags.halt_flag,
            pause_flag: flags.pause_flag,
            ..Default::default()
        };

        loop {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            if self.check(TokenType::QElif) {
                node.elif_branches.push(self.parse_elif());
            } else if self.check(TokenType::QElse) {
                node.else_branch = Some(Box::new(self.parse_else()));
                break;
            } else {
                break;
            }
        }
        node
    }

    fn parse_elif(&mut self) -> Conditional {
        self.advance(); // skip ?ELIF
        let cond = self.consume_until_action_sep();
        let (action, flags) = self.parse_branch_tail();
        Conditional {
            condition: cond.trim().to_string(),
            action,
            break_flag: flags.break_flag,
            skip_flag: flags.skip_flag,
            override_flag: flags.override_flag,
            halt_flag: flags.halt_flag,
            pause_flag: flags.pause_flag,
            ..Default::default()
        }
    }

    fn parse_else(&mut self) -> Conditional {
        self.advance(); // skip ?ELSE
        let (action, flags) = self.parse_branch_tail();
        Conditional {
            condition: String::new(),
            action,
            break_flag: flags.break_flag,
            skip_flag: flags.skip_flag,
            override_flag: flags.override_flag,
            halt_flag: flags.halt_flag,
            pause_flag: flags.pause_flag,
            ..Default::default()
        }
    }

    /// Parse the tail of a conditional branch: either `→ action [flags]` or a
    /// bare rest-of-line carrying only flags. Returns the optional action and
    /// the parsed control flags.
    fn parse_branch_tail(&mut self) -> (Option<CommandCall>, BranchFlags) {
        if self.check(TokenType::Arrow) {
            self.advance();
            let action_str = self.consume_rest_of_line();
            let flags = extract_flags(&action_str);
            let stripped = strip_flags(&action_str);
            (self.try_parse_command_from_string(&stripped), flags)
        } else if self.check(TokenType::Colon) {
            self.advance();
            self.skip_to_newline();
            (None, BranchFlags::default())
        } else {
            let rest = self.consume_rest_of_line();
            (None, extract_flags(&rest))
        }
    }

    // ── Control flow (loops, parallel) ───────────────────────────────────────

    fn parse_control_flow(&mut self) -> Option<Stmt> {
        match self.cur_ty() {
            TokenType::TildeFor => Some(Stmt::ForLoop(self.parse_for_loop())),
            TokenType::TildeUntil => Some(Stmt::UntilLoop(self.parse_until_loop())),
            TokenType::TildeParallel => Some(Stmt::Parallel(self.parse_parallel())),
            TokenType::TildeMap => Some(Stmt::Map(self.parse_map())),
            TokenType::TildeEnd | TokenType::TildeJoin => {
                self.advance();
                self.skip_to_newline();
                None
            }
            _ => {
                self.advance();
                None
            }
        }
    }

    fn parse_for_loop(&mut self) -> ForLoop {
        self.advance(); // skip ~FOR
        self.skip_noise();
        let mut variable = String::new();
        if self.check(TokenType::Variable) {
            variable = self.cur_val();
            self.advance();
        }
        self.skip_noise();
        if self.check(TokenType::In) {
            self.advance();
        }
        self.skip_noise();
        let mut collection = String::new();
        if self.check(TokenType::Variable) {
            collection = self.cur_val();
            self.advance();
        }
        if self.check(TokenType::Colon) {
            self.advance();
        }
        self.skip_to_newline();
        let body = self.collect_body_until_end();
        ForLoop {
            variable,
            collection,
            body,
        }
    }

    fn parse_until_loop(&mut self) -> UntilLoop {
        self.advance(); // skip ~UNTIL
        let mut cond_parts: Vec<String> = Vec::new();
        let mut max_iterations = None;

        while !self.at_end() {
            match self.cur_ty() {
                TokenType::Pipe => {
                    self.advance();
                    self.skip_noise();
                    if self.check(TokenType::Max) {
                        max_iterations = self.cur_val().parse::<u32>().ok();
                        self.advance();
                    }
                }
                TokenType::Colon => {
                    self.advance();
                    break;
                }
                TokenType::Newline => break,
                _ => {
                    cond_parts.push(self.cur_val());
                    self.advance();
                }
            }
        }

        self.skip_to_newline();
        let body = self.collect_body_until_end();
        UntilLoop {
            condition: cond_parts.join(" ").trim().to_string(),
            max_iterations,
            body,
        }
    }

    /// Parse a `~RETRY:n` bound. Returns `n`, or `0` when the bound is missing
    /// or unparseable — the validator rejects a `0` (and any `n > 10`) bound.
    fn parse_retry_bound(&mut self) -> u32 {
        self.advance(); // skip ~RETRY
        if self.check(TokenType::Colon) {
            self.advance();
        }
        if self.check(TokenType::Number) {
            let n = self.cur_val().parse::<u32>().unwrap_or(0);
            self.advance();
            n
        } else {
            0
        }
    }

    /// Parse `~MAP $coll: CMD($it) → $out` — a single-line collection transform.
    /// The per-element command's `→ $out` becomes the map's output target; the
    /// implicit `$it` binding is supplied at execution time.
    fn parse_map(&mut self) -> MapBlock {
        self.advance(); // skip ~MAP
        self.skip_noise();
        let mut collection = String::new();
        if self.check(TokenType::Variable) {
            collection = self.cur_val();
            self.advance();
        }
        if self.check(TokenType::Colon) {
            self.advance();
        }
        self.skip_noise();
        let mut command = if matches!(self.cur_ty(), TokenType::CommandName | TokenType::Run) {
            self.parse_command_call()
        } else {
            CommandCall::default()
        };
        // The command's pipeline target is the map's output collection target.
        let target = command.pipeline_target.take();
        MapBlock {
            collection,
            command,
            target,
        }
    }

    fn parse_parallel(&mut self) -> ParallelBlock {
        self.advance(); // skip ~PARALLEL
        if self.check(TokenType::Colon) {
            self.advance();
        }
        self.skip_to_newline();

        let mut branches = Vec::new();
        let mut join_target = None;

        self.nested_body_depth += 1;
        while !self.at_end() {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            match self.cur_ty() {
                TokenType::TildeJoin => {
                    self.advance();
                    if self.check(TokenType::Arrow) {
                        self.advance();
                        self.skip_noise();
                        if self.check(TokenType::Variable) {
                            join_target = Some(self.cur_val());
                            self.advance();
                        }
                    }
                    self.skip_to_newline();
                    break;
                }
                TokenType::TildeEnd => {
                    self.advance();
                    self.skip_to_newline();
                    break;
                }
                TokenType::CommandName | TokenType::Run => {
                    branches.push(Stmt::Command(self.parse_command_call()));
                }
                TokenType::Comment => self.advance(),
                _ => self.advance(),
            }
        }
        self.nested_body_depth -= 1;

        ParallelBlock {
            branches,
            join_target,
        }
    }

    /// Parse `?SWITCH $v: value → action … * → action ~END`. Arms are collected
    /// in declaration order; a `*` value becomes the default arm. First match
    /// wins is enforced at execution, not here.
    fn parse_switch(&mut self) -> SwitchBlock {
        self.advance(); // skip ?SWITCH
        self.skip_noise();
        let mut scrutinee = String::new();
        if self.check(TokenType::Variable) {
            scrutinee = self.cur_val();
            self.advance();
        }
        if self.check(TokenType::Colon) {
            self.advance();
        }
        self.skip_to_newline();

        let mut arms = Vec::new();
        let mut default = None;
        while !self.at_end() {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            match self.cur_ty() {
                TokenType::TildeEnd => {
                    self.advance();
                    self.skip_to_newline();
                    break;
                }
                TokenType::Comment => self.advance(),
                TokenType::StepNumber
                | TokenType::Section
                | TokenType::AtTest
                | TokenType::AtMacro
                | TokenType::Eof => break,
                _ => {
                    let value = self.consume_until_action_sep();
                    if self.check(TokenType::Arrow) {
                        self.advance();
                        let action_str = self.consume_rest_of_line();
                        if let Some(action) = self.try_parse_command_from_string(action_str.trim())
                        {
                            if value.trim() == "*" {
                                default = Some(action);
                            } else {
                                arms.push((value.trim().to_string(), action));
                            }
                        }
                    } else {
                        self.skip_to_newline();
                    }
                }
            }
        }

        SwitchBlock {
            scrutinee,
            arms,
            default,
        }
    }

    fn collect_body_until_end(&mut self) -> Vec<Stmt> {
        self.nested_body_depth += 1;
        let mut body = Vec::new();
        while !self.at_end() {
            self.skip_noise();
            if self.at_end() {
                break;
            }
            if self.check(TokenType::TildeEnd) {
                self.advance();
                self.skip_to_newline();
                break;
            }
            if self.check(TokenType::Comment) {
                self.advance();
                continue;
            }
            if let Some(node) = self.parse_step_body() {
                body.push(node);
            }
        }
        self.nested_body_depth -= 1;
        body
    }

    // ── Test / macro blocks ──────────────────────────────────────────────────

    fn parse_test_block(&mut self) -> TestBlock {
        let name = self.cur_val();
        self.advance(); // skip @test:name
        self.skip_noise();
        let raw_lines = self.collect_braced_raw_lines();
        let (input, expected, tags) = Self::parse_test_body(&raw_lines);
        self.skip_to_newline();
        TestBlock {
            name,
            input,
            expected,
            tags,
            raw_lines,
        }
    }

    /// Parse structured `input:` / `expected:` / `tags:` fields from the flat
    /// list of raw token values collected from a `@test:` block body.
    ///
    /// Section keywords ("input", "expected", "tags") followed by ":" switch
    /// the active section. Within Input/Expected, consecutive triples
    /// `key ":" value` are collected. Within Tags, identifiers between `[` `]`
    /// are collected as tag strings.
    #[allow(clippy::type_complexity)]
    fn parse_test_body(
        raw: &[String],
    ) -> (Vec<(String, String)>, Vec<(String, String)>, Vec<String>) {
        #[derive(PartialEq, Clone, Copy)]
        enum Section {
            None,
            Input,
            Expected,
            Tags,
        }

        let mut input = Vec::new();
        let mut expected = Vec::new();
        let mut tags = Vec::new();
        let mut section = Section::None;
        let mut i = 0usize;

        while i < raw.len() {
            let tok = &raw[i];

            // Detect section header: section keyword followed by ":"
            if matches!(tok.as_str(), "input" | "expected" | "tags")
                && raw.get(i + 1).map(|s| s == ":").unwrap_or(false)
            {
                section = match tok.as_str() {
                    "input" => Section::Input,
                    "expected" => Section::Expected,
                    "tags" => Section::Tags,
                    _ => Section::None,
                };
                i += 2; // consume keyword + ":"
                continue;
            }

            match section {
                Section::Input | Section::Expected => {
                    // key ":" value pattern: key is identifier or $variable
                    if raw.get(i + 1).map(|s| s == ":").unwrap_or(false)
                        && let Some(val) = raw.get(i + 2)
                    {
                        if section == Section::Input {
                            input.push((tok.clone(), val.clone()));
                        } else {
                            expected.push((tok.clone(), val.clone()));
                        }
                        i += 3;
                        continue;
                    }
                }
                Section::Tags => {
                    if !matches!(tok.as_str(), "[" | "]" | ",") {
                        tags.push(tok.clone());
                    }
                }
                Section::None => {}
            }
            i += 1;
        }

        (input, expected, tags)
    }

    fn parse_macro_block(&mut self) -> MacroBlock {
        let name = self.cur_val();
        self.advance(); // skip @macro:name
        self.skip_noise();
        let raw_lines = self.collect_braced_raw_lines();
        self.skip_to_newline();
        MacroBlock { name, raw_lines }
    }

    fn collect_braced_raw_lines(&mut self) -> Vec<String> {
        let mut raw = Vec::new();
        if !self.check(TokenType::LBrace) {
            return raw;
        }
        self.advance();
        let mut depth = 1i32;
        while !self.at_end() && depth > 0 {
            let ty = self.cur_ty();
            if ty == TokenType::LBrace {
                depth += 1;
            } else if ty == TokenType::RBrace {
                depth -= 1;
                if depth == 0 {
                    self.advance();
                    break;
                }
            }
            if ty != TokenType::Newline {
                raw.push(self.cur_val());
            }
            self.advance();
        }
        raw
    }

    // ── Assignment ───────────────────────────────────────────────────────────

    fn parse_assignment_or_expr(&mut self) -> Stmt {
        let var_name = self.cur_val();
        self.advance();
        if self.check(TokenType::Equals) {
            self.advance();
            let val = self.consume_rest_of_line().trim().to_string();
            Stmt::Command(CommandCall {
                name: "ASSIGN".to_string(),
                args: vec![var_name.clone(), val],
                pipeline_target: Some(var_name),
                ..Default::default()
            })
        } else {
            self.skip_to_newline();
            Stmt::VarRef(Variable { name: var_name })
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len() || self.tokens[self.pos].ty == TokenType::Eof
    }

    fn cur(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn cur_ty(&self) -> TokenType {
        self.tokens[self.pos].ty
    }

    fn cur_val(&self) -> String {
        self.tokens[self.pos].value.clone()
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn check(&self, ty: TokenType) -> bool {
        !self.at_end() && self.cur_ty() == ty
    }

    /// Look ahead `offset` tokens from the current position without consuming.
    fn peek_ty(&self, offset: usize) -> Option<TokenType> {
        self.tokens.get(self.pos + offset).map(|t| t.ty)
    }

    fn span(&self) -> Span {
        if self.at_end() {
            Span::new(0, 0)
        } else {
            Span::new(self.cur().line, self.cur().column)
        }
    }

    fn skip_noise(&mut self) {
        while !self.at_end() && self.cur_ty() == TokenType::Newline {
            self.advance();
        }
    }

    fn skip_to_newline(&mut self) {
        while !self.at_end() && self.cur_ty() != TokenType::Newline {
            self.advance();
        }
        if self.check(TokenType::Newline) {
            self.advance();
        }
    }

    fn consume_rest_of_line(&mut self) -> String {
        let mut parts = Vec::new();
        while !self.at_end() && !matches!(self.cur_ty(), TokenType::Newline | TokenType::Eof) {
            parts.push(self.cur_val());
            self.advance();
        }
        if self.check(TokenType::Newline) {
            self.advance();
        }
        parts.join(" ")
    }

    fn consume_until_arrow(&mut self) -> String {
        let mut parts = Vec::new();
        while !self.at_end()
            && !matches!(
                self.cur_ty(),
                TokenType::Arrow | TokenType::Newline | TokenType::Eof
            )
        {
            parts.push(self.cur_val());
            self.advance();
        }
        parts.join(" ")
    }

    fn consume_until_action_sep(&mut self) -> String {
        let mut parts = Vec::new();
        while !self.at_end() {
            if matches!(
                self.cur_ty(),
                TokenType::Arrow
                    | TokenType::Colon
                    | TokenType::Newline
                    | TokenType::Eof
                    | TokenType::BangBreak
                    | TokenType::BangSkip
                    | TokenType::BangOverride
                    | TokenType::BangHalt
                    | TokenType::BangPause
            ) {
                break;
            }
            parts.push(self.cur_val());
            self.advance();
        }
        parts.join(" ")
    }

    fn collect_comment_block(&mut self) -> String {
        let mut lines = Vec::new();
        while !self.at_end() && self.cur_ty() == TokenType::Comment {
            lines.push(self.cur_val());
            self.advance();
            self.skip_noise();
        }
        lines.join("\n")
    }

    fn collect_brace_raw(&mut self) -> String {
        let mut parts = vec!["{".to_string()];
        self.advance(); // skip {
        let mut depth = 1i32;
        while !self.at_end() && depth > 0 {
            match self.cur_ty() {
                TokenType::LBrace => depth += 1,
                TokenType::RBrace => depth -= 1,
                _ => {}
            }
            parts.push(self.cur_val());
            self.advance();
        }
        parts.join(" ")
    }

    fn collect_bracket_raw(&mut self) -> String {
        let mut parts = vec!["[".to_string()];
        self.advance(); // skip [
        let mut depth = 1i32;
        while !self.at_end() && depth > 0 {
            match self.cur_ty() {
                TokenType::LBracket => depth += 1,
                TokenType::RBracket => depth -= 1,
                _ => {}
            }
            parts.push(self.cur_val());
            self.advance();
        }
        parts.join(" ")
    }

    fn try_parse_command_from_string(&self, raw: &str) -> Option<CommandCall> {
        if raw.is_empty() {
            return None;
        }
        // A branch action / `@err:` handler MAY carry a trailing `→ $target`
        // pipeline destination (e.g. `CMD(args) → $target`). Split it off
        // BEFORE the `(` split below — otherwise the no-paren path folds the
        // arrow and target straight into the command name, and the
        // paren path folds it into the last argument.
        let (raw, pipeline_target) = match raw.rsplit_once('→') {
            Some((cmd, target)) => {
                let target = target.trim();
                (cmd.trim(), (!target.is_empty()).then(|| target.to_string()))
            }
            None => (raw.trim(), None),
        };
        if raw.is_empty() {
            return None;
        }
        let (name, rest) = match raw.split_once('(') {
            Some((n, r)) => (n.trim(), Some(r)),
            None => (raw, None),
        };
        if name.is_empty() || !name.chars().next().is_some_and(char::is_uppercase) {
            return Some(CommandCall {
                name: raw.to_string(),
                pipeline_target,
                ..Default::default()
            });
        }
        let args = match rest {
            Some(r) => {
                let arg_str = r.split_once(')').map_or(r, |(a, _)| a);
                arg_str
                    .split(',')
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .map(String::from)
                    .collect()
            }
            None => Vec::new(),
        };
        Some(CommandCall {
            name: name.to_string(),
            args,
            pipeline_target,
            ..Default::default()
        })
    }
}

/// The control-flow flags a conditional branch action may carry.
#[derive(Debug, Default, Clone, Copy)]
struct BranchFlags {
    break_flag: bool,
    skip_flag: bool,
    override_flag: bool,
    halt_flag: bool,
    pause_flag: bool,
}

/// Extract the control-flow flags present in an action string.
fn extract_flags(text: &str) -> BranchFlags {
    BranchFlags {
        break_flag: text.contains("!BREAK"),
        skip_flag: text.contains("!SKIP"),
        override_flag: text.contains("!OVERRIDE"),
        halt_flag: text.contains("!HALT"),
        pause_flag: text.contains("!PAUSE"),
    }
}

/// Remove the control-flow flags from an action string.
fn strip_flags(text: &str) -> String {
    text.replace("!BREAK", "")
        .replace("!SKIP", "")
        .replace("!OVERRIDE", "")
        .replace("!HALT", "")
        .replace("!PAUSE", "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"§wf:beautiful_mention v1.0
§runtime: { core: schema, mode: production }

@ON: new_mention
@in:  { post_url, tone?=neutral }
@ctx: [brand_voice]
@out: $draft
@err: ESCALATE(human)
!!NEVER: publish WITHOUT validate
!PREF: tone = brand_voice OVER tone = user

@steps:
  1. FETCH($post_url)               → $raw
  2. ANALYZE($raw) ~sentiment       → $meta
  3. ?IF $meta.sentiment < 0.2      → ROUTE(wf:crisis) !BREAK
  4. GEN(reply) +tone=$tone         → $draft
  5. VALIDATE($draft) ^brand_voice ^len:280
  6. PUBLISH($draft)
  7. LOG($draft)
"#;

    #[test]
    fn parses_header_and_version() {
        let wf = Parser::parse(SAMPLE).unwrap();
        let h = wf.header.unwrap();
        assert_eq!(h.file_type, FileType::Workflow);
        assert_eq!(h.name, "beautiful_mention");
        assert_eq!(h.version, "v1.0");
    }

    #[test]
    fn parses_runtime_mode() {
        let wf = Parser::parse(SAMPLE).unwrap();
        let rt = wf.runtime.unwrap();
        assert_eq!(rt.mode, "production");
        assert_eq!(rt.core, "schema");
    }

    #[test]
    fn parses_restart_max_when_declared() {
        let src = "§wf:retryable v1.0\n§runtime: { core: schema.nodus, restart_max: 3 }\n@steps:\n  1. LOG(x)\n";
        let wf = Parser::parse(src).unwrap();
        let rt = wf.runtime.unwrap();
        assert_eq!(rt.restart_max, Some(3));
    }

    #[test]
    fn restart_max_absent_when_not_declared() {
        let wf = Parser::parse(SAMPLE).unwrap();
        let rt = wf.runtime.unwrap();
        assert_eq!(rt.restart_max, None);
    }

    #[test]
    fn parses_trigger_rule_and_preference() {
        let wf = Parser::parse(SAMPLE).unwrap();
        assert_eq!(wf.triggers.len(), 1);
        assert_eq!(wf.triggers[0].condition, "new_mention");
        assert_eq!(wf.rules.len(), 1);
        assert_eq!(wf.rules[0].kind, Some(RuleKind::Never));
        assert_eq!(wf.rules[0].content, "publish WITHOUT validate");
        assert_eq!(wf.preferences.len(), 1);
        assert_eq!(wf.preferences[0].preferred, "tone = brand_voice");
        assert_eq!(wf.preferences[0].over, "tone = user");
    }

    #[test]
    fn parses_input_context_output_error() {
        let wf = Parser::parse(SAMPLE).unwrap();
        let input = wf.input_decl.unwrap();
        assert_eq!(input.fields.len(), 2);
        assert_eq!(input.fields[0].name, "post_url");
        let tone = &input.fields[1];
        assert_eq!(tone.name, "tone");
        assert!(tone.optional);
        assert_eq!(tone.default.as_deref(), Some("neutral"));

        assert_eq!(wf.context_decl.unwrap().contexts, vec!["brand_voice"]);
        assert_eq!(wf.output_decl.unwrap().variable, "$draft");

        let err = wf.error_decl.unwrap();
        assert_eq!(err.handler.unwrap().name, "ESCALATE");
    }

    #[test]
    fn parses_steps_with_pipeline_and_modifiers() {
        let wf = Parser::parse(SAMPLE).unwrap();
        assert_eq!(wf.steps.len(), 7);

        // Step 1: FETCH($post_url) → $raw
        let s1 = &wf.steps[0];
        assert_eq!(s1.number, 1);
        match s1.body.as_ref().unwrap() {
            Stmt::Command(c) => {
                assert_eq!(c.name, "FETCH");
                assert_eq!(c.args, vec!["$post_url"]);
                assert_eq!(c.pipeline_target.as_deref(), Some("$raw"));
            }
            other => panic!("expected command, got {other:?}"),
        }

        // Step 2: ANALYZE($raw) ~sentiment → $meta
        match wf.steps[1].body.as_ref().unwrap() {
            Stmt::Command(c) => {
                assert_eq!(c.name, "ANALYZE");
                assert_eq!(c.flags, vec!["sentiment"]);
                assert_eq!(c.pipeline_target.as_deref(), Some("$meta"));
            }
            other => panic!("expected command, got {other:?}"),
        }

        // Step 4: GEN(reply) +tone=$tone → $draft
        match wf.steps[3].body.as_ref().unwrap() {
            Stmt::Command(c) => {
                assert_eq!(c.name, "GEN");
                assert_eq!(
                    c.modifiers,
                    vec![("+tone".to_string(), "$tone".to_string())]
                );
            }
            other => panic!("expected command, got {other:?}"),
        }

        // Step 5: VALIDATE($draft) ^brand_voice ^len:280
        match wf.steps[4].body.as_ref().unwrap() {
            Stmt::Command(c) => {
                assert_eq!(c.name, "VALIDATE");
                assert_eq!(c.validators, vec!["^brand_voice", "^len:280"]);
            }
            other => panic!("expected command, got {other:?}"),
        }
    }

    #[test]
    fn parses_conditional_with_break() {
        let wf = Parser::parse(SAMPLE).unwrap();
        // Step 3: ?IF $meta.sentiment < 0.2 → ROUTE(wf:crisis) !BREAK
        match wf.steps[2].body.as_ref().unwrap() {
            Stmt::Conditional(c) => {
                assert_eq!(c.condition, "$meta.sentiment < 0.2");
                assert!(c.break_flag);
                let action = c.action.as_ref().unwrap();
                assert_eq!(action.name, "ROUTE");
                assert_eq!(action.args, vec!["wf:crisis"]);
            }
            other => panic!("expected conditional, got {other:?}"),
        }
    }

    #[test]
    fn parses_retry_bound_on_step() {
        let src = "§wf:retrier v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. ~RETRY:3 FETCH($url) → $data\n";
        let wf = Parser::parse(src).unwrap();
        assert_eq!(wf.steps[0].retry, Some(3), "the retry bound is captured");
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Command(c) => assert_eq!(c.name, "FETCH", "the action parses after the modifier"),
            other => panic!("expected command, got {other:?}"),
        }
    }

    #[test]
    fn retry_without_bound_marks_invalid() {
        let src = "§wf:retrier v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. ~RETRY GEN(x) → $out\n";
        let wf = Parser::parse(src).unwrap();
        assert_eq!(
            wf.steps[0].retry,
            Some(0),
            "a missing bound is recorded as Some(0)"
        );
    }

    #[test]
    fn parses_compensate_clause_trailing_pipeline_target() {
        let src = "§wf:publisher v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. PUBLISH($doc) → $url ~COMPENSATE: NOTIFY($url)\n";
        let wf = Parser::parse(src).unwrap();
        let comp = wf.steps[0]
            .compensation
            .as_ref()
            .expect("compensation must be captured");
        assert_eq!(comp.name, "NOTIFY");
        assert_eq!(comp.args, vec!["$url"]);
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Command(c) => {
                assert_eq!(c.name, "PUBLISH");
                assert_eq!(c.pipeline_target.as_deref(), Some("$url"));
            }
            other => panic!("expected command, got {other:?}"),
        }
    }

    #[test]
    fn no_compensation_when_clause_absent() {
        let src = "§wf:publisher v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. PUBLISH($doc) → $url\n";
        let wf = Parser::parse(src).unwrap();
        assert_eq!(wf.steps[0].compensation, None);
    }

    #[test]
    fn compensation_inside_for_loop_body_does_not_leak_to_outer_step() {
        let src = "§wf:looper v1.0\n§runtime: { core: schema.nodus }\n@in: { items: list }\n@steps:\n  1. ~FOR $item IN $in.items\n       PUBLISH($item) → $url ~COMPENSATE: NOTIFY($url)\n     ~END\n  2. LOG(done)\n";
        let wf = Parser::parse(src).unwrap();
        assert_eq!(
            wf.steps[0].compensation, None,
            "a ~COMPENSATE clause nested inside a ~FOR body must not attach to the enclosing step"
        );
        assert_eq!(wf.steps[1].compensation, None);
    }

    #[test]
    fn parses_map_block() {
        let src = "§wf:mapper v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. ~MAP $items: GEN($it) → $out\n";
        let wf = Parser::parse(src).unwrap();
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Map(mb) => {
                assert_eq!(mb.collection, "$items");
                assert_eq!(mb.command.name, "GEN");
                assert_eq!(mb.command.args, vec!["$it"]);
                assert_eq!(mb.target.as_deref(), Some("$out"));
                assert!(
                    mb.command.pipeline_target.is_none(),
                    "the → target moves to MapBlock, leaving the command targetless"
                );
            }
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn parses_switch_with_arms_and_default() {
        let src = "§wf:dispatch v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. ?SWITCH $category:\n    urgent → ROUTE(wf:crisis)\n    spam → LOG($m)\n    * → GEN(reply)\n  ~END\n";
        let wf = Parser::parse(src).unwrap();
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Switch(sw) => {
                assert_eq!(sw.scrutinee, "$category");
                assert_eq!(sw.arms.len(), 2, "two value arms (default excluded)");
                assert_eq!(sw.arms[0].0, "urgent");
                assert_eq!(sw.arms[0].1.name, "ROUTE");
                assert_eq!(sw.arms[1].0, "spam");
                assert_eq!(sw.arms[1].1.name, "LOG");
                let default = sw.default.as_ref().expect("default arm parsed");
                assert_eq!(default.name, "GEN");
            }
            other => panic!("expected switch, got {other:?}"),
        }
    }

    #[test]
    fn switch_arm_action_captures_pipeline_target() {
        let src = "§wf:dispatch v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. ?SWITCH $category:\n    urgent → GEN(x) → $picked\n  ~END\n";
        let wf = Parser::parse(src).unwrap();
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Switch(sw) => {
                assert_eq!(sw.arms[0].1.name, "GEN");
                assert_eq!(sw.arms[0].1.args, vec!["x"]);
                assert_eq!(
                    sw.arms[0].1.pipeline_target.as_deref(),
                    Some("$picked"),
                    "an arm action's trailing → target must be captured, not folded into args"
                );
            }
            other => panic!("expected switch, got {other:?}"),
        }
    }

    #[test]
    fn switch_default_action_captures_pipeline_target() {
        let src = "§wf:dispatch v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. ?SWITCH $category:\n    urgent → ROUTE(x)\n    * → GEN(reply) → $picked\n  ~END\n";
        let wf = Parser::parse(src).unwrap();
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Switch(sw) => {
                let default = sw.default.as_ref().expect("default arm parsed");
                assert_eq!(default.name, "GEN");
                assert_eq!(
                    default.pipeline_target.as_deref(),
                    Some("$picked"),
                    "the `*` default's trailing → target must be captured too"
                );
            }
            other => panic!("expected switch, got {other:?}"),
        }
    }

    #[test]
    fn inline_if_action_captures_pipeline_target() {
        let src = "§wf:guard v1\n@steps:\n  1. ?IF $r > 0.9 → GEN(x) → $picked\n";
        let wf = Parser::parse(src).unwrap();
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Conditional(c) => {
                let action = c.action.as_ref().expect("action parsed");
                assert_eq!(action.name, "GEN");
                assert_eq!(action.args, vec!["x"]);
                assert_eq!(
                    action.pipeline_target.as_deref(),
                    Some("$picked"),
                    "an inline ?IF action's trailing → target must attach, not be discarded"
                );
            }
            other => panic!("expected conditional, got {other:?}"),
        }
    }

    #[test]
    fn error_handler_captures_pipeline_target() {
        let src = "§wf:notifier v1.0\n§runtime: { core: schema.nodus }\n@err: NOTIFY(admin) → $picked\n@steps:\n  1. GEN(x) → $out\n";
        let wf = Parser::parse(src).unwrap();
        let handler = wf
            .error_decl
            .as_ref()
            .expect("error_decl parsed")
            .handler
            .as_ref()
            .expect("handler parsed");
        assert_eq!(handler.name, "NOTIFY");
        assert_eq!(handler.args, vec!["admin"]);
        assert_eq!(
            handler.pipeline_target.as_deref(),
            Some("$picked"),
            "an @err: handler's trailing → target must be captured too"
        );
    }

    #[test]
    fn parses_conditional_with_halt_and_pause() {
        let src = "§wf:guard v1\n@steps:\n  1. ?IF $r > 0.9 → ESCALATE(human) !HALT\n  2. ?IF $r > 0.5 → ASK(human) !PAUSE\n";
        let wf = Parser::parse(src).unwrap();
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::Conditional(c) => {
                assert!(c.halt_flag, "expected halt_flag set by !HALT");
                assert!(!c.pause_flag);
                assert_eq!(c.action.as_ref().unwrap().name, "ESCALATE");
            }
            other => panic!("expected conditional, got {other:?}"),
        }
        match wf.steps[1].body.as_ref().unwrap() {
            Stmt::Conditional(c) => {
                assert!(c.pause_flag, "expected pause_flag set by !PAUSE");
                assert!(!c.halt_flag);
            }
            other => panic!("expected conditional, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_workflow_and_empty() {
        assert!(Parser::parse("").is_err());
        assert!(Parser::parse("§schema:core v1.0\n").is_err());
        assert!(Parser::parse("no section here\n").is_err());
    }

    #[test]
    fn parses_until_loop_with_max() {
        let src = "§wf:loop v1\n@steps:\n  ~UNTIL $q > 0.85 | MAX:3:\n    REFINE($draft) → $draft\n  ~END\n";
        let wf = Parser::parse(src).unwrap();
        assert_eq!(wf.steps.len(), 1);
        match wf.steps[0].body.as_ref().unwrap() {
            Stmt::UntilLoop(u) => {
                assert_eq!(u.max_iterations, Some(3));
                assert_eq!(u.condition, "$q > 0.85");
                assert_eq!(u.body.len(), 1);
            }
            other => panic!("expected until loop, got {other:?}"),
        }
    }

    #[test]
    fn parse_test_body_extracts_structured_fields() {
        // raw_lines as the lexer would produce for:
        //   {
        //     input:
        //       query: "hello"
        //     expected:
        //       $out: "hello"
        //     tags: [smoke, unit]
        //   }
        let raw: Vec<String> = vec![
            "input", ":", "query", ":", "hello", "expected", ":", "$out", ":", "hello", "tags",
            ":", "[", "smoke", ",", "unit", "]",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();

        let (input, expected, tags) = Parser::parse_test_body(&raw);

        assert_eq!(input, vec![("query".to_owned(), "hello".to_owned())]);
        assert_eq!(expected, vec![("$out".to_owned(), "hello".to_owned())]);
        assert_eq!(tags, vec!["smoke".to_owned(), "unit".to_owned()]);
    }

    #[test]
    fn parse_test_body_empty_sections() {
        let raw: Vec<String> = vec![];
        let (input, expected, tags) = Parser::parse_test_body(&raw);
        assert!(input.is_empty());
        assert!(expected.is_empty());
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_test_block_structured() {
        let src = "\
§wf:tb_test v1.0
§runtime: { core: schema.nodus }
@in: { query }
@out: $out
@err: ESCALATE(human)
@steps:
  1. GEN($in.query) → $out
@test: smoke {
  input:
    query: \"world\"
  expected:
    $out: \"world\"
  tags: [unit]
}
";
        let wf = Parser::parse(src).unwrap();
        assert_eq!(wf.tests.len(), 1);
        let tb = &wf.tests[0];
        assert_eq!(tb.name, "smoke");
        assert_eq!(tb.input, vec![("query".to_owned(), "world".to_owned())]);
        assert_eq!(tb.expected, vec![("$out".to_owned(), "world".to_owned())]);
        assert_eq!(tb.tags, vec!["unit".to_owned()]);
    }

    // ── §config: parsing ────────────────────────────────────────────

    const CONFIG_SAMPLE: &str = "\
§config:settings v1.0
max_retries : int
  default: 3
  range: 1, 10
  describe: \"Maximum retry attempts\"
api_key : str
  required
  secret
  describe: \"External API credential\"
level : str
  default: medium
  one_of: low | medium | high
";

    #[test]
    fn parse_config_reads_header() {
        let decl = Parser::parse_config(CONFIG_SAMPLE).expect("parse_config");
        let h = decl.header.unwrap();
        assert_eq!(h.file_type, FileType::Config);
        assert_eq!(h.name, "settings");
        assert_eq!(h.version, "v1.0");
    }

    #[test]
    fn parse_config_reads_all_fields_in_order() {
        let decl = Parser::parse_config(CONFIG_SAMPLE).expect("parse_config");
        assert_eq!(decl.fields.len(), 3);
        assert_eq!(decl.fields[0].name, "max_retries");
        assert_eq!(decl.fields[1].name, "api_key");
        assert_eq!(decl.fields[2].name, "level");
    }

    #[test]
    fn parse_config_reads_range_constraint_and_default() {
        let decl = Parser::parse_config(CONFIG_SAMPLE).expect("parse_config");
        let f = &decl.fields[0];
        assert_eq!(f.type_name, "int");
        assert_eq!(f.default.as_deref(), Some("3"));
        match &f.constraint {
            Some(FieldConstraint::Range { lo, hi }) => {
                assert_eq!(lo, "1");
                assert_eq!(hi, "10");
            }
            other => panic!("expected Range constraint, got {other:?}"),
        }
        assert_eq!(f.describe.as_deref(), Some("Maximum retry attempts"));
    }

    #[test]
    fn parse_config_reads_required_and_secret() {
        let decl = Parser::parse_config(CONFIG_SAMPLE).expect("parse_config");
        let f = &decl.fields[1];
        assert_eq!(f.type_name, "str");
        assert!(f.required);
        assert!(f.secret);
        assert!(!decl.fields[0].secret, "unrelated field must stay unmarked");
    }

    #[test]
    fn parse_config_reads_one_of_constraint() {
        let decl = Parser::parse_config(CONFIG_SAMPLE).expect("parse_config");
        let f = &decl.fields[2];
        assert_eq!(f.default.as_deref(), Some("medium"));
        match &f.constraint {
            Some(FieldConstraint::OneOf(vals)) => {
                assert_eq!(
                    vals,
                    &vec!["low".to_string(), "medium".to_string(), "high".to_string()]
                );
            }
            other => panic!("expected OneOf constraint, got {other:?}"),
        }
    }

    #[test]
    fn parse_config_rejects_non_config_header() {
        let err = Parser::parse_config("§wf:x v1.0\n@steps:\n  1. LOG(x)\n").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn parse_rejects_config_header_with_redirect() {
        let err = Parser::parse(CONFIG_SAMPLE).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("parse_config"),
            "expected a redirect to parse_config, got: {msg}"
        );
    }

    #[test]
    fn parse_schema_header_still_deferred() {
        let err = Parser::parse("§schema:x v1.0\n").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not yet implemented"),
            "schema deferral message must be preserved, got: {msg}"
        );
    }
}
