use lib_ruby_parser::nodes::Send;
use lib_ruby_parser::traverse::visitor::{visit_send, Visitor};
use lib_ruby_parser::Node;

use crate::diagnostic::{Diagnostic, Severity};
use crate::rules::{LintContext, Rule};

pub struct NilComparisonRule;

impl Rule for NilComparisonRule {
    fn name(&self) -> &'static str {
        "R061"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let ast = match ctx.ast() {
            Some(node) => node,
            None => return Vec::new(),
        };

        let mut collector = NilComparisonCollector {
            source: ctx.source.as_bytes(),
            source_str: ctx.source,
            file: ctx.file,
            lines: ctx.lines,
            diagnostics: Vec::new(),
        };
        collector.visit(ast);
        collector.diagnostics
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

struct NilComparisonCollector<'a> {
    source: &'a [u8],
    source_str: &'a str,
    file: &'a str,
    lines: &'a [&'a str],
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Visitor for NilComparisonCollector<'a> {
    fn on_send(&mut self, node: &Send) {
        let is_eq = node.method_name == "==";
        let is_neq = node.method_name == "!=";

        if (is_eq || is_neq)
            && node.args.len() == 1
            && matches!(node.args[0], Node::Nil(_))
            && node.recv.is_some()
        {
            // Extract receiver source text using the range from
            // expression start to just before the operator (selector).
            let selector_begin = match &node.selector_l {
                Some(loc) => loc.begin,
                None => return,
            };
            let recv_src = self.source_str[node.expression_l.begin..selector_begin].trim_end();

            let line = byte_offset_to_line(self.source, node.expression_l.begin);
            let line_content = self.lines.get(line - 1).unwrap_or(&"");

            let replacement = if is_eq {
                format!("{}.nil?", recv_src)
            } else {
                format!("!{}.nil?", recv_src)
            };

            let operator = if is_eq { "==" } else { "!=" };
            let old_expr = &self.source_str[node.expression_l.begin..node.expression_l.end];
            let fix_line = line_content.replace(old_expr, &replacement);

            let msg = format!(
                "Use `{}` instead of `{} {} nil`",
                replacement, recv_src, operator
            );

            self.diagnostics.push(
                Diagnostic::new(self.file, line, 0, "R061", msg, Severity::Info).with_fix(fix_line),
            );
        }

        // Continue traversing child nodes
        visit_send(self, node);
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
        NilComparisonRule.check(&ctx)
    }

    #[test]
    fn detects_eq_nil() {
        let diags = run_rule("x = 1\nif x == nil\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].fix.as_ref().unwrap().contains(".nil?"));
    }

    #[test]
    fn detects_neq_nil() {
        let diags = run_rule("x = 1\nif x != nil\nend\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("!x.nil?"));
    }

    #[test]
    fn no_warning_for_nil_check_method() {
        let diags = run_rule("x.nil?\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn no_warning_for_comparison_with_non_nil() {
        let diags = run_rule("x == 0\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn fix_for_eq_nil() {
        let diags = run_rule("x = 1\nif x == nil\nend\n");
        assert_eq!(diags.len(), 1);
        let fix = diags[0].fix.as_ref().unwrap();
        assert!(fix.contains(".nil?"));
        assert!(!fix.contains("=="));
    }
}
