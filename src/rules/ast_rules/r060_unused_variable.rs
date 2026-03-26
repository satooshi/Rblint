use std::collections::{HashMap, HashSet};

use lib_ruby_parser::nodes::{Arg, Blockarg, Kwarg, Kwoptarg, Lvar, Lvasgn, Optarg, Restarg};
use lib_ruby_parser::traverse::visitor::{
    visit_arg, visit_blockarg, visit_kwarg, visit_kwoptarg, visit_lvar, visit_lvasgn, visit_optarg,
    visit_restarg, Visitor,
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

        let mut collector = VariableCollector {
            source: ctx.source.as_bytes(),
            assignments: HashMap::new(),
            used: HashSet::new(),
        };
        collector.visit(ast);

        let mut diagnostics = Vec::new();
        for (name, loc) in &collector.assignments {
            // Skip underscore-prefixed variables (Ruby convention for intentionally unused)
            if name.starts_with('_') {
                continue;
            }
            if !collector.used.contains(name.as_str()) {
                let line = byte_offset_to_line(collector.source, loc.begin);
                diagnostics.push(Diagnostic::new(
                    ctx.file,
                    line,
                    0,
                    "R060",
                    format!("Variable `{}` is assigned but never used", name),
                    Severity::Warning,
                ));
            }
        }

        // Sort by line for deterministic output
        diagnostics.sort_by_key(|d| d.line);
        diagnostics
    }
}

/// Convert a byte offset to a 1-based line number.
fn byte_offset_to_line(source: &[u8], offset: usize) -> usize {
    let mut line = 1;
    for &b in &source[..offset.min(source.len())] {
        if b == b'\n' {
            line += 1;
        }
    }
    line
}

struct VariableCollector<'a> {
    source: &'a [u8],
    /// Map from variable name to the location of its first assignment.
    /// We only track the first assignment for the diagnostic location.
    assignments: HashMap<String, Loc>,
    /// Set of variable names that have been referenced.
    used: HashSet<String>,
}

impl<'a> Visitor for VariableCollector<'a> {
    fn on_lvasgn(&mut self, node: &Lvasgn) {
        self.assignments
            .entry(node.name.clone())
            .or_insert(node.expression_l);
        // Continue visiting child nodes (e.g. the assigned value may contain references)
        visit_lvasgn(self, node);
    }

    fn on_lvar(&mut self, node: &Lvar) {
        self.used.insert(node.name.clone());
        visit_lvar(self, node);
    }

    fn on_arg(&mut self, node: &Arg) {
        self.assignments
            .entry(node.name.clone())
            .or_insert(node.expression_l);
        visit_arg(self, node);
    }

    fn on_optarg(&mut self, node: &Optarg) {
        self.assignments
            .entry(node.name.clone())
            .or_insert(node.expression_l);
        visit_optarg(self, node);
    }

    fn on_blockarg(&mut self, node: &Blockarg) {
        if let Some(name) = &node.name {
            self.assignments
                .entry(name.clone())
                .or_insert(node.expression_l);
        }
        visit_blockarg(self, node);
    }

    fn on_restarg(&mut self, node: &Restarg) {
        if let Some(name) = &node.name {
            self.assignments
                .entry(name.clone())
                .or_insert(node.expression_l);
        }
        visit_restarg(self, node);
    }

    fn on_kwarg(&mut self, node: &Kwarg) {
        self.assignments
            .entry(node.name.clone())
            .or_insert(node.expression_l);
        visit_kwarg(self, node);
    }

    fn on_kwoptarg(&mut self, node: &Kwoptarg) {
        self.assignments
            .entry(node.name.clone())
            .or_insert(node.expression_l);
        visit_kwoptarg(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn run_rule(source: &str) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
        UnusedVariableRule.check(&ctx)
    }

    #[test]
    fn detects_unused_variable() {
        let diags = run_rule("def foo\n  unused = 1\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unused"));
    }

    #[test]
    fn no_warning_for_used_variable() {
        let diags = run_rule("def foo\n  x = 1\n  puts x\nend\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn ignores_underscore_prefixed() {
        let diags = run_rule("def foo\n  _unused = 1\nend\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn detects_unused_method_param() {
        let diags = run_rule("def foo(a, b)\n  puts a\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("b"));
    }
}
