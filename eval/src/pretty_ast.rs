//! Pretty-printed format for the rnix AST representation.
//!
//! The AST is serialised into a JSON structure that can then be
//! printed in either minimised or well-formatted style.

use rnix::ast::{self, AstToken, HasEntry};
use serde::{ser::SerializeMap, Serialize, Serializer};

pub fn pretty_print_expr(expr: &ast::Expr) -> String {
    serde_json::ser::to_string_pretty(&SerializeAST(expr))
        .expect("serializing AST should always succeed")
}

#[repr(transparent)]
struct SerializeAST<S>(S);

impl<'a> Serialize for SerializeAST<&'a ast::Apply> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "apply")?;
        map.serialize_entry("fn", &SerializeAST(&self.0.lambda().unwrap()))?;
        map.serialize_entry("arg", &SerializeAST(&self.0.argument().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Assert> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "assert")?;
        map.serialize_entry("condition", &SerializeAST(&self.0.condition().unwrap()))?;
        map.serialize_entry("body", &SerializeAST(&self.0.body().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Error> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "error")?;
        map.serialize_entry("node", &self.0.to_string())?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::IfElse> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("kind", "if_else")?;
        map.serialize_entry("condition", &SerializeAST(&self.0.condition().unwrap()))?;
        map.serialize_entry("then_body", &SerializeAST(&self.0.body().unwrap()))?;
        map.serialize_entry("else_body", &SerializeAST(&self.0.else_body().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Select> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let size = match self.0.default_expr() {
            Some(_) => 4,
            None => 3,
        };

        let mut map = serializer.serialize_map(Some(size))?;
        map.serialize_entry("kind", "select")?;
        map.serialize_entry("set", &SerializeAST(&self.0.expr().unwrap()))?;
        map.serialize_entry("path", &SerializeAST(self.0.attrpath().unwrap()))?;

        if let Some(default) = self.0.default_expr() {
            map.serialize_entry("default", &SerializeAST(&default))?;
        }

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<ast::InterpolPart<String>> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ast::InterpolPart::Literal(s) => Serialize::serialize(s, serializer),
            ast::InterpolPart::Interpolation(node) => {
                Serialize::serialize(&SerializeAST(&node.expr().unwrap()), serializer)
            }
        }
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Str> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "string")?;

        map.serialize_entry(
            "parts",
            &self
                .0
                .normalized_parts()
                .into_iter()
                .map(|part| SerializeAST(part))
                .collect::<Vec<_>>(),
        )?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<ast::InterpolPart<ast::PathContent>> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ast::InterpolPart::Literal(p) => Serialize::serialize(p.syntax().text(), serializer),
            ast::InterpolPart::Interpolation(node) => {
                Serialize::serialize(&SerializeAST(&node.expr().unwrap()), serializer)
            }
        }
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Path> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "path")?;

        map.serialize_entry(
            "parts",
            &self
                .0
                .parts()
                .map(|part| SerializeAST(part))
                .collect::<Vec<_>>(),
        )?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Literal> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0.kind() {
            ast::LiteralKind::Float(val) => serializer.serialize_f64(val.value().unwrap()),
            ast::LiteralKind::Integer(val) => serializer.serialize_i64(val.value().unwrap()),
            ast::LiteralKind::Uri(val) => {
                let url = val.syntax().text();
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "url")?;
                map.serialize_entry("url", url)?;
                map.end()
            }
        }
    }
}

impl<'a> Serialize for SerializeAST<ast::PatEntry> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("ident", &SerializeAST(&self.0.ident().unwrap()))?;

        if let Some(default) = self.0.default() {
            map.serialize_entry("default", &SerializeAST(&default))?;
        }

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<ast::Param> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ast::Param::Pattern(pat) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("kind", "formals")?;

                map.serialize_entry(
                    "entries",
                    &pat.pat_entries()
                        .map(|entry| SerializeAST(entry))
                        .collect::<Vec<_>>(),
                )?;

                if let Some(bind) = pat.pat_bind() {
                    map.serialize_entry("bind", &SerializeAST(&bind.ident().unwrap()))?;
                }

                map.serialize_entry("ellipsis", &pat.ellipsis_token().is_some())?;

                map.end()
            }

            ast::Param::IdentParam(node) => {
                Serialize::serialize(&SerializeAST(&node.ident().unwrap()), serializer)
            }
        }
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Lambda> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "lambda")?;
        map.serialize_entry("param", &SerializeAST(self.0.param().unwrap()))?;
        map.serialize_entry("body", &SerializeAST(self.0.body().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::LegacyLet> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "legacy_let")?;

        map.serialize_entry(
            "entries",
            &self
                .0
                .attrpath_values()
                .map(|val| SerializeAST(val))
                .collect::<Vec<_>>(),
        )?;

        map.serialize_entry(
            "inherits",
            &self
                .0
                .inherits()
                .map(|val| SerializeAST(val))
                .collect::<Vec<_>>(),
        )?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::LetIn> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "let")?;

        map.serialize_entry(
            "entries",
            &self
                .0
                .attrpath_values()
                .map(|val| SerializeAST(val))
                .collect::<Vec<_>>(),
        )?;

        map.serialize_entry(
            "inherits",
            &self
                .0
                .inherits()
                .map(|val| SerializeAST(val))
                .collect::<Vec<_>>(),
        )?;

        map.serialize_entry("body", &SerializeAST(&self.0.body().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::List> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let list = self
            .0
            .items()
            .map(|elem| SerializeAST(elem))
            .collect::<Vec<_>>();

        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "list")?;
        map.serialize_entry("items", &list)?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::BinOp> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("kind", "binary_op")?;

        map.serialize_entry(
            "operator",
            match self.0.operator().unwrap() {
                ast::BinOpKind::Concat => "concat",
                ast::BinOpKind::Update => "update",
                ast::BinOpKind::Add => "add",
                ast::BinOpKind::Sub => "sub",
                ast::BinOpKind::Mul => "mul",
                ast::BinOpKind::Div => "div",
                ast::BinOpKind::And => "and",
                ast::BinOpKind::Equal => "equal",
                ast::BinOpKind::Implication => "implication",
                ast::BinOpKind::Less => "less",
                ast::BinOpKind::LessOrEq => "less_or_eq",
                ast::BinOpKind::More => "more",
                ast::BinOpKind::MoreOrEq => "more_or_eq",
                ast::BinOpKind::NotEqual => "not_equal",
                ast::BinOpKind::Or => "or",
            },
        )?;

        map.serialize_entry("lhs", &SerializeAST(&self.0.lhs().unwrap()))?;
        map.serialize_entry("rhs", &SerializeAST(&self.0.rhs().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Paren> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "paren")?;
        map.serialize_entry("expr", &SerializeAST(&self.0.expr().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Root> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "root")?;
        map.serialize_entry("expr", &SerializeAST(&self.0.expr().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<ast::AttrpathValue> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("name", &SerializeAST(self.0.attrpath().unwrap()))?;
        map.serialize_entry("value", &SerializeAST(self.0.value().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<ast::Inherit> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;

        if let Some(from) = self.0.from() {
            map.serialize_entry("namespace", &SerializeAST(&from.expr().unwrap()))?;
        }

        map.serialize_entry(
            "names",
            &self.0.attrs().map(|a| SerializeAST(a)).collect::<Vec<_>>(),
        )?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::AttrSet> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("kind", "attrset")?;
        map.serialize_entry("recursive", &self.0.rec_token().is_some())?;

        map.serialize_entry(
            "entries",
            &self
                .0
                .attrpath_values()
                .map(|val| SerializeAST(val))
                .collect::<Vec<_>>(),
        )?;

        map.serialize_entry(
            "inherits",
            &self
                .0
                .inherits()
                .map(|val| SerializeAST(val))
                .collect::<Vec<_>>(),
        )?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::UnaryOp> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "unary_op")?;

        map.serialize_entry(
            "operator",
            match self.0.operator().unwrap() {
                ast::UnaryOpKind::Invert => "invert",
                ast::UnaryOpKind::Negate => "negate",
            },
        )?;

        map.serialize_entry("expr", &SerializeAST(&self.0.expr().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Ident> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "ident")?;
        map.serialize_entry("ident", self.0.ident_token().unwrap().text())?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::With> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "with")?;
        map.serialize_entry("with", &SerializeAST(&self.0.namespace().unwrap()))?;
        map.serialize_entry("body", &SerializeAST(&self.0.body().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Dynamic> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "dynamic")?;
        map.serialize_entry("expr", &SerializeAST(&self.0.expr().unwrap()))?;
        map.end()
    }
}

impl Serialize for SerializeAST<ast::Attr> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ast::Attr::Ident(ident) => Serialize::serialize(&SerializeAST(ident), serializer),
            ast::Attr::Dynamic(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Attr::Str(node) => Serialize::serialize(&SerializeAST(node), serializer),
        }
    }
}

impl Serialize for SerializeAST<ast::Attrpath> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("kind", "attrpath")?;

        map.serialize_entry(
            "path",
            &self
                .0
                .attrs()
                .map(|attr| SerializeAST(attr))
                .collect::<Vec<_>>(),
        )?;

        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::HasAttr> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "has_attr")?;
        map.serialize_entry("expr", &SerializeAST(&self.0.expr().unwrap()))?;
        map.serialize_entry("attrpath", &SerializeAST(self.0.attrpath().unwrap()))?;
        map.end()
    }
}

impl<'a> Serialize for SerializeAST<&'a ast::Expr> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ast::Expr::Apply(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Assert(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Error(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::IfElse(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Select(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Str(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Path(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Literal(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Lambda(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::LegacyLet(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::LetIn(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::List(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::BinOp(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Paren(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Root(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::AttrSet(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::UnaryOp(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::Ident(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::With(node) => Serialize::serialize(&SerializeAST(node), serializer),
            ast::Expr::HasAttr(node) => Serialize::serialize(&SerializeAST(node), serializer),
        }
    }
}

impl Serialize for SerializeAST<ast::Expr> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SerializeAST(&self.0).serialize(serializer)
    }
}
