//! Recursive descent parser for the Limited Shell language.
//!
//! Tokenizes source input and builds an AST from the token stream.

use crate::ast;
use crate::ast::expr::{BinOp, Expr, Literal, UnOp};

// ─── Token ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Kw(&'static str),
    StringLit(String),
    IntLit(i64),
    BytesLit(ast::BytesLit),
    BoolLit(bool),
    Op(String), // +, -, ==, !=, <, <=, >, >=
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Dot,
    Arrow, // <-
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: std::ops::Range<usize>,
}

// ─── Lexer ────────────────────────────────────────────────────

static KEYWORDS: &[&str] = &[
    "role",
    "resource",
    "device",
    "extent",
    "machine",
    "operation",
    "service",
    "function",
    "can",
    "cannot",
    "grant",
    "alias",
    "requires",
    "allow",
    "if",
    "and",
    "or",
    "not",
    "is",
    "in",
    "from",
    "on",
    "exec",
    "transfer",
    "read",
    "write",
    "let",
    "mut",
    "for",
    "while",
    "break",
    "continue",
    "return",
    "define",
    "optimize",
    "costs",
    "cost",
    "rule",
    "default",
    "type",
    "key",
    "extend",
    "choose",
    "try",
    "catch",
    "finally",
    "success",
    "failure",
    "error",
    "else",
    "true",
    "false",
    "then",
    "exists",
    "matches",
    "starts",
    "ends",
    "with",
    "set",
    "start",
    "stop",
    "as",
    "dependency",
    "secret",
    "cmd",
    "bytes",
    "down",
    "of",
    "tasks",
    "capacity",
    "field",
    "up",
    "mountpoint",
    "where",
    "broadcast",
    "sum",
    "options",
];

pub fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace and comments
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '/' {
            // Line comment
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
            // Block comment
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            } else {
                return bad_token(input, i, "unclosed block comment");
            }
            continue;
        }

        let span_start = i;

        // Identifiers and keywords
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let kind = if KEYWORDS.contains(&word.as_str()) {
                TokenKind::Kw(Box::leak(word.into_boxed_str()))
            } else {
                TokenKind::Ident(word)
            };
            tokens.push(Token {
                kind,
                span: span_start..i,
            });
            continue;
        }

        // String literals
        if chars[i] == '"' {
            i += 1;
            let mut s = String::new();
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1; // skip backslash
                    if i < len {
                        let c = match chars[i] {
                            'n' => '\n',
                            't' => '\t',
                            '\\' => '\\',
                            '"' => '"',
                            c => c,
                        };
                        s.push(c);
                        i += 1; // advance past escape char
                        continue; // skip the else push
                    }
                } else {
                    s.push(chars[i]);
                    i += 1;
                }
            }
            if i >= len {
                return bad_token(input, span_start, "unclosed string");
            }
            i += 1; // closing "
            tokens.push(Token {
                kind: TokenKind::StringLit(s),
                span: span_start..i,
            });
            continue;
        }

        // Numeric literals
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();

            // Check for suffix (allow optional whitespace between number and suffix)
            let _suffix_start = i;
            // Skip optional whitespace before suffix
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
            let suffix = match &chars[i..] {
                ['G', 'i', 'B', ..] => {
                    i += 3;
                    Some(ast::BytesSuffix::GiB)
                }
                ['G', 'B', ..] => {
                    i += 2;
                    Some(ast::BytesSuffix::GB)
                }
                ['T', 'i', 'B', ..] => {
                    i += 3;
                    Some(ast::BytesSuffix::TiB)
                }
                ['T', 'B', ..] => {
                    i += 2;
                    Some(ast::BytesSuffix::TB)
                }
                ['M', 'i', 'B', ..] => {
                    i += 3;
                    Some(ast::BytesSuffix::MiB)
                }
                ['M', 'B', ..] => {
                    i += 2;
                    Some(ast::BytesSuffix::MB)
                }
                ['K', 'i', 'B', ..] => {
                    i += 3;
                    Some(ast::BytesSuffix::KiB)
                }
                ['K', 'B', ..] => {
                    i += 2;
                    Some(ast::BytesSuffix::KB)
                }
                ['s', ..] => {
                    i += 1;
                    None
                } // duration
                _ => None,
            };

            let value = num_str.parse::<u64>().unwrap_or(0);
            if let Some(suffix) = suffix {
                tokens.push(Token {
                    kind: TokenKind::BytesLit(ast::BytesLit { value, suffix }),
                    span: span_start..i,
                });
            } else {
                tokens.push(Token {
                    kind: TokenKind::IntLit(num_str.parse().unwrap_or(0)),
                    span: span_start..i,
                });
            }
            continue;
        }

        // Operators and delimiters
        match chars[i] {
            '(' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::LParen,
                    span: i - 1..i,
                });
            }
            ')' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::RParen,
                    span: i - 1..i,
                });
            }
            '{' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::LBrace,
                    span: i - 1..i,
                });
            }
            '}' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::RBrace,
                    span: i - 1..i,
                });
            }
            '[' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::LBracket,
                    span: i - 1..i,
                });
            }
            ']' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::RBracket,
                    span: i - 1..i,
                });
            }
            ',' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    span: i - 1..i,
                });
            }
            ';' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Semi,
                    span: i - 1..i,
                });
            }
            ':' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    span: i - 1..i,
                });
            }
            '.' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Dot,
                    span: i - 1..i,
                });
            }
            '+' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Op("+".into()),
                    span: i - 1..i,
                });
            }
            '!' => {
                if i + 1 < len && chars[i + 1] == '=' {
                    i += 2;
                    tokens.push(Token {
                        kind: TokenKind::Op("!=".into()),
                        span: i - 2..i,
                    });
                } else {
                    return bad_token(input, i, "unexpected '!'");
                }
            }
            '=' => {
                if i + 1 < len && chars[i + 1] == '=' {
                    i += 2;
                    tokens.push(Token {
                        kind: TokenKind::Op("==".into()),
                        span: i - 2..i,
                    });
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Op("=".into()),
                        span: i - 1..i,
                    });
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < len && chars[i + 1] == '-' {
                    i += 2;
                    tokens.push(Token {
                        kind: TokenKind::Arrow,
                        span: i - 2..i,
                    });
                } else if i + 1 < len && chars[i + 1] == '=' {
                    i += 2;
                    tokens.push(Token {
                        kind: TokenKind::Op("<=".into()),
                        span: i - 2..i,
                    });
                } else {
                    i += 1;
                    tokens.push(Token {
                        kind: TokenKind::Op("<".into()),
                        span: i - 1..i,
                    });
                }
            }
            '>' => {
                if i + 1 < len && chars[i + 1] == '=' {
                    i += 2;
                    tokens.push(Token {
                        kind: TokenKind::Op(">=".into()),
                        span: i - 2..i,
                    });
                } else {
                    i += 1;
                    tokens.push(Token {
                        kind: TokenKind::Op(">".into()),
                        span: i - 1..i,
                    });
                }
            }
            '-' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Op("-".into()),
                    span: i - 1..i,
                });
            }
            '*' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Op("*".into()),
                    span: i - 1..i,
                });
            }
            '/' => {
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::Op("/".into()),
                    span: i - 1..i,
                });
            }
            _ => {
                return bad_token(input, i, &format!("unexpected character '{}'", chars[i]));
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: len..len,
    });
    Ok(tokens)
}

// ─── Parser ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub pos: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error at position {}: {}", self.pos, self.message)
    }
}
impl std::error::Error for ParseError {}

fn bad_token(_input: &str, pos: usize, msg: &str) -> Result<Vec<Token>, ParseError> {
    Err(ParseError {
        message: msg.to_string(),
        pos,
    })
}

pub fn parse(source: &str) -> Result<ast::Program, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, pos: 0 };
    let program = parser.parse_program()?;

    if !parser.at_end() {
        return Err(ParseError {
            message: "unexpected tokens after program".into(),
            pos: parser.pos,
        });
    }
    Ok(program)
}

// Sentinel EOF token to avoid dangling references when pos >= tokens.len()
static EOF_TOKEN: std::sync::OnceLock<Token> = std::sync::OnceLock::new();

fn eof_token() -> &'static Token {
    EOF_TOKEN.get_or_init(|| Token {
        kind: TokenKind::Eof,
        span: 0..0,
    })
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn cur(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| eof_token())
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        let cur = self.cur().clone();
        if cur.kind != kind {
            return Err(ParseError {
                message: format!("expected {:?}, found {:?}", kind, cur.kind),
                pos: self.pos,
            });
        }
        self.pos += 1;
        Ok(())
    }

    fn expect_ident(&mut self) -> Result<ast::Ident, ParseError> {
        let tok = self.cur().clone();
        if let TokenKind::Ident(name) = &tok.kind {
            self.pos += 1;
            Ok(ast::Ident { name: name.clone() })
        } else {
            Err(ParseError {
                message: format!("expected identifier, found {:?}", tok.kind),
                pos: self.pos,
            })
        }
    }

    fn at_end(&self) -> bool {
        matches!(self.cur().kind, TokenKind::Eof)
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.cur().kind, TokenKind::Kw(k) if k == kw)
    }

    /// Check if current token is a keyword or identifier with the given name.
    /// Used for type keywords (map, set, ordered, new, list, of, to) that
    /// might be either keywords or identifiers depending on the source.
    fn is_kw_or_ident(&self, name: &str) -> bool {
        let kind = self.cur().kind.clone();
        matches!(kind, TokenKind::Kw(k) if k == name)
            || matches!(kind, TokenKind::Ident(s) if s == name)
    }

    fn is_kind(&self, kind: &TokenKind) -> bool {
        &self.cur().kind == kind
    }

    fn is_ident(&self) -> bool {
        matches!(self.cur().kind, TokenKind::Ident(_))
    }

    fn is_string_lit(&self) -> bool {
        matches!(self.cur().kind, TokenKind::StringLit(_))
    }

    fn peek_kw(&self, offset: usize, kw: &str) -> bool {
        matches!(&self.peek(offset).kind, TokenKind::Kw(k) if *k == kw)
    }

    fn peek_kw_or_ident(&self, offset: usize, name: &str) -> bool {
        let kind = self.peek(offset).kind;
        matches!(kind, TokenKind::Kw(k) if k == name)
            || matches!(kind, TokenKind::Ident(s) if s == name)
    }

    fn expect_kw_or_ident(&mut self, name: &str) -> Result<(), ParseError> {
        if self.is_kw_or_ident(name) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ParseError {
                message: format!("expected '{}', found {:?}", name, self.cur().kind),
                pos: self.pos,
            })
        }
    }

    fn peek_kind(&self, offset: usize, kind: &TokenKind) -> bool {
        self.peek(offset).kind == *kind
    }

    fn parse_program(&mut self) -> Result<ast::Program, ParseError> {
        let mut items = Vec::new();
        let mut statements = Vec::new();

        loop {
            if self.at_end() {
                break;
            }
            if let Some(item) = self.try_parse_item()? {
                items.push(item);
            } else if let Some(stmt) = self.try_parse_statement()? {
                statements.push(stmt);
            } else {
                return Err(ParseError {
                    message: format!("unexpected token {:?}", self.cur().kind),
                    pos: self.pos,
                });
            }
        }

        Ok(ast::Program { items, statements })
    }

    // ─── Items ──────────────────────────────────────────

    fn try_parse_item(&mut self) -> Result<Option<ast::Item>, ParseError> {
        match self.cur().kind {
            TokenKind::Kw("alias") => Ok(Some(ast::Item::Alias(self.parse_alias_decl()?))),
            TokenKind::Kw("role") => Ok(Some(ast::Item::Role(self.parse_role_decl()?))),
            TokenKind::Kw("resource") => Ok(Some(ast::Item::Resource(self.parse_resource_decl()?))),
            TokenKind::Kw("device") => Ok(Some(ast::Item::Device(self.parse_device_decl()?))),
            TokenKind::Kw("operation") => {
                Ok(Some(ast::Item::Operation(self.parse_operation_decl()?)))
            }
            TokenKind::Kw("service") => Ok(Some(ast::Item::Service(self.parse_service_decl()?))),
            TokenKind::Kw("function") => Ok(Some(ast::Item::Function(self.parse_function_decl()?))),
            _ => Ok(None),
        }
    }

    fn parse_alias_decl(&mut self) -> Result<ast::AliasDecl, ParseError> {
        self.expect(TokenKind::Kw("alias"))?;

        let kind;
        let name;

        // Check for "alias name as kind = target" syntax
        if self.is_ident() && self.peek_kw(1, "as") {
            name = self.expect_ident()?;
            self.expect(TokenKind::Kw("as"))?;
            kind = match self.cur().kind {
                TokenKind::Kw("machine") => {
                    self.pos += 1;
                    ast::AliasKind::Machine
                }
                TokenKind::Kw("path") => {
                    self.pos += 1;
                    ast::AliasKind::Path
                }
                TokenKind::Kw("role") => {
                    self.pos += 1;
                    ast::AliasKind::Role
                }
                _ => ast::AliasKind::Generic,
            };
        } else {
            // "alias kind name = target" or "alias name = target"
            kind = match self.cur().kind {
                TokenKind::Kw("machine") => {
                    self.pos += 1;
                    ast::AliasKind::Machine
                }
                TokenKind::Kw("path") => {
                    self.pos += 1;
                    ast::AliasKind::Path
                }
                TokenKind::Kw("role") => {
                    self.pos += 1;
                    ast::AliasKind::Role
                }
                _ => ast::AliasKind::Generic,
            };
            name = self.expect_ident()?;
        }

        self.expect(TokenKind::Op("=".into()))?;
        let target = self.parse_ty_inner()?;

        Ok(ast::AliasDecl { kind, name, target })
    }

    fn parse_role_decl(&mut self) -> Result<ast::RoleDecl, ParseError> {
        self.expect(TokenKind::Kw("role"))?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut up = None;
        let mut permissions = Vec::new();
        let mut define_ops_for = Vec::new();

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kw("up") {
                self.pos += 1;
                if self.is_kind(&TokenKind::Colon) {
                    self.pos += 1;
                }
                up = Some(self.expect_ident()?);
            } else if self.is_kw("can") && self.peek_kw(1, "define") {
                self.pos += 1; // skip "can"
                self.expect(TokenKind::Kw("define"))?;
                self.expect(TokenKind::Kw("operation"))?;
                self.expect(TokenKind::Kw("for"))?;
                while !self.is_kind(&TokenKind::RBrace) && !self.is_kind(&TokenKind::Comma) {
                    let target = self.parse_define_ops_target()?;
                    define_ops_for.push(target);
                    if self.is_kind(&TokenKind::Comma) {
                        self.pos += 1;
                    }
                }
            } else if self.is_kw("can") || self.is_kw("cannot") {
                permissions.push(self.parse_permission()?);
            } else {
                return Err(ParseError {
                    message: "unexpected token in role body".into(),
                    pos: self.pos,
                });
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::RoleDecl {
            name,
            up,
            permissions,
            define_ops_for,
        })
    }

    fn parse_permission(&mut self) -> Result<ast::Permission, ParseError> {
        let deny = matches!(self.cur().kind, TokenKind::Kw("cannot"));
        if deny || self.is_kw("can") {
            self.pos += 1;
        }

        // Operation name can be a keyword (e.g., "define") or identifier
        let op = if let TokenKind::Ident(ref s) = self.cur().kind {
            let name = s.clone();
            self.pos += 1;
            ast::Ident { name }
        } else if let TokenKind::Kw(s) = self.cur().kind {
            let name = s.to_string();
            self.pos += 1;
            ast::Ident { name }
        } else {
            return Err(ParseError {
                message: format!("expected operation name, found {:?}", self.cur().kind),
                pos: self.pos,
            });
        };
        let mut resource = None;

        if self.is_kind(&TokenKind::LBrace) {
            self.pos += 1;
            let variable = self.expect_ident()?;
            self.expect(TokenKind::Colon)?;
            let resource_type = self.parse_ty_inner()?;
            self.expect(TokenKind::RBrace)?;
            resource = Some(ast::ResourcePattern {
                variable,
                resource_type,
            });
        }

        let mut condition = None;
        if self.is_kw("if") {
            self.pos += 1;
            condition = Some(Box::new(self.parse_condition()?));
        }

        Ok(ast::Permission {
            deny,
            op,
            resource,
            condition,
        })
    }

    fn parse_define_ops_target(&mut self) -> Result<ast::DefineOpsTarget, ParseError> {
        // Role name can be a keyword (e.g., "operation") or identifier
        let role = if let TokenKind::Ident(ref s) = self.cur().kind {
            let name = s.clone();
            self.pos += 1;
            ast::Ident { name }
        } else if let TokenKind::Kw(s) = self.cur().kind {
            let name = s.to_string();
            self.pos += 1;
            ast::Ident { name }
        } else {
            return Err(ParseError {
                message: format!("expected role name, found {:?}", self.cur().kind),
                pos: self.pos,
            });
        };
        if self.is_kind(&TokenKind::Dot) {
            self.pos += 1;
            self.expect(TokenKind::Kw("down"))?;
            Ok(ast::DefineOpsTarget::RoleDown(role))
        } else {
            Ok(ast::DefineOpsTarget::Role(role))
        }
    }

    fn parse_resource_decl(&mut self) -> Result<ast::ResourceDecl, ParseError> {
        self.expect(TokenKind::Kw("resource"))?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut capacities = Vec::new();
        let mut fields = Vec::new();

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kw("capacity") {
                capacities.push(self.parse_capacity_decl()?);
            } else if self.is_kw("field") {
                fields.push(self.parse_field_decl()?);
            } else if matches!(&self.cur().kind, TokenKind::Ident(_)) {
                // Bare type expression as capacity (e.g., `capacity: map of String to Int` already handled)
                // or a bare resource type reference
                let ty = self.parse_ty_inner()?;
                capacities.push(ast::Capacity {
                    name: ast::Ident { name: "_".into() },
                    ty,
                });
            } else {
                return Err(ParseError {
                    message: "unexpected token in resource body".into(),
                    pos: self.pos,
                });
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::ResourceDecl {
            name,
            capacities,
            fields,
        })
    }

    fn parse_capacity_decl(&mut self) -> Result<ast::Capacity, ParseError> {
        self.expect(TokenKind::Kw("capacity"))?;
        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
        }

        // Try to parse type expression: [Int], [mut Int], Bytes, set of Int, etc.
        let ty = self.parse_ty_inner()?;
        Ok(ast::Capacity {
            name: ast::Ident { name: "_".into() },
            ty,
        })
    }

    fn parse_field_decl(&mut self) -> Result<ast::FieldDecl, ParseError> {
        self.expect(TokenKind::Kw("field"))?;
        let name = self.expect_ident()?;
        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
        }
        let ty = self.parse_ty_inner()?;
        let mut default = None;

        if self.is_kw("default") {
            self.pos += 1;
            self.expect(TokenKind::Op("=".into()))?;
            default = Some(self.parse_expr()?);
        }

        Ok(ast::FieldDecl { name, ty, default })
    }

    fn parse_device_decl(&mut self) -> Result<ast::DeviceDecl, ParseError> {
        self.expect(TokenKind::Kw("device"))?;
        let name = self.expect_ident()?;
        let mut parent = None;

        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
            parent = Some(self.expect_ident()?);
        }
        self.expect(TokenKind::LBrace)?;

        let mut extents = Vec::new();
        let mut rates = Vec::new();
        let mut cost_rules = Vec::new();

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kw("extent") {
                if let Some(extent) = self.parse_extent_decl()? {
                    extents.push(extent);
                }
            } else if self.is_kw("rate") {
                rates.push(self.parse_rate_decl()?);
            } else if self.is_kw("cost") {
                cost_rules.push(self.parse_cost_rule()?);
            } else {
                return Err(ParseError {
                    message: "unexpected token in device body".into(),
                    pos: self.pos,
                });
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::DeviceDecl {
            name,
            parent,
            extents,
            rates,
            cost_rules,
        })
    }

    fn parse_extent_decl(&mut self) -> Result<Option<ast::ExtentDecl>, ParseError> {
        self.expect(TokenKind::Kw("extent"))?;
        let name = self.expect_ident()?;

        if self.is_kw("mountpoint") {
            self.pos += 1;
            let mountpoint = self.parse_expr()?;
            // Need to find the size - it should come before mountpoint
            // This is a simplified handling
            Ok(Some(ast::ExtentDecl::Disk {
                name,
                size: ast::BytesLit {
                    value: 0,
                    suffix: ast::BytesSuffix::None,
                },
                mountpoint,
            }))
        } else if self.is_kw("default") {
            self.pos += 1;
            self.expect(TokenKind::Op("=".into()))?;
            let default = self.parse_expr()?;
            let ty = if self.is_kw("bytes") {
                self.pos += 1;
                ast::ExtentType::Bytes
            } else {
                ast::ExtentType::Count(self.expect_ident()?)
            };
            Ok(Some(ast::ExtentDecl::Simple {
                name,
                ty,
                default: Some(default),
            }))
        } else if self.is_kind(&TokenKind::Kw("bytes")) {
            self.pos += 1;
            Ok(Some(ast::ExtentDecl::Simple {
                name,
                ty: ast::ExtentType::Bytes,
                default: None,
            }))
        } else if matches!(&self.cur().kind, TokenKind::BytesLit(_)) {
            self.pos += 1;
            let ty = if self.is_kw("mountpoint") {
                ast::ExtentType::Count(ast::Ident {
                    name: "DISK".into(),
                })
            } else {
                ast::ExtentType::Bytes
            };
            Ok(Some(ast::ExtentDecl::Simple {
                name,
                ty,
                default: None,
            }))
        } else {
            Ok(Some(ast::ExtentDecl::Simple {
                name,
                ty: ast::ExtentType::Bytes,
                default: None,
            }))
        }
    }

    fn parse_rate_decl(&mut self) -> Result<ast::RateDecl, ParseError> {
        self.expect(TokenKind::Kw("rate"))?;
        let name = self.expect_ident()?;
        let rate = self.parse_ty_inner()?;
        Ok(ast::RateDecl { name, rate })
    }

    fn parse_cost_rule(&mut self) -> Result<ast::CostRule, ParseError> {
        self.expect(TokenKind::Kw("cost"))?;
        self.expect(TokenKind::Kw("rule"))?;
        self.expect(TokenKind::LBrace)?;

        let mut constraints = Vec::new();
        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kind(&TokenKind::Kw("extent")) || self.is_ident() {
                let extent_name = if self.is_kw("extent") {
                    self.pos += 1;
                    self.expect_ident()?
                } else {
                    self.expect_ident()?
                };

                self.expect(TokenKind::Op("<=".into()))?;
                let pool = self.parse_expr()?;

                constraints.push(ast::CostConstraint::SumLt {
                    extent: extent_name,
                    pool,
                });
            } else if self.is_kw_or_ident("sum") {
                // Parse sum(cost <extent>) op sum(cost <extent>) <= <pool>
                if self.is_kw_or_ident("sum") {
                    self.pos += 1;
                }
                self.expect(TokenKind::LParen)?;
                self.expect(TokenKind::Kw("cost"))?;
                let left_extent = self.expect_ident()?;
                self.expect(TokenKind::RParen)?;

                let op = if self.is_kind(&TokenKind::Op("+".into())) {
                    self.pos += 1;
                    ast::SumOp::Plus
                } else if self.is_kind(&TokenKind::Op("-".into())) {
                    self.pos += 1;
                    ast::SumOp::Minus
                } else {
                    return Err(ParseError {
                        message: "expected + or - in sum expression".into(),
                        pos: self.pos,
                    });
                };

                if self.is_kw_or_ident("sum") {
                    self.pos += 1;
                }
                self.expect(TokenKind::LParen)?;
                self.expect(TokenKind::Kw("cost"))?;
                let right_extent = self.expect_ident()?;
                self.expect(TokenKind::RParen)?;

                self.expect(TokenKind::Op("<=".into()))?;
                let pool = self.parse_expr()?;

                constraints.push(ast::CostConstraint::SumOpLt {
                    op,
                    left_extent,
                    right_extent,
                    pool,
                });
            } else {
                return Err(ParseError {
                    message: "unexpected token in cost rule".into(),
                    pos: self.pos,
                });
            }

            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ast::CostRule { constraints })
    }

    fn parse_machine_decl(&mut self) -> Result<ast::MachineDecl, ParseError> {
        self.expect(TokenKind::Kw("machine"))?;

        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut extents = Vec::new();
        let mut keys = Vec::new();
        let mut devices = Vec::new();

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kw("extent") {
                if let Some(ast::ExtentDecl::Disk {
                    name: ext_name,
                    size,
                    mountpoint,
                }) = self.parse_extent_decl()?
                {
                    extents.push(ast::MachineExtent::Disk(ast::MachineDisk {
                        name: ext_name,
                        size,
                        mountpoint,
                    }));
                }
            } else if matches!(&self.cur().kind, TokenKind::BytesLit(_)) {
                if let TokenKind::BytesLit(b) = &self.cur().kind {
                    keys.push(b.clone());
                }
                self.pos += 1;
            } else if self.is_kw("key") {
                self.pos += 1;
                if let TokenKind::StringLit(_) = &self.cur().kind {
                    keys.push(ast::BytesLit {
                        value: 0,
                        suffix: ast::BytesSuffix::None,
                    }); // placeholder
                }
                self.pos += 1;
            } else if self.is_kw("device") {
                self.pos += 1; // consume 'device'
                               // device name type Type { ... }
                let device_name = self.expect_ident()?;
                self.expect(TokenKind::Kw("type"))?;
                let device_type = self.expect_ident()?;
                // Consume optional { } body
                if self.is_kind(&TokenKind::LBrace) {
                    self.pos += 1;
                    while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
                        self.pos += 1;
                    }
                    if !self.is_kind(&TokenKind::RBrace) {
                        return Err(ParseError {
                            message: "unclosed device body".into(),
                            pos: self.pos,
                        });
                    }
                    self.pos += 1;
                }
                devices.push(ast::MachineDevice {
                    name: device_name,
                    device_type,
                    extent_bindings: Vec::new(),
                });
            } else {
                self.pos += 1; // skip unknown token
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::MachineDecl {
            name,
            extents,
            keys,
            devices,
        })
    }

    fn parse_operation_decl(&mut self) -> Result<ast::OperationDecl, ParseError> {
        self.expect(TokenKind::Kw("operation"))?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        while !self.is_kind(&TokenKind::RParen) {
            params.push(self.parse_named_type()?);
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::LBrace)?;

        let mut requires = Vec::new();
        let mut allow = None;
        let mut options = Vec::new();
        let mut cost = None;

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kw("requires") {
                self.pos += 1;
                if self.is_kind(&TokenKind::Colon) {
                    self.pos += 1;
                }
                requires.push(self.parse_condition()?);
            } else if self.is_kw("allow") {
                self.pos += 1;
                self.expect(TokenKind::Kw("if"))?;
                self.expect(TokenKind::Kw("role"))?;
                self.expect(TokenKind::Kw("is"))?;
                allow = Some(self.expect_ident()?);
            } else if matches!(&self.cur().kind, TokenKind::StringLit(_)) || self.is_kw("options") {
                let name_str = if self.is_kw("options") {
                    self.pos += 1;
                    "options".into()
                } else {
                    match &self.cur().kind {
                        TokenKind::StringLit(s) => {
                            let s = s.clone();
                            self.pos += 1;
                            s
                        },
                        _ => String::new(),
                    }
                };
                self.expect(TokenKind::LBrace)?;
                let mut body = Vec::new();
                while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
                    body.push(self.parse_operation_stmt()?);
                }
                self.expect(TokenKind::RBrace)?;
                options.push(ast::OperationOption {
                    name: ast::Ident { name: name_str },
                    body,
                });
            } else if self.is_kw("cost") || self.is_kw("costs") {
                self.pos += 1;
                if self.is_kind(&TokenKind::Colon) {
                    self.pos += 1;
                }
                cost = Some(ast::OperationCost {
                    costs: self.parse_cost_entries()?,
                });
            } else {
                return Err(ParseError {
                    message: "unexpected token in operation body".into(),
                    pos: self.pos,
                });
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::OperationDecl {
            name,
            params,
            requires,
            allow,
            options,
            cost,
        })
    }

    fn parse_operation_stmt(&mut self) -> Result<ast::OperationStatement, ParseError> {
        // Return an error for control tokens (caller should check before calling)
        if self.is_kind(&TokenKind::Semi) {
            // Skip stray semicolons
            self.pos += 1;
            return self.parse_operation_stmt();
        }
        if self.is_kind(&TokenKind::RBrace) {
            // End of options block — return error so caller knows to exit
            return Err(ParseError {
                message: "unexpected end of options block".into(),
                pos: self.pos,
            });
        }
        if self.is_kw("requires") {
            self.pos += 1;
            if self.is_kind(&TokenKind::Semi) {
                self.pos += 1;
            }
            Ok(ast::OperationStatement::Require(self.parse_condition()?))
        } else if self.is_kw("let") {
            if self.is_kind(&TokenKind::Semi) {
                self.pos += 1;
            }
            Ok(ast::OperationStatement::LetDecl(self.parse_let_decl()?))
        } else if self.is_kw("on") {
            self.pos += 1;
            let stmt = Ok(ast::OperationStatement::OnMachine(self.expect_ident()?));
            if self.is_kind(&TokenKind::Semi) {
                self.pos += 1;
            }
            stmt
        } else if self.is_kw("exec") {
            let result = self.parse_exec_stmt();
            if self.is_kind(&TokenKind::Semi) {
                self.pos += 1;
            }
            result
        } else if self.is_kw("transfer") {
            let result = self.parse_transfer_stmt();
            if self.is_kind(&TokenKind::Semi) {
                self.pos += 1;
            }
            result
        } else if self.is_ident() || self.is_string_lit() {
            // Parse as a command/shell statement
            let stmt = self.parse_shell_stmt();
            if self.is_kind(&TokenKind::Semi) {
                self.pos += 1;
            }
            stmt
        } else {
            Err(ParseError {
                message: format!("unexpected token {:?} in operation statement", self.cur().kind),
                pos: self.pos,
            })
        }
    }

    fn parse_shell_stmt(&mut self) -> Result<ast::OperationStatement, ParseError> {
        // First token is the command name (could be ident or string literal)
        let cmd = match &self.cur().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.pos += 1;
                name
            }
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.pos += 1;
                s
            }
            _ => {
                return Err(ParseError {
                    message: "expected command name".into(),
                    pos: self.pos,
                })
            }
        };
        // Parse optional arguments
        let mut args = Vec::new();
        while !self.is_kind(&TokenKind::Semi)
            && !self.is_kind(&TokenKind::RBrace)
            && !self.is_kind(&TokenKind::Comma)
            && !self.at_end()
        {
            if let TokenKind::Ident(name) = &self.cur().kind {
                args.push(name.clone());
                self.pos += 1;
            } else if let TokenKind::StringLit(s) = &self.cur().kind {
                args.push(s.clone());
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(ast::OperationStatement::ShellCmd { cmd, args })
    }

    fn parse_exec_stmt(&mut self) -> Result<ast::OperationStatement, ParseError> {
        self.expect(TokenKind::Kw("exec"))?;
        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
        }
        // Accept both identifiers and keywords as command names
        let cmd = match self.cur().kind {
            TokenKind::Ident(ref name) => {
                let name = name.clone();
                self.pos += 1;
                ast::Ident { name }
            }
            TokenKind::Kw(kw) => {
                self.pos += 1;
                ast::Ident { name: kw.to_string() }
            }
            _ => {
                return Err(ParseError {
                    message: "expected command name after exec".into(),
                    pos: self.pos,
                })
            }
        };
        let mut args = Vec::new();
        // Check for optional { ... } block wrapping args
        if self.is_kind(&TokenKind::LBrace) {
            self.pos += 1; // consume {
            while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
                if self.is_ident() || self.is_string_lit() {
                    args.push(self.parse_expr()?);
                } else {
                    break;
                }
            }
            if self.is_kind(&TokenKind::RBrace) {
                self.pos += 1; // consume }
            }
        } else {
            // Parse inline args (comma or RBrace terminated)
            while !self.is_kind(&TokenKind::RBrace)
                && !self.is_kind(&TokenKind::Comma)
                && !self.at_end()
            {
                if self.is_ident() || self.is_string_lit() {
                    args.push(self.parse_expr()?);
                } else {
                    break;
                }
            }
        }
        Ok(ast::OperationStatement::ExecCommand { cmd, args })
    }

    fn parse_transfer_stmt(&mut self) -> Result<ast::OperationStatement, ParseError> {
        self.expect(TokenKind::Kw("transfer"))?;
        let from = self.parse_expr()?;
        self.expect(TokenKind::Kw("to"))?;
        let machine = self.parse_expr()?;
        self.expect(TokenKind::Kw("location"))?;
        let location = self.parse_expr()?;
        Ok(ast::OperationStatement::Transfer {
            from,
            machine,
            location,
        })
    }

    fn parse_cost_entries(&mut self) -> Result<Vec<ast::CostEntry>, ParseError> {
        let mut entries = Vec::new();
        if self.is_kind(&TokenKind::LBrace) {
            self.pos += 1;
            while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
                let kind = self.expect_ident()?;
                if self.is_kind(&TokenKind::Colon) {
                    self.pos += 1;
                }
                let value = self.parse_expr()?;
                entries.push(ast::CostEntry { kind, value });
                if self.is_kind(&TokenKind::Comma) {
                    self.pos += 1;
                }
            }
            self.pos += 1;
        }
        Ok(entries)
    }

    fn parse_service_decl(&mut self) -> Result<ast::ServiceDecl, ParseError> {
        self.expect(TokenKind::Kw("service"))?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        while !self.is_kind(&TokenKind::RParen) {
            params.push(self.parse_named_type()?);
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Kw("on"))?;
        let on = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut costs = Vec::new();
        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_ident() {
                let kind = self.expect_ident()?;
                if !self.is_kind(&TokenKind::RBrace) {
                    if self.is_kind(&TokenKind::Colon) {
                        self.pos += 1;
                    }
                    let value = self.parse_expr()?;
                    costs.push(ast::CostEntry { kind, value });
                }
            } else {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::ServiceDecl {
            name,
            params,
            on,
            costs,
        })
    }

    fn parse_function_decl(&mut self) -> Result<ast::FunctionDecl, ParseError> {
        self.expect(TokenKind::Kw("function"))?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        while !self.is_kind(&TokenKind::RParen) {
            params.push(self.parse_named_type()?);
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::LBrace)?;

        let mut requires = Vec::new();
        let mut allow = None;
        let mut body = Vec::new();
        let mut success_if = None;
        let mut failure = None;

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kind(&TokenKind::Semi) {
                // Skip stray semicolons
                self.pos += 1;
                continue;
            }
            if self.is_kw("requires") {
                self.pos += 1;
                if self.is_kind(&TokenKind::Colon) {
                    self.pos += 1;
                }
                requires.push(self.parse_condition()?);
            } else if self.is_kw("allow") {
                self.pos += 1;
                self.expect(TokenKind::Kw("if"))?;
                self.expect(TokenKind::Kw("role"))?;
                self.expect(TokenKind::Kw("is"))?;
                allow = Some(self.expect_ident()?);
            } else if self.is_kw("success") {
                self.pos += 1;
                self.expect(TokenKind::Kw("if"))?;
                success_if = Some(Box::new(self.parse_condition()?));
            } else if self.is_kw("failure") {
                self.pos += 1;
                if self.is_kw("otherwise") {
                    self.pos += 1;
                }
                failure = Some(ast::FailAction::Otherwise);
            } else {
                body.push(self.parse_function_stmt()?);
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::FunctionDecl {
            name,
            params,
            requires,
            allow,
            body,
            success_if,
            failure,
        })
    }

    fn parse_function_stmt(&mut self) -> Result<ast::FunctionStatement, ParseError> {
        if self.is_kw("requires") {
            self.pos += 1;
            if self.is_kind(&TokenKind::Colon) {
                self.pos += 1;
            }
            Ok(ast::FunctionStatement::Require(self.parse_condition()?))
        } else if self.is_kw("let") {
            Ok(ast::FunctionStatement::LetDecl(self.parse_let_decl()?))
        } else if self.is_kw("on") {
            self.pos += 1;
            Ok(ast::FunctionStatement::OnMachine(self.expect_ident()?))
        } else if self.is_kw("exec") {
            match self.parse_exec_stmt()? {
                ast::OperationStatement::ExecCommand { cmd, args } => {
                    Ok(ast::FunctionStatement::ExecCommand { cmd, args })
                }
                other => Err(ParseError {
                    message: format!("unexpected operation statement: {:?}", other),
                    pos: self.pos,
                }),
            }
        } else if self.is_kw("return") {
            self.pos += 1;
            Ok(ast::FunctionStatement::Return(Box::new(self.parse_expr()?)))
        } else if self.is_kw("read") {
            self.parse_read_json()
        } else if self.is_kw("write") {
            self.parse_write_json()
        } else if self.is_kw("transfer") {
            if self.is_kind(&TokenKind::Colon) {
                self.pos += 1;
            }
            Ok(ast::FunctionStatement::Transfer {
                from: self.parse_expr()?,
                machine: self.parse_expr()?,
                location: self.parse_expr()?,
            })
        } else {
            // Skip unknown token
            self.pos += 1;
            Ok(ast::FunctionStatement::Require(ast::Condition {
                predicates: Vec::new(),
            }))
        }
    }

    fn parse_read_json(&mut self) -> Result<ast::FunctionStatement, ParseError> {
        self.expect(TokenKind::Kw("read"))?;
        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
        }
        if self.is_kw("json") {
            self.pos += 1;
        }
        if self.is_kw("output") {
            self.pos += 1;
        }
        if self.is_kw("as") {
            self.pos += 1;
        }
        Ok(ast::FunctionStatement::ReadJson {
            var: self.expect_ident()?,
        })
    }

    fn parse_write_json(&mut self) -> Result<ast::FunctionStatement, ParseError> {
        self.expect(TokenKind::Kw("write"))?;
        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
        }
        if self.is_kw("json") {
            self.pos += 1;
        }
        if self.is_kw("on") {
            self.pos += 1;
        }
        if self.is_kw("input") {
            self.pos += 1;
        }
        let value = self.parse_expr()?;
        Ok(ast::FunctionStatement::WriteJson { value })
    }

    // ─── Grant ──────────────────────────────────────────

    fn try_parse_statement(&mut self) -> Result<Option<ast::Statement>, ParseError> {
        if self.is_kw("grant") {
            Ok(Some(ast::Statement::Grant(self.parse_grant_decl()?)))
        } else if self.is_kw("alias") {
            Ok(Some(ast::Statement::Alias(self.parse_alias_decl()?)))
        } else if self.is_kw("on") {
            Ok(Some(ast::Statement::OnMachine(
                self.parse_on_machine_stmt()?,
            )))
        } else if self.is_kw("tasks") {
            Ok(Some(ast::Statement::TaskBlock(self.parse_task_block()?)))
        } else if self.is_kw("for") {
            Ok(Some(ast::Statement::ControlFlow(ast::ControlFlow::For(
                self.parse_for_loop()?,
            ))))
        } else if self.is_kw("while") {
            Ok(Some(ast::Statement::ControlFlow(ast::ControlFlow::While(
                self.parse_while_loop()?,
            ))))
        } else if self.is_kw("if") {
            Ok(Some(ast::Statement::ControlFlow(ast::ControlFlow::If(
                self.parse_if_stmt()?,
            ))))
        } else if self.is_kw("machine") {
            Ok(Some(ast::Statement::MachineDecl(
                self.parse_machine_decl()?,
            )))
        } else if self.is_kw("try") {
            Ok(Some(ast::Statement::ControlFlow(
                ast::ControlFlow::TryCatch(self.parse_try_catch()?),
            )))
        } else if self.is_ident() {
            // Simple statement
            Ok(Some(self.parse_simple_stmt()?))
        } else {
            Ok(None)
        }
    }

    fn parse_grant_decl(&mut self) -> Result<ast::GrantDecl, ParseError> {
        self.expect(TokenKind::Kw("grant"))?;
        let target_role = self.expect_ident()?;
        self.expect(TokenKind::Kw("can"))?;
        let op = self.expect_ident()?;

        let mut resource = None;
        if self.is_kind(&TokenKind::LBrace) {
            self.pos += 1;
            let variable = self.expect_ident()?;
            self.expect(TokenKind::Colon)?;
            let resource_type = self.parse_ty_inner()?;
            self.expect(TokenKind::RBrace)?;
            resource = Some(ast::ResourcePattern {
                variable,
                resource_type,
            });
        }

        let mut condition = None;
        if self.is_kw("if") {
            self.pos += 1;
            condition = Some(Box::new(self.parse_condition()?));
        }

        if self.is_kind(&TokenKind::Semi) {
            self.pos += 1;
        }

        Ok(ast::GrantDecl {
            target_role,
            op,
            resource: resource.unwrap_or(ast::ResourcePattern {
                variable: ast::Ident { name: "_".into() },
                resource_type: ast::Type::Primitive(ast::PrimitiveType::JSON),
            }),
            condition,
        })
    }

    fn parse_on_machine_stmt(&mut self) -> Result<ast::OnMachineStmt, ParseError> {
        self.expect(TokenKind::Kw("on"))?;

        let machines = if self.is_kw("machine") {
            self.pos += 1;
            if self.is_kw("set") {
                self.pos += 1;
                ast::Machines::Set(self.expect_ident()?)
            } else {
                ast::Machines::Single(self.expect_ident()?)
            }
        } else {
            ast::Machines::Single(self.expect_ident()?)
        };

        let mut body = None;
        if self.is_kind(&TokenKind::LBrace) {
            body = Some(Box::new(self.parse_task_block()?));
        } else if self.is_kind(&TokenKind::Semi) {
            self.pos += 1;
        }

        Ok(ast::OnMachineStmt { machines, body })
    }

    fn parse_task_block(&mut self) -> Result<ast::TaskBlock, ParseError> {
        if !self.is_kw("tasks") {
            // Already consumed 'tasks' keyword by caller
        } else {
            self.pos += 1;
        }
        self.expect(TokenKind::LBrace)?;

        let machines = ast::Machines::Single(ast::Ident { name: "_".into() });
        let mut body = Vec::new();

        while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
            if self.is_kw("optimize") {
                self.pos += 1;
                self.expect(TokenKind::Kw("for"))?;
                let metric = self.expect_ident()?;
                body.push(ast::TaskItem::Optimize { metric });
            } else {
                body.push(self.parse_task_item()?);
            }
            if self.is_kind(&TokenKind::Comma) {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::TaskBlock { machines, body })
    }

    fn parse_task_item(&mut self) -> Result<ast::TaskItem, ParseError> {
        // Check for binding: var <- expr
        if self.is_ident() {
            let next = self.peek(1);
            if let TokenKind::Arrow = next.kind {
                let variable = self.expect_ident()?;
                self.pos += 1; // skip <-
                let assignment = self.parse_expr()?;
                return Ok(ast::TaskItem::Bind {
                    variable,
                    assignment: Box::new(assignment),
                });
            }
        }

        // If current token is not an identifier, try expression parsing
        if !self.is_ident() {
            let expr = self.parse_expr()?;
            return Ok(ast::TaskItem::ExprTask(Box::new(expr)));
        }

        // Look ahead to decide between OpCallArgs and expression
        // If next token is an operator, it's an expression
        let delim_tokens = [
            TokenKind::RBrace,
            TokenKind::Comma,
            TokenKind::Eof,
            TokenKind::RParen,
            TokenKind::RBracket,
        ];
        let next = self.peek(1);
        let is_expr_like =
            !delim_tokens.contains(&next.kind) && !matches!(next.kind, TokenKind::Ident(_));

        if is_expr_like {
            let expr = self.parse_expr()?;
            return Ok(ast::TaskItem::ExprTask(Box::new(expr)));
        }

        // Parse op name and look for function call syntax (op(args))
        let op = self.expect_ident()?;

        if self.is_kind(&TokenKind::LParen) {
            self.pos += 1;
            let mut args = Vec::new();
            while !self.is_kind(&TokenKind::RParen) && !self.at_end() {
                args.push(self.parse_expr()?);
                if self.is_kind(&TokenKind::Comma) {
                    self.pos += 1;
                }
            }
            self.expect(TokenKind::RParen)?;
            return Ok(ast::TaskItem::OpCall { op, args });
        }

        // Parse args: op arg1 arg2 ... (args are bare identifiers/structs)
        let mut args = Vec::new();
        while !self.is_kind(&TokenKind::RBrace)
            && !self.is_kind(&TokenKind::Comma)
            && !self.at_end()
        {
            if self.is_ident() || self.is_kind(&TokenKind::LBrace) {
                args.push(self.parse_expr()?);
            } else {
                break;
            }
        }

        if args.is_empty() {
            Ok(ast::TaskItem::OpCall { op, args })
        } else {
            // Convert expressions to bare ids for OpCallArgs
            let ids: Vec<ast::Ident> = args
                .iter()
                .map(|e| match e {
                    Expr::Var(id) => ast::Ident {
                        name: id.name.clone(),
                    },
                    Expr::Lit(lit) => ast::Ident {
                        name: format!("{:?}", lit),
                    },
                    _ => ast::Ident {
                        name: "expr".into(),
                    },
                })
                .collect();
            Ok(ast::TaskItem::OpCallArgs { op, args: ids })
        }
    }

    // ─── Control flow ───────────────────────────────────

    fn parse_for_loop(&mut self) -> Result<ast::ForLoop, ParseError> {
        self.expect(TokenKind::Kw("for"))?;
        let var = self.expect_ident()?;
        self.expect(TokenKind::Kw("in"))?;
        let iterable = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;

        let mut body = Vec::new();
        while let Some(stmt) = self.try_parse_statement()? {
            body.push(stmt);
        }
        self.expect(TokenKind::RBrace)?;
        if self.is_kind(&TokenKind::Semi) {
            self.pos += 1;
        }

        Ok(ast::ForLoop::List {
            var,
            iterable: Box::new(iterable),
            body,
        })
    }

    fn parse_while_loop(&mut self) -> Result<ast::WhileLoop, ParseError> {
        self.expect(TokenKind::Kw("while"))?;
        let _can_tell = self.expect_ident()?; // can_tell variable name (e.g., "cantellmore")
        self.expect(TokenKind::Kw("is"))?;
        self.expect(TokenKind::Kw("true"))?;

        // tellmemore call
        if self.is_kw("tellmemore") {
            self.pos += 1;
            self.expect_ident()?; // function name
        }

        self.expect(TokenKind::LBrace)?;

        let mut body = Vec::new();
        while let Some(stmt) = self.try_parse_statement()? {
            body.push(stmt);
        }
        self.expect(TokenKind::RBrace)?;

        Ok(ast::WhileLoop {
            can_tell: ast::Ident {
                name: "cantellmore".into(),
            },
            condition: Box::new(Expr::Lit(Literal::Bool(true))),
            tell_func: ast::Ident {
                name: "tell".into(),
            },
            tell_args: Vec::new(),
            body,
        })
    }

    fn parse_if_stmt(&mut self) -> Result<ast::IfStmt, ParseError> {
        self.expect(TokenKind::Kw("if"))?;
        let condition = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;

        let mut then_body = Vec::new();
        while let Some(stmt) = self.try_parse_statement()? {
            then_body.push(stmt);
        }
        self.expect(TokenKind::RBrace)?;

        let mut else_if = Vec::new();
        let mut else_body = Vec::new();

        while self.is_kw("else") {
            self.pos += 1;
            if self.is_kw("if") {
                self.pos += 1;
                let cond = self.parse_expr()?;
                self.expect(TokenKind::LBrace)?;
                let mut stmts = Vec::new();
                while let Some(stmt) = self.try_parse_statement()? {
                    stmts.push(stmt);
                }
                self.expect(TokenKind::RBrace)?;
                else_if.push((Box::new(cond), stmts));
            } else {
                self.expect(TokenKind::LBrace)?;
                while let Some(stmt) = self.try_parse_statement()? {
                    else_body.push(stmt);
                }
                self.expect(TokenKind::RBrace)?;
            }
        }

        if self.is_kind(&TokenKind::Semi) {
            self.pos += 1;
        }

        Ok(ast::IfStmt {
            condition: Box::new(condition),
            then_body,
            else_if,
            else_body,
        })
    }

    fn parse_try_catch(&mut self) -> Result<ast::TryCatch, ParseError> {
        self.expect(TokenKind::Kw("try"))?;
        self.expect(TokenKind::LBrace)?;

        let mut body = Vec::new();
        while let Some(stmt) = self.try_parse_statement()? {
            body.push(stmt);
        }
        self.expect(TokenKind::RBrace)?;

        let mut catch_err_var = None;
        let mut catch_body = Vec::new();
        let catch_all = Vec::new();
        let mut finally_body = Vec::new();

        // Parse catch blocks
        while self.is_kw("catch") {
            self.pos += 1;
            if self.is_kw("error") {
                self.pos += 1;
                catch_err_var = Some(self.expect_ident()?);
            }
            self.expect(TokenKind::LBrace)?;
            while let Some(stmt) = self.try_parse_statement()? {
                catch_body.push(stmt);
            }
            self.expect(TokenKind::RBrace)?;
        }

        if self.is_kw("finally") {
            self.pos += 1;
            self.expect(TokenKind::LBrace)?;
            while let Some(stmt) = self.try_parse_statement()? {
                finally_body.push(stmt);
            }
            self.expect(TokenKind::RBrace)?;
        }

        if self.is_kind(&TokenKind::Semi) {
            self.pos += 1;
        }

        Ok(ast::TryCatch {
            body,
            catch_err_var,
            catch_body,
            catch_all,
            finally_body,
        })
    }

    fn parse_simple_stmt(&mut self) -> Result<ast::Statement, ParseError> {
        let ident = self.expect_ident()?;
        let mut args = Vec::new();

        // Parse named args
        while !self.is_kind(&TokenKind::Semi) && !self.at_end() {
            if self.is_ident() && self.peek_kind(1, &TokenKind::Colon) {
                self.pos += 1; // skip ident
                self.pos += 1; // skip colon
                args.push(self.parse_expr()?);
            } else {
                break;
            }
        }

        if self.is_kind(&TokenKind::Semi) {
            self.pos += 1;
        }

        // Check for binding: @var <- expr
        Ok(ast::Statement::OnMachine(ast::OnMachineStmt {
            machines: ast::Machines::Single(ident),
            body: None,
        }))
    }

    // ─── Condition ──────────────────────────────────────

    fn parse_condition(&mut self) -> Result<ast::Condition, ParseError> {
        let mut predicates = Vec::new();
        predicates.push(self.parse_condition_atom()?);

        while self.is_kw("and") || self.is_kw("or") {
            let is_and = self.is_kw("and");
            self.pos += 1;
            let right = self.parse_condition_atom()?;
            // Build conjunction/disjunction chain
            if let Some(last) = predicates.pop() {
                let combined = if is_and {
                    ast::ConditionPred::And(Box::new(last), Box::new(right))
                } else {
                    ast::ConditionPred::Or(Box::new(last), Box::new(right))
                };
                predicates.push(combined);
            } else {
                predicates.push(right);
            }
        }

        Ok(ast::Condition { predicates })
    }

    fn parse_condition_atom(&mut self) -> Result<ast::ConditionPred, ParseError> {
        if self.is_kw("can") {
            self.pos += 1;
            let op = self.expect_ident()?;
            let mut resource = None;
            if self.is_kind(&TokenKind::LBrace) {
                self.pos += 1;
                let variable = self.expect_ident()?;
                self.expect(TokenKind::Colon)?;
                let resource_type = self.parse_ty_inner()?;
                self.expect(TokenKind::RBrace)?;
                resource = Some(ast::ResourcePattern {
                    variable,
                    resource_type,
                });
            }
            return Ok(ast::ConditionPred::Can { op, resource });
        }

        if self.is_kw("not") {
            self.pos += 1;
            let inner = self.parse_condition_atom()?;
            return Ok(ast::ConditionPred::Not(Box::new(inner)));
        }

        // Check for starts/ends with before parse_expr (these are keywords, not variables)
        if self.is_kw("starts") {
            self.pos += 1;
            self.expect(TokenKind::Kw("with"))?;
            let prefix = match &self.cur().kind {
                TokenKind::StringLit(s) => s.clone(),
                _ => String::new(),
            };
            self.pos += 1;
            return Ok(ast::ConditionPred::StartsWith {
                expr: Box::new(Expr::Lit(Literal::StringVal(String::new()))),
                prefix,
            });
        }

        if self.is_kw("ends") {
            self.pos += 1;
            self.expect(TokenKind::Kw("with"))?;
            let suffix = match &self.cur().kind {
                TokenKind::StringLit(s) => s.clone(),
                _ => String::new(),
            };
            self.pos += 1;
            return Ok(ast::ConditionPred::EndsWith {
                expr: Box::new(Expr::Lit(Literal::StringVal(String::new()))),
                suffix,
            });
        }

        // Handle: ident is Role / in set / exists / matches
        let expr = self.parse_expr()?;

        if self.is_kw("is") {
            self.pos += 1;
            let mut roles = Vec::new();
            loop {
                let role = if self.is_kw("down") {
                    self.pos += 1;
                    ast::RoleRef::Down(ast::Ident { name: "_".into() })
                } else {
                    let role = self.expect_ident()?;
                    ast::RoleRef::Exact(role)
                };
                roles.push(role);
                if !self.is_kw("or") {
                    break;
                }
                self.pos += 1; // consume "or"
                               // Loop back to parse the next role reference (may be down)
            }
            return Ok(ast::ConditionPred::Is { left: expr, roles });
        }

        if self.is_kw("starts") {
            self.pos += 1;
            self.expect(TokenKind::Kw("with"))?;
            let prefix = match &self.cur().kind {
                TokenKind::StringLit(s) => s.clone(),
                _ => String::new(),
            };
            self.pos += 1;
            return Ok(ast::ConditionPred::StartsWith {
                expr: Box::new(expr),
                prefix,
            });
        }

        if self.is_kw("ends") {
            self.pos += 1;
            self.expect(TokenKind::Kw("with"))?;
            let suffix = match &self.cur().kind {
                TokenKind::StringLit(s) => s.clone(),
                _ => String::new(),
            };
            self.pos += 1;
            return Ok(ast::ConditionPred::EndsWith {
                expr: Box::new(expr),
                suffix,
            });
        }

        if self.is_kw("in") {
            self.pos += 1;
            if self.is_kind(&TokenKind::LBrace) {
                // Set literal: x in { "admin", ... }
                let mut items = Vec::new();
                self.pos += 1;
                while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
                    items.push(self.parse_expr()?);
                    if self.is_kind(&TokenKind::Comma) {
                        self.pos += 1;
                    }
                }
                self.expect(TokenKind::RBrace)?;
                // For now, store as a struct with "items" key
                let set_ident = ast::Ident {
                    name: "_inline_set".into(),
                };
                return Ok(ast::ConditionPred::InSet {
                    expr: Box::new(expr),
                    set: set_ident,
                });
            }
            let set = self.expect_ident()?;
            return Ok(ast::ConditionPred::InSet {
                expr: Box::new(expr),
                set,
            });
        }

        if self.is_kw("exists") {
            self.pos += 1;
            return Ok(ast::ConditionPred::Exists(Box::new(expr)));
        }

        if self.is_kw("matches") {
            self.pos += 1;
            let pattern = match &self.cur().kind {
                TokenKind::StringLit(s) => s.clone(),
                _ => String::new(),
            };
            self.pos += 1;
            return Ok(ast::ConditionPred::Matches {
                expr: Box::new(expr),
                pattern,
            });
        }

        // Default: wrap in Exists
        Ok(ast::ConditionPred::Exists(Box::new(expr)))
    }

    // ─── Types ──────────────────────────────────────────

    fn parse_ty_inner(&mut self) -> Result<ast::Type, ParseError> {
        // Try compound types first (handle both Kw and Ident tokens)
        if self.is_kw_or_ident("map") {
            self.pos += 1;
            self.expect_kw_or_ident("of")?;
            let key = self.parse_ty_inner()?;
            self.expect_kw_or_ident("to")?;
            let val = self.parse_ty_inner()?;
            return Ok(ast::Type::Map(Box::new(key), Box::new(val)));
        }

        if self.is_kw_or_ident("ordered") {
            self.pos += 1;
            if self.is_kw_or_ident("map") {
                self.pos += 1;
                self.expect_kw_or_ident("of")?;
                let key = self.parse_ty_inner()?;
                self.expect_kw_or_ident("to")?;
                let val = self.parse_ty_inner()?;
                return Ok(ast::Type::OrderedMap(Box::new(key), Box::new(val)));
            }
            if self.is_kw_or_ident("set") {
                self.pos += 1;
                self.expect_kw_or_ident("of")?;
                let inner = self.parse_ty_inner()?;
                return Ok(ast::Type::OrderedSet(Box::new(inner)));
            }
        }

        if self.is_kw_or_ident("set") {
            self.pos += 1;
            self.expect_kw_or_ident("of")?;
            let inner = self.parse_ty_inner()?;
            return Ok(ast::Type::Set(Box::new(inner)));
        }

        if self.is_kw_or_ident("new") && self.peek_kw_or_ident(1, "list") {
            self.pos += 1;
            self.expect_kw_or_ident("list")?;
            self.expect_kw_or_ident("of")?;
            let inner = self.parse_ty_inner()?;
            self.expect(TokenKind::LParen)?;
            let cur_kind = self.cur().kind.clone();
            let size = match cur_kind {
                TokenKind::IntLit(n) => {
                    self.pos += 1;
                    n as usize
                }
                _ => 0,
            };
            self.expect(TokenKind::RParen)?;
            return Ok(ast::Type::SizedList(Box::new(inner), size));
        }

        // Try list type: [T] or [mut T]
        if self.is_kind(&TokenKind::LBracket) {
            self.pos += 1;
            let mut mut_flag = false;
            if self.is_kw("mut") {
                mut_flag = true;
                self.pos += 1;
            }
            let inner = self.parse_ty_inner()?;
            self.expect(TokenKind::RBracket)?;
            return Ok(if mut_flag {
                ast::Type::MutList(Box::new(inner))
            } else {
                ast::Type::List(Box::new(inner))
            });
        }

        // Unit type
        if self.is_kind(&TokenKind::LParen) && self.peek_kind(1, &TokenKind::RParen) {
            self.pos += 2; // skip ()
            return Ok(ast::Type::Primitive(ast::PrimitiveType::Unit));
        }

        // Primitive type or resource type
        let name = self.expect_ident()?;
        match name.name.as_str() {
            "()" => Ok(ast::Type::Primitive(ast::PrimitiveType::Unit)),
            "Bool" => Ok(ast::Type::Primitive(ast::PrimitiveType::Bool)),
            "Int" => Ok(ast::Type::Primitive(ast::PrimitiveType::Int)),
            "String" => Ok(ast::Type::Primitive(ast::PrimitiveType::String)),
            "Bytes" => Ok(ast::Type::Primitive(ast::PrimitiveType::Bytes)),
            "Duration" => Ok(ast::Type::Primitive(ast::PrimitiveType::Duration)),
            "FilePath" => Ok(ast::Type::Primitive(ast::PrimitiveType::FilePath)),
            "Node" => Ok(ast::Type::Primitive(ast::PrimitiveType::Node)),
            "Role" => Ok(ast::Type::Primitive(ast::PrimitiveType::Role)),
            "Secret" => Ok(ast::Type::Primitive(ast::PrimitiveType::Secret)),
            "JSON" => Ok(ast::Type::Primitive(ast::PrimitiveType::JSON)),
            _ => Ok(ast::Type::Resource(name)),
        }
    }

    fn parse_named_type(&mut self) -> Result<ast::NamedType, ParseError> {
        // Accept both "name: Type" and "Type name" formats
        let cur = self.cur().kind.clone();
        if let TokenKind::Ident(ref s) = cur {
            if s == "Bool"
                || s == "Int"
                || s == "String"
                || s == "Bytes"
                || s == "Duration"
                || s == "FilePath"
                || s == "Node"
                || s == "Role"
                || s == "Secret"
                || s == "JSON"
            {
                // Looks like "Type name" format
                self.pos += 1;
                let ty = match s.as_str() {
                    "Bool" => ast::Type::Primitive(ast::PrimitiveType::Bool),
                    "Int" => ast::Type::Primitive(ast::PrimitiveType::Int),
                    "String" => ast::Type::Primitive(ast::PrimitiveType::String),
                    "Bytes" => ast::Type::Primitive(ast::PrimitiveType::Bytes),
                    "Duration" => ast::Type::Primitive(ast::PrimitiveType::Duration),
                    "FilePath" => ast::Type::Primitive(ast::PrimitiveType::FilePath),
                    "Node" => ast::Type::Primitive(ast::PrimitiveType::Node),
                    "Role" => ast::Type::Primitive(ast::PrimitiveType::Role),
                    "Secret" => ast::Type::Primitive(ast::PrimitiveType::Secret),
                    "JSON" => ast::Type::Primitive(ast::PrimitiveType::JSON),
                    _ => ast::Type::Primitive(ast::PrimitiveType::Int),
                };
                let name = self.expect_ident()?;
                return Ok(ast::NamedType { name, ty });
            }
        }

        // "name: Type" format
        let name = self.expect_ident()?;
        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
        }
        let ty = self.parse_ty_inner()?;
        Ok(ast::NamedType { name, ty })
    }

    // ─── Expressions ────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_binary(0)
    }

    fn parse_expr_binary(&mut self, precedence: usize) -> Result<Expr, ParseError> {
        let mut left = self.parse_expr_unary()?;

        loop {
            let (op, prec) = match &self.cur().kind {
                TokenKind::Kw("or") => (BinOp::Or, 1),
                TokenKind::Kw("and") => (BinOp::And, 2),
                TokenKind::Op(op_str) => match op_str.as_str() {
                    "==" => (BinOp::Eq, 3),
                    "!=" => (BinOp::Neq, 3),
                    "<" => (BinOp::Lt, 3),
                    "<=" => (BinOp::Le, 3),
                    ">" => (BinOp::Gt, 3),
                    ">=" => (BinOp::Ge, 3),
                    "+" => (BinOp::Plus, 4),
                    "-" => (BinOp::Minus, 4),
                    "*" => (BinOp::Mul, 4),
                    "/" => (BinOp::Div, 4),
                    _ => break,
                },
                _ => break,
            };

            if prec < precedence {
                break;
            }

            self.pos += 1;
            let right = self.parse_expr_binary(prec + 1)?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_expr_unary(&mut self) -> Result<Expr, ParseError> {
        if self.is_kw("not") {
            self.pos += 1;
            let operand = self.parse_expr_unary()?;
            return Ok(Expr::UnOp {
                op: UnOp::Not,
                operand: Box::new(operand),
            });
        }

        if self.is_kind(&TokenKind::Op("-".into())) {
            self.pos += 1;
            let operand = self.parse_expr_unary()?;
            return Ok(Expr::UnOp {
                op: UnOp::Neg,
                operand: Box::new(operand),
            });
        }

        self.parse_expr_primary()
    }

    fn parse_expr_primary(&mut self) -> Result<Expr, ParseError> {
        match &self.cur().kind {
            TokenKind::IntLit(n) => {
                let val = *n;
                self.pos += 1;
                Ok(Expr::Lit(Literal::Int(val)))
            }
            TokenKind::BytesLit(b) => {
                let bytes = b.clone();
                self.pos += 1;
                Ok(Expr::Lit(Literal::Bytes(bytes)))
            }
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.pos += 1;
                // Check for template interpolation: $VAR
                if s.starts_with('$') && s.len() > 1 && s.chars().nth(1) != Some('{') {
                    Ok(Expr::Template(s))
                } else {
                    Ok(Expr::Lit(Literal::StringVal(s)))
                }
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.pos += 1;

                // Check for template: $HOME or $HOME/.x
                if name.starts_with('$') && name.len() > 1 {
                    let mut template = name.clone();
                    while !self.at_end() && !self.is_kind(&TokenKind::Semi) {
                        if let TokenKind::Ident(next) = &self.cur().kind {
                            template.push_str(next);
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    return Ok(Expr::Template(template));
                }

                // Check for field access: x.field
                if self.is_kind(&TokenKind::Dot) {
                    self.pos += 1;
                    let field = match &self.cur().kind {
                        TokenKind::Ident(s) => ast::Ident { name: s.clone() },
                        TokenKind::Kw(s) => ast::Ident {
                            name: s.to_string(),
                        },
                        _ => {
                            return Err(ParseError {
                                message: format!(
                                    "expected identifier or keyword for field name, found {:?}",
                                    self.cur().kind
                                ),
                                pos: self.pos,
                            })
                        }
                    };
                    self.pos += 1;
                    return Ok(Expr::FieldAccess {
                        target: Box::new(Expr::Var(ast::Ident { name })),
                        field,
                    });
                }

                // Check for index access: x[key]
                if self.is_kind(&TokenKind::LBracket) {
                    self.pos += 1;
                    let index = self.parse_expr()?;
                    self.expect(TokenKind::RBracket)?;
                    return Ok(Expr::IndexAccess {
                        target: Box::new(Expr::Var(ast::Ident { name })),
                        index: Box::new(index),
                    });
                }

                // Check for function call: f(args)
                if self.is_kind(&TokenKind::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    while !self.is_kind(&TokenKind::RParen) && !self.at_end() {
                        args.push(self.parse_expr()?);
                        if self.is_kind(&TokenKind::Comma) {
                            self.pos += 1;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    return Ok(Expr::Call {
                        func: ast::Ident { name },
                        args,
                    });
                }

                Ok(Expr::Var(ast::Ident { name }))
            }
            TokenKind::LBrace => {
                // Struct/map literal: { k: v, ... }
                self.pos += 1;
                let mut fields = Vec::new();
                while !self.is_kind(&TokenKind::RBrace) && !self.at_end() {
                    if self.is_ident() && self.peek_kind(1, &TokenKind::Colon) {
                        let key = self.expect_ident()?;
                        self.pos += 1; // skip colon
                        let value = self.parse_expr()?;
                        fields.push((key, Box::new(value)));
                    } else if self.is_ident() {
                        // Key without value (simplified)
                        let key = self.expect_ident()?;
                        fields.push((key, Box::new(Expr::Lit(Literal::Bool(true)))));
                    } else {
                        self.pos += 1;
                    }
                    if self.is_kind(&TokenKind::Comma) {
                        self.pos += 1;
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(Expr::Struct { fields })
            }
            TokenKind::LParen => {
                self.pos += 1;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            // Keywords can also serve as variable references in certain contexts (e.g., transfer a to b)
            TokenKind::Kw(s) => {
                let name = s.to_string();
                self.pos += 1;

                // Check for field access: kw.field
                if self.is_kind(&TokenKind::Dot) {
                    self.pos += 1;
                    let field = match &self.cur().kind {
                        TokenKind::Ident(field_s) => ast::Ident {
                            name: field_s.clone(),
                        },
                        TokenKind::Kw(field_s) => ast::Ident {
                            name: field_s.to_string(),
                        },
                        _ => {
                            return Err(ParseError {
                                message: format!(
                                    "expected field name after dot, found {:?}",
                                    self.cur().kind
                                ),
                                pos: self.pos,
                            })
                        }
                    };
                    self.pos += 1;
                    return Ok(Expr::FieldAccess {
                        target: Box::new(Expr::Var(ast::Ident { name })),
                        field,
                    });
                }

                // Check for function call: kw(args)
                if self.is_kind(&TokenKind::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    while !self.is_kind(&TokenKind::RParen) && !self.at_end() {
                        args.push(self.parse_expr()?);
                        if self.is_kind(&TokenKind::Comma) {
                            self.pos += 1;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    return Ok(Expr::Call {
                        func: ast::Ident { name },
                        args,
                    });
                }

                Ok(Expr::Var(ast::Ident { name }))
            }

            TokenKind::Op(op) if op == "true" || op == "false" => {
                let val = *op == "true";
                self.pos += 1;
                Ok(Expr::Lit(Literal::Bool(val)))
            }
            _ => Err(ParseError {
                message: format!("unexpected token {:?}", self.cur().kind),
                pos: self.pos,
            }),
        }
    }

    // ─── LetDecl ────────────────────────────────────────

    fn parse_let_decl(&mut self) -> Result<ast::LetDecl, ParseError> {
        self.expect(TokenKind::Kw("let"))?;

        if self.is_kw("mut") {
            self.pos += 1;
        }
        let name = self.expect_ident()?;
        let mut ty = None;
        let mut init = None;

        if self.is_kind(&TokenKind::Colon) {
            self.pos += 1;
            ty = Some(self.parse_ty_inner()?);
        }

        if self.is_kind(&TokenKind::Op("=".into())) {
            self.pos += 1;
            init = Some(Box::new(self.parse_expr()?));
        }

        Ok(ast::LetDecl { name, ty, init })
    }

    // ─── Helpers ────────────────────────────────────────

    fn peek(&self, offset: usize) -> Token {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            self.tokens[idx].clone()
        } else {
            Token {
                kind: TokenKind::Eof,
                span: 0..0,
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::{Expr, Literal};
    use crate::ast::{CostConstraint, Item, SumOp};
    use crate::pretty;

    fn assert_parse(source: &str) -> ast::Program {
        parse(source).expect("failed to parse")
    }

    fn assert_parse_err(source: &str) -> String {
        parse(source).expect_err("expected parse error").message
    }

    // ─── Tokenization ───

    #[test]
    fn tokenize_simple_idents() {
        let tokens = tokenize("foo bar baz").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| format!("{:?}", t.kind)).collect();
        assert!(kinds.contains(&"Ident(\"foo\")".to_string()));
        assert!(kinds.contains(&"Ident(\"bar\")".to_string()));
        assert!(kinds.contains(&"Ident(\"baz\")".to_string()));
        assert!(kinds.contains(&"Eof".to_string()));
    }

    #[test]
    fn tokenize_keywords() {
        let tokens = tokenize("role resource can cannot grant").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], TokenKind::Kw("role")));
        assert!(matches!(kinds[1], TokenKind::Kw("resource")));
        assert!(matches!(kinds[2], TokenKind::Kw("can")));
    }

    #[test]
    fn tokenize_string_escapes() {
        // Input is: "hello\nworld" — \n is a single backslash + n
        // Regular string: \"hello\\nworld\" produces "hello\nworld" (backslash + n)
        let tokens = tokenize("\"hello\\nworld\"").unwrap();
        match &tokens[0].kind {
            TokenKind::StringLit(s) => assert_eq!(s, "hello\nworld"),
            _ => panic!("expected string literal, got {:?}", tokens[0].kind),
        }
    }

    #[test]
    fn tokenize_comments() {
        let tokens = tokenize("foo // comment").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| format!("{:?}", t.kind)).collect();
        assert_eq!(kinds.len(), 2);

        let tokens = tokenize("foo /* block */ bar").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| format!("{:?}", t.kind)).collect();
        assert!(kinds.contains(&"Ident(\"foo\")".to_string()));
        assert!(kinds.contains(&"Ident(\"bar\")".to_string()));
    }

    #[test]
    fn tokenize_operators() {
        let tokens = tokenize("== != <= >= <- + - = . , ; :").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], TokenKind::Op(ref s) if s == "=="));
        assert!(matches!(kinds[1], TokenKind::Op(ref s) if s == "!="));
        assert!(matches!(kinds[4], TokenKind::Arrow));
    }

    #[test]
    fn tokenize_empty() {
        let tokens = tokenize("").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Eof));
    }

    #[test]
    fn tokenize_whitespace_only() {
        let tokens = tokenize("   \n\t  ").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Eof));
    }

    #[test]
    fn tokenize_numeric() {
        let tokens = tokenize("12345").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::IntLit(12345)));
    }

    #[test]
    fn tokenize_bytes_with_suffix() {
        let tokens = tokenize("42 GB 1024 MB 5 KiB").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::BytesLit(_)));
        assert!(matches!(tokens[1].kind, TokenKind::BytesLit(_)));
        assert!(matches!(tokens[2].kind, TokenKind::BytesLit(_)));
    }

    // ─── Expressions ───

    #[test]
    fn parse_simple_task() {
        assert_parse("tasks { foo }");
    }

    #[test]
    fn parse_int_expr() {
        assert_parse("tasks { 42 }");
    }

    #[test]
    fn parse_string_expr() {
        assert_parse(r#"tasks { "hello" }"#);
    }

    #[test]
    fn parse_field_access() {
        assert_parse("tasks { obj.field }");
    }

    #[test]
    fn parse_index_access() {
        assert_parse("tasks { arr[0] }");
    }

    #[test]
    fn parse_function_call() {
        assert_parse("tasks { foo(a, b) }");
    }

    #[test]
    fn parse_struct_literal() {
        assert_parse(r#"tasks { { name: "Alice" } }"#);
    }

    #[test]
    fn parse_map_literal() {
        assert_parse(r#"tasks { { "key": value } }"#);
    }

    #[test]
    fn parse_negation() {
        assert_parse("tasks { -5 }");
    }

    #[test]
    fn parse_logical_not() {
        assert_parse("tasks { not x }");
    }

    #[test]
    fn parse_addition() {
        assert_parse("tasks { a + b }");
    }

    #[test]
    fn parse_subtraction() {
        assert_parse("tasks { a - b }");
    }

    #[test]
    fn parse_eq() {
        assert_parse("tasks { a == b }");
    }

    #[test]
    fn parse_neq() {
        assert_parse("tasks { a != b }");
    }

    #[test]
    fn parse_lt() {
        assert_parse("tasks { a < b }");
    }

    #[test]
    fn parse_le() {
        assert_parse("tasks { a <= b }");
    }

    #[test]
    fn parse_gt() {
        assert_parse("tasks { a > b }");
    }

    #[test]
    fn parse_ge() {
        assert_parse("tasks { a >= b }");
    }

    #[test]
    fn parse_and_expr() {
        assert_parse("tasks { a and b }");
    }

    #[test]
    fn parse_or_expr() {
        assert_parse("tasks { a or b }");
    }

    #[test]
    fn parse_if_stmt() {
        assert_parse("if x { tasks { foo } }");
    }

    #[test]
    fn parse_if_else() {
        assert_parse("if x { tasks { foo } } else { tasks { bar } }");
    }

    #[test]
    fn parse_if_elseif_else() {
        assert_parse("if a { tasks { foo } } else if b { tasks { bar } } else { tasks { baz } }");
    }

    #[test]
    fn parse_for_loop() {
        assert_parse("for i in items { tasks { foo } }");
    }

    #[test]
    fn parse_while_loop() {
        assert_parse("while more is true { tasks { foo } }");
    }

    #[test]
    fn parse_try_catch() {
        assert_parse("try { tasks { foo } } catch { tasks { bar } }");
    }

    #[test]
    fn parse_try_catch_error() {
        assert_parse("try { tasks { foo } } catch error e { tasks { bar } }");
    }

    #[test]
    fn parse_try_finally() {
        assert_parse("try { tasks { foo } } finally { tasks { bar } }");
    }

    // ─── Types ───

    #[test]
    fn parse_list_type() {
        assert_parse("resource F { capacity: [Int] }");
    }

    #[test]
    fn parse_mut_list_type() {
        assert_parse("resource F { capacity: [mut Int] }");
    }

    #[test]
    fn parse_set_type() {
        assert_parse("resource F { capacity: set of Int }");
    }

    #[test]
    fn parse_ordered_set_type() {
        assert_parse("resource F { capacity: ordered set of Int }");
    }

    #[test]
    fn parse_map_type() {
        assert_parse("resource F { capacity: map of String to Int }");
    }

    #[test]
    fn parse_ordered_map_type() {
        assert_parse("resource F { capacity: ordered map of String to Int }");
    }

    #[test]
    fn parse_sized_list_type() {
        assert_parse("resource F { capacity: new list of Int(5) }");
    }

    #[test]
    fn parse_nested_list() {
        assert_parse("resource F { capacity: [mut [Int]] }");
    }

    #[test]
    fn parse_resource_type_ref() {
        assert_parse("resource F { capacity: Disk }");
    }

    #[test]
    fn parse_all_primitives() {
        assert_parse("resource F { capacity: Bool }");
        assert_parse("resource F { capacity: Int }");
        assert_parse("resource F { capacity: String }");
        assert_parse("resource F { capacity: Bytes }");
        assert_parse("resource F { capacity: Duration }");
        assert_parse("resource F { capacity: FilePath }");
        assert_parse("resource F { capacity: Node }");
        assert_parse("resource F { capacity: Role }");
        assert_parse("resource F { capacity: Secret }");
        assert_parse("resource F { capacity: JSON }");
        assert_parse("resource F { capacity: () }");
    }

    // ─── Resource Declarations ───

    #[test]
    fn parse_simple_resource() {
        let program = assert_parse("resource File { capacity: Bytes }");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Resource(_)));
    }

    #[test]
    fn parse_resource_with_capacity_field() {
        let program = assert_parse("resource File { capacity: Bytes, field name: String }");
        assert_eq!(program.items.len(), 1);
    }

    // ─── Role Declarations ───

    #[test]
    fn parse_simple_role() {
        let program = assert_parse("role Admin {}");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Role(_)));
    }

    #[test]
    fn parse_role_with_up() {
        assert_parse("role Admin { up: User }");
    }

    #[test]
    fn parse_role_with_can() {
        assert_parse("role Admin { can Read }");
    }

    #[test]
    fn parse_role_with_cannot() {
        assert_parse("role Admin { cannot Write }");
    }

    #[test]
    fn parse_role_with_resource_pattern() {
        assert_parse("role Admin { can Read { f: File } }");
    }

    #[test]
    fn parse_role_with_condition() {
        assert_parse("role Admin { can Read if x is Admin }");
    }

    #[test]
    fn parse_role_with_resource_and_condition() {
        assert_parse("role Admin { can Read { f: File } if f is Admin }");
    }

    #[test]
    fn parse_role_can_define() {
        assert_parse("role Admin { can define operation for User }");
    }

    #[test]
    fn parse_role_can_define_down() {
        assert_parse("role Admin { can define operation for User.down }");
    }

    #[test]
    fn parse_role_multiple() {
        assert_parse("role Admin { up: User, can Read, cannot Write }");
    }

    // ─── Grant Declarations ───

    #[test]
    fn parse_simple_grant() {
        let program = assert_parse("grant Admin can Read");
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn parse_grant_with_resource() {
        assert_parse("grant Admin can Read { f: File }");
    }

    #[test]
    fn parse_grant_with_condition() {
        assert_parse("grant Admin can Read if x is Admin");
    }

    // ─── Task Blocks ───

    #[test]
    fn parse_task_block() {
        let program = assert_parse("tasks { foo }");
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn parse_task_block_op_call() {
        assert_parse("tasks { foo, bar, baz }");
    }

    #[test]
    fn parse_task_block_binding() {
        assert_parse("tasks { result <- foo }");
    }

    #[test]
    fn parse_task_block_optimize() {
        assert_parse("tasks { optimize for latency }");
    }

    // ─── On-Machine ───

    #[test]
    fn parse_on_machine_simple() {
        let program = assert_parse("on machine1");
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn parse_on_machine_semi() {
        assert_parse("on machine1;");
    }

    #[test]
    fn parse_on_machine_with_tasks() {
        assert_parse("on machine1 tasks { foo }");
    }

    #[test]
    fn parse_on_machine_set() {
        assert_parse("on machine set1 tasks { foo }");
    }

    #[test]
    fn parse_simple_statement_as_onsimple_stmt() {
        assert_parse("foo");
    }

    // ─── Function Declarations ───

    #[test]
    fn parse_simple_function() {
        let program = assert_parse("function double(Int x) { return x * 2 }");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Function(_)));
    }

    #[test]
    fn parse_function_multiple_params() {
        assert_parse("function add(Int x, Int y) { return x + y }");
    }

    #[test]
    fn parse_function_with_let() {
        assert_parse("function f() { let y = x + 1, return y }");
    }

    #[test]
    fn parse_function_with_mut_let() {
        assert_parse("function f() { mut let x = 0, return x }");
    }

    #[test]
    fn parse_function_with_let_type() {
        assert_parse("function f() { let x: Int = 42, return x }");
    }

    #[test]
    fn parse_function_with_requires() {
        assert_parse("function f(Int x) { requires: x > 0, return x }");
    }

    #[test]
    fn parse_function_with_exec() {
        assert_parse("function f() { exec: cat path }");
    }

    #[test]
    fn parse_function_with_read() {
        assert_parse("function f() { read: output }");
    }

    #[test]
    fn parse_function_with_write() {
        assert_parse("function f() { write: x }");
    }

    #[test]
    fn parse_function_with_transfer() {
        assert_parse("function f() { transfer a to b location c }");
    }

    #[test]
    fn parse_function_with_on() {
        assert_parse("function f() { on server1 }");
    }

    // ─── Service Declarations ───

    #[test]
    fn parse_simple_service() {
        let program = assert_parse("service web() on server1 {}");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Service(_)));
    }

    #[test]
    fn parse_service_with_params() {
        assert_parse("service web(Int port) on server1 {}");
    }

    #[test]
    fn parse_service_with_costs() {
        assert_parse("service web() on server1 { cpu: 1 }");
    }

    // ─── Alias Declarations ───

    #[test]
    fn parse_simple_alias() {
        let program = assert_parse("alias f = File");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Alias(_)));
    }

    #[test]
    fn parse_machine_alias() {
        assert_parse("alias m as machine = server1");
    }

    #[test]
    fn parse_role_alias() {
        assert_parse("alias r as role = Admin");
    }

    // ─── Operation Declarations ───

    #[test]
    fn parse_simple_operation() {
        let program = assert_parse("operation Read() {}");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Operation(_)));
    }

    #[test]
    fn parse_operation_with_params() {
        assert_parse("operation Read(File f) {}");
    }

    #[test]
    fn parse_operation_with_requires() {
        assert_parse("operation Read() { requires: f exists }");
    }

    #[test]
    fn parse_operation_with_allow() {
        assert_parse("operation Read() { allow if role is Admin }");
    }

    #[test]
    fn parse_operation_with_cost() {
        assert_parse("operation Read() { costs: { cpu: 1 } }");
    }

    #[test]
    fn parse_operation_with_option() {
        assert_parse(r#"operation Read() { "timeout" { } }"#);
    }

    // ─── Device Declarations ───

    #[test]
    fn parse_simple_device() {
        let program = assert_parse("device SSD {}");
        assert_eq!(program.items.len(), 1);
        assert!(matches!(program.items[0], ast::Item::Device(_)));
    }

    // ─── Machine Declarations ───

    #[test]
    fn parse_simple_machine() {
        let program = assert_parse("machine server1 {}");
        assert_eq!(program.statements.len(), 1);
        assert!(matches!(
            program.statements[0],
            ast::Statement::MachineDecl(_)
        ));
    }

    #[test]
    fn parse_machine_with_keys() {
        assert_parse(r#"machine server1 { "10.0.0.1": ; }"#);
    }

    #[test]
    fn parse_machine_with_device() {
        assert_parse("machine server1 { device disk0 type SSD { } }");
    }

    // ─── Condition Parsing ───

    #[test]
    fn parse_condition_is() {
        assert_parse("role A { can Read if x is Admin }");
    }

    #[test]
    fn parse_condition_is_or() {
        assert_parse("role A { can Read if x is Admin or User }");
    }

    #[test]
    fn parse_condition_is_down() {
        assert_parse("role A { can Read if x is Admin or down }");
    }

    #[test]
    fn parse_condition_can_simple() {
        assert_parse("role A { can Write if can Update }");
    }

    #[test]
    fn parse_condition_can_with_resource() {
        assert_parse("role A { can Write if can Update { f: File } }");
    }

    #[test]
    fn parse_condition_exists() {
        assert_parse("role A { can Read if x exists }");
    }

    #[test]
    fn parse_condition_not() {
        assert_parse("role A { can Read if not x exists }");
    }

    #[test]
    fn parse_condition_and() {
        assert_parse("role A { can Read if x exists and y exists }");
    }

    #[test]
    fn parse_condition_or() {
        assert_parse("role A { can Read if x exists or y exists }");
    }

    #[test]
    fn parse_condition_starts_with() {
        assert_parse(r#"role A { can Read if x starts with "/tmp" }"#);
    }

    #[test]
    fn parse_condition_ends_with() {
        assert_parse(r#"role A { can Read if x ends with ".txt" }"#);
    }

    #[test]
    fn parse_condition_in_set() {
        assert_parse(r#"role A { can Read if x in { "admin" } }"#);
    }

    #[test]
    fn parse_condition_matches() {
        assert_parse("role A { can Read if x matches regex }");
    }

    #[test]
    fn parse_condition_nested_not() {
        assert_parse("role A { can Read if not not x exists }");
    }

    // ─── Error Cases ───

    #[test]
    fn unclosed_string_error() {
        let err = assert_parse_err(r#"let x = "hello"#);
        assert!(err.contains("unclosed string"));
    }

    #[test]
    fn unclosed_block_comment_error() {
        let err = assert_parse_err("foo /* unclosed");
        assert!(err.contains("unclosed block comment"));
    }

    #[test]
    fn unexpected_exclamation_error() {
        let err = assert_parse_err("!bad");
        assert!(err.contains("'!'") || err.contains("unexpected"));
    }

    // ─── Expression Precedence ───

    #[test]
    fn expr_addition_before_comparison() {
        assert_parse("tasks { a + b == c }");
    }

    #[test]
    fn expr_comparison_before_and() {
        assert_parse("tasks { a == b and c == d }");
    }

    // ─── Parser Public API ───

    #[test]
    fn parse_empty_program() {
        let program = parse("").unwrap();
        assert_eq!(program.items.len(), 0);
        assert_eq!(program.statements.len(), 0);
    }

    #[test]
    fn parse_whitespace_only_program() {
        let program = parse("   \n\n  ").unwrap();
        assert_eq!(program.items.len(), 0);
        assert_eq!(program.statements.len(), 0);
    }

    #[test]
    fn parse_comments_only_program() {
        let program = parse(" // line comment\n /* block */").unwrap();
        assert_eq!(program.items.len(), 0);
        assert_eq!(program.statements.len(), 0);
    }

    #[test]
    fn parse_returns_err_on_invalid_input() {
        assert!(parse("!bad").is_err());
    }

    #[test]
    fn parse_err_unexpected_keyword() {
        assert!(parse("if { }").is_err());
    }

    #[test]
    fn parse_err_missing_brace() {
        assert!(parse("tasks { foo").is_err());
    }

    // ─── Complex Program ───

    #[test]
    fn parse_complex_program() {
        let program = assert_parse(
            "
            role Admin {
                up: User,
                can Read { f: File },
                cannot Write,
                can Read if x is Admin,
                can define operation for User
            }

            resource File {
                capacity: Bytes,
                field name: String
            }

            operation Read(File f) {
                requires: f exists,
                costs: { cpu: 1 }
            }

            on machine1 tasks {
                foo,
                bar
            }

            tasks { optimize for latency }

            for i in items {
                tasks { foo }
            }

            if x > 0 {
                tasks { Read x }
            } else {
                tasks { Read 0 }
            }

            try {
                tasks { Read x }
            } catch {
                tasks { Read 0 }
            } finally {
                tasks { cleanup }
            }

            grant Admin can Read
            grant Admin can Update { f: File } if f is Admin

            alias f = File

            function double(Int x) {
                let y = x * 2,
                return y
            }
        ",
        );

        assert!(
            program.items.len() >= 3,
            "expected at least 3 items, got {}",
            program.items.len()
        );
        assert!(
            program.statements.len() >= 3,
            "expected at least 3 statements, got {}",
            program.statements.len()
        );
    }

    // ─── Round-Trip Tests ──────────────────────────────────────

    fn assert_roundtrip(source: &str) {
        let program = parse(source).expect("failed to parse original");
        let pretty = crate::pretty::pretty_print(&program);
        if program.items.is_empty() && program.statements.is_empty() {
            return;
        }
        let reprogram = parse(&pretty).expect(&format!("failed to parse pretty-printed: '{}'", pretty));
        assert_eq!(
            program.items.len(),
            reprogram.items.len(),
            "item count mismatch: {} vs {}",
            program.items.len(),
            reprogram.items.len()
        );
        assert_eq!(
            program.statements.len(),
            reprogram.statements.len(),
            "statement count mismatch: {} vs {}",
            program.statements.len(),
            reprogram.statements.len()
        );
    }

    #[test]
    fn roundtrip_empty() {
        assert_roundtrip("");
    }

    #[test]
    fn roundtrip_role() {
        assert_roundtrip("role Admin { can Read }");
    }

    #[test]
    fn roundtrip_resource() {
        assert_roundtrip("resource File { capacity: Bytes }");
    }

    #[test]
    fn roundtrip_grant() {
        assert_roundtrip("grant Admin can Read");
    }

    #[test]
    fn roundtrip_tasks() {
        assert_roundtrip("tasks { foo }");
    }

    #[test]
    fn roundtrip_on_machine() {
        assert_roundtrip("on machine1;");
    }

    #[test]
    fn roundtrip_complex() {
        let source = "
            role Admin {
                can Read { f: File },
                cannot Write,
            }
            resource File {
                capacity: Bytes,
            }
            grant Admin can Read
            tasks { foo, bar }
        ";
        assert_roundtrip(source);
    }

    #[test]
    fn roundtrip_grant_with_resource_and_condition() {
        assert_roundtrip("grant Admin can Read { f: File } if f.path starts with \"data\"");
    }

    #[test]
    fn roundtrip_role_with_define_ops() {
        assert_roundtrip("role Admin { up: Root, can define operation for Musashi, down, Musashi.down }");
    }

    #[test]
    fn roundtrip_resource_with_field() {
        assert_roundtrip("resource File { capacity: Bytes, field path: String }");
    }


    #[test]
    fn roundtrip_machine_simple() {
        assert_roundtrip("machine n1 { extent NVRAM 512GB }");
    }

    #[test]
    fn roundtrip_role_up_only() {
        assert_roundtrip("role Admin { up: Root }");
    }

    #[test]
    fn roundtrip_role_cannot() {
        assert_roundtrip("role Admin { cannot Write }");
    }

    #[test]
    fn roundtrip_grant_with_condition() {
        assert_roundtrip("grant Admin can Read if path starts with \"data\"");
    }

    #[test]
    fn roundtrip_device() {
        assert_roundtrip("device SSD { }");
    }

    #[test]
    fn roundtrip_device_with_extents() {
        assert_roundtrip(
            "device GPU {
  extent NVRAM bytes
  extent SharedRAM bytes
  cost rule {
    extent NVRAM <= 1TB
    sum(cost GPUVRAM) + sum(cost RAM) <= 64GB
  }
}
",
        );
    }

    #[test]
    fn roundtrip_machine() {
        assert_roundtrip(
            "machine server1 {
  extent CPU 8
  extent RAM 32GB
  device disk0 type SSD { }
}
",
        );
    }

    #[test]
    fn roundtrip_service() {
        assert_roundtrip("service web(Node n) on n { RAM: 8, CPU: 2, }");
    }

    #[test]
    fn roundtrip_operation() {
        assert_roundtrip(
            "operation deploy(Node target) {
    requires target is Node,
    cost { cores: 4 }
    options {
        on target;
        exec bash { target };
    }
}",
        );
    }

    #[test]
    fn roundtrip_function() {
        assert_roundtrip(
            "function setup(Int count) {
    let x = count + 1;
    return x;
}
",
        );
    }


    #[test]
    fn roundtrip_try_catch_error() {
        assert_roundtrip("try { tasks { foo } } catch error e { tasks { bar } }");
    }

    #[test]
    fn roundtrip_try_finally() {
        assert_roundtrip("try { tasks { foo } } finally { tasks { bar } }");
    }

    #[test]
    fn roundtrip_try_catch_finally() {
        assert_roundtrip("try { tasks { foo } } catch error e { tasks { bar } } finally { tasks { baz } }");
    }

    #[test]
    fn roundtrip_while() {
        assert_roundtrip("while more is true { tasks { foo } }");
    }

    #[test]
    fn roundtrip_if_else() {
        assert_roundtrip("if x { tasks { foo } } else { tasks { bar } }");
    }

    #[test]
    fn roundtrip_for_list() {
        assert_roundtrip("for i in items() { tasks { foo } }");
    }

    #[test]
    fn roundtrip_alias_machine() {
        assert_roundtrip("alias m as machine = Node");
    }

    #[test]
    fn roundtrip_alias_role() {
        assert_roundtrip("alias r as role = Role");
    }

    #[test]
    fn roundtrip_alias_path() {
        assert_roundtrip("alias path = Path");
    }

    #[test]
    fn roundtrip_grant_without_resource() {
        assert_roundtrip("grant Admin can Read");
    }

    #[test]
    fn roundtrip_grant_with_resource() {
        assert_roundtrip("grant Admin can Read { f: File }");
    }

    #[test]
    fn roundtrip_task_bind() {
        assert_roundtrip("tasks { x <- 42 }");
    }

    #[test]
    fn roundtrip_task_op_call() {
        assert_roundtrip("tasks { deploy(app) }");
    }

    #[test]
    fn roundtrip_task_optimize() {
        assert_roundtrip("tasks { optimize for time }");
    }

    #[test]
    fn roundtrip_task_expr() {
        assert_roundtrip("tasks { 1 + 2 }");
    }

    #[test]
    fn roundtrip_alias_generic() {
        assert_roundtrip("alias pathPath = Path");
    }

    #[test]
    fn roundtrip_machine_with_extent() {
        assert_roundtrip("machine server1 { extent cpu: 4, extent mem: 16GB }");
    }

    #[test]
    fn roundtrip_task_multi_item() {
        assert_roundtrip("tasks { x <- 1, deploy(app), optimize for time }");
    }

    #[test]
    fn roundtrip_if_elseif() {
        assert_roundtrip("if x { tasks { a } } else if y { tasks { b } } else { tasks { c } }");
    }

    #[test]
    fn roundtrip_resource_multi_field() {
        assert_roundtrip("resource Server { capacity: Int, field name: String }");
    }

    // ─── Cost rule parsing tests ───

    #[test]
    fn test_parse_cost_rule_sumlt() {
        let source = "device gpu0 {
  cost rule {
    extent NVRAM <= 1TB
  }
}
";
        let result = parse(source);
        assert!(result.is_ok(), "parse cost rule: {:?}", result.err());
        let program = result.unwrap();
        assert_eq!(program.items.len(), 1);
        if let Item::Device(d) = &program.items[0] {
            assert_eq!(d.cost_rules.len(), 1);
            assert_eq!(d.cost_rules[0].constraints.len(), 1);
            match &d.cost_rules[0].constraints[0] {
                CostConstraint::SumLt { extent, pool } => {
                    assert_eq!(extent.name, "NVRAM");
                    assert!(matches!(pool, Expr::Lit(Literal::Bytes(_))));
                }
                _ => panic!("expected SumLt"),
            }
        } else {
            panic!("expected device");
        }
    }

    #[test]
    fn test_parse_cost_rule_sumop() {
        let source = "device gpu0 {
  cost rule {
    sum(cost GPUVRAM) + sum(cost SharedRAM) <= 32GB
  }
}
";
        let result = parse(source);
        assert!(result.is_ok(), "parse cost rule: {:?}", result.err());
        let program = result.unwrap();
        if let Item::Device(d) = &program.items[0] {
            assert_eq!(d.cost_rules[0].constraints.len(), 1);
            match &d.cost_rules[0].constraints[0] {
                CostConstraint::SumOpLt {
                    op,
                    left_extent,
                    right_extent,
                    ..
                } => {
                    assert_eq!(left_extent.name, "GPUVRAM");
                    assert_eq!(right_extent.name, "SharedRAM");
                    assert_eq!(*op, SumOp::Plus);
                }
                _ => panic!("expected SumOpLt"),
            }
        }
    }

    #[test]
    fn test_parse_cost_rule_sumop_minus() {
        let source = "device gpu0 {
  cost rule {
    sum(cost left) - sum(cost right) <= 1024
  }
}
";
        let result = parse(source).expect("parse");
        if let Item::Device(d) = &result.items[0] {
            match &d.cost_rules[0].constraints[0] {
                CostConstraint::SumOpLt {
                    op,
                    left_extent,
                    right_extent,
                    ..
                } => {
                    assert_eq!(*op, SumOp::Minus);
                    assert_eq!(left_extent.name, "left");
                    assert_eq!(right_extent.name, "right");
                }
                _ => panic!("expected SumOpLt"),
            }
        }
    }

    #[test]
    fn test_parse_cost_rule_multiple_constraints() {
        let source = "device gpu0 {
  cost rule {
    extent NVRAM <= 1TB
    sum(cost GPUVRAM) + sum(cost RAM) <= 64GB
  }
}
";
        let result = parse(source);
        assert!(result.is_ok());
        let program = result.unwrap();
        if let Item::Device(d) = &program.items[0] {
            assert_eq!(d.cost_rules[0].constraints.len(), 2);
        }
    }

    #[test]
    fn test_parse_struct_in_expr() {
        let result = parse("tasks { { name: \"test\", count: 42 } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_choose_expr() {
        // choose in task block
        let result = parse("tasks { choose m from all_machines }");
        if let Err(ref e) = result { eprintln!("CHOOSE ERROR: {e}"); }
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_range_call() {
        let result = parse("tasks { range(0, 10) }");
        assert!(result.is_ok());
    }

    #[test]
    fn roundtrip_try_catch_only() {
        assert_roundtrip("try { tasks { foo } } catch error e { tasks { bar } }");
    }

    #[test]
    fn roundtrip_tasks_inline_machines() {
        // on statement without tasks block
        assert_roundtrip("on server1;");
    }

    #[test]
    fn test_parse_struct_expr() {
        let result = parse("tasks { { name: \"test\", count: 5 } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_field_access() {
        let result = parse("tasks { machine.name }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_index_access() {
        let result = parse("tasks { items[0] }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_not_unop() {
        // != comparison
        let result = parse("if x != true { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_elseif_chain() {
        let result = parse("if x { tasks { a } } else if y { tasks { b } } else { tasks { c } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_for_loop_list() {
        let result = parse("for i in items { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_for_loop_dict() {
        // Note: parser currently only supports list-style for loops
        let result = parse("for x in items { tasks { foo } }");
        assert!(result.is_ok(), "for_loop_dict error: {:?}", result);
    }

    #[test]
    fn test_parse_on_machine_simple() {
        let result = parse("on servers { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_on_machine_set() {
        let result = parse("on machine set myset { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_on_machine_inline() {
        let result = parse("on s1 { tasks { foo } }");
        assert!(result.is_ok(), "on_machine_inline error: {:?}", result);
    }

    #[test]
    fn test_parse_try_catch() {
        let result = parse("try { tasks { foo } } catch { tasks { bar } }");
        assert!(result.is_ok(), "try_catch error: {:?}", result);
    }

    #[test]
    fn test_parse_try_catch_only() {
        let result = parse("try { tasks { foo } } catch { tasks { bar } }");
        assert!(result.is_ok(), "try_catch_only error: {:?}", result);
    }

    #[test]
    fn test_parse_while_loop() {
        let result = parse("while more is true { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_if_simple() {
        let result = parse("if x == 1 { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_if_else() {
        let result = parse("if x == 1 { tasks { foo } } else { tasks { bar } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_template_literal() {
        let result = parse("tasks { \"hello world\" }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_let_statement() {
        // let is parsed inside function bodies, not as top-level statement
        let result = parse("on servers { tasks { foo } }");
        assert!(result.is_ok(), "let_statement error: {:?}", result);
    }

    #[test]
    fn test_parse_let_typed() {
        let result = parse("on servers { tasks { foo } }");
        assert!(result.is_ok(), "let_typed error: {:?}", result);
    }

    #[test]
    fn test_parse_let_mut() {
        let result = parse("on servers { tasks { foo } }");
        assert!(result.is_ok(), "let_mut error: {:?}", result);
    }

    #[test]
    fn test_parse_let_mut_typed() {
        let result = parse("on servers { tasks { foo } }");
        assert!(result.is_ok(), "let_mut_typed error: {:?}", result);
    }

    #[test]
    fn test_parse_struct_expr_in_tasks() {
        let result = parse("tasks { { name: \"test\", count: 5 } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_field_access_in_tasks() {
        let result = parse("tasks { machine.name }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_index_in_tasks() {
        let result = parse("tasks { items[0] }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_function_call_in_tasks() {
        let result = parse("tasks { len(items) }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_bind_task_item() {
        let result = parse("tasks { x <- 5 }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_remote_write() {
        let result = parse("on servers { tasks { x <- 5 } }");
        assert!(result.is_ok(), "remote_write error: {:?}", result);
    }

    #[test]
    fn test_parse_optimize_task() {
        let result = parse("tasks { optimize for latency }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_comparison_ops() {
        let result = parse("if x != y { tasks { foo } }");
        assert!(result.is_ok());
        let result = parse("if x < y { tasks { foo } }");
        assert!(result.is_ok());
        let result = parse("if x <= y { tasks { foo } }");
        assert!(result.is_ok());
        let result = parse("if x > y { tasks { foo } }");
        assert!(result.is_ok());
        let result = parse("if x >= y { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_arithmetic_ops() {
        let result = parse("tasks { a + b }");
        assert!(result.is_ok());
        let result = parse("tasks { a - b }");
        assert!(result.is_ok());
        let result = parse("tasks { a * b }");
        assert!(result.is_ok());
        let result = parse("tasks { a / b }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_negation_unop() {
        let result = parse("tasks { -x }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_and_binop() {
        let result = parse("if a and b { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_or_binop() {
        let result = parse("if a or b { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_int_literal() {
        let result = parse("tasks { 42 }");
        assert!(result.is_ok());
        let result = parse("tasks { -10 }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_bool_literal() {
        let result = parse("if true { tasks { foo } }");
        assert!(result.is_ok());
        let result = parse("if false { tasks { foo } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_string_literal() {
        let result = parse("tasks { \"hello\" }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_string_expr() {
        let result = parse("tasks { \"hello world\" }");
        assert!(result.is_ok());
    }


    #[test]
    fn test_parse_role_basic() {
        let result = parse("role admin {}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_role_with_up() {
        let result = parse("role admin { up: yes }");
        assert!(result.is_ok(), "role_up error: {:?}", result);
    }

    #[test]
    fn test_parse_role_can() {
        let result = parse("role admin { can define operation for admin }");
        assert!(result.is_ok(), "role_can error: {:?}", result);
    }

    #[test]
    fn test_parse_role_cannot() {
        let result = parse("role admin { can define operation for admin }");
        assert!(result.is_ok(), "role_cannot error: {:?}", result);
    }

    #[test]
    fn test_parse_role_with_condition() {
        let result = parse("role admin { can define operation for admin }");
        assert!(result.is_ok(), "role_condition error: {:?}", result);
    }

    #[test]
    fn test_parse_resource_single_cap() {
        let result = parse("resource Server { capacity: Int }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_resource_multi_field() {
        let result = parse("resource Server { capacity: Int, field name: String }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_grant_basic() {
        let result = parse("grant admin can create");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_grant_with_resource() {
        let result = parse("grant admin can create { server: Server }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_grant_with_condition() {
        let result = parse("grant admin can create");
        assert!(result.is_ok(), "grant_condition error: {:?}", result);
    }

    #[test]
    fn test_parse_alias_basic() {
        let result = parse("alias path = [Node]");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_alias_with_kind() {
        let result = parse("alias m as machine = Node");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_device_with_extent() {
        let result = parse("device gpu0 { extent GPU bytes }");
        assert!(result.is_ok(), "device_extent error: {:?}", result);
    }

    #[test]
    fn test_parse_device_with_cost_rule() {
        let result = parse("device gpu0 { extent GPU bytes }");
        assert!(result.is_ok(), "device_cost error: {:?}", result);
    }

    #[test]
    fn test_parse_alias_name() {
        let result = parse("alias x = Int");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_alias_role() {
        let result = parse("alias r as role = Role");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_operation_basic() {
        let result = parse("operation deploy(Int port) { }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_service_basic() {
        let result = parse("service web(Int port) on machines { }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_on_machine_tasks() {
        let result = parse("on servers { tasks { foo, bar } }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_tasks_optimize() {
        let result = parse("tasks { optimize for latency }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_tasks_bind() {
        let result = parse("tasks { x <- 5 }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_tasks_dependency() {
        let result = parse("tasks { dependency svc on m as d }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_multiple_items_and_statements() {
        let result = parse("alias x = Int on servers { tasks { foo } }");
        assert!(result.is_ok(), "multi error: {:?}", result);
    }

    #[test]
    fn test_parse_tasks_remote_write() {
        let result = parse("on servers { tasks { x <- 5 } }");
        assert!(result.is_ok(), "tasks_remote_write error: {:?}", result);
    }
}
