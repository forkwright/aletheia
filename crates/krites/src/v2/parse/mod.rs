//! Datalog parser for krites v2.
//!
//! Hand-written recursive-descent parser producing the AST defined in
//! [`ast`].  Entry point: [`parse`].

pub mod ast;
mod lexer;

use ast::*;
use lexer::Token;

/// Convenience alias for parse operation results.
pub type ParseResult<T> = crate::v2::error::Result<T>;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Parse a Datalog source string into a [`Statement`].
pub fn parse(source: &str) -> ParseResult<Statement> {
    let tokens = lexer::tokenize(source)?;
    let mut parser = Parser::new(tokens);
    let stmt = parser.statement()?;
    parser.expect(Token::Eof)?;
    Ok(stmt)
}

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: Token) -> ParseResult<Token> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(&expected) {
            Ok(tok)
        } else {
            Err(crate::v2::error::ParseSnafu {
                message: format!("expected {expected:?}, got {tok:?}"),
                span: self.span(),
            }
            .build())
        }
    }

    fn expect_ident(&mut self) -> ParseResult<String> {
        match self.advance() {
            Token::Ident(name) => Ok(name),
            other => Err(crate::v2::error::ParseSnafu {
                message: format!("expected identifier, got {other:?}"),
                span: self.span(),
            }
            .build()),
        }
    }

    fn span(&self) -> String {
        format!("token {}", self.pos)
    }

    fn is_next_lbrace(&self) -> bool {
        self.tokens.get(self.pos + 1) == Some(&Token::LBrace)
    }

    // -----------------------------------------------------------------------
    // Statement
    // -----------------------------------------------------------------------

    fn statement(&mut self) -> ParseResult<Statement> {
        match self.peek() {
            Token::Question => self.query_stmt(),
            Token::ColonPut => self.put_stmt(),
            Token::ColonCreate => self.create_stmt(),
            Token::ColonReplace => self.replace_stmt(),
            Token::ColonRemove => self.remove_stmt(),
            Token::DoubleColonFts => self.fts_create_stmt(),
            Token::DoubleColonHnsw => self.hnsw_create_stmt(),
            other => Err(crate::v2::error::ParseSnafu {
                message: format!("unexpected token at start of statement: {other:?}"),
                span: self.span(),
            }
            .build()),
        }
    }

    // -----------------------------------------------------------------------
    // Query
    // -----------------------------------------------------------------------

    fn query_stmt(&mut self) -> ParseResult<Statement> {
        Ok(Statement::Query(self.query()?))
    }

    fn query(&mut self) -> ParseResult<Query> {
        self.expect(Token::Question)?;
        self.expect(Token::LBracket)?;

        let mut outputs = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            outputs.push(self.output_col()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance(); // consume ,
                outputs.push(self.output_col()?);
            }
        }
        self.expect(Token::RBracket)?;
        self.expect(Token::Arrow)?;

        let mut rules = Vec::new();
        rules.push(self.rule()?);
        while matches!(self.peek(), Token::Comma) {
            // Check if the next comma starts a new rule or is inside modifiers.
            // Rules are separated by comma; modifiers (:order, :limit) are not.
            let saved = self.pos;
            self.advance(); // consume ,
            if matches!(
                self.peek(),
                Token::ColonOrder | Token::ColonLimit | Token::Eof
            ) {
                // This comma was actually trailing before a modifier.
                self.pos = saved; // backtrack
                break;
            }
            rules.push(self.rule()?);
        }

        let mut ordering = Vec::new();
        while matches!(self.peek(), Token::ColonOrder) {
            self.advance();
            let descending = matches!(self.peek(), Token::Minus);
            if descending {
                self.advance();
            }
            let col = self.expect_ident()?;
            ordering.push(OrderSpec {
                column: col,
                descending,
            });
        }

        let mut limit = None;
        if matches!(self.peek(), Token::ColonLimit) {
            self.advance();
            limit = Some(self.expr()?);
        }

        Ok(Query {
            outputs,
            rules,
            ordering,
            limit,
        })
    }

    fn output_col(&mut self) -> ParseResult<OutputCol> {
        let name = self.expect_ident()?;
        if matches!(self.peek(), Token::LParen) {
            // Aggregation: count(y), sum(y), etc.
            self.advance(); // (
            let inner = self.expect_ident()?;
            self.expect(Token::RParen)?;
            let agg = match name.as_str() {
                "count" => Aggregation::Count,
                "sum" => Aggregation::Sum,
                "max" => Aggregation::Max,
                "min" => Aggregation::Min,
                "mean" => Aggregation::Mean,
                _ => {
                    return Err(crate::v2::error::ParseSnafu {
                        message: format!("unknown aggregation: {name}"),
                        span: self.span(),
                    }
                    .build());
                }
            };
            Ok(OutputCol {
                name: inner,
                aggregation: Some(agg),
            })
        } else {
            Ok(OutputCol {
                name,
                aggregation: None,
            })
        }
    }

    // -----------------------------------------------------------------------
    // Rule
    // -----------------------------------------------------------------------

    fn rule(&mut self) -> ParseResult<Rule> {
        let mut atoms = Vec::new();
        let mut filters = Vec::new();

        // First item is always an atom (a rule must have at least one atom).
        atoms.push(self.atom()?);

        while matches!(self.peek(), Token::Comma) {
            self.advance(); // consume ,

            // Peek ahead to decide if next item is an atom or a filter.
            if self.looks_like_atom() {
                atoms.push(self.atom()?);
            } else {
                filters.push(Filter {
                    expr: self.expr()?,
                });
            }
        }

        Ok(Rule { atoms, filters })
    }

    fn looks_like_atom(&self) -> bool {
        match self.peek() {
            Token::Star | Token::Tilde | Token::LtTilde => true,
            Token::Ident(_) => self.is_next_lbrace(),
            _ => false,
        }
    }

    fn atom(&mut self) -> ParseResult<Atom> {
        match self.peek() {
            Token::Star => {
                self.advance();
                let relation = self.expect_ident()?;
                self.expect(Token::LBrace)?;
                let bindings = self.binding_list()?;
                self.expect(Token::RBrace)?;
                Ok(Atom::Stored {
                    relation,
                    bindings,
                })
            }
            Token::Tilde => {
                self.advance();
                let relation = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let index = self.expect_ident()?;
                self.expect(Token::LBrace)?;
                let bindings = self.binding_list()?;
                let params = if matches!(self.peek(), Token::Pipe) {
                    self.advance();
                    self.param_list()?
                } else {
                    Vec::new()
                };
                self.expect(Token::RBrace)?;
                Ok(Atom::Index {
                    relation,
                    index,
                    bindings,
                    params,
                })
            }
            Token::LtTilde => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::LBrace)?;
                let inputs = if self.looks_like_input_relation() {
                    self.input_relation_list()?
                } else {
                    Vec::new()
                };
                let options = if matches!(self.peek(), Token::Pipe) {
                    self.advance();
                    self.param_list()?
                } else {
                    Vec::new()
                };
                self.expect(Token::RBrace)?;
                Ok(Atom::FixedRule {
                    name,
                    inputs,
                    options,
                })
            }
            Token::Ident(_) => {
                let name = self.expect_ident()?;
                self.expect(Token::LBrace)?;
                let bindings = self.binding_list()?;
                self.expect(Token::RBrace)?;
                Ok(Atom::Temp {
                    name,
                    bindings,
                })
            }
            other => Err(crate::v2::error::ParseSnafu {
                message: format!("expected atom, got {other:?}"),
                span: self.span(),
            }
            .build()),
        }
    }

    fn looks_like_input_relation(&self) -> bool {
        matches!(self.peek(), Token::Ident(_)) && self.is_next_lbrace()
    }

    fn binding_list(&mut self) -> ParseResult<Vec<Binding>> {
        let mut bindings = Vec::new();
        if matches!(self.peek(), Token::RBrace | Token::Pipe) {
            return Ok(bindings);
        }
        bindings.push(self.binding()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            if matches!(self.peek(), Token::RBrace | Token::Pipe) {
                break;
            }
            bindings.push(self.binding()?);
        }
        Ok(bindings)
    }

    fn binding(&mut self) -> ParseResult<Binding> {
        let first = self.expect_ident()?;
        if matches!(self.peek(), Token::Colon) {
            self.advance(); // :
            let variable = self.expect_ident()?;
            Ok(Binding {
                column: Some(first),
                variable,
            })
        } else {
            Ok(Binding {
                column: None,
                variable: first,
            })
        }
    }

    fn param_list(&mut self) -> ParseResult<Vec<(String, Expr)>> {
        let mut params = Vec::new();
        if matches!(self.peek(), Token::RBrace) {
            return Ok(params);
        }
        params.push(self.param()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            if matches!(self.peek(), Token::RBrace) {
                break;
            }
            params.push(self.param()?);
        }
        Ok(params)
    }

    fn param(&mut self) -> ParseResult<(String, Expr)> {
        let name = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let expr = self.expr()?;
        Ok((name, expr))
    }

    fn input_relation_list(&mut self) -> ParseResult<Vec<InputRelation>> {
        let mut inputs = Vec::new();
        inputs.push(self.input_relation()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            if matches!(self.peek(), Token::Pipe | Token::RBrace) {
                break;
            }
            inputs.push(self.input_relation()?);
        }
        Ok(inputs)
    }

    fn input_relation(&mut self) -> ParseResult<InputRelation> {
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let bindings = self.binding_list()?;
        self.expect(Token::RBrace)?;
        Ok(InputRelation { name, bindings })
    }

    // -----------------------------------------------------------------------
    // Expressions (recursive descent with precedence)
    // -----------------------------------------------------------------------

    fn expr(&mut self) -> ParseResult<Expr> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> ParseResult<Expr> {
        let mut left = self.and_expr()?;
        // WHY: Datalog filters do not use `||` in the exercised syntax, but
        // the AST supports it for completeness.
        while self.peek() == &Token::Ident("||".to_owned()) {
            self.advance();
            let right = self.and_expr()?;
            left = Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> ParseResult<Expr> {
        let mut left = self.equality_expr()?;
        while self.peek() == &Token::Ident("&&".to_owned()) {
            self.advance();
            let right = self.equality_expr()?;
            left = Expr::BinOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn equality_expr(&mut self) -> ParseResult<Expr> {
        let mut left = self.relational_expr()?;
        loop {
            match self.peek() {
                Token::Eq => {
                    self.advance();
                    let right = self.relational_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Eq,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Neq => {
                    self.advance();
                    let right = self.relational_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Neq,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn relational_expr(&mut self) -> ParseResult<Expr> {
        let mut left = self.additive_expr()?;
        loop {
            match self.peek() {
                Token::Lt => {
                    self.advance();
                    let right = self.additive_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Lt,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Gt => {
                    self.advance();
                    let right = self.additive_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Gt,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Lte => {
                    self.advance();
                    let right = self.additive_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Lte,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Gte => {
                    self.advance();
                    let right = self.additive_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Gte,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn additive_expr(&mut self) -> ParseResult<Expr> {
        let mut left = self.multiplicative_expr()?;
        loop {
            match self.peek() {
                Token::Plus => {
                    self.advance();
                    let right = self.multiplicative_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Add,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Minus => {
                    self.advance();
                    let right = self.multiplicative_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Sub,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn multiplicative_expr(&mut self) -> ParseResult<Expr> {
        let mut left = self.unary_expr()?;
        loop {
            match self.peek() {
                Token::Star => {
                    self.advance();
                    let right = self.unary_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Mul,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Token::Slash => {
                    self.advance();
                    let right = self.unary_expr()?;
                    left = Expr::BinOp {
                        op: BinOp::Div,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn unary_expr(&mut self) -> ParseResult<Expr> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let operand = self.unary_expr()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                })
            }
            Token::Ident(name) if name == "!" => {
                // WHY: `!` is not a standalone token; it appears as part of
                // `!=`.  We handle logical NOT by checking for a `!` ident
                // which can only happen if the lexer produced it... which it
                // doesn't.  This branch is unreachable but kept for symmetry.
                self.advance();
                let operand = self.unary_expr()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                })
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> ParseResult<Expr> {
        match self.advance() {
            Token::Ident(name) => {
                // Check for boolean literals and the unary `!` operator.
                match name.as_str() {
                    "true" => Ok(Expr::Literal(crate::v2::value::Value::Bool(true))),
                    "false" => Ok(Expr::Literal(crate::v2::value::Value::Bool(false))),
                    _ => {
                        // Function call?
                        if matches!(self.peek(), Token::LParen) {
                            self.advance(); // (
                            let args = if matches!(self.peek(), Token::RParen) {
                                Vec::new()
                            } else {
                                self.arg_list()?
                            };
                            self.expect(Token::RParen)?;
                            Ok(Expr::FnCall { name, args })
                        } else {
                            Ok(Expr::Var(name))
                        }
                    }
                }
            }
            Token::Dollar => {
                let name = self.expect_ident()?;
                Ok(Expr::Param(name))
            }
            Token::Int(n) => Ok(Expr::Literal(crate::v2::value::Value::Int(n))),
            Token::Float(f) => Ok(Expr::Literal(crate::v2::value::Value::Float(f))),
            Token::String(s) => Ok(Expr::Literal(crate::v2::value::Value::from(s))),
            Token::LParen => {
                let expr = self.expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            other => Err(crate::v2::error::ParseSnafu {
                message: format!("unexpected token in expression: {other:?}"),
                span: self.span(),
            }
            .build()),
        }
    }

    fn arg_list(&mut self) -> ParseResult<Vec<Expr>> {
        let mut args = Vec::new();
        args.push(self.expr()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            args.push(self.expr()?);
        }
        Ok(args)
    }

    // -----------------------------------------------------------------------
    // Put statement
    // -----------------------------------------------------------------------

    fn put_stmt(&mut self) -> ParseResult<Statement> {
        self.expect(Token::ColonPut)?;
        let relation = self.expect_ident()?;
        let mut rows = Vec::new();
        rows.push(self.row()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            rows.push(self.row()?);
        }
        Ok(Statement::Put { relation, rows })
    }

    fn row(&mut self) -> ParseResult<Vec<(String, Expr)>> {
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            fields.push(self.field()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                fields.push(self.field()?);
            }
        }
        self.expect(Token::RBrace)?;
        Ok(fields)
    }

    fn field(&mut self) -> ParseResult<(String, Expr)> {
        let name = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let expr = self.expr()?;
        Ok((name, expr))
    }

    // -----------------------------------------------------------------------
    // Create / Replace statements
    // -----------------------------------------------------------------------

    fn create_stmt(&mut self) -> ParseResult<Statement> {
        self.expect(Token::ColonCreate)?;
        let relation = self.expect_ident()?;
        let schema = self.schema_spec()?;
        Ok(Statement::Create { relation, schema })
    }

    fn replace_stmt(&mut self) -> ParseResult<Statement> {
        self.expect(Token::ColonReplace)?;
        let relation = self.expect_ident()?;
        let schema = self.schema_spec()?;
        Ok(Statement::Replace { relation, schema })
    }

    fn schema_spec(&mut self) -> ParseResult<SchemaSpec> {
        self.expect(Token::LBrace)?;
        let mut key_columns = Vec::new();
        key_columns.push(self.expect_ident()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            if matches!(self.peek(), Token::FatArrow) {
                break;
            }
            key_columns.push(self.expect_ident()?);
        }
        self.expect(Token::FatArrow)?;
        let mut value_columns = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            value_columns.push(self.value_col_spec()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                value_columns.push(self.value_col_spec()?);
            }
        }
        self.expect(Token::RBrace)?;
        Ok(SchemaSpec {
            key_columns,
            value_columns,
        })
    }

    fn value_col_spec(&mut self) -> ParseResult<ValueColumnSpec> {
        let name = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let column_type = self.column_type()?;
        let default = if matches!(self.peek(), Token::Ident(d) if d == "default") {
            self.advance();
            Some(self.expr()?)
        } else {
            None
        };
        Ok(ValueColumnSpec {
            name,
            column_type,
            default,
        })
    }

    fn column_type(&mut self) -> ParseResult<crate::v2::schema::ColumnType> {
        use crate::v2::schema::{ColumnType, VectorDtype};
        let name = self.expect_ident()?;
        match name.as_str() {
            "String" => Ok(ColumnType::String),
            "Int" => Ok(ColumnType::Int),
            "Float" => Ok(ColumnType::Float),
            "Bool" => Ok(ColumnType::Bool),
            "Bytes" => Ok(ColumnType::Bytes),
            "Any" => Ok(ColumnType::Any),
            "Timestamp" => Ok(ColumnType::Timestamp),
            "Vector" => {
                // Parse Vector<F32, 384> or Vector<F64, 384>
                self.expect(Token::Lt)?;
                let dtype_name = self.expect_ident()?;
                let dtype = match dtype_name.as_str() {
                    "F32" => VectorDtype::F32,
                    "F64" => VectorDtype::F64,
                    _ => {
                        return Err(crate::v2::error::ParseSnafu {
                            message: format!(
                                "expected F32 or F64 in Vector dtype, got {dtype_name}"
                            ),
                            span: self.span(),
                        }
                        .build());
                    }
                };
                self.expect(Token::Comma)?;
                let dim = match self.advance() {
                    Token::Int(n) if n >= 0 => n as u32,
                    Token::Float(f) if f >= 0.0 && f.fract() == 0.0 => f as u32,
                    other => {
                        return Err(crate::v2::error::ParseSnafu {
                            message: format!(
                                "expected positive integer for Vector dimension, got {other:?}"
                            ),
                            span: self.span(),
                        }
                        .build());
                    }
                };
                self.expect(Token::Gt)?;
                Ok(ColumnType::Vector { dtype, dim })
            }
            _ => Err(crate::v2::error::ParseSnafu {
                message: format!("unknown column type: {name}"),
                span: self.span(),
            }
            .build()),
        }
    }

    // -----------------------------------------------------------------------
    // Remove statement
    // -----------------------------------------------------------------------

    fn remove_stmt(&mut self) -> ParseResult<Statement> {
        self.expect(Token::ColonRemove)?;
        let relation = self.expect_ident()?;
        Ok(Statement::Remove { relation })
    }

    // -----------------------------------------------------------------------
    // FTS / HNSW create statements
    // -----------------------------------------------------------------------

    fn fts_create_stmt(&mut self) -> ParseResult<Statement> {
        self.expect(Token::DoubleColonFts)?;
        let relation = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut columns = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            columns.push(self.expect_ident()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                columns.push(self.expect_ident()?);
            }
        }
        self.expect(Token::RBrace)?;
        let options = if matches!(self.peek(), Token::LBrace) {
            self.advance();
            let opts = self.param_list()?;
            self.expect(Token::RBrace)?;
            opts
        } else {
            Vec::new()
        };
        Ok(Statement::FtsCreate {
            relation,
            config: FtsConfig { columns, options },
        })
    }

    fn hnsw_create_stmt(&mut self) -> ParseResult<Statement> {
        self.expect(Token::DoubleColonHnsw)?;
        let relation = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let column = self.expect_ident()?;
        self.expect(Token::RBrace)?;
        let options = if matches!(self.peek(), Token::LBrace) {
            self.advance();
            let opts = self.param_list()?;
            self.expect(Token::RBrace)?;
            opts
        } else {
            Vec::new()
        };
        Ok(Statement::HnswCreate {
            relation,
            config: HnswConfig { column, options },
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::v2::schema::ColumnType;
    use crate::v2::value::Value;

    #[test]
    fn parse_simple_query() {
        let stmt = parse("?[x] := *facts{id: x}").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.outputs.len(), 1);
                assert_eq!(q.outputs[0].name, "x");
                assert!(q.outputs[0].aggregation.is_none());
                assert_eq!(q.rules.len(), 1);
                assert_eq!(q.rules[0].atoms.len(), 1);
                assert!(q.rules[0].filters.is_empty());
                match &q.rules[0].atoms[0] {
                    Atom::Stored { relation, bindings } => {
                        assert_eq!(relation, "facts");
                        assert_eq!(bindings.len(), 1);
                        assert_eq!(bindings[0].column, Some("id".to_owned()));
                        assert_eq!(bindings[0].variable, "x");
                    }
                    _ => panic!("expected Stored atom"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_query_with_filter() {
        let stmt = parse("?[x, y] := *facts{id: x, content: y}, x = $param").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.outputs.len(), 2);
                assert_eq!(q.rules[0].atoms.len(), 1);
                assert_eq!(q.rules[0].filters.len(), 1);
                match &q.rules[0].filters[0].expr {
                    Expr::BinOp { op, left, right } => {
                        assert_eq!(*op, BinOp::Eq);
                        assert!(matches!(left.as_ref(), Expr::Var(v) if v == "x"));
                        assert!(matches!(right.as_ref(), Expr::Param(p) if p == "param"));
                    }
                    _ => panic!("expected BinOp filter"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_query_with_aggregation() {
        let stmt = parse("?[x, count(y)] := *rel{x, y}").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.outputs.len(), 2);
                assert_eq!(q.outputs[1].name, "y");
                assert_eq!(q.outputs[1].aggregation, Some(Aggregation::Count));
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_query_with_order_and_limit() {
        let stmt = parse("?[x] := *rel{x} :order -x :limit 10").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.ordering.len(), 1);
                assert_eq!(q.ordering[0].column, "x");
                assert!(q.ordering[0].descending);
                assert!(q.limit.is_some());
                match q.limit.unwrap() {
                    Expr::Literal(Value::Int(10)) => {}
                    other => panic!("expected limit 10, got {other:?}"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_put_statement() {
        let stmt = parse(":put facts {id: $id, content: $content}").unwrap();
        match stmt {
            Statement::Put { relation, rows } => {
                assert_eq!(relation, "facts");
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[0][0].0, "id");
                assert!(matches!(rows[0][0].1, Expr::Param(ref p) if p == "id"));
                assert_eq!(rows[0][1].0, "content");
                assert!(matches!(rows[0][1].1, Expr::Param(ref p) if p == "content"));
            }
            _ => panic!("expected Put"),
        }
    }

    #[test]
    fn parse_create_statement() {
        let stmt = parse(":create rel {key => val: String}").unwrap();
        match stmt {
            Statement::Create { relation, schema } => {
                assert_eq!(relation, "rel");
                assert_eq!(schema.key_columns, vec!["key"]);
                assert_eq!(schema.value_columns.len(), 1);
                assert_eq!(schema.value_columns[0].name, "val");
                assert_eq!(schema.value_columns[0].column_type, ColumnType::String);
                assert!(schema.value_columns[0].default.is_none());
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn parse_create_with_default() {
        let stmt = parse(":create rel {key => val: Float default 0.0}").unwrap();
        match stmt {
            Statement::Create { relation: _, schema } => {
                assert_eq!(schema.value_columns[0].column_type, ColumnType::Float);
                assert!(schema.value_columns[0].default.is_some());
                match schema.value_columns[0].default.as_ref().unwrap() {
                    Expr::Literal(Value::Float(f)) => assert!((f - 0.0).abs() < f64::EPSILON),
                    other => panic!("expected Float(0.0), got {other:?}"),
                }
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn parse_index_lookup() {
        let stmt = parse("?[id] := ~facts:content_fts{id | query: $q, k: 5}").unwrap();
        match stmt {
            Statement::Query(q) => {
                match &q.rules[0].atoms[0] {
                    Atom::Index {
                        relation,
                        index,
                        bindings,
                        params,
                    } => {
                        assert_eq!(relation, "facts");
                        assert_eq!(index, "content_fts");
                        assert_eq!(bindings.len(), 1);
                        assert_eq!(bindings[0].variable, "id");
                        assert_eq!(params.len(), 2);
                        assert_eq!(params[0].0, "query");
                        assert!(matches!(params[0].1, Expr::Param(ref p) if p == "q"));
                        assert_eq!(params[1].0, "k");
                        assert!(matches!(params[1].1, Expr::Literal(Value::Int(5))));
                    }
                    _ => panic!("expected Index atom"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_fixed_rule() {
        let stmt = parse("?[node, rank] := <~PageRank{g{a, b} | max_iter: 100}").unwrap();
        match stmt {
            Statement::Query(q) => {
                match &q.rules[0].atoms[0] {
                    Atom::FixedRule { name, inputs, options } => {
                        assert_eq!(name, "PageRank");
                        assert_eq!(inputs.len(), 1);
                        assert_eq!(inputs[0].name, "g");
                        assert_eq!(inputs[0].bindings.len(), 2);
                        assert_eq!(options.len(), 1);
                        assert_eq!(options[0].0, "max_iter");
                        assert!(matches!(options[0].1, Expr::Literal(Value::Int(100))));
                    }
                    _ => panic!("expected FixedRule atom"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_temp_relation() {
        let stmt = parse("?[x] := temp{x, y}, *facts{id: x}").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.rules[0].atoms.len(), 2);
                match &q.rules[0].atoms[0] {
                    Atom::Temp { name, bindings } => {
                        assert_eq!(name, "temp");
                        assert_eq!(bindings.len(), 2);
                    }
                    _ => panic!("expected Temp atom"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_multiple_filters() {
        let stmt = parse("?[x] := *rel{x}, x > 0, x < 100").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.rules[0].filters.len(), 2);
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_remove_statement() {
        let stmt = parse(":remove old_rel").unwrap();
        match stmt {
            Statement::Remove { relation } => assert_eq!(relation, "old_rel"),
            _ => panic!("expected Remove"),
        }
    }

    #[test]
    fn parse_fts_create() {
        let stmt = parse("::fts docs {title, body}").unwrap();
        match stmt {
            Statement::FtsCreate { relation, config } => {
                assert_eq!(relation, "docs");
                assert_eq!(config.columns, vec!["title", "body"]);
            }
            _ => panic!("expected FtsCreate"),
        }
    }

    #[test]
    fn parse_hnsw_create() {
        let stmt = parse("::hnsw embeddings {vec}").unwrap();
        match stmt {
            Statement::HnswCreate { relation, config } => {
                assert_eq!(relation, "embeddings");
                assert_eq!(config.column, "vec");
            }
            _ => panic!("expected HnswCreate"),
        }
    }

    #[test]
    fn parse_error_missing_arrow() {
        let err = parse("?[x] *facts{x}").unwrap_err();
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn parse_error_unmatched_brace() {
        let err = parse("?[x] := *facts{x").unwrap_err();
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn parse_function_call_in_filter() {
        let stmt = parse("?[x] := *rel{x}, contains(x, 'foo')").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.rules[0].filters.len(), 1);
                match &q.rules[0].filters[0].expr {
                    Expr::FnCall { name, args } => {
                        assert_eq!(name, "contains");
                        assert_eq!(args.len(), 2);
                    }
                    _ => panic!("expected FnCall"),
                }
            }
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn parse_vector_type() {
        let stmt = parse(":create vec_rel {id => embedding: Vector<F32, 384>}").unwrap();
        match stmt {
            Statement::Create { schema, .. } => {
                assert_eq!(schema.value_columns[0].column_type, ColumnType::Vector {
                    dtype: crate::v2::schema::VectorDtype::F32,
                    dim: 384,
                });
            }
            _ => panic!("expected Create"),
        }
    }
}
