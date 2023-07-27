use std::fmt::{Debug, Display, Formatter};

use regex_syntax as re;
use thiserror::Error;

use crate::re::hir::Hir;
use yara_x_parser::ast;

#[derive(Error, Debug)]
pub(crate) enum Error {
    SyntaxError {
        msg: String,
        span: re::ast::Span,
    },
    MixedGreediness {
        is_greedy_1: bool,
        is_greedy_2: bool,
        span_1: re::ast::Span,
        span_2: re::ast::Span,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SyntaxError { msg, .. } => write!(f, "{}", msg),
            Error::MixedGreediness { .. } => write!(f, "mixed greediness"),
        }
    }
}

/// A regular expression parser
pub(crate) struct Parser {
    force_case_sensitive: bool,
    allow_mixed_greediness: bool,
}

impl Parser {
    pub fn new() -> Self {
        Self { force_case_sensitive: false, allow_mixed_greediness: true }
    }

    /// Parses the regexp as a case-sensitive one, no matter whether the regexp
    /// was actually flagged as case-insensitive or not.
    pub fn force_case_sensitive(self, yes: bool) -> Self {
        Self {
            force_case_sensitive: yes,
            allow_mixed_greediness: self.allow_mixed_greediness,
        }
    }

    /// If true, allows regular expressions that mixes greedy and non-greedy
    /// quantifiers (e.g: `/ab.*cd.*?ef/`). When mixed greediness is not allowed
    /// [`Parser::parse`] returns an error if the regular expression contains
    /// both greedy and non-greedy quantifiers. By default mixed greediness is
    /// allowed.
    pub fn allow_mixed_greediness(self, yes: bool) -> Self {
        Self {
            force_case_sensitive: self.force_case_sensitive,
            allow_mixed_greediness: yes,
        }
    }

    pub fn parse(&self, regexp: &ast::Regexp) -> Result<Hir, Error> {
        let ast = re::ast::parse::Parser::new().parse(regexp.src).map_err(
            |err| Error::SyntaxError {
                msg: err.kind().to_string(),
                span: *err.span(),
            },
        )?;

        let greedy = Validator::new().validate(&ast);

        let greedy = if self.allow_mixed_greediness {
            greedy.unwrap_or(None)
        } else {
            greedy?
        };

        let case_insensitive = if self.force_case_sensitive {
            false
        } else {
            regexp.case_insensitive
        };

        let mut translator =
            regex_syntax::hir::translate::TranslatorBuilder::new()
                .case_insensitive(case_insensitive)
                .dot_matches_new_line(regexp.dot_matches_new_line)
                .unicode(false)
                .utf8(false)
                .build();

        let hir = translator.translate(regexp.src, &ast).map_err(|err| {
            Error::SyntaxError {
                msg: err.kind().to_string(),
                span: *err.span(),
            }
        })?;

        Ok(Hir { inner: hir, greedy })
    }
}

struct Validator {
    first_rep: Option<(bool, re::ast::Span)>,
}

impl Validator {
    fn new() -> Self {
        Self { first_rep: None }
    }

    fn validate(&mut self, ast: &re::ast::Ast) -> Result<Option<bool>, Error> {
        re::ast::visit(ast, self)
    }
}

impl re::ast::Visitor for &mut Validator {
    type Output = Option<bool>;
    type Err = Error;

    fn finish(self) -> Result<Self::Output, Self::Err> {
        Ok(self.first_rep.map(|rep| rep.0))
    }

    fn visit_pre(&mut self, ast: &re::ast::Ast) -> Result<(), Self::Err> {
        if let re::ast::Ast::Repetition(rep) = ast {
            if let Some(first_rep) = self.first_rep {
                if rep.greedy != first_rep.0 {
                    return Err(Error::MixedGreediness {
                        is_greedy_1: rep.greedy,
                        is_greedy_2: first_rep.0,
                        span_1: *ast.span(),
                        span_2: first_rep.1,
                    });
                }
            } else {
                self.first_rep = Some((rep.greedy, rep.span));
            }
        }
        Ok(())
    }
}