//! The tree-sitter walk. One node table per language says which node kinds
//! are strings, templates, calls, imports, annotations and base types; the
//! walk itself is shared.
use tree_sitter::Node;

use super::{Arg, Fact, Language, Part, env_callee, split_template, unquote};

struct Table {
    strings: &'static [&'static str],
    templates: &'static [&'static str],
    /// Template children that are substitutions; everything else is literal.
    template_vars: &'static [&'static str],
    /// Template children that are punctuation, not content.
    template_skip: &'static [&'static str],
    calls: &'static [&'static str],
    /// Node kinds holding a call's arguments.
    args: &'static [&'static str],
    imports: &'static [&'static str],
    annotations: &'static [&'static str],
    extends: &'static [&'static str],
}

const JS: Table = Table {
    strings: &["string"],
    templates: &["template_string"],
    template_vars: &["template_substitution"],
    template_skip: &[],
    calls: &["call_expression", "new_expression"],
    args: &["arguments"],
    imports: &["import_statement"],
    annotations: &["decorator"],
    extends: &["class_heritage"],
};

const PYTHON: Table = Table {
    strings: &["string"],
    templates: &[],
    template_vars: &["interpolation"],
    template_skip: &["string_start", "string_end"],
    calls: &["call"],
    args: &["argument_list", "generator_expression"],
    imports: &["import_statement", "import_from_statement"],
    annotations: &["decorator"],
    extends: &["argument_list"],
};

const GO: Table = Table {
    strings: &["interpreted_string_literal", "raw_string_literal"],
    templates: &[],
    template_vars: &[],
    template_skip: &[],
    calls: &["call_expression", "composite_literal"],
    args: &["argument_list", "literal_value"],
    imports: &["import_spec"],
    annotations: &[],
    extends: &[],
};

const JAVA: Table = Table {
    strings: &["string_literal"],
    templates: &[],
    template_vars: &[],
    template_skip: &[],
    calls: &["method_invocation", "object_creation_expression"],
    args: &["argument_list"],
    imports: &["import_declaration"],
    annotations: &["annotation", "marker_annotation"],
    extends: &["superclass", "super_interfaces"],
};

const CSHARP: Table = Table {
    strings: &[
        "string_literal",
        "verbatim_string_literal",
        "raw_string_literal",
    ],
    templates: &["interpolated_string_expression"],
    template_vars: &["interpolation"],
    template_skip: &[
        "interpolation_start",
        "interpolation_end",
        "interpolation_quote",
        "interpolation_brace",
    ],
    calls: &[
        "invocation_expression",
        "object_creation_expression",
        "implicit_object_creation_expression",
    ],
    args: &["argument_list"],
    imports: &["using_directive"],
    annotations: &["attribute"],
    extends: &["base_list"],
};

const PHP: Table = Table {
    strings: &["string"],
    templates: &["encapsed_string", "heredoc", "shell_command_expression"],
    template_vars: &[
        "variable_name",
        "encapsed_variable",
        "member_access_expression",
        "subscript_expression",
        "simple_variable",
        "dynamic_variable_name",
    ],
    template_skip: &["heredoc_start", "heredoc_end"],
    calls: &[
        "function_call_expression",
        "member_call_expression",
        "scoped_call_expression",
        "nullsafe_member_call_expression",
        "object_creation_expression",
    ],
    args: &["arguments"],
    imports: &["namespace_use_declaration"],
    annotations: &["attribute"],
    extends: &["base_clause", "class_interface_clause"],
};

pub(super) fn extract(language: Language, text: &str) -> Option<Vec<Fact>> {
    let (ts_language, table): (tree_sitter::Language, &Table) = match language {
        Language::JavaScript => (tree_sitter_javascript::LANGUAGE.into(), &JS),
        Language::TypeScript => (tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), &JS),
        Language::Tsx => (tree_sitter_typescript::LANGUAGE_TSX.into(), &JS),
        Language::Python => (tree_sitter_python::LANGUAGE.into(), &PYTHON),
        Language::Go => (tree_sitter_go::LANGUAGE.into(), &GO),
        Language::Java => (tree_sitter_java::LANGUAGE.into(), &JAVA),
        Language::CSharp => (tree_sitter_c_sharp::LANGUAGE.into(), &CSHARP),
        Language::Php => (tree_sitter_php::LANGUAGE_PHP.into(), &PHP),
        Language::Other => return None,
    };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_language).ok()?;
    let tree = parser.parse(text, None)?;
    let mut walker = Walker {
        src: text.as_bytes(),
        table,
        language,
        facts: Vec::new(),
    };
    walker.walk(tree.root_node());
    Some(walker.facts)
}

struct Walker<'a> {
    src: &'a [u8],
    table: &'a Table,
    language: Language,
    facts: Vec<Fact>,
}

impl Walker<'_> {
    fn text(&self, node: Node<'_>) -> &str {
        node.utf8_text(self.src).unwrap_or("")
    }

    fn line(node: Node<'_>) -> u32 {
        u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX)
    }

    fn walk(&mut self, node: Node<'_>) {
        let kind = node.kind();
        let t = self.table;
        let mut recurse = true;
        if t.strings.contains(&kind) {
            self.on_string(node);
            recurse = false;
        } else if t.templates.contains(&kind) {
            self.on_template(node);
            recurse = false;
        } else if t.calls.contains(&kind) {
            self.on_call(node);
        } else if t.imports.contains(&kind) {
            self.on_import(node);
        } else if t.annotations.contains(&kind) {
            self.on_annotation(node);
        } else if t.extends.contains(&kind) {
            self.on_extends(node);
        } else if self.language == Language::CSharp && kind == "element_access_expression" {
            self.on_cs_configuration(node);
        } else if self.language == Language::JavaScript
            || self.language == Language::TypeScript
            || self.language == Language::Tsx
        {
            self.on_js_env(node);
        } else if self.language == Language::Python && kind == "subscript" {
            self.on_python_subscript(node);
        }
        if recurse {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.walk(child);
            }
        }
    }

    /// A string node is a template when it holds interpolation children
    /// (Python f-strings); otherwise a plain literal.
    fn on_string(&mut self, node: Node<'_>) {
        let line = Self::line(node);
        let has_interpolation = {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .any(|c| c.kind() == "interpolation")
        };
        if has_interpolation {
            let parts = self.template_parts(node);
            self.facts.push(Fact::Template { parts, line });
            return;
        }
        if let Some(value) = unquote(self.text(node)) {
            self.facts.push(Fact::Str { value, line });
        }
    }

    fn on_template(&mut self, node: Node<'_>) {
        let line = Self::line(node);
        let parts = self.template_parts(node);
        if parts.iter().any(|p| matches!(p, Part::Var(_))) {
            self.facts.push(Fact::Template { parts, line });
        } else {
            let value: String = parts
                .into_iter()
                .map(|p| match p {
                    Part::Lit(s) | Part::Var(s) => s,
                })
                .collect();
            self.facts.push(Fact::Str { value, line });
        }
    }

    fn template_parts(&self, node: Node<'_>) -> Vec<Part> {
        let t = self.table;
        let mut parts: Vec<Part> = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let kind = child.kind();
            if t.template_skip.contains(&kind) {
                continue;
            }
            let text = self.text(child);
            if t.template_vars.contains(&kind) {
                let inner = text
                    .trim_start_matches(['$', '#'])
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .trim();
                parts.push(Part::Var(inner.to_string()));
            } else {
                match parts.last_mut() {
                    Some(Part::Lit(prev)) => prev.push_str(text),
                    _ => parts.push(Part::Lit(text.to_string())),
                }
            }
        }
        if parts.is_empty() {
            if let Some(value) = unquote(self.text(node)) {
                return split_template(&value);
            }
        }
        parts
    }

    fn arguments_node<'n>(&self, node: Node<'n>) -> Option<Node<'n>> {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|c| self.table.args.contains(&c.kind()))
    }

    fn args_of(&self, args: Node<'_>) -> Vec<Arg> {
        let mut out = Vec::new();
        let mut cursor = args.walk();
        for child in args.named_children(&mut cursor) {
            out.push(self.arg_of(Self::unwrap_argument(child)));
        }
        out
    }

    /// `argument`, `keyword_argument`, `named_argument` and Go's
    /// `keyed_element` wrap the value they pass; `literal_element` wraps once
    /// more. The value is the last named child at each level.
    fn unwrap_argument(node: Node<'_>) -> Node<'_> {
        let mut node = node;
        for _ in 0..3 {
            match node.kind() {
                "argument" | "keyword_argument" | "named_argument" | "keyed_element"
                | "literal_element" => {
                    let mut cursor = node.walk();
                    match node.named_children(&mut cursor).last() {
                        Some(inner) => node = inner,
                        None => return node,
                    }
                }
                _ => return node,
            }
        }
        node
    }

    fn arg_of(&self, node: Node<'_>) -> Arg {
        let t = self.table;
        let kind = node.kind();
        if t.strings.contains(&kind) {
            let parts = self.template_parts(node);
            if parts.iter().any(|p| matches!(p, Part::Var(_))) {
                return Arg::Template(parts);
            }
            return unquote(self.text(node))
                .map_or_else(|| Arg::Other(self.text(node).to_string()), Arg::Str);
        }
        if t.templates.contains(&kind) {
            return Arg::Template(self.template_parts(node));
        }
        if matches!(
            kind,
            "binary_expression"
                | "binary_operator"
                | "concatenated_string"
                | "additive_expression"
                | "parenthesized_expression"
        ) {
            let mut parts: Vec<Part> = Vec::new();
            self.concat_parts(node, &mut parts);
            if parts
                .iter()
                .any(|p| matches!(p, Part::Lit(l) if !l.is_empty()))
            {
                return Arg::Template(parts);
            }
        }
        Arg::Other(self.text(node).to_string())
    }

    /// Operands of a string concatenation, in order: literals become Lit,
    /// anything else becomes Var of its source text.
    fn concat_parts(&self, node: Node<'_>, parts: &mut Vec<Part>) {
        let t = self.table;
        let kind = node.kind();
        if t.strings.contains(&kind) || t.templates.contains(&kind) {
            parts.extend(self.template_parts(node));
            return;
        }
        if matches!(
            kind,
            "binary_expression"
                | "binary_operator"
                | "concatenated_string"
                | "additive_expression"
                | "parenthesized_expression"
        ) {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.concat_parts(child, parts);
            }
            return;
        }
        parts.push(Part::Var(self.text(node).to_string()));
    }

    fn on_call(&mut self, node: Node<'_>) {
        let line = Self::line(node);
        let Some(args_node) = self.arguments_node(node) else {
            return;
        };
        let start = node.start_byte();
        let end = args_node.start_byte();
        let callee = std::str::from_utf8(&self.src[start..end])
            .unwrap_or("")
            .trim()
            .to_string();
        if callee.is_empty() {
            return;
        }
        let args = self.args_of(args_node);
        if callee == "require" || callee == "import" {
            if let Some(Arg::Str(path)) = args.first() {
                self.facts.push(Fact::Import {
                    path: path.clone(),
                    line,
                });
            }
        }
        if env_callee(&callee) {
            if let Some(Arg::Str(name)) = args.first() {
                let default = match args.get(1) {
                    Some(Arg::Str(d)) => Some(d.clone()),
                    _ => self.default_after(node),
                };
                self.facts.push(Fact::EnvRef {
                    name: name.clone(),
                    default,
                    line,
                });
            }
        }
        self.facts.push(Fact::Call { callee, args, line });
    }

    /// `x || 'default'`, `x ?? 'default'`, `x or 'default'`: the literal on
    /// the right of an enclosing binary expression.
    fn default_after(&self, node: Node<'_>) -> Option<String> {
        let parent = node.parent()?;
        if !matches!(
            parent.kind(),
            "binary_expression" | "boolean_operator" | "binary_operator"
        ) {
            return None;
        }
        let mut cursor = parent.walk();
        let last = parent.named_children(&mut cursor).last()?;
        if last.id() == node.id() || !self.table.strings.contains(&last.kind()) {
            return None;
        }
        unquote(self.text(last))
    }

    fn on_import(&mut self, node: Node<'_>) {
        let line = Self::line(node);
        let t = self.table;
        let text = self.text(node);
        let mut paths: Vec<String> = Vec::new();
        match node.kind() {
            "import_statement" if self.language == Language::Python => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    match child.kind() {
                        "dotted_name" => paths.push(self.text(child).to_string()),
                        "aliased_import" => {
                            if let Some(first) = child.named_child(0) {
                                paths.push(self.text(first).to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            "import_from_statement" => {
                if let Some(module) = node.child_by_field_name("module_name") {
                    paths.push(self.text(module).to_string());
                }
            }
            "import_declaration" => {
                let body = text
                    .trim_start_matches("import")
                    .trim()
                    .trim_start_matches("static")
                    .trim();
                paths.push(body.trim_end_matches(';').trim().to_string());
            }
            "using_directive" => {
                let body = text
                    .trim_start_matches("using")
                    .trim()
                    .trim_start_matches("static")
                    .trim();
                let body = body.rsplit_once('=').map_or(body, |(_, rhs)| rhs.trim());
                paths.push(body.trim_end_matches(';').trim().to_string());
            }
            "namespace_use_declaration" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "namespace_use_clause" {
                        let clause = self.text(child);
                        let name = clause.split_whitespace().next().unwrap_or(clause);
                        paths.push(name.to_string());
                    }
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if t.strings.contains(&child.kind()) {
                        if let Some(s) = unquote(self.text(child)) {
                            paths.push(s);
                        }
                    }
                }
            }
        }
        for path in paths {
            if !path.is_empty() {
                self.facts.push(Fact::Import { path, line });
            }
        }
    }

    fn on_annotation(&mut self, node: Node<'_>) {
        let line = Self::line(node);
        let text = self.text(node);
        let name_end = text.find(['(', ' ', '\n']).unwrap_or(text.len());
        let name = text[..name_end]
            .trim_start_matches(['@', '#', '['])
            .trim()
            .to_string();
        let args = match self.arguments_node(node).or_else(|| {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).find(|c| {
                matches!(
                    c.kind(),
                    "annotation_argument_list" | "attribute_argument_list" | "call"
                )
            })
        }) {
            Some(inner) if inner.kind() == "call" => self
                .arguments_node(inner)
                .map(|a| self.args_of(a))
                .unwrap_or_default(),
            Some(inner) => {
                let mut args = self.string_args_within(inner);
                args.extend(self.symbol_args_within(inner));
                args
            }
            None => Vec::new(),
        };
        if !name.is_empty() {
            self.facts.push(Fact::Annotation { name, args, line });
        }
    }

    /// Every string literal anywhere under a node, in order.
    fn string_args_within(&self, node: Node<'_>) -> Vec<Arg> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if self.table.strings.contains(&n.kind()) {
                if let Some(s) = unquote(self.text(n)) {
                    out.push(Arg::Str(s));
                }
                continue;
            }
            let mut cursor = n.walk();
            let children: Vec<Node<'_>> = n.named_children(&mut cursor).collect();
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }
        out
    }

    /// Annotation values that are identifiers or member paths, not strings:
    /// `queues = Queues.queueName`, `[Trigger(Queues.Name)]`. Kept as Other
    /// so a matcher can look the symbol up.
    fn symbol_args_within(&self, node: Node<'_>) -> Vec<Arg> {
        let mut out = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let value = match child.kind() {
                "element_value_pair" | "attribute_argument" => {
                    let mut inner = child.walk();
                    child.named_children(&mut inner).last()
                }
                _ => None,
            };
            let Some(value) = value else { continue };
            let text = self.text(value).trim();
            let is_symbol = !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
                && text
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            if is_symbol {
                out.push(Arg::Other(text.to_string()));
            }
        }
        out
    }

    fn on_extends(&mut self, node: Node<'_>) {
        let line = Self::line(node);
        // Python: only the argument_list directly under a class_definition.
        if self.language == Language::Python
            && node.parent().is_none_or(|p| p.kind() != "class_definition")
        {
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let kind = child.kind();
            if kind == "type_list" {
                let mut inner = child.walk();
                for ty in child.named_children(&mut inner) {
                    self.facts.push(Fact::Extends {
                        name: self.text(ty).to_string(),
                        line,
                    });
                }
                continue;
            }
            if kind == "keyword_argument" {
                continue;
            }
            self.facts.push(Fact::Extends {
                name: self.text(child).to_string(),
                line,
            });
        }
    }

    /// `process.env.NAME`, `process.env["NAME"]`, with `|| 'default'`.
    fn on_js_env(&mut self, node: Node<'_>) {
        let kind = node.kind();
        if kind != "member_expression" && kind != "subscript_expression" {
            return;
        }
        let text = self.text(node);
        let Some(rest) = text.strip_prefix("process.env") else {
            return;
        };
        let name = if let Some(n) = rest.strip_prefix('.') {
            n.trim().to_string()
        } else if rest.starts_with('[') {
            match unquote(rest.trim_start_matches('[').trim_end_matches(']')) {
                Some(n) => n,
                None => return,
            }
        } else {
            return;
        };
        if !super::is_var_name(&name) {
            return;
        }
        let default = self.default_after(node);
        self.facts.push(Fact::EnvRef {
            name,
            default,
            line: Self::line(node),
        });
    }

    /// `Configuration["KEY"]`, `builder.Configuration["KEY"]`: .NET reads
    /// environment variables through configuration.
    fn on_cs_configuration(&mut self, node: Node<'_>) {
        let text = self.text(node);
        let Some((receiver, index)) = text.split_once('[') else {
            return;
        };
        let receiver = receiver.trim().to_lowercase();
        if !(receiver.ends_with("configuration") || receiver.ends_with("config")) {
            return;
        }
        let Some(name) = unquote(index.trim_end_matches(']')) else {
            return;
        };
        if !super::is_var_name(&name) {
            return;
        }
        let default = self.default_after(node);
        self.facts.push(Fact::EnvRef {
            name,
            default,
            line: Self::line(node),
        });
    }

    /// `os.environ["NAME"]`.
    fn on_python_subscript(&mut self, node: Node<'_>) {
        let text = self.text(node);
        if !text.starts_with("os.environ[") {
            return;
        }
        let Some(index) = node.named_child(1) else {
            return;
        };
        let Some(name) = unquote(self.text(index)) else {
            return;
        };
        let default = self.default_after(node);
        self.facts.push(Fact::EnvRef {
            name,
            default,
            line: Self::line(node),
        });
    }
}
