//! Transpiler — lossless conversion between the compact NODUS form and the
//! human-readable HUMAN form.
//!
//! Both directions route through the [`crate::ast`] node model, so a compact →
//! AST → compact round-trip is guaranteed to produce an AST that compares equal
//! to the original (the dual-representation invariant). The human form is not
//! re-parseable back to AST; it is a one-way readable rendering.
//!
//! Verb mapping and vocabulary are consumed from [`crate::vocab`] constants so
//! the transpiler stays in sync with the schema without duplicating data.

use crate::ast::{
    CommandCall, Conditional, ConfigDecl, FieldConstraint, ForLoop, MapBlock, ParallelBlock,
    RuleKind, Step, Stmt, SwitchBlock, UntilLoop, WorkflowFile,
};
use crate::vocab::TRANSPILER_VERB_MAP;

/// Converts a [`WorkflowFile`] AST between the compact NODUS and readable
/// HUMAN representations.
pub struct Transpiler;

impl Transpiler {
    // ─── NODUS → HUMAN ───────────────────────────────────────────────────────

    /// Render the AST as a plain-language HUMAN mode description.
    pub fn to_human(ast: &WorkflowFile) -> String {
        let mut lines: Vec<String> = Vec::new();

        if let Some(h) = &ast.header {
            lines.push(format!("WORKFLOW: {}", h.name));
        }
        lines.push(String::new());

        if !ast.triggers.is_empty() {
            let desc: Vec<String> = ast
                .triggers
                .iter()
                .map(|t| Self::humanize_trigger(&t.condition))
                .collect();
            lines.push(format!("TRIGGER: {}", desc.join(", ")));
            lines.push(String::new());
        }

        if let Some(input) = &ast.input_decl
            && !input.fields.is_empty()
        {
            let parts: Vec<String> = input
                .fields
                .iter()
                .map(|f| {
                    let mut s = f.name.clone();
                    if f.optional {
                        if let Some(d) = &f.default {
                            s.push_str(&format!(" (default: {d})"));
                        } else {
                            s.push_str(" (optional)");
                        }
                    }
                    s
                })
                .collect();
            lines.push(format!("INPUT: {}", parts.join(", ")));
        }

        if let Some(ctx) = &ast.context_decl
            && !ctx.contexts.is_empty()
        {
            lines.push(format!("CONTEXT: load {}", ctx.contexts.join(", ")));
        }

        lines.push(String::new());

        if !ast.rules.is_empty() || !ast.preferences.is_empty() {
            lines.push("RULES:".to_string());
            for rule in &ast.rules {
                let kind = match rule.kind {
                    Some(RuleKind::Never) => "NEVER",
                    Some(RuleKind::Always) => "ALWAYS",
                    None => "",
                };
                lines.push(format!(
                    "  - {} {}",
                    kind,
                    Self::humanize_rule(&rule.content)
                ));
            }
            for pref in &ast.preferences {
                let mut s = format!("Prefer {} over {}", pref.preferred, pref.over);
                if let Some(c) = &pref.condition {
                    s.push_str(&format!(" if {c}"));
                }
                lines.push(format!("  - {s}"));
            }
            lines.push(String::new());
        }

        if !ast.steps.is_empty() {
            lines.push("STEPS:".to_string());
            for step in &ast.steps {
                let desc = Self::humanize_step(step);
                if !desc.is_empty() {
                    lines.push(format!("  {}. {}", step.number, desc));
                }
            }
            lines.push(String::new());
        }

        if let Some(out) = &ast.output_decl {
            lines.push(format!("OUTPUT: {}", Self::humanize_var(&out.variable)));
        }

        if let Some(err) = &ast.error_decl {
            lines.push(format!("ON ERROR: {}", Self::humanize_error(&err.raw)));
        }

        for test in &ast.tests {
            lines.push(String::new());
            lines.push(format!("TEST CASE: {}", test.name));
            if !test.input.is_empty() {
                let inputs: Vec<String> = test
                    .input
                    .iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect();
                lines.push(format!("  Inputs: {}", inputs.join(", ")));
            }
            if !test.expected.is_empty() {
                let asserts: Vec<String> = test
                    .expected
                    .iter()
                    .map(|(k, v)| format!("{} → {}", Self::humanize_var(k), v))
                    .collect();
                lines.push(format!("  Expects: {}", asserts.join(", ")));
            }
            if !test.tags.is_empty() {
                lines.push(format!("  Tags: {}", test.tags.join(", ")));
            }
        }

        lines.join("\n")
    }

    // ─── HUMAN → NODUS ───────────────────────────────────────────────────────

    /// Reconstruct the compact NODUS symbolic form from the AST.
    pub fn to_nodus(ast: &WorkflowFile) -> String {
        let mut lines: Vec<String> = Vec::new();

        if let Some(h) = &ast.header {
            if h.version.is_empty() {
                lines.push(format!("§wf:{}", h.name));
            } else {
                lines.push(format!("§wf:{} {}", h.name, h.version));
            }
        }

        // Free-standing `;;` comments (WorkflowFile.comments) carry no source
        // position — the parser accepts one at any point between recognized
        // sections — so re-emitting them right after the header (the common
        // fixture convention) is a faithful, order-preserving reconstruction.
        // `comment.text` already includes the leading `;;` (the lexer's
        // Comment token captures the whole line, unlike Stmt::Comment's text,
        // which is trimmed of it during a different construction path).
        for comment in &ast.comments {
            lines.push(comment.text.clone());
        }

        if let Some(rt) = &ast.runtime {
            lines.push("§runtime: {".to_string());
            lines.push(format!("  core:    {}", rt.core));
            if !rt.extends.is_empty() {
                lines.push(format!("  extends: [{}]", rt.extends.join(", ")));
            }
            if !rt.agents.is_empty() {
                let parts: Vec<String> =
                    rt.agents.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                lines.push(format!("  agents:  {{ {} }}", parts.join(", ")));
            }
            lines.push(format!("  mode:    {}", rt.mode));
            if let Some(n) = rt.restart_max {
                lines.push(format!("  restart_max: {n}"));
            }
            lines.push("}".to_string());
        }

        for trigger in &ast.triggers {
            lines.push(format!(
                "@ON: {} \u{2192} {}",
                trigger.condition, trigger.action
            ));
        }

        lines.push(String::new());

        for rule in &ast.rules {
            let kind = match rule.kind {
                Some(RuleKind::Never) => "NEVER",
                Some(RuleKind::Always) => "ALWAYS",
                None => "",
            };
            lines.push(format!("!!{kind}: {}", rule.content));
        }
        for pref in &ast.preferences {
            let mut s = format!("!PREF: {} OVER {}", pref.preferred, pref.over);
            if let Some(c) = &pref.condition {
                s.push_str(&format!(" IF {c}"));
            }
            lines.push(s);
        }

        lines.push(String::new());

        if let Some(input) = &ast.input_decl {
            let fields: Vec<String> = input
                .fields
                .iter()
                .map(|f| {
                    let mut s = f.name.clone();
                    if f.optional {
                        s.push('?');
                    }
                    if !f.type_name.is_empty() && f.type_name != "any" {
                        s.push_str(&format!(": {}", f.type_name));
                    }
                    if let Some(d) = &f.default {
                        s.push_str(&format!(" = {d}"));
                    }
                    s
                })
                .collect();
            lines.push(format!("@in: {{ {} }}", fields.join(", ")));
        }

        if let Some(ctx) = &ast.context_decl {
            lines.push(format!("@ctx: [{}]", ctx.contexts.join(", ")));
        }

        if let Some(out) = &ast.output_decl {
            lines.push(format!("@out: {}", out.variable));
        }

        if let Some(err) = &ast.error_decl {
            lines.push(format!("@err: {}", err.raw));
        }

        lines.push(String::new());

        if !ast.steps.is_empty() {
            lines.push("@steps:".to_string());
            for step in &ast.steps {
                let line = Self::nodus_step(step);
                if !line.is_empty() {
                    lines.push(format!("  {}. {}", step.number, line));
                }
            }
        }

        for test in &ast.tests {
            lines.push(String::new());
            lines.push(format!("@test:{} {{", test.name));
            // raw_lines is the lexed token stream (see the parser's test-body
            // handling) and is the emission source whenever present — it is the only
            // representation that reproduces the body in the form it was
            // written; the structured fields are a derived view and, being
            // non-empty whenever raw_lines is, previously left this branch
            // unreachable. Fall back to structured emission only for a
            // TestBlock built programmatically with no raw_lines at all.
            if !test.raw_lines.is_empty() {
                lines.push(format!(
                    "  {}",
                    Self::nodus_braced_raw_body(&test.raw_lines)
                ));
            } else if !test.input.is_empty() || !test.expected.is_empty() || !test.tags.is_empty() {
                if !test.input.is_empty() {
                    lines.push("  input:".to_string());
                    for (k, v) in &test.input {
                        lines.push(format!("    {k}: {v}"));
                    }
                }
                if !test.expected.is_empty() {
                    lines.push("  expected:".to_string());
                    for (k, v) in &test.expected {
                        lines.push(format!("    {k}: {v}"));
                    }
                }
                if !test.tags.is_empty() {
                    lines.push(format!("  tags: [{}]", test.tags.join(", ")));
                }
            }
            lines.push("}".to_string());
        }

        // @macro: blocks (previously never emitted at all). Shares
        // MacroBlock's raw_lines shape and the same collect_braced_raw_lines
        // parser helper as @test:, so it reuses the same renderer rather than
        // forking a second one. The normative corpus's own macro_expand.nodus
        // uses the non-braced `@macro: name` form (no body captured today —
        // macro body expansion is a deferred feature),
        // which collect_braced_raw_lines represents identically to an empty
        // braced body, so raw_lines is empty and no body line is emitted.
        for macro_block in &ast.macros {
            lines.push(String::new());
            if macro_block.raw_lines.is_empty() {
                lines.push(format!("@macro:{}", macro_block.name));
            } else {
                lines.push(format!("@macro:{} {{", macro_block.name));
                lines.push(format!(
                    "  {}",
                    Self::nodus_braced_raw_body(&macro_block.raw_lines)
                ));
                lines.push("}".to_string());
            }
        }

        // `;; HUMAN MODE` block (previously never emitted at all).
        // Emitted LAST and deliberately: the parser routes a comment
        // containing "HUMAN MODE" into collect_comment_block(), which
        // greedily consumes every following Comment token — any
        // free-standing comment emitted after this point would be silently
        // absorbed into human_mode on re-parse, corrupting both it and
        // `comments` (§ above). collect_comment_block joins raw token values
        // with "\n" and the lines already carry their own ";;" prefix, so
        // this is emitted verbatim.
        if let Some(human_mode) = &ast.human_mode {
            lines.push(String::new());
            lines.push(human_mode.clone());
        }

        lines.join("\n")
    }

    // ─── Humanizers ──────────────────────────────────────────────────────────

    fn humanize_trigger(condition: &str) -> String {
        let cond = condition.trim();
        if cond.starts_with("new_") {
            return format!("when a {} is received", cond.replace('_', " "));
        }
        if cond.contains("schedule:") {
            let time = cond
                .split_once("schedule:")
                .map(|x| x.1)
                .unwrap_or("")
                .trim();
            return format!("every day at {time}");
        }
        if cond.contains("webhook:") {
            let hook = cond
                .split_once("webhook:")
                .map(|x| x.1)
                .unwrap_or("")
                .trim();
            return format!("when {} webhook fires", hook.replace('_', " "));
        }
        if cond.contains("CONTAINS") {
            return format!("when {}", cond.to_lowercase());
        }
        format!("when {cond}")
    }

    fn humanize_rule(content: &str) -> String {
        content
            .to_lowercase()
            .replace("$out", "the output")
            .replace("$error.level", "error level")
    }

    fn humanize_step(step: &Step) -> String {
        if !step.comment.is_empty() {
            let text = step.comment.trim_start_matches(';').trim();
            let text = text.trim_start_matches('\u{2014}').trim();
            return text.to_string();
        }
        match &step.body {
            Some(Stmt::Command(cmd)) => Self::humanize_command(cmd),
            Some(Stmt::Conditional(cond)) => Self::humanize_conditional(cond),
            Some(Stmt::ForLoop(fl)) => Self::humanize_for(fl),
            Some(Stmt::UntilLoop(ul)) => Self::humanize_until(ul),
            Some(Stmt::Parallel(_)) => "Run the following in parallel".to_string(),
            Some(Stmt::Switch(sw)) => Self::humanize_switch(sw),
            Some(Stmt::Map(mb)) => Self::humanize_map(mb),
            Some(Stmt::Comment(c)) => c.text.trim_start_matches(';').trim().to_string(),
            Some(Stmt::VarRef(v)) => v.name.clone(),
            None => String::new(),
        }
    }

    fn humanize_map(mb: &MapBlock) -> String {
        let mut s = format!(
            "Map {} over {}",
            Self::humanize_command(&mb.command),
            Self::humanize_var(&mb.collection)
        );
        if let Some(target) = &mb.target {
            s.push_str(&format!(
                " \u{2192} store as {}",
                Self::humanize_var(target)
            ));
        }
        s
    }

    fn humanize_switch(sw: &SwitchBlock) -> String {
        let mut s = format!("Switch on {}", Self::humanize_var(&sw.scrutinee));
        for (value, action) in &sw.arms {
            s.push_str(&format!("; {value} → {}", Self::humanize_command(action)));
        }
        if let Some(default) = &sw.default {
            s.push_str(&format!(
                "; otherwise → {}",
                Self::humanize_command(default)
            ));
        }
        s
    }

    fn humanize_command(cmd: &CommandCall) -> String {
        let verb = TRANSPILER_VERB_MAP
            .iter()
            .find(|(k, _)| *k == cmd.name)
            .map(|(_, v)| *v)
            .unwrap_or(cmd.name.as_str());

        let args = cmd.args.join(", ");
        let mut desc = if args.is_empty() {
            verb.to_string()
        } else {
            format!("{verb} {args}")
        };

        if !cmd.flags.is_empty() {
            desc.push_str(&format!(" (extract: {})", cmd.flags.join(", ")));
        }
        if !cmd.validators.is_empty() {
            let rules: Vec<&str> = cmd
                .validators
                .iter()
                .map(|v| v.trim_start_matches('^'))
                .collect();
            desc.push_str(&format!(" against rules: {}", rules.join(", ")));
        }
        if let Some(target) = &cmd.pipeline_target {
            desc.push_str(&format!(
                " \u{2192} store as {}",
                Self::humanize_var(target)
            ));
        }
        desc
    }

    fn humanize_conditional(cond: &Conditional) -> String {
        let mut s = format!("IF {}", cond.condition);
        if let Some(action) = &cond.action {
            s.push_str(&format!(" \u{2192} {}", Self::humanize_command(action)));
        }
        if cond.break_flag {
            s.push_str(", STOP");
        }
        if cond.halt_flag {
            s.push_str(", HALT");
        }
        if cond.pause_flag {
            s.push_str(", PAUSE");
        }
        s
    }

    fn humanize_for(fl: &ForLoop) -> String {
        format!(
            "For each {} in {}",
            Self::humanize_var(&fl.variable),
            Self::humanize_var(&fl.collection)
        )
    }

    fn humanize_until(ul: &UntilLoop) -> String {
        let mut s = format!("Repeat until {}", ul.condition);
        if let Some(max) = ul.max_iterations {
            s.push_str(&format!(" (max {max} attempts)"));
        }
        s
    }

    fn humanize_var(var: &str) -> String {
        if var.is_empty() {
            return String::new();
        }
        var.trim_start_matches('$').replace('.', " \u{2192} ")
    }

    fn humanize_error(raw: &str) -> String {
        if raw.contains("ESCALATE") && raw.to_lowercase().contains("human") {
            return "escalate to human".to_string();
        }
        raw.to_string()
    }

    // ─── NODUS reconstructors ─────────────────────────────────────────────────

    fn nodus_step(step: &Step) -> String {
        match &step.body {
            Some(body) => {
                let mut first = String::new();
                if let Some(n) = step.retry {
                    first.push_str(&format!("~RETRY:{n} "));
                }
                first.push_str(&Self::nodus_stmt(body));
                // ~COMPENSATE only ever attaches when the step's own top-level
                // action is a direct command: ?IF/?SWITCH actions are parsed
                // via try_parse_command_from_string (a string re-parse), which
                // never sets pending_compensation — only parse_command_call
                // (used for a step's direct Stmt::Command body) does.
                if matches!(body, Stmt::Command(_))
                    && let Some(comp) = &step.compensation
                {
                    first.push_str(&format!(" ~COMPENSATE: {}", Self::nodus_command(comp)));
                }
                let mut lines = vec![first];
                // Indented sub-steps (e.g. a block-form `?IF cond:` header's
                // action lines) are collected by parse_step into Step.sub_steps
                // rather than into Conditional.body — they terminate on the
                // next StepNumber/section token, not an explicit ~END, so no
                // terminator is emitted here.
                Self::push_indented_body(&mut lines, &step.sub_steps);
                lines.join("\n")
            }
            None => {
                if !step.comment.is_empty() {
                    format!(";; {}", step.comment)
                } else {
                    String::new()
                }
            }
        }
    }

    /// Render any `Stmt` to its compact source form. Exhaustive by construction
    /// (no wildcard arm) so a future `Stmt` variant is a compile error here,
    /// not a silent drop — the shape of the defect this function used to have.
    fn nodus_stmt(stmt: &Stmt) -> String {
        match stmt {
            Stmt::Command(cmd) => Self::nodus_command_or_assignment(cmd),
            Stmt::Comment(c) => format!(";; {}", c.text),
            Stmt::VarRef(v) => v.name.clone(),
            Stmt::Conditional(cond) => Self::nodus_conditional_chain(cond),
            Stmt::Switch(sw) => Self::nodus_switch(sw),
            Stmt::ForLoop(fl) => Self::nodus_for(fl),
            Stmt::UntilLoop(ul) => Self::nodus_until(ul),
            Stmt::Parallel(pb) => Self::nodus_parallel(pb),
            Stmt::Map(mb) => Self::nodus_map(mb),
        }
    }

    /// Render an `?IF`/`?ELIF`/`?ELSE` chain. `Conditional.body` (the
    /// block-form `?IF cond:` shape) is never populated by the parser today —
    /// `parse_if_chain` never calls a body-collecting routine — so it is
    /// deliberately not rendered here rather than inventing untested syntax
    /// for an AST shape nothing currently produces.
    fn nodus_conditional_chain(cond: &Conditional) -> String {
        let mut lines = vec![Self::nodus_conditional_branch("?IF", cond)];
        for br in &cond.elif_branches {
            lines.push(Self::nodus_conditional_branch("?ELIF", br));
        }
        if let Some(else_br) = &cond.else_branch {
            lines.push(Self::nodus_conditional_branch("?ELSE", else_br));
        }
        lines.join("\n")
    }

    fn nodus_conditional_branch(keyword: &str, cond: &Conditional) -> String {
        let mut parts = vec![keyword.to_string()];
        if !cond.condition.is_empty() {
            parts.push(cond.condition.clone());
        }
        let mut flags: Vec<&str> = Vec::new();
        if cond.break_flag {
            flags.push("!BREAK");
        }
        if cond.skip_flag {
            flags.push("!SKIP");
        }
        if cond.override_flag {
            flags.push("!OVERRIDE");
        }
        if cond.halt_flag {
            flags.push("!HALT");
        }
        if cond.pause_flag {
            flags.push("!PAUSE");
        }

        match &cond.action {
            Some(action) => {
                parts.push("\u{2192}".to_string());
                parts.push(Self::nodus_command(action));
                parts.extend(flags.iter().map(|f| f.to_string()));
            }
            None if flags.is_empty() => {
                // No action, no flags — the block-form `?IF cond:` shape
                // (or a bare `?ELSE:`), matching parse_branch_tail's Colon arm.
                if let Some(last) = parts.last_mut() {
                    last.push(':');
                } else {
                    parts.push(":".to_string());
                }
            }
            None => {
                parts.extend(flags.iter().map(|f| f.to_string()));
            }
        }
        parts.join(" ")
    }

    fn nodus_switch(sw: &SwitchBlock) -> String {
        let mut lines = vec![format!("?SWITCH {}:", sw.scrutinee)];
        for (value, action) in &sw.arms {
            lines.push(format!(
                "  {value} \u{2192} {}",
                Self::nodus_command(action)
            ));
        }
        if let Some(default) = &sw.default {
            lines.push(format!("  * \u{2192} {}", Self::nodus_command(default)));
        }
        lines.push("~END".to_string());
        lines.join("\n")
    }

    fn nodus_for(fl: &ForLoop) -> String {
        let mut lines = vec![format!("~FOR {} IN {}", fl.variable, fl.collection)];
        Self::push_indented_body(&mut lines, &fl.body);
        lines.push("~END".to_string());
        lines.join("\n")
    }

    fn nodus_until(ul: &UntilLoop) -> String {
        let mut header = format!("~UNTIL {}", ul.condition);
        if let Some(max) = ul.max_iterations {
            header.push_str(&format!(" | MAX:{max}"));
        }
        header.push(':');
        let mut lines = vec![header];
        Self::push_indented_body(&mut lines, &ul.body);
        lines.push("~END".to_string());
        lines.join("\n")
    }

    fn nodus_parallel(pb: &ParallelBlock) -> String {
        let mut lines = vec!["~PARALLEL:".to_string()];
        Self::push_indented_body(&mut lines, &pb.branches);
        match &pb.join_target {
            Some(target) => lines.push(format!("~JOIN \u{2192} {target}")),
            None => lines.push("~END".to_string()),
        }
        lines.join("\n")
    }

    fn nodus_map(mb: &MapBlock) -> String {
        // The block's `→ target` lives on MapBlock.target, not the inner
        // command (parse_map moves it there via pipeline_target.take()) —
        // attach it back onto a clone so nodus_command renders it, mirroring
        // the parser's move in reverse.
        let mut cmd = mb.command.clone();
        cmd.pipeline_target = mb.target.clone();
        format!("~MAP {}: {}", mb.collection, Self::nodus_command(&cmd))
    }

    /// Append each body statement's rendered lines to `lines`, indented two
    /// spaces per nesting level. Indentation is cosmetic — the lexer is
    /// whitespace-insensitive between tokens, nesting is bounded by `~END`
    /// (and `~JOIN`) tokens, not indentation — but keeps emitted source
    /// readable and mirrors the human-authored fixture style.
    fn push_indented_body(lines: &mut Vec<String>, body: &[Stmt]) {
        for stmt in body {
            for line in Self::nodus_stmt(stmt).lines() {
                lines.push(format!("  {line}"));
            }
        }
    }

    /// `$var = expr` is surface sugar `parse_assignment_or_expr` represents
    /// internally as a synthetic `Stmt::Command { name: "ASSIGN", args:
    /// [var, expr], pipeline_target: Some(var), .. }` — reusing the Command
    /// shape rather than adding a new Stmt variant. "ASSIGN" is not in
    /// `KNOWN_COMMANDS`, so it lexes as a plain Identifier, not a
    /// CommandName: emitting it via the generic call syntax cannot round-trip
    /// (confirmed by the corpus harness on ticket_triage.nodus's step 9).
    /// Detect the exact shape `parse_assignment_or_expr` produces and emit
    /// the shorthand back instead; anything else falls through to the
    /// ordinary command-call rendering.
    /// Render a `raw_lines` token stream (shared by `@test:` and `@macro:`
    /// bodies — `collect_braced_raw_lines` backs both) back to source text.
    /// `raw_lines` is a `Vec<String>` of already-lexed token
    /// *values*, with no type information, so this cannot distinguish a
    /// separator from a value that happens to look like one — it can only
    /// name the two token values (`{`, `}`) that must stay unquoted.
    ///
    /// `{`/`}` must remain literal `LBrace`/`RBrace` tokens: the depth
    /// counter in `collect_braced_raw_lines` matches on token *type*, not
    /// value, so quoting one would desynchronize where the reparse thinks
    /// the block ends. Every other element — including `:`, `,`, `[`, `]`,
    /// and the `input`/`expected`/`tags` section keywords — is matched by
    /// `parse_test_body` purely on string *value* (`tok.as_str() == ":"`,
    /// never a token-type check), so quoting them changes nothing there;
    /// quoting is what makes a value containing whitespace or a
    /// token-splitting character (`"When is my invoice due?"`, `"T-001"`)
    /// re-lex to the single token it came from instead of splitting.
    fn nodus_braced_raw_body(raw_lines: &[String]) -> String {
        raw_lines
            .iter()
            .map(|tok| {
                if tok == "{" || tok == "}" {
                    tok.clone()
                } else {
                    format!("\"{tok}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn nodus_command_or_assignment(cmd: &CommandCall) -> String {
        let is_assignment_shorthand = cmd.name == "ASSIGN"
            && cmd.args.len() == 2
            && cmd.modifiers.is_empty()
            && cmd.validators.is_empty()
            && cmd.flags.is_empty()
            && cmd.pipeline_target.as_deref() == Some(cmd.args[0].as_str());
        if is_assignment_shorthand {
            return format!("{} = {}", cmd.args[0], cmd.args[1]);
        }
        Self::nodus_command(cmd)
    }

    fn nodus_command(cmd: &CommandCall) -> String {
        let mut parts = vec![format!("{}({})", cmd.name, cmd.args.join(", "))];
        for (mod_name, mod_val) in &cmd.modifiers {
            if !mod_val.is_empty() {
                // A modifier value containing whitespace was necessarily
                // quoted in the source (a bare multi-token value would have
                // been lexed as separate tokens and parse_modifier_value only
                // consumes one) — re-quote it, or the reparse silently drops
                // everything after the first token (an AST-equality loss
                // the corpus round-trip harness caught on ticket_triage.nodus's
                // `+msg="Critical ticket"`).
                let val = if mod_val.chars().any(char::is_whitespace) {
                    format!("\"{mod_val}\"")
                } else {
                    mod_val.clone()
                };
                parts.push(format!("{mod_name}={val}"));
            } else {
                parts.push(mod_name.clone());
            }
        }
        for v in &cmd.validators {
            parts.push(v.clone());
        }
        for f in &cmd.flags {
            parts.push(format!("~{f}"));
        }
        if let Some(target) = &cmd.pipeline_target {
            parts.push(format!("\u{2192} {target}"));
        }
        parts.join(" ")
    }

    // ─── §config: round-trip ────────────────────────────────────────

    /// Render a [`ConfigDecl`] back to its compact `§config:` NODUS form.
    ///
    /// Round-trip safe: `Parser::parse_config(&Transpiler::config_to_nodus(&decl))`
    /// reproduces `decl`. `secret` and `describe:` survive the trip; a `describe:`
    /// value is re-quoted so it re-lexes as a `StringLit`.
    pub fn config_to_nodus(decl: &ConfigDecl) -> String {
        let mut lines: Vec<String> = Vec::new();

        if let Some(h) = &decl.header {
            if h.version.is_empty() {
                lines.push(format!("§config:{}", h.name));
            } else {
                lines.push(format!("§config:{} {}", h.name, h.version));
            }
        }

        for f in &decl.fields {
            lines.push(format!("{} : {}", f.name, f.type_name));
            if let Some(d) = &f.default {
                lines.push(format!("  default: {d}"));
            }
            match &f.constraint {
                Some(FieldConstraint::Range { lo, hi }) => {
                    lines.push(format!("  range: {lo}, {hi}"))
                }
                Some(FieldConstraint::OneOf(vals)) => {
                    lines.push(format!("  one_of: {}", vals.join(" | ")))
                }
                None => {}
            }
            if f.required {
                lines.push("  required".to_string());
            }
            if f.secret {
                lines.push("  secret".to_string());
            }
            if let Some(desc) = &f.describe {
                lines.push(format!("  describe: \"{desc}\""));
            }
        }

        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AbsoluteRule, CommandCall, ContextDecl, ErrorDecl, FileHeader, FileType, InputDecl,
        InputField, OutputDecl, Preference, RuleKind, Step, Stmt, Trigger, WorkflowFile,
    };
    use crate::parser::Parser;

    fn minimal_wf() -> WorkflowFile {
        WorkflowFile {
            header: Some(FileHeader {
                file_type: FileType::Workflow,
                name: "greet".to_string(),
                version: "v1.0".to_string(),
            }),
            triggers: vec![Trigger {
                condition: "new_message".to_string(),
                action: "handle".to_string(),
            }],
            rules: vec![AbsoluteRule {
                kind: Some(RuleKind::Never),
                content: "publish WITHOUT validate".to_string(),
            }],
            preferences: vec![Preference {
                preferred: "brief".to_string(),
                over: "verbose".to_string(),
                condition: Some("$user.is_vip".to_string()),
            }],
            input_decl: Some(InputDecl {
                fields: vec![
                    InputField {
                        name: "query".to_string(),
                        ..Default::default()
                    },
                    InputField {
                        name: "tone".to_string(),
                        type_name: "string".to_string(),
                        optional: true,
                        default: Some("neutral".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            context_decl: Some(ContextDecl {
                contexts: vec!["user_profile".to_string()],
            }),
            output_decl: Some(OutputDecl {
                variable: "$out".to_string(),
            }),
            error_decl: Some(ErrorDecl {
                raw: "ESCALATE(human)".to_string(),
                handler: None,
            }),
            steps: vec![
                Step {
                    number: 1,
                    body: Some(Stmt::Command(CommandCall {
                        name: "ANALYZE".to_string(),
                        args: vec!["$in.query".to_string()],
                        flags: vec!["sentiment".to_string()],
                        pipeline_target: Some("$sentiment".to_string()),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                Step {
                    number: 2,
                    body: Some(Stmt::Command(CommandCall {
                        name: "GEN".to_string(),
                        args: vec!["reply".to_string()],
                        modifiers: vec![("+tone".to_string(), "warm".to_string())],
                        validators: vec!["^len:280".to_string()],
                        pipeline_target: Some("$out".to_string()),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn nodus_command_reconstruction() {
        let cmd = CommandCall {
            name: "GEN".to_string(),
            args: vec!["reply".to_string()],
            modifiers: vec![("+tone".to_string(), "warm".to_string())],
            validators: vec!["^len:280".to_string()],
            flags: vec!["extract_key".to_string()],
            pipeline_target: Some("$draft".to_string()),
        };
        let out = Transpiler::nodus_command(&cmd);
        assert_eq!(
            out,
            "GEN(reply) +tone=warm ^len:280 ~extract_key \u{2192} $draft"
        );
    }

    #[test]
    fn nodus_command_no_args() {
        let cmd = CommandCall {
            name: "LOG".to_string(),
            ..Default::default()
        };
        assert_eq!(Transpiler::nodus_command(&cmd), "LOG()");
    }

    #[test]
    fn human_command_verb_map() {
        let gen_cmd = CommandCall {
            name: "GEN".to_string(),
            args: vec!["summary".to_string()],
            ..Default::default()
        };
        assert!(Transpiler::humanize_command(&gen_cmd).starts_with("Generate summary"));

        let fetch = CommandCall {
            name: "FETCH".to_string(),
            args: vec!["$url".to_string()],
            pipeline_target: Some("$raw".to_string()),
            ..Default::default()
        };
        let out = Transpiler::humanize_command(&fetch);
        assert!(out.starts_with("Fetch $url"));
        assert!(out.contains("store as raw"));

        // Unknown command falls back to its own name.
        let unknown = CommandCall {
            name: "CUSTOM_OP".to_string(),
            ..Default::default()
        };
        assert!(Transpiler::humanize_command(&unknown).starts_with("CUSTOM_OP"));
    }

    #[test]
    fn human_command_with_flags_and_validators() {
        let cmd = CommandCall {
            name: "ANALYZE".to_string(),
            args: vec!["$text".to_string()],
            flags: vec!["sentiment".to_string(), "tone".to_string()],
            validators: vec!["^len:500".to_string()],
            ..Default::default()
        };
        let out = Transpiler::humanize_command(&cmd);
        assert!(out.contains("(extract: sentiment, tone)"), "got: {out}");
        assert!(out.contains("against rules: len:500"), "got: {out}");
    }

    #[test]
    fn human_trigger_patterns() {
        assert_eq!(
            Transpiler::humanize_trigger("new_mention"),
            "when a new mention is received"
        );
        assert_eq!(
            Transpiler::humanize_trigger("schedule:08:00"),
            "every day at 08:00"
        );
        assert_eq!(
            Transpiler::humanize_trigger("webhook:order_placed"),
            "when order placed webhook fires"
        );
        assert_eq!(
            Transpiler::humanize_trigger("$in CONTAINS urgent"),
            "when $in contains urgent"
        );
        assert_eq!(
            Transpiler::humanize_trigger("user_request"),
            "when user_request"
        );
    }

    #[test]
    fn to_human_sections_present() {
        let wf = minimal_wf();
        let out = Transpiler::to_human(&wf);

        assert!(out.contains("WORKFLOW: greet"), "header: {out}");
        assert!(out.contains("TRIGGER:"), "trigger: {out}");
        assert!(out.contains("INPUT:"), "input: {out}");
        assert!(out.contains("CONTEXT: load user_profile"), "ctx: {out}");
        assert!(out.contains("RULES:"), "rules: {out}");
        assert!(out.contains("NEVER"), "rule kind: {out}");
        assert!(out.contains("Prefer brief over verbose"), "pref: {out}");
        assert!(out.contains("STEPS:"), "steps: {out}");
        assert!(out.contains("1."), "step 1: {out}");
        assert!(out.contains("2."), "step 2: {out}");
        assert!(out.contains("OUTPUT:"), "output: {out}");
        assert!(out.contains("ON ERROR:"), "error: {out}");
    }

    #[test]
    fn to_nodus_sections_present() {
        let wf = minimal_wf();
        let out = Transpiler::to_nodus(&wf);

        assert!(out.contains("§wf:greet v1.0"), "header: {out}");
        assert!(out.contains("@ON:"), "trigger: {out}");
        assert!(out.contains("!!NEVER:"), "rule: {out}");
        assert!(out.contains("!PREF:"), "pref: {out}");
        assert!(out.contains("@in:"), "in: {out}");
        assert!(out.contains("@ctx:"), "ctx: {out}");
        assert!(out.contains("@out: $out"), "out: {out}");
        assert!(out.contains("@err: ESCALATE(human)"), "err: {out}");
        assert!(out.contains("@steps:"), "steps: {out}");
        assert!(out.contains("1. ANALYZE"), "step 1: {out}");
        assert!(out.contains("~sentiment"), "flag: {out}");
        assert!(out.contains("2. GEN"), "step 2: {out}");
        assert!(out.contains("+tone=warm"), "modifier: {out}");
        assert!(out.contains("^len:280"), "validator: {out}");
    }

    #[test]
    fn round_trip_ast_equality() {
        // Parse a compact workflow to AST₁.
        let source = "\
§wf:rt_test v1.0
@ON: new_request → handle
!!NEVER: publish WITHOUT validate
!PREF: brief OVER verbose IF $user.is_vip
@in: { query, tone?: string = neutral }
@ctx: [user_profile]
@out: $out
@err: ESCALATE(human)
@steps:
  1. ANALYZE($in.query) ~sentiment → $sentiment
  2. GEN(reply) +tone=warm ^len:280 → $out
";
        let ast1 = Parser::parse(source).expect("parse source");

        // Reconstruct NODUS from AST₁.
        let nodus2 = Transpiler::to_nodus(&ast1);

        // Re-parse to AST₂ and compare.
        let ast2 = Parser::parse(&nodus2).expect("re-parse reconstructed NODUS");
        assert_eq!(ast1, ast2, "ASTs differ after round-trip:\n{nodus2}");
    }

    #[test]
    fn humanize_var_strips_dollar_and_replaces_dot() {
        assert_eq!(Transpiler::humanize_var("$out.data"), "out \u{2192} data");
        assert_eq!(Transpiler::humanize_var("$draft"), "draft");
        assert_eq!(Transpiler::humanize_var(""), "");
    }

    #[test]
    fn humanize_map_renders_collection_and_target() {
        use crate::ast::MapBlock;
        let mb = MapBlock {
            collection: "$items".to_string(),
            command: CommandCall {
                name: "GEN".to_string(),
                args: vec!["$it".to_string()],
                ..Default::default()
            },
            target: Some("$out".to_string()),
        };
        let human = Transpiler::humanize_map(&mb);
        assert!(human.contains("Map"), "got: {human}");
        assert!(human.contains("over items"), "got: {human}");
        assert!(human.contains("store as out"), "got: {human}");
    }

    #[test]
    fn humanize_switch_renders_arms_and_default() {
        use crate::ast::SwitchBlock;
        let sw = SwitchBlock {
            scrutinee: "$category".to_string(),
            arms: vec![(
                "urgent".to_string(),
                CommandCall {
                    name: "ROUTE".to_string(),
                    ..Default::default()
                },
            )],
            default: Some(CommandCall {
                name: "LOG".to_string(),
                ..Default::default()
            }),
        };
        let human = Transpiler::humanize_switch(&sw);
        assert!(human.contains("Switch on category"), "got: {human}");
        assert!(human.contains("urgent"), "got: {human}");
        assert!(human.contains("otherwise"), "got: {human}");
    }

    #[test]
    fn humanize_conditional_renders_halt_and_pause() {
        use crate::ast::Conditional;
        let halt = Conditional {
            condition: "$r > 0.9".to_string(),
            halt_flag: true,
            ..Default::default()
        };
        assert!(
            Transpiler::humanize_conditional(&halt).contains("HALT"),
            "human form must render !HALT"
        );
        let pause = Conditional {
            condition: "$r > 0.5".to_string(),
            pause_flag: true,
            ..Default::default()
        };
        assert!(
            Transpiler::humanize_conditional(&pause).contains("PAUSE"),
            "human form must render !PAUSE"
        );
    }

    #[test]
    fn humanize_error_escalate() {
        assert_eq!(
            Transpiler::humanize_error("ESCALATE(human)"),
            "escalate to human"
        );
        assert_eq!(Transpiler::humanize_error("LOG($error)"), "LOG($error)");
    }

    // ── §config: round-trip ─────────────────────────────────────────

    #[test]
    fn config_round_trips_through_nodus() {
        use crate::parser::Parser;

        let src = "\
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
        let decl = Parser::parse_config(src).expect("parse_config");
        let emitted = Transpiler::config_to_nodus(&decl);
        let decl2 = Parser::parse_config(&emitted).expect("re-parse emitted §config");
        assert_eq!(decl, decl2, "round-trip must reproduce the declaration");

        // Secret and describe survive the trip.
        assert!(decl2.fields[1].secret);
        assert_eq!(
            decl2.fields[1].describe.as_deref(),
            Some("External API credential")
        );
    }

    #[test]
    fn config_to_nodus_emits_header() {
        use crate::ast::{ConfigDecl, FileHeader, FileType};

        let decl = ConfigDecl {
            header: Some(FileHeader {
                file_type: FileType::Config,
                name: "settings".to_string(),
                version: "v1.0".to_string(),
            }),
            fields: vec![],
        };
        let out = Transpiler::config_to_nodus(&decl);
        assert!(out.contains("§config:settings v1.0"), "header: {out}");
    }

    #[test]
    fn compensation_clause_survives_compact_round_trip() {
        use crate::parser::Parser;

        let src = "§wf:publisher v1.0\n§runtime: { core: schema.nodus }\n@steps:\n  1. PUBLISH($doc) → $url ~COMPENSATE: NOTIFY($url)\n";
        let ast = Parser::parse(src).expect("parse");
        let compact = Transpiler::to_nodus(&ast);
        assert!(
            compact.contains("~COMPENSATE:"),
            "compact form must emit the clause: {compact}"
        );
        let ast2 = Parser::parse(&compact).expect("compact form must re-parse");
        let comp = ast2.steps[0]
            .compensation
            .as_ref()
            .expect("compensation must survive the round-trip");
        assert_eq!(comp.name, "NOTIFY");
        assert_eq!(comp.args, vec!["$url"]);
    }

    // ─── compact-form control-flow round-trip ────────
    //
    // Each asserts `parse(src).steps == parse(to_nodus(parse(src))).steps` —
    // the AST-equality the language actually mandates, never source-text equality.

    fn steps_round_trip(src: &str) {
        let ast = Parser::parse(src).expect("fixture must parse");
        let compact = Transpiler::to_nodus(&ast);
        let ast2 = Parser::parse(&compact)
            .unwrap_or_else(|e| panic!("compact re-parse failed: {e:?}\ncompact:\n{compact}"));
        assert_eq!(
            ast.steps, ast2.steps,
            "steps must be AST-equal after a compact round-trip\ncompact:\n{compact}"
        );
    }

    #[test]
    fn conditional_inline_action_round_trips() {
        steps_round_trip("§wf:t v1\n@steps:\n  1. ?IF $r > 0.9 → ESCALATE(human) !HALT\n");
    }

    #[test]
    fn conditional_chain_with_target_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@steps:\n  1. ?IF $r > 0.9 → GEN(x) → $picked\n     ?ELIF $r > 0.5 → GEN(y) → $picked\n     ?ELSE → GEN(z) → $picked\n",
        );
    }

    #[test]
    fn switch_with_arms_and_default_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@in: { category?=urgent }\n@steps:\n  1. ?SWITCH $in.category:\n    urgent → GEN(crisis) → $urgent_pick\n    spam → GEN(reply) → $spam_pick\n    * → GEN(default) → $def\n  ~END\n",
        );
    }

    #[test]
    fn for_loop_body_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@in: { items: list }\n@steps:\n  1. ~FOR $item IN $in.items\n       LOG($item) → $out\n     ~END\n",
        );
    }

    #[test]
    fn until_loop_with_max_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@steps:\n  1. ~UNTIL $q > 0.85 | MAX:3:\n       REFINE($draft) → $draft\n     ~END\n",
        );
    }

    #[test]
    fn until_loop_without_max_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@steps:\n  1. ~UNTIL $q > 0.85:\n       REFINE($draft) → $draft\n     ~END\n",
        );
    }

    #[test]
    fn parallel_with_join_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@steps:\n  1. ~PARALLEL:\n       GEN(a) → $x\n       ANALYZE(b) → $y\n     ~JOIN → $t\n",
        );
    }

    #[test]
    fn parallel_without_join_round_trips() {
        steps_round_trip("§wf:t v1\n@steps:\n  1. ~PARALLEL:\n       GEN(a) → $x\n     ~END\n");
    }

    #[test]
    fn map_block_target_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@in: { items: list }\n@steps:\n  1. ~MAP $in.items: GEN($it) → $out\n",
        );
    }

    #[test]
    fn retry_bound_round_trips() {
        steps_round_trip("§wf:t v1\n@steps:\n  1. ~RETRY:3 FETCH($url) → $data\n");
    }

    #[test]
    fn nested_switch_inside_for_loop_round_trips() {
        steps_round_trip(
            "§wf:t v1\n@in: { items: list, category?=urgent }\n@steps:\n  1. ~FOR $item IN $in.items\n       ?SWITCH $in.category:\n         urgent → GEN($item) → $r\n       ~END\n     ~END\n",
        );
    }

    // ─── @test: block round-trip ────────
    //
    // Asserts `parse(src).tests == parse(to_nodus(parse(src))).tests` — the
    // AST-equality the language mandates, not source-text equality. `TestBlock`
    // includes `raw_lines`, so this exercises the emission-source rule (raw_lines is the
    // emission source) and re-quoting together.

    fn tests_round_trip(src: &str) {
        let ast = Parser::parse(src).expect("fixture must parse");
        let compact = Transpiler::to_nodus(&ast);
        let ast2 = Parser::parse(&compact)
            .unwrap_or_else(|e| panic!("compact re-parse failed: {e:?}\ncompact:\n{compact}"));
        assert_eq!(
            ast.tests, ast2.tests,
            "tests must be AST-equal after a compact round-trip\ncompact:\n{compact}"
        );
    }

    #[test]
    fn test_block_canonical_line_per_pair_round_trips() {
        tests_round_trip(
            "§wf:t v1\n@in: { query }\n@out: $out\n@steps:\n  1. GEN($in.query) → $out\n@test: smoke {\n  input:\n    query: hello\n  expected:\n    $out: hello\n  tags: [smoke]\n}\n",
        );
    }

    #[test]
    fn test_block_inline_brace_round_trips() {
        tests_round_trip(
            "§wf:t v1\n@in: { query }\n@out: $out\n@steps:\n  1. GEN($in.query) → $out\n@test: smoke {\n  input: { query: hello }\n  tags: [smoke]\n}\n",
        );
    }

    #[test]
    fn test_block_whitespace_and_hyphen_values_round_trip() {
        // The two corpus shapes that corrupted before the fix: a value
        // containing whitespace and one containing a token-splitting
        // character, both of which the pre-fix emitter split on reparse.
        tests_round_trip(
            "§wf:t v1\n@in: { ticket_id, body }\n@out: $out\n@steps:\n  1. GEN($in.body) → $out\n@test: t1 {\n  input:    { ticket_id: \"T-001\", body: \"When is my invoice due?\" }\n  tags: [smoke]\n}\n",
        );
    }

    // ─── @macro: and human_mode round-trip ───────────────

    #[test]
    fn macro_block_round_trips() {
        let src = "§wf:t v1\n@in: { name: text }\n@out: $out\n@steps:\n  1. RUN(@greet) → $out\n@macro: greet\n  GEN($in.name) → $draft\n";
        let ast = Parser::parse(src).expect("fixture must parse");
        let compact = Transpiler::to_nodus(&ast);
        let ast2 = Parser::parse(&compact)
            .unwrap_or_else(|e| panic!("compact re-parse failed: {e:?}\ncompact:\n{compact}"));
        assert_eq!(
            ast.macros, ast2.macros,
            "macros must be AST-equal after a compact round-trip\ncompact:\n{compact}"
        );
    }

    #[test]
    fn human_mode_block_round_trips() {
        let src = "§wf:t v1\n@out: $out\n@steps:\n  1. GEN(x) → $out\n\n;; HUMAN MODE\n;; WORKFLOW: t\n;; a plain-language description\n";
        let ast = Parser::parse(src).expect("fixture must parse");
        let compact = Transpiler::to_nodus(&ast);
        let ast2 = Parser::parse(&compact)
            .unwrap_or_else(|e| panic!("compact re-parse failed: {e:?}\ncompact:\n{compact}"));
        assert_eq!(
            ast.human_mode, ast2.human_mode,
            "human_mode must be AST-equal after a compact round-trip\ncompact:\n{compact}"
        );
    }

    #[test]
    fn free_standing_comment_after_human_mode_stays_out_of_it() {
        // The load-bearing ordering guardrail: collect_comment_block greedily
        // consumes every following Comment token, so a free-standing comment
        // emitted after human_mode would be silently absorbed into it on
        // re-parse. Emitting human_mode last (§ above) is what prevents this;
        // this test would catch a regression that reordered the emission.
        let src = "§wf:t v1\n;; a free-standing comment\n@out: $out\n@steps:\n  1. GEN(x) → $out\n\n;; HUMAN MODE\n;; WORKFLOW: t\n";
        let ast = Parser::parse(src).expect("fixture must parse");
        let compact = Transpiler::to_nodus(&ast);
        let ast2 = Parser::parse(&compact)
            .unwrap_or_else(|e| panic!("compact re-parse failed: {e:?}\ncompact:\n{compact}"));
        assert_eq!(
            ast.comments, ast2.comments,
            "the free-standing comment must not be absorbed into human_mode\ncompact:\n{compact}"
        );
        assert_eq!(ast.human_mode, ast2.human_mode);
    }
}
