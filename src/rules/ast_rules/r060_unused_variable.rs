use std::collections::HashMap;

use lib_ruby_parser::nodes::{
    Arg, Block, Blockarg, Def, Defs, Kwarg, Kwoptarg, Kwrestarg, Lvar, Lvasgn, Optarg, Restarg,
};
use lib_ruby_parser::traverse::visitor::{
    visit_arg, visit_block, visit_blockarg, visit_def, visit_defs, visit_kwarg, visit_kwoptarg,
    visit_kwrestarg, visit_lvar, visit_lvasgn, visit_optarg, visit_restarg, Visitor,
};
use lib_ruby_parser::Loc;

use crate::diagnostic::{Diagnostic, Severity};
use crate::rules::{LintContext, Rule};

pub struct UnusedVariableRule;

impl Rule for UnusedVariableRule {
    fn name(&self) -> &'static str {
        "R060"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let ast = match ctx.ast() {
            Some(node) => node,
            None => return Vec::new(),
        };

        let mut visitor = VariableVisitor {
            source: ctx.source,
            file: ctx.file,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        };
        visitor.enter_scope(); // root scope for top-level locals
        visitor.visit(ast);
        visitor.exit_scope(); // emit diagnostics for unused top-level locals

        // Sort by line for deterministic output
        visitor.diagnostics.sort_by_key(|d| d.line);
        visitor.diagnostics
    }
}

/// Convert a byte offset to a 1-based line number.
fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    let mut line = 1;
    for &b in &source.as_bytes()[..offset.min(source.len())] {
        if b == b'\n' {
            line += 1;
        }
    }
    line
}

/// Scope-aware visitor that tracks variable assignments and usages per scope.
/// Each scope maps variable names to (location, was_used).
struct VariableVisitor<'a> {
    source: &'a str,
    file: &'a str,
    /// Stack of scopes. Each scope: map of name -> (loc, was_used).
    scopes: Vec<HashMap<String, (Loc, bool)>>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> VariableVisitor<'a> {
    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for (name, (loc, used)) in scope {
                if !used && !name.starts_with('_') {
                    let line = byte_offset_to_line(self.source, loc.begin);
                    self.diagnostics.push(Diagnostic::new(
                        self.file,
                        line,
                        1,
                        "R060",
                        format!("Variable `{}` is assigned but never used", name),
                        Severity::Warning,
                    ));
                }
            }
        }
    }

    fn record_assignment(&mut self, name: &str, loc: Loc) {
        // In Ruby, assignment inside a block rebinds an outer local if one exists.
        // Walk outer scopes first; only create a new entry in the innermost scope
        // if no outer binding is found.
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                // Rebinding outer variable — don't create a new entry
                return;
            }
        }
        // No existing binding found — create new entry in innermost scope
        if let Some(scope) = self.scopes.last_mut() {
            scope.entry(name.to_string()).or_insert((loc, false));
        }
    }

    fn record_usage(&mut self, name: &str) {
        // Walk all scopes from innermost outward to find the binding.
        for scope in self.scopes.iter_mut().rev() {
            if let Some(entry) = scope.get_mut(name) {
                entry.1 = true;
                return;
            }
        }
    }
}

impl<'a> Visitor for VariableVisitor<'a> {
    fn on_def(&mut self, node: &Def) {
        self.enter_scope();
        visit_def(self, node);
        self.exit_scope();
    }

    fn on_defs(&mut self, node: &Defs) {
        self.enter_scope();
        visit_defs(self, node);
        self.exit_scope();
    }

    fn on_block(&mut self, node: &Block) {
        self.enter_scope();
        visit_block(self, node);
        self.exit_scope();
    }

    fn on_lvasgn(&mut self, node: &Lvasgn) {
        self.record_assignment(&node.name.clone(), node.expression_l);
        // Continue visiting child nodes (e.g. the assigned value may contain references).
        visit_lvasgn(self, node);
    }

    fn on_lvar(&mut self, node: &Lvar) {
        self.record_usage(&node.name.clone());
        visit_lvar(self, node);
    }

    fn on_arg(&mut self, node: &Arg) {
        self.record_assignment(&node.name.clone(), node.expression_l);
        visit_arg(self, node);
    }

    fn on_optarg(&mut self, node: &Optarg) {
        self.record_assignment(&node.name.clone(), node.expression_l);
        visit_optarg(self, node);
    }

    fn on_blockarg(&mut self, node: &Blockarg) {
        if let Some(name) = &node.name {
            self.record_assignment(name, node.expression_l);
        }
        visit_blockarg(self, node);
    }

    fn on_restarg(&mut self, node: &Restarg) {
        if let Some(name) = &node.name {
            self.record_assignment(name, node.expression_l);
        }
        visit_restarg(self, node);
    }

    fn on_kwarg(&mut self, node: &Kwarg) {
        self.record_assignment(&node.name.clone(), node.expression_l);
        visit_kwarg(self, node);
    }

    fn on_kwoptarg(&mut self, node: &Kwoptarg) {
        self.record_assignment(&node.name.clone(), node.expression_l);
        visit_kwoptarg(self, node);
    }

    fn on_kwrestarg(&mut self, node: &Kwrestarg) {
        if let Some(name) = &node.name {
            self.record_assignment(name, node.expression_l);
        }
        visit_kwrestarg(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn check(source: &str) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
        UnusedVariableRule.check(&ctx)
    }

    #[test]
    fn detects_unused_variable() {
        let diags = check("def foo\n  unused = 1\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unused"));
    }

    #[test]
    fn no_warning_for_used_variable() {
        let diags = check("def foo\n  x = 1\n  puts x\nend\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn ignores_underscore_prefixed() {
        let diags = check("def foo\n  _unused = 1\nend\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn detects_unused_method_param() {
        let diags = check("def foo(a, b)\n  puts a\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("b"));
    }

    #[test]
    fn ignores_used_kwrestarg() {
        let diags = check("def foo(**opts)\n  puts opts\nend\n");
        assert!(
            diags.is_empty(),
            "used **opts should not trigger: {:?}",
            diags
        );
    }

    #[test]
    fn detects_unused_kwrestarg() {
        let diags = check("def foo(**opts)\n  puts 'nothing'\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("opts"));
    }

    #[test]
    fn scope_isolation_different_methods() {
        // x used in foo should not prevent x from being flagged as unused in bar
        let diags = check("def foo\n  x = 1\n  puts x\nend\ndef bar\n  x = 2\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("x"));
    }

    #[test]
    fn block_closure_can_use_outer_variable() {
        // x assigned in outer method, used inside a block closure — should not warn
        let diags = check("def foo\n  x = 1\n  [1].each { puts x }\nend\n");
        assert!(
            diags.is_empty(),
            "x used in block should not trigger: {:?}",
            diags
        );
    }

    #[test]
    fn block_reassignment_does_not_false_positive() {
        let src = "def foo\n  x = 1\n  [1].each do\n    x = 2\n  end\n  puts x\nend\n";
        let diags = check(src);
        assert!(
            diags.is_empty(),
            "x rebind in block should not false positive: {:?}",
            diags
        );
    }

    #[test]
    fn detects_unused_top_level_local() {
        let src = "# frozen_string_literal: true\nx = 1\nputs 42\n";
        let diags = check(src);
        let r060: Vec<_> = diags.iter().filter(|d| d.rule == "R060").collect();
        assert!(
            !r060.is_empty(),
            "top-level unused x should be detected: {:?}",
            diags
        );
    }
}
