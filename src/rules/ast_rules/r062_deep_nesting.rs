use lib_ruby_parser::nodes::{
    Begin, Case, CaseMatch, Class, Def, Defs, For, If, Module, Until, While,
};
use lib_ruby_parser::traverse::visitor::{
    visit_begin, visit_case, visit_case_match, visit_class, visit_def, visit_defs, visit_for,
    visit_if, visit_module, visit_until, visit_while, Visitor,
};

use crate::diagnostic::{Diagnostic, Severity};
use crate::rules::{LintContext, Rule};

pub struct DeepNestingRule {
    pub(crate) max_nesting: usize,
}

impl Rule for DeepNestingRule {
    fn name(&self) -> &'static str {
        "R062"
    }

    fn check(&self, ctx: &LintContext<'_>) -> Vec<Diagnostic> {
        let ast = match ctx.ast() {
            Some(node) => node,
            None => return Vec::new(),
        };

        let mut collector = NestingCollector {
            source: ctx.source.as_bytes(),
            file: ctx.file,
            max_nesting: self.max_nesting,
            current_depth: 0,
            in_method: false,
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

struct NestingCollector<'a> {
    source: &'a [u8],
    file: &'a str,
    max_nesting: usize,
    current_depth: usize,
    in_method: bool,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> NestingCollector<'a> {
    /// Helper for nesting nodes: increment depth, check threshold, visit children, decrement.
    fn visit_nesting_node(
        &mut self,
        expression_l_begin: usize,
        visit_children: impl FnOnce(&mut Self),
    ) {
        if !self.in_method {
            visit_children(self);
            return;
        }

        self.current_depth += 1;
        if self.current_depth > self.max_nesting {
            let line = byte_offset_to_line(self.source, expression_l_begin);
            self.diagnostics.push(Diagnostic::new(
                self.file,
                line,
                0,
                "R062",
                format!(
                    "Nesting depth {} exceeds maximum allowed {}",
                    self.current_depth, self.max_nesting
                ),
                Severity::Warning,
            ));
        }
        visit_children(self);
        self.current_depth -= 1;
    }
}

impl<'a> Visitor for NestingCollector<'a> {
    fn on_def(&mut self, node: &Def) {
        let saved_depth = self.current_depth;
        let saved_in_method = self.in_method;
        self.current_depth = 0;
        self.in_method = true;
        visit_def(self, node);
        self.current_depth = saved_depth;
        self.in_method = saved_in_method;
    }

    fn on_defs(&mut self, node: &Defs) {
        let saved_depth = self.current_depth;
        let saved_in_method = self.in_method;
        self.current_depth = 0;
        self.in_method = true;
        visit_defs(self, node);
        self.current_depth = saved_depth;
        self.in_method = saved_in_method;
    }

    fn on_class(&mut self, node: &Class) {
        let saved_depth = self.current_depth;
        let saved_in_method = self.in_method;
        self.current_depth = 0;
        self.in_method = false;
        visit_class(self, node);
        self.current_depth = saved_depth;
        self.in_method = saved_in_method;
    }

    fn on_module(&mut self, node: &Module) {
        let saved_depth = self.current_depth;
        let saved_in_method = self.in_method;
        self.current_depth = 0;
        self.in_method = false;
        visit_module(self, node);
        self.current_depth = saved_depth;
        self.in_method = saved_in_method;
    }

    fn on_if(&mut self, node: &If) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_if(s, node));
    }

    fn on_while(&mut self, node: &While) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_while(s, node));
    }

    fn on_until(&mut self, node: &Until) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_until(s, node));
    }

    fn on_for(&mut self, node: &For) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_for(s, node));
    }

    fn on_begin(&mut self, node: &Begin) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_begin(s, node));
    }

    fn on_case(&mut self, node: &Case) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_case(s, node));
    }

    fn on_case_match(&mut self, node: &CaseMatch) {
        self.visit_nesting_node(node.expression_l.begin, |s| visit_case_match(s, node));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn run_rule(source: &str, max_nesting: usize) -> Vec<Diagnostic> {
        let lines: Vec<&str> = source.lines().collect();
        let tokens = Lexer::new(source).tokenize();
        let ctx = LintContext::new("test.rb", source, &lines, &tokens);
        let rule = DeepNestingRule { max_nesting };
        rule.check(&ctx)
    }

    #[test]
    fn no_warning_for_shallow_nesting() {
        let source = r#"
def shallow
  if true
    if true
      puts "ok"
    end
  end
end
"#;
        let diags = run_rule(source, 4);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn warns_at_depth_exceeding_max() {
        let source = r#"
def too_deep
  if a
    if b
      if c
        if d
          if e
            puts "too deep"
          end
        end
      end
    end
  end
end
"#;
        let diags = run_rule(source, 4);
        assert!(
            !diags.is_empty(),
            "expected at least 1 diagnostic for depth 5 with max 4"
        );
        assert_eq!(diags[0].rule, "R062");
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn configurable_threshold() {
        let source = r#"
def foo
  if true
    if true
      puts "two levels"
    end
  end
end
"#;
        let diags = run_rule(source, 1);
        assert!(
            !diags.is_empty(),
            "expected diagnostic for depth 2 with max 1"
        );
    }

    #[test]
    fn class_module_nesting_not_counted() {
        let source = r#"
class Outer
  module Inner
    def method_in_nested_class
      if a
        if b
          if c
            if d
              puts "counted from method, not module"
            end
          end
        end
      end
    end
  end
end
"#;
        // 4 ifs = depth 4, threshold 4 => should NOT fire
        let diags = run_rule(source, 4);
        assert_eq!(
            diags.len(),
            0,
            "class/module nesting should not count; 4 ifs at threshold 4 should pass"
        );
    }

    #[test]
    fn while_and_case_count_as_nesting() {
        let source = r#"
def foo
  while true
    case x
    when 1
      if a
        if b
          puts "deep"
        end
      end
    end
  end
end
"#;
        // while(1) + case(2) + if(3) + if(4) => depth 4
        let diags = run_rule(source, 3);
        assert!(
            !diags.is_empty(),
            "while + case + ifs should count as nesting"
        );
    }
}
