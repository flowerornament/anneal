use crate::ast::{
    Aggregate, AggregateFunction, ArithmeticOp, Atom, Body, CallArg, CallStyle, Comparison,
    ComparisonOp, ConfigBlock, Declaration, DerivedAtom, DocDecl, Expr, FieldPattern, Head, Ident,
    ImportDirective, IncludeDirective, Literal, NamedArg, NegatedAtom, Negation, NumberLiteral,
    PredicateDecl, PredicateRef, Program, Query, Rule, RuleLayer, RuleOrigin, SourceBlock,
    SourceLocation, Statement, StoredAtom, Term, TimeBlock, VerbDecl, named_string_arg,
};

pub fn parse_program(source: &str, input: &str) -> Result<Program, ParseError> {
    Parser::new(source, input)?.parse_program()
}

pub fn parse_prelude_program(source: &str, input: &str) -> Result<Program, ParseError> {
    let mut program = parse_program(source, input)?;
    program.assign_rule_layer(RuleLayer::Prelude);
    Ok(program)
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{location}: {message}")]
pub struct ParseError {
    pub location: SourceLocation,
    pub message: String,
}

impl ParseError {
    fn new(source: &str, token: &Token, message: impl Into<String>) -> Self {
        Self {
            location: token.location(source),
            message: message.into(),
        }
    }
}

struct Parser {
    source: String,
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn new(source: &str, input: &str) -> Result<Self, ParseError> {
        Ok(Self {
            source: source.to_string(),
            tokens: Lexer::new(source, input).lex()?,
            cursor: 0,
        })
    }

    fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut statements = Vec::new();
        while !self.at(&TokenKind::Eof) {
            statements.push(self.parse_statement()?);
        }
        Ok(Program::new(statements))
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let statement_start = self.peek().clone();
        if self.eat(&TokenKind::Question) {
            return self
                .parse_query(statement_start.location(&self.source))
                .map(Statement::Query);
        }
        if self.eat_keyword("include") {
            let location = statement_start.location(&self.source);
            let path = self.expect_string()?;
            self.expect(&TokenKind::Dot)?;
            return Ok(Statement::Include(IncludeDirective { path, location }));
        }
        if self.eat_keyword("import") {
            let location = statement_start.location(&self.source);
            let module = self.expect_ident()?;
            self.expect_keyword("from")?;
            let path = self.expect_string()?;
            self.expect(&TokenKind::Dot)?;
            return Ok(Statement::Import(ImportDirective {
                module,
                path,
                location,
            }));
        }
        if self.eat_keyword("optional") {
            let head = self.parse_head()?;
            self.expect(&TokenKind::Dot)?;
            return Ok(Statement::OptionalFact(head));
        }
        if self.peek_keyword("config")
            && matches!(self.peek_n(1).kind, TokenKind::Ident(_))
            && self.peek_n(2).kind == TokenKind::LBrace
        {
            return self.parse_config_block(statement_start.location(&self.source));
        }
        if self.peek_keyword("source")
            && matches!(self.peek_n(1).kind, TokenKind::Ident(_))
            && self.peek_n(2).kind == TokenKind::LBrace
        {
            return self.parse_source_block(statement_start.location(&self.source));
        }
        if self.eat(&TokenKind::AtSign) {
            let location = statement_start.location(&self.source);
            let annotation = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            let args = self.parse_named_args(&TokenKind::RParen)?;
            self.expect(&TokenKind::RParen)?;
            self.eat(&TokenKind::Dot);
            return match annotation.as_str() {
                "verb" => Ok(Statement::Verb(VerbDecl::new(args, location))),
                "doc" => self
                    .parse_doc_annotation(&args, location, &statement_start)
                    .map(Statement::Doc),
                "predicate" => Ok(Statement::Predicate(PredicateDecl::new(args, location))),
                other => Err(ParseError::new(
                    &self.source,
                    &statement_start,
                    format!("unknown annotation @{other}"),
                )),
            };
        }
        if self.peek_keyword("at") && self.peek_n(1).kind == TokenKind::LParen {
            self.bump();
            let reference = self.parse_reference_arg()?;
            self.expect(&TokenKind::LBrace)?;
            let mut statements = Vec::new();
            while !self.eat(&TokenKind::RBrace) {
                statements.push(self.parse_statement()?);
            }
            return Ok(Statement::AtBlock {
                reference,
                statements,
            });
        }

        let head = self.parse_head()?;
        if self.eat(&TokenKind::Dot) {
            return Ok(Statement::Fact(head));
        }
        self.expect(&TokenKind::ColonEq)?;
        let body = self.parse_body_until(&TokenKind::Dot)?;
        self.expect(&TokenKind::Dot)?;
        Ok(Statement::Rule(Rule::with_origin(
            head,
            body,
            rule_origin(RuleLayer::Unknown, &self.source, &statement_start),
        )))
    }

    fn parse_config_block(&mut self, location: SourceLocation) -> Result<Statement, ParseError> {
        self.expect_keyword("config")?;
        let section = self.expect_ident()?;
        let declarations = self.parse_declaration_block()?;
        Ok(Statement::ConfigBlock(ConfigBlock::new(
            section,
            declarations,
            location,
        )))
    }

    fn parse_source_block(&mut self, location: SourceLocation) -> Result<Statement, ParseError> {
        self.expect_keyword("source")?;
        let source = self.expect_ident()?;
        let declarations = self.parse_declaration_block()?;
        Ok(Statement::SourceBlock(SourceBlock::new(
            source,
            declarations,
            location,
        )))
    }

    fn parse_declaration_block(&mut self) -> Result<Vec<Declaration>, ParseError> {
        self.expect(&TokenKind::LBrace)?;
        let mut declarations = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            declarations.push(self.parse_declaration()?);
        }
        Ok(declarations)
    }

    fn parse_declaration(&mut self) -> Result<Declaration, ParseError> {
        let location = self.peek().location(&self.source);
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let args = if self.at(&TokenKind::RParen) {
            Vec::new()
        } else {
            self.parse_comma_list(&TokenKind::RParen, Parser::parse_call_arg)?
        };
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Dot)?;
        Ok(Declaration::new(name, args, location))
    }

    fn parse_doc_annotation(
        &self,
        args: &[NamedArg],
        location: SourceLocation,
        annotation_start: &Token,
    ) -> Result<DocDecl, ParseError> {
        let name = required_annotation_string(args, "name").ok_or_else(|| {
            ParseError::new(
                &self.source,
                annotation_start,
                "@doc requires string argument name",
            )
        })?;
        let doc = required_annotation_string(args, "doc").ok_or_else(|| {
            ParseError::new(
                &self.source,
                annotation_start,
                "@doc requires string argument doc",
            )
        })?;
        Ok(DocDecl::new(name, doc, location))
    }

    fn parse_query(&mut self, location: SourceLocation) -> Result<Query, ParseError> {
        let mut local_rules = Vec::new();
        while self.eat_keyword("where") {
            let rule_start = self.peek().clone();
            let head = self.parse_head()?;
            self.expect(&TokenKind::ColonEq)?;
            let body = self.parse_body_until(&TokenKind::Dot)?;
            self.expect(&TokenKind::Dot)?;
            local_rules.push(Rule::with_origin(
                head,
                body,
                rule_origin(RuleLayer::Inline, &self.source, &rule_start),
            ));
        }
        let body = self.parse_body_until(&TokenKind::Dot)?;
        self.expect(&TokenKind::Dot)?;
        Ok(Query {
            local_rules,
            body,
            location,
        })
    }

    fn parse_head(&mut self) -> Result<Head, ParseError> {
        let location = self.peek().location(&self.source);
        let predicate = self.parse_predicate_ref()?;
        self.expect(&TokenKind::LParen)?;
        let terms = if self.at(&TokenKind::RParen) {
            Vec::new()
        } else {
            self.parse_comma_list(&TokenKind::RParen, Parser::parse_term)?
        };
        self.expect(&TokenKind::RParen)?;
        Ok(Head {
            predicate,
            terms,
            location,
        })
    }

    fn parse_body_until(&mut self, end: &TokenKind) -> Result<Body, ParseError> {
        let mut atoms = Vec::new();
        while !self.at(end) {
            atoms.push(self.parse_atom()?);
            if !self.at(end) {
                self.expect(&TokenKind::Comma)?;
            }
        }
        Ok(Body { atoms })
    }

    fn parse_atom(&mut self) -> Result<Atom, ParseError> {
        let location = self.peek().location(&self.source);
        if self.eat_keyword("not") {
            let atom = if self.at(&TokenKind::Star) {
                self.parse_stored_atom().map(NegatedAtom::Stored)
            } else {
                self.parse_derived_atom().map(NegatedAtom::Derived)
            }?;
            return Ok(Atom::Negation(Negation { atom, location }));
        }
        if self.at(&TokenKind::Star) {
            return self.parse_stored_atom().map(Atom::Stored);
        }
        if self.peek_keyword("at") && self.peek_n(1).kind == TokenKind::LParen {
            self.bump();
            let reference = self.parse_reference_arg()?;
            self.expect(&TokenKind::LBrace)?;
            let body = self.parse_body_until(&TokenKind::RBrace)?;
            self.expect(&TokenKind::RBrace)?;
            return Ok(Atom::TimeBlock(TimeBlock {
                reference,
                body,
                location,
            }));
        }
        if self.starts_derived_atom() {
            let checkpoint = self.cursor;
            let derived = self.parse_derived_atom()?;
            if !self.starts_comparison_or_aggregation() {
                return Ok(Atom::Derived(derived));
            }
            self.cursor = checkpoint;
        }

        let left = self.parse_expr()?;
        if self.peek().kind == TokenKind::Eq && self.peek_aggregate_function_at(1).is_some() {
            self.expect(&TokenKind::Eq)?;
            let function = self.parse_aggregate_function()?;
            self.expect(&TokenKind::LBrace)?;
            let args = if aggregate_accepts_args(function) && self.next_named_arg_before_colon() {
                let args = self.parse_named_args(&TokenKind::Colon)?;
                self.expect(&TokenKind::Colon)?;
                args
            } else {
                Vec::new()
            };
            let value = self.parse_expr()?;
            self.expect(&TokenKind::Colon)?;
            let body = self.parse_body_until(&TokenKind::RBrace)?;
            self.expect(&TokenKind::RBrace)?;
            return Ok(Atom::Aggregation(Aggregate {
                result: left,
                function,
                args,
                value,
                body,
                location,
            }));
        }
        let op = self.parse_comparison_op()?;
        let right = self.parse_expr()?;
        Ok(Atom::Comparison(Comparison {
            left,
            op,
            right,
            location,
        }))
    }

    fn parse_stored_atom(&mut self) -> Result<StoredAtom, ParseError> {
        let location = self.peek().location(&self.source);
        self.expect(&TokenKind::Star)?;
        let relation = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;
        let fields = if self.at(&TokenKind::RBrace) {
            Vec::new()
        } else {
            self.parse_comma_list(&TokenKind::RBrace, Parser::parse_field_pattern)?
        };
        self.expect(&TokenKind::RBrace)?;
        Ok(StoredAtom {
            relation,
            fields,
            location,
        })
    }

    fn parse_field_pattern(&mut self) -> Result<FieldPattern, ParseError> {
        let location = self.peek().location(&self.source);
        let field = self.expect_ident()?;
        let term = if self.eat(&TokenKind::Colon) {
            self.parse_term()?
        } else {
            Term::Expr(Expr::Var(field.clone()))
        };
        Ok(FieldPattern {
            field,
            term,
            location,
        })
    }

    fn parse_derived_atom(&mut self) -> Result<DerivedAtom, ParseError> {
        let location = self.peek().location(&self.source);
        let predicate = self.parse_predicate_ref()?;
        let (args, style) = if self.eat(&TokenKind::LParen) {
            let args = if self.at(&TokenKind::RParen) {
                Vec::new()
            } else {
                self.parse_comma_list(&TokenKind::RParen, Parser::parse_call_arg)?
            };
            self.expect(&TokenKind::RParen)?;
            (args, CallStyle::Complete)
        } else {
            self.expect(&TokenKind::LBrace)?;
            let args = if self.at(&TokenKind::RBrace) {
                Vec::new()
            } else {
                self.parse_comma_list(&TokenKind::RBrace, Parser::parse_pattern_call_arg)?
            };
            self.expect(&TokenKind::RBrace)?;
            (args, CallStyle::Pattern)
        };
        Ok(DerivedAtom {
            predicate,
            args,
            style,
            location,
        })
    }

    fn parse_predicate_ref(&mut self) -> Result<PredicateRef, ParseError> {
        let first = self.expect_ident()?;
        if self.eat(&TokenKind::Dot) {
            let second = self.expect_ident()?;
            Ok(PredicateRef::qualified(first, second))
        } else {
            Ok(PredicateRef::new(first))
        }
    }

    fn parse_call_arg(&mut self) -> Result<CallArg, ParseError> {
        let location = self.peek().location(&self.source);
        if self.eat(&TokenKind::Underscore) {
            return Ok(CallArg::Wildcard { location });
        }
        if matches!(self.peek().kind, TokenKind::Ident(_))
            && self.peek_n(1).kind == TokenKind::Colon
        {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let expr = self.parse_expr()?;
            return Ok(CallArg::Named {
                name,
                expr,
                location,
            });
        }
        let expr = self.parse_expr()?;
        Ok(CallArg::Positional { expr, location })
    }

    fn parse_function_call_arg(&mut self) -> Result<CallArg, ParseError> {
        let location = self.peek().location(&self.source);
        if matches!(self.peek().kind, TokenKind::Ident(_))
            && self.peek_n(1).kind == TokenKind::Colon
        {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let expr = self.parse_expr()?;
            return Ok(CallArg::Named {
                name,
                expr,
                location,
            });
        }
        let expr = self.parse_expr()?;
        Ok(CallArg::Positional { expr, location })
    }

    fn parse_pattern_call_arg(&mut self) -> Result<CallArg, ParseError> {
        let location = self.peek().location(&self.source);
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        if self.eat(&TokenKind::Underscore) {
            return Ok(CallArg::Named {
                name,
                expr: Expr::Var(Ident::new_unchecked("_")),
                location,
            });
        }
        let expr = self.parse_expr()?;
        Ok(CallArg::Named {
            name,
            expr,
            location,
        })
    }

    fn parse_named_args(&mut self, end: &TokenKind) -> Result<Vec<NamedArg>, ParseError> {
        if self.at(end) {
            return Ok(Vec::new());
        }
        self.parse_comma_list(end, |parser| {
            let name = parser.expect_ident()?;
            parser.expect(&TokenKind::Colon)?;
            let expr = parser.parse_expr()?;
            Ok(NamedArg { name, expr })
        })
    }

    fn parse_term(&mut self) -> Result<Term, ParseError> {
        if self.eat(&TokenKind::Underscore) {
            Ok(Term::Wildcard)
        } else {
            self.parse_expr().map(Term::Expr)
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary_expr(0)
    }

    fn parse_binary_expr(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut left = self.parse_primary_expr()?;
        while let Some((op, prec)) = self.peek_arithmetic_op() {
            if prec < min_prec {
                break;
            }
            self.bump();
            let right = self.parse_binary_expr(prec + 1)?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, ParseError> {
        match self.peek().kind.clone() {
            TokenKind::Ident(name) => {
                self.bump();
                let ident = Ident::new_unchecked(name);
                if self.eat(&TokenKind::LParen) {
                    let args = if self.at(&TokenKind::RParen) {
                        Vec::new()
                    } else {
                        self.parse_comma_list(&TokenKind::RParen, Parser::parse_function_call_arg)?
                    };
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::FunctionCall {
                        function: ident,
                        args,
                    })
                } else {
                    match ident.as_str() {
                        "true" => Ok(Expr::Literal(Literal::Bool(true))),
                        "false" => Ok(Expr::Literal(Literal::Bool(false))),
                        "null" => Ok(Expr::Literal(Literal::Null)),
                        _ => Ok(Expr::Var(ident)),
                    }
                }
            }
            TokenKind::String(value) => {
                self.bump();
                Ok(Expr::Literal(Literal::String(value)))
            }
            TokenKind::Number(raw) => {
                self.bump();
                Ok(Expr::Literal(Literal::Number(parse_number(&raw))))
            }
            TokenKind::LBracket => {
                self.bump();
                let items = if self.at(&TokenKind::RBracket) {
                    Vec::new()
                } else {
                    self.parse_comma_list(&TokenKind::RBracket, Parser::parse_literal)?
                };
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr::Literal(Literal::List(items)))
            }
            TokenKind::LParen => {
                self.bump();
                let first = self.parse_expr()?;
                if self.eat(&TokenKind::Comma) {
                    let mut items = vec![first];
                    while !self.at(&TokenKind::RParen) {
                        items.push(self.parse_expr()?);
                        if !self.at(&TokenKind::RParen) {
                            self.expect(&TokenKind::Comma)?;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Tuple(items))
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first)
                }
            }
            _ => Err(ParseError::new(
                &self.source,
                self.peek(),
                "expected expression",
            )),
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, ParseError> {
        match self.parse_expr()? {
            Expr::Literal(literal) => Ok(literal),
            _ => Err(ParseError::new(
                &self.source,
                self.previous(),
                "list literals may contain only literal values",
            )),
        }
    }

    fn parse_reference_arg(&mut self) -> Result<String, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let reference = self.expect_string()?;
        self.expect(&TokenKind::RParen)?;
        Ok(reference)
    }

    fn parse_comparison_op(&mut self) -> Result<ComparisonOp, ParseError> {
        let op = match &self.peek().kind {
            TokenKind::Eq => ComparisonOp::Eq,
            TokenKind::BangEq => ComparisonOp::Ne,
            TokenKind::Lt => ComparisonOp::Lt,
            TokenKind::Gt => ComparisonOp::Gt,
            TokenKind::Le => ComparisonOp::Le,
            TokenKind::Ge => ComparisonOp::Ge,
            TokenKind::Ident(word) if word == "in" => ComparisonOp::In,
            TokenKind::Ident(word) if word == "matches" => ComparisonOp::Matches,
            TokenKind::Ident(word) if word == "contains" => ComparisonOp::Contains,
            TokenKind::Ident(word) if word == "starts_with" => ComparisonOp::StartsWith,
            TokenKind::Ident(word) if word == "ends_with" => ComparisonOp::EndsWith,
            _ => {
                return Err(ParseError::new(
                    &self.source,
                    self.peek(),
                    "expected comparison operator",
                ));
            }
        };
        self.bump();
        Ok(op)
    }

    fn parse_aggregate_function(&mut self) -> Result<AggregateFunction, ParseError> {
        let token = self.bump().clone();
        let TokenKind::Ident(ref name) = token.kind else {
            return Err(ParseError::new(
                &self.source,
                &token,
                "expected aggregate function",
            ));
        };
        match name.as_str() {
            "Count" => Ok(AggregateFunction::Count),
            "Sum" => Ok(AggregateFunction::Sum),
            "Min" => Ok(AggregateFunction::Min),
            "Max" => Ok(AggregateFunction::Max),
            "Avg" => Ok(AggregateFunction::Avg),
            "List" => Ok(AggregateFunction::List),
            "Set" => Ok(AggregateFunction::Set),
            "TopK" => Ok(AggregateFunction::TopK),
            "Rank" => Ok(AggregateFunction::Rank),
            "TakeUntil" => Ok(AggregateFunction::TakeUntil),
            _ => Err(ParseError::new(
                &self.source,
                &token,
                "expected aggregate function",
            )),
        }
    }

    fn parse_comma_list<T>(
        &mut self,
        end: &TokenKind,
        mut parse_one: impl FnMut(&mut Self) -> Result<T, ParseError>,
    ) -> Result<Vec<T>, ParseError> {
        let mut items = Vec::new();
        while !self.at(end) {
            items.push(parse_one(self)?);
            if !self.at(end) {
                self.expect(&TokenKind::Comma)?;
            }
        }
        Ok(items)
    }

    fn starts_comparison_or_aggregation(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Eq
                | TokenKind::BangEq
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::Le
                | TokenKind::Ge
        ) || matches!(
            &self.peek().kind,
            TokenKind::Ident(word)
                if matches!(
                    word.as_str(),
                    "in" | "matches" | "contains" | "starts_with" | "ends_with"
                )
        )
    }

    fn starts_derived_atom(&self) -> bool {
        self.at_ident()
            && (self.peek_n(1).kind == TokenKind::LParen
                || self.peek_n(1).kind == TokenKind::LBrace
                || (self.peek_n(1).kind == TokenKind::Dot
                    && matches!(self.peek_n(2).kind, TokenKind::Ident(_))
                    && matches!(self.peek_n(3).kind, TokenKind::LParen | TokenKind::LBrace)))
    }

    fn next_named_arg_before_colon(&self) -> bool {
        matches!(
            (&self.peek().kind, &self.peek_n(1).kind),
            (TokenKind::Ident(_), TokenKind::Colon)
        )
    }

    fn peek_aggregate_function_at(&self, offset: usize) -> Option<AggregateFunction> {
        match &self.peek_n(offset).kind {
            TokenKind::Ident(name) => match name.as_str() {
                "Count" => Some(AggregateFunction::Count),
                "Sum" => Some(AggregateFunction::Sum),
                "Min" => Some(AggregateFunction::Min),
                "Max" => Some(AggregateFunction::Max),
                "Avg" => Some(AggregateFunction::Avg),
                "List" => Some(AggregateFunction::List),
                "Set" => Some(AggregateFunction::Set),
                "TopK" => Some(AggregateFunction::TopK),
                "Rank" => Some(AggregateFunction::Rank),
                "TakeUntil" => Some(AggregateFunction::TakeUntil),
                _ => None,
            },
            _ => None,
        }
    }

    fn peek_arithmetic_op(&self) -> Option<(ArithmeticOp, u8)> {
        match self.peek().kind {
            TokenKind::Plus => Some((ArithmeticOp::Add, 1)),
            TokenKind::Minus => Some((ArithmeticOp::Sub, 1)),
            TokenKind::Star => Some((ArithmeticOp::Mul, 2)),
            TokenKind::Slash => Some((ArithmeticOp::Div, 2)),
            TokenKind::Percent => Some((ArithmeticOp::Rem, 2)),
            _ => None,
        }
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_keyword(expected) {
            Ok(())
        } else {
            Err(ParseError::new(
                &self.source,
                self.peek(),
                format!("expected keyword {expected:?}"),
            ))
        }
    }

    fn eat_keyword(&mut self, expected: &str) -> bool {
        if self.peek_keyword(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn peek_keyword(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == expected)
    }

    fn expect_ident(&mut self) -> Result<Ident, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(ref value) => Ident::new(value.clone())
                .map_err(|err| ParseError::new(&self.source, &token, err.to_string())),
            _ => Err(ParseError::new(&self.source, &token, "expected identifier")),
        }
    }

    fn expect_string(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::String(value) => Ok(value),
            _ => Err(ParseError::new(&self.source, &token, "expected string")),
        }
    }

    fn expect(&mut self, expected: &TokenKind) -> Result<(), ParseError> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(ParseError::new(
                &self.source,
                self.peek(),
                format!("expected {}", expected.name()),
            ))
        }
    }

    fn eat(&mut self, expected: &TokenKind) -> bool {
        if self.at(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at(&self, expected: &TokenKind) -> bool {
        self.peek().kind.same_variant(expected)
    }

    fn at_ident(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_))
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.cursor.saturating_sub(1)]
    }

    fn peek_n(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.cursor + offset)
            .unwrap_or_else(|| self.tokens.last().expect("lexer always emits eof"))
    }

    fn bump(&mut self) -> &Token {
        let idx = self.cursor;
        self.cursor += 1;
        &self.tokens[idx]
    }
}

fn parse_number(raw: &str) -> NumberLiteral {
    if raw.contains('.') {
        NumberLiteral::Float(raw.parse().expect("lexer produced numeric literal"))
    } else {
        NumberLiteral::Int(raw.parse().expect("lexer produced numeric literal"))
    }
}

fn aggregate_accepts_args(function: AggregateFunction) -> bool {
    matches!(
        function,
        AggregateFunction::TopK | AggregateFunction::Rank | AggregateFunction::TakeUntil
    )
}

fn rule_origin(layer: RuleLayer, source: &str, token: &Token) -> RuleOrigin {
    RuleOrigin::new(layer, token.location(source))
}

fn required_annotation_string<'a>(args: &'a [NamedArg], name: &str) -> Option<&'a str> {
    named_string_arg(args, name)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    line: usize,
    column: usize,
}

impl Token {
    fn location(&self, source: &str) -> SourceLocation {
        SourceLocation::new(source.to_string(), self.line, self.column)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    String(String),
    Number(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Colon,
    ColonEq,
    Question,
    AtSign,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Eq,
    BangEq,
    Lt,
    Gt,
    Le,
    Ge,
    Underscore,
    Eof,
}

impl TokenKind {
    fn same_variant(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Ident(_) => "identifier",
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::LParen => "'('",
            Self::RParen => "')'",
            Self::LBrace => "'{'",
            Self::RBrace => "'}'",
            Self::LBracket => "'['",
            Self::RBracket => "']'",
            Self::Comma => "','",
            Self::Dot => "'.'",
            Self::Colon => "':'",
            Self::ColonEq => "':='",
            Self::Question => "'?'",
            Self::AtSign => "'@'",
            Self::Star => "'*'",
            Self::Plus => "'+'",
            Self::Minus => "'-'",
            Self::Slash => "'/'",
            Self::Percent => "'%'",
            Self::Eq => "'='",
            Self::BangEq => "'!='",
            Self::Lt => "'<'",
            Self::Gt => "'>'",
            Self::Le => "'<='",
            Self::Ge => "'>='",
            Self::Underscore => "'_'",
            Self::Eof => "end of file",
        }
    }
}

struct Lexer<'a> {
    source: &'a str,
    input: &'a str,
    offset: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, input: &'a str) -> Self {
        Self {
            source,
            input,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    fn lex(mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek_char() {
            match ch {
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                '\n' => {
                    self.advance();
                    self.line += 1;
                    self.column = 1;
                }
                '#' => self.skip_comment(),
                '"' => tokens.push(self.lex_string()?),
                '0'..='9' => tokens.push(self.lex_number()),
                'a'..='z' | 'A'..='Z' => tokens.push(self.lex_ident()),
                '_' => {
                    let token = if self.peek_next_ident_char() {
                        self.lex_ident()
                    } else {
                        self.single(TokenKind::Underscore)
                    };
                    tokens.push(token);
                }
                '(' => tokens.push(self.single(TokenKind::LParen)),
                ')' => tokens.push(self.single(TokenKind::RParen)),
                '{' => tokens.push(self.single(TokenKind::LBrace)),
                '}' => tokens.push(self.single(TokenKind::RBrace)),
                '[' => tokens.push(self.single(TokenKind::LBracket)),
                ']' => tokens.push(self.single(TokenKind::RBracket)),
                ',' => tokens.push(self.single(TokenKind::Comma)),
                '.' => tokens.push(self.single(TokenKind::Dot)),
                '?' => tokens.push(self.single(TokenKind::Question)),
                '@' => tokens.push(self.single(TokenKind::AtSign)),
                '*' => tokens.push(self.single(TokenKind::Star)),
                '+' => tokens.push(self.single(TokenKind::Plus)),
                '-' => tokens.push(self.single(TokenKind::Minus)),
                '/' => tokens.push(self.single(TokenKind::Slash)),
                '%' => tokens.push(self.single(TokenKind::Percent)),
                ':' => {
                    let line = self.line;
                    let column = self.column;
                    self.advance();
                    if self.peek_char() == Some('=') {
                        self.advance();
                        tokens.push(Token {
                            kind: TokenKind::ColonEq,
                            line,
                            column,
                        });
                    } else {
                        tokens.push(Token {
                            kind: TokenKind::Colon,
                            line,
                            column,
                        });
                    }
                }
                '=' => tokens.push(self.single(TokenKind::Eq)),
                '!' => {
                    tokens.push(self.two_char('=', TokenKind::BangEq, "expected '=' after '!'")?);
                }
                '<' => tokens.push(self.optional_eq(TokenKind::Lt, TokenKind::Le)),
                '>' => tokens.push(self.optional_eq(TokenKind::Gt, TokenKind::Ge)),
                _ => {
                    return Err(ParseError {
                        location: self.location(),
                        message: format!("unexpected character {ch:?}"),
                    });
                }
            }
        }
        tokens.push(Token {
            kind: TokenKind::Eof,
            line: self.line,
            column: self.column,
        });
        Ok(tokens)
    }

    fn lex_ident(&mut self) -> Token {
        let line = self.line;
        let column = self.column;
        let start = self.offset;
        while matches!(self.peek_char(), Some(ch) if ch.is_ascii_alphanumeric() || ch == '_') {
            self.advance();
        }
        Token {
            kind: TokenKind::Ident(self.input[start..self.offset].to_string()),
            line,
            column,
        }
    }

    fn lex_number(&mut self) -> Token {
        let line = self.line;
        let column = self.column;
        let start = self.offset;
        while matches!(self.peek_char(), Some('0'..='9')) {
            self.advance();
        }
        if self.peek_char() == Some('.') && matches!(self.peek_char_n(1), Some('0'..='9')) {
            self.advance();
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.advance();
            }
        }
        Token {
            kind: TokenKind::Number(self.input[start..self.offset].to_string()),
            line,
            column,
        }
    }

    fn lex_string(&mut self) -> Result<Token, ParseError> {
        let line = self.line;
        let column = self.column;
        self.advance();
        let mut value = String::new();
        while let Some(ch) = self.peek_char() {
            match ch {
                '"' => {
                    self.advance();
                    return Ok(Token {
                        kind: TokenKind::String(value),
                        line,
                        column,
                    });
                }
                '\\' => {
                    self.advance();
                    let Some(escaped) = self.peek_char() else {
                        break;
                    };
                    let decoded = match escaped {
                        '"' => '"',
                        '\\' => '\\',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => other,
                    };
                    value.push(decoded);
                    self.advance();
                }
                '\n' => {
                    return Err(ParseError {
                        location: self.location(),
                        message: "unterminated string".to_string(),
                    });
                }
                other => {
                    value.push(other);
                    self.advance();
                }
            }
        }
        Err(ParseError {
            location: SourceLocation::new(self.source.to_string(), line, column),
            message: "unterminated string".to_string(),
        })
    }

    fn skip_comment(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn single(&mut self, kind: TokenKind) -> Token {
        let token = Token {
            kind,
            line: self.line,
            column: self.column,
        };
        self.advance();
        token
    }

    fn two_char(
        &mut self,
        second: char,
        kind: TokenKind,
        message: &'static str,
    ) -> Result<Token, ParseError> {
        let line = self.line;
        let column = self.column;
        self.advance();
        if self.peek_char() == Some(second) {
            self.advance();
            Ok(Token { kind, line, column })
        } else {
            Err(ParseError {
                location: SourceLocation::new(self.source.to_string(), line, column),
                message: message.to_string(),
            })
        }
    }

    fn optional_eq(&mut self, base: TokenKind, with_eq: TokenKind) -> Token {
        let line = self.line;
        let column = self.column;
        self.advance();
        let kind = if self.peek_char() == Some('=') {
            self.advance();
            with_eq
        } else {
            base
        };
        Token { kind, line, column }
    }

    fn peek_next_ident_char(&self) -> bool {
        matches!(self.peek_char_n(1), Some(ch) if ch.is_ascii_alphanumeric() || ch == '_')
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }

    fn peek_char_n(&self, n: usize) -> Option<char> {
        self.input[self.offset..].chars().nth(n)
    }

    fn advance(&mut self) {
        if let Some(ch) = self.peek_char() {
            self.offset += ch.len_utf8();
            self.column += 1;
        }
    }

    fn location(&self) -> SourceLocation {
        SourceLocation::new(self.source.to_string(), self.line, self.column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AggregateFunction, Atom, CallArg, Literal};

    #[test]
    fn parses_stored_query_with_negation_and_comparison() {
        let program = parse_program(
            "inline",
            r#"? *handle{id: h, kind: "label", namespace: "OQ"}, not terminal(h), h != "OQ-99"."#,
        )
        .expect("program parses");
        let query = program.queries().next().expect("query");
        assert_eq!(query.body.atoms.len(), 3);
        assert!(matches!(query.body.atoms[0], Atom::Stored(_)));
        assert!(matches!(query.body.atoms[1], Atom::Negation(_)));
        assert!(matches!(query.body.atoms[2], Atom::Comparison(_)));
    }

    #[test]
    fn parses_inline_rules_and_count_aggregation() {
        let program = parse_program(
            "inline",
            r#"
            ?
              where open_oq(h) := *handle{id: h, kind: "label"}, not terminal(h).
              area_count(area, n),
              n = Count{ h : open_oq(h), *handle{id: h, area} }.
            "#,
        )
        .expect("program parses");
        let query = program.queries().next().expect("query");
        assert_eq!(query.local_rules.len(), 1);
        assert!(matches!(query.body.atoms[1], Atom::Aggregation(_)));
    }

    #[test]
    fn parses_row_producing_aggregations_with_named_args() {
        let program = parse_program(
            "inline",
            r"
            ? (h, score) = TopK{ k: 10, key: score : (h, score) : score(h, score) }.
            ? (span_id, tokens) =
              TakeUntil{ budget: 4000, sum: tokens, key: line :
                (span_id, tokens) :
                span(span_id, line, tokens)
              }.
            ",
        )
        .expect("program parses");
        let queries = program.queries().collect::<Vec<_>>();
        let Atom::Aggregation(top_k) = &queries[0].body.atoms[0] else {
            panic!("expected TopK aggregation");
        };
        assert_eq!(top_k.function, AggregateFunction::TopK);
        assert_eq!(top_k.args.len(), 2);

        let Atom::Aggregation(take_until) = &queries[1].body.atoms[0] else {
            panic!("expected TakeUntil aggregation");
        };
        assert_eq!(take_until.function, AggregateFunction::TakeUntil);
        assert_eq!(take_until.args.len(), 3);
    }

    #[test]
    fn parses_directives() {
        let program = parse_program(
            "anneal.dl",
            r#"
            include "checks/release.dl".
            import strict_checks from "checks/release.dl".
            optional code.module_pattern("**/*.rs").
            @verb(name: "broken", query: "diagnostic(code)").
            @doc(name: "convergence", doc: "Convergence vocabulary.").
            @predicate(name: "diagnostic", args: ["code", "severity", "subject"]).
            at("HEAD~1") { old(h) := *handle{id: h}. }
            "#,
        )
        .expect("program parses");
        assert_eq!(program.statements.len(), 7);
        assert!(matches!(program.statements[2], Statement::OptionalFact(_)));
        let Statement::Doc(doc) = &program.statements[4] else {
            panic!("expected @doc");
        };
        assert_eq!(doc.name(), "convergence");
        assert_eq!(doc.doc(), "Convergence vocabulary.");
        assert!(matches!(program.statements[5], Statement::Predicate(_)));
    }

    #[test]
    fn parses_static_config_and_source_blocks() {
        let program = parse_program(
            "anneal.dl",
            r#"
            config convergence {
              ordering(["raw", "draft", "current"]).
              active(["draft", "current"]).
            }

            source md {
              file_extension(".md").
              label_pattern("OQ", regex: "OQ-(\\d+)", scope: "any").
            }
            "#,
        )
        .expect("program parses");

        let Statement::ConfigBlock(config) = &program.statements[0] else {
            panic!("expected config block");
        };
        assert_eq!(config.section.as_str(), "convergence");
        assert_eq!(config.declarations.len(), 2);
        assert_eq!(config.declarations[0].name.as_str(), "ordering");

        let Statement::SourceBlock(source) = &program.statements[1] else {
            panic!("expected source block");
        };
        assert_eq!(source.source.as_str(), "md");
        assert_eq!(source.declarations.len(), 2);
        assert!(matches!(
            &source.declarations[1].args[1],
            CallArg::Named { name, .. } if name.as_str() == "regex"
        ));
    }

    #[test]
    fn rejects_malformed_doc_annotations() {
        let err = parse_program(
            "anneal.dl",
            r#"@doc(name: convergence, doc: "Convergence vocabulary.")."#,
        )
        .expect_err("name must be a string");

        assert_eq!(err.location, SourceLocation::new("anneal.dl", 1, 1));
        assert!(err.message.contains("@doc requires string argument name"));
    }

    #[test]
    fn parses_verb_annotation_with_nested_query_and_schema_text() {
        const CONTEXT_OUTPUT_SCHEMA: &str =
            r#"{"goal":"String","hits":[{"handle":"HandleId","span_id":"String|null"}]}"#;
        let source = format!(
            r#"@verb(name: "context", query: "context_hit(h). ? context_hit(h), (span_id, tokens) = TakeUntil{{ budget: 10, sum: tokens, key: line : (span_id, tokens) : span(span_id, line, tokens) }}.", output_schema: "{}")."#,
            CONTEXT_OUTPUT_SCHEMA.replace('"', "\\\"")
        );
        let program = parse_program("views.dl", &source).expect("views.dl parses");
        let Some(verb) = program
            .statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Verb(verb) if verb.string_arg("name") == Some("context") => Some(verb),
                _ => None,
            })
        else {
            panic!("expected context @verb");
        };

        assert_eq!(verb.string_arg("name"), Some("context"));
        let query = verb.string_arg("query").expect("context query arg");
        assert!(query.contains("context_hit"));
        assert!(query.contains("TakeUntil"));
        parse_program("views.dl:context.query", query).expect("context query parses");

        let output_schema = verb
            .string_arg("output_schema")
            .expect("context output schema");
        assert_eq!(output_schema, CONTEXT_OUTPUT_SCHEMA);
        let schema: serde_json::Value =
            serde_json::from_str(output_schema).expect("context schema is json");
        assert_eq!(schema["goal"], "String");
        assert_eq!(schema["hits"][0]["handle"], "HandleId");
        assert_eq!(schema["hits"][0]["span_id"], "String|null");
    }

    #[test]
    fn parses_source_locations_for_heads_queries_and_atoms() {
        let program =
            parse_program("inline", "fact(\"a\").\n? fact(h), h = \"a\".").expect("program parses");
        let Statement::Fact(head) = &program.statements[0] else {
            panic!("expected fact");
        };
        assert_eq!(head.location, SourceLocation::new("inline", 1, 1));

        let Statement::Query(query) = &program.statements[1] else {
            panic!("expected query");
        };
        assert_eq!(query.location, SourceLocation::new("inline", 2, 1));
        assert_eq!(
            query.body.atoms[0].location(),
            &SourceLocation::new("inline", 2, 3)
        );
        assert_eq!(
            query.body.atoms[1].location(),
            &SourceLocation::new("inline", 2, 12)
        );
    }

    #[test]
    fn parses_negation_location_at_not_keyword() {
        let program = parse_program("inline", "fact(\"a\").\n? not fact(h), h = \"a\".")
            .expect("program parses");
        let query = program.queries().next().expect("query");
        assert_eq!(
            query.body.atoms[0].location(),
            &SourceLocation::new("inline", 2, 3)
        );
        assert_eq!(
            query.body.atoms[1].location(),
            &SourceLocation::new("inline", 2, 16)
        );
    }

    #[test]
    fn parses_named_call_site_arguments() {
        let program = parse_program(
            "inline",
            r#"? pair(right: r, left: lower("A")), lower(value: "A") = "a"."#,
        )
        .expect("program parses");
        let query = program.queries().next().expect("query");
        let Atom::Derived(derived) = &query.body.atoms[0] else {
            panic!("expected derived atom");
        };
        assert_eq!(derived.args.len(), 2);
        assert!(matches!(derived.args[0], CallArg::Named { .. }));
        assert!(matches!(derived.args[1], CallArg::Named { .. }));

        let Atom::Comparison(comparison) = &query.body.atoms[1] else {
            panic!("expected comparison");
        };
        let Expr::FunctionCall { args, .. } = &comparison.left else {
            panic!("expected function call");
        };
        assert!(matches!(args[0], CallArg::Named { .. }));
    }

    #[test]
    fn parses_relation_pattern_calls_and_positional_wildcards() {
        let program = parse_program(
            "inline",
            r#"? diagnostic{code: "E001", subject: h}, search{query: "x", handle: h}, diagnostic(_, "error", h, _, _, _)."#,
        )
        .expect("program parses");
        let query = program.queries().next().expect("query");
        let Atom::Derived(first) = &query.body.atoms[0] else {
            panic!("expected first derived atom");
        };
        assert_eq!(first.style, CallStyle::Pattern);
        assert_eq!(first.args.len(), 2);
        assert!(matches!(first.args[0], CallArg::Named { .. }));

        let Atom::Derived(third) = &query.body.atoms[2] else {
            panic!("expected third derived atom");
        };
        assert_eq!(third.style, CallStyle::Complete);
        assert!(matches!(third.args[0], CallArg::Wildcard { .. }));
        assert!(matches!(third.args[3], CallArg::Wildcard { .. }));
    }

    #[test]
    fn rejects_wildcard_inside_expression_function_calls() {
        let err = parse_program("inline", r#"? lower(_) = "x"."#)
            .expect_err("wildcard is not an expression argument");
        assert!(err.to_string().contains("expected expression"));
    }

    #[test]
    fn parses_string_escapes() {
        let program = parse_program("inline", r#"? "a\nb" = "a\nb"."#).expect("program parses");
        let query = program.queries().next().expect("query");
        let Atom::Comparison(comparison) = &query.body.atoms[0] else {
            panic!("expected comparison");
        };
        let Expr::Literal(Literal::String(value)) = &comparison.left else {
            panic!("expected string");
        };
        assert_eq!(value, "a\nb");
    }

    #[test]
    fn rejects_uppercase_identifiers() {
        let err = parse_program("inline", r"? Bad(h).").expect_err("uppercase ident rejected");
        assert!(err.message.contains("invalid identifier"));
    }
}
