use super::GrammarRule;
use crate::parser::context::SourceCode;
use crate::parser::{Error, Span};
use ariadne::{Color, Label, ReportKind, Source};
use pest::error::ErrorVariant::{CustomError, ParsingError};
use pest::error::InputLocation;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::ops::Range;
use yansi::Style;

/// Types of reports created by [`ReportBuilder`].
pub(crate) enum ReportType {
    Error,
    Warning,
}

/// Build error and warning reports.
pub(crate) struct ReportBuilder {
    with_colors: bool,
    cache: HashMap<String, ariadne::Source>,
}

/// ReportBuilder implements the [`ariadne::Cache`] trait.
impl ariadne::Cache<String> for ReportBuilder {
    fn fetch(&mut self, id: &String) -> Result<&Source, Box<dyn Debug + '_>> {
        self.cache
            .get(id)
            .ok_or(Box::new(format!("Failed to fetch source `{}`", id)) as _)
    }

    fn display<'a>(&self, id: &'a String) -> Option<Box<dyn Display + 'a>> {
        Some(Box::new(id))
    }
}

impl ReportBuilder {
    /// Creates a new instance of [`ReportBuilder`].
    pub(crate) fn new() -> Self {
        Self { with_colors: false, cache: HashMap::new() }
    }

    /// Indicates whether the reports should have colors. By default this is `false`.
    pub(crate) fn with_colors(&mut self, b: bool) -> &mut Self {
        self.with_colors = b;
        self
    }

    /// Registers a source code with the report builder.
    ///
    /// Before calling [`ReportBuilder::create_report`] with some [`SourceCode`]
    /// the source code must be registered by calling this function.
    pub(crate) fn register_source(&mut self, src: &SourceCode) -> &mut Self {
        let origin = src.origin.clone().unwrap_or_else(|| "line".to_string());
        self.cache.insert(origin, ariadne::Source::from(src.text));
        self
    }

    /// Creates a new error or warning report.
    pub(crate) fn create_report(
        &mut self,
        report_type: ReportType,
        src: &SourceCode,
        span: Span,
        title: String,
        labels: Vec<(Span, String, Style)>,
        note: Option<String>,
    ) -> String {
        let id = src.origin.clone().unwrap_or_else(|| "line".to_string());

        let kind = match report_type {
            ReportType::Error => {
                ReportKind::Custom("error", self.color(Color::Red))
            }
            ReportType::Warning => {
                ReportKind::Custom("warning", self.color(Color::Yellow))
            }
        };

        let title = if self.with_colors {
            Color::Default.style().bold().paint(title)
        } else {
            Color::Unset.paint(title)
        };

        let mut report_builder =
            ariadne::Report::build(kind, id.clone(), span.start)
                .with_config(
                    ariadne::Config::default().with_color(self.with_colors),
                )
                .with_message(title);

        for (span, label, style) in labels {
            let label = if self.with_colors {
                style.paint(label)
            } else {
                Color::Unset.paint(label)
            };
            report_builder = report_builder.with_label(
                Label::new((
                    id.clone(),
                    Range { start: span.start, end: span.end },
                ))
                .with_message(label),
            );
        }

        if let Some(note) = note {
            report_builder = report_builder.with_note(note);
        }

        let report = report_builder.finish();
        let mut buffer = Vec::<u8>::new();

        report.write(self, &mut buffer).unwrap();

        String::from_utf8(buffer).unwrap()
    }

    pub(crate) fn convert_pest_error(
        &mut self,
        src: &SourceCode,
        pest_error: pest::error::Error<GrammarRule>,
    ) -> Error {
        // Start and ending offset within the original code that is going
        // to be highlighted in the error message. The span can cover
        // multiple lines.
        let error_span = match pest_error.location {
            InputLocation::Pos(p) => Span { start: p, end: p },
            InputLocation::Span(span) => Span { start: span.0, end: span.1 },
        };

        let error_msg = match &pest_error.variant {
            CustomError { message } => message.to_owned(),
            ParsingError { positives, negatives } => {
                Error::syntax_error_message(
                    positives,
                    negatives,
                    Error::printable_string,
                )
            }
        };

        let detailed_report = self.create_report(
            ReportType::Error,
            src,
            error_span,
            "syntax error".to_string(),
            vec![(error_span, error_msg.clone(), Color::Red.style().bold())],
            None,
        );

        Error::SyntaxError { detailed_report, error_msg, error_span }
    }

    fn color(&self, c: Color) -> Color {
        if self.with_colors {
            c
        } else {
            Color::Unset
        }
    }
}