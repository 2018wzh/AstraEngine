use astra_core::{Diagnostic, SourceRef};
use chumsky::{
    input::{Stream, ValueInput},
    prelude::*,
};
use logos::Logos;
use rowan::{GreenNodeBuilder, Language};

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum LexToken {
    #[regex(r"[ \t]+")]
    Whitespace,
    #[regex(r"\r\n|\n|\r")]
    Newline,
    #[token("#@id", priority = 4)]
    SourceId,
    #[regex(r"#[^\r\n]*", allow_greedy = true)]
    Comment,
    #[token("->")]
    Arrow,
    #[token(":")]
    Colon,
    #[regex(r#""([^"\\]|\\.)*""#)]
    Quoted,
    #[regex(r##"[^ \t\r\n:"#]+"##)]
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    Root,
    Story,
    State,
    Scene,
    Command,
    Whitespace,
    Newline,
    SourceId,
    Comment,
    Arrow,
    Colon,
    Quoted,
    Word,
    Error,
}

impl From<LexToken> for SyntaxKind {
    fn from(value: LexToken) -> Self {
        match value {
            LexToken::Whitespace => Self::Whitespace,
            LexToken::Newline => Self::Newline,
            LexToken::SourceId => Self::SourceId,
            LexToken::Comment => Self::Comment,
            LexToken::Arrow => Self::Arrow,
            LexToken::Colon => Self::Colon,
            LexToken::Quoted => Self::Quoted,
            LexToken::Word => Self::Word,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AstraLanguage {}

impl Language for AstraLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= SyntaxKind::Error as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<AstraLanguage>;

pub(crate) fn build_lossless_cst(path: &str, text: &str) -> (SyntaxNode, Vec<Diagnostic>) {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(AstraLanguage::kind_to_raw(SyntaxKind::Root));
    let mut diagnostics = Vec::new();
    let mut open_levels = Vec::<usize>::new();
    let mut offset = 0_usize;
    for line in text.split_inclusive('\n') {
        if let Some((level, kind)) = command_node(line) {
            validate_command_tokens(path, text, line, offset, &mut diagnostics);
            while open_levels.last().is_some_and(|open| *open >= level) {
                builder.finish_node();
                open_levels.pop();
            }
            builder.start_node(AstraLanguage::kind_to_raw(kind));
            open_levels.push(level);
        }
        let mut lexer = LexToken::lexer(line);
        while let Some(token) = lexer.next() {
            let local_span = lexer.span();
            let span = (offset + local_span.start)..(offset + local_span.end);
            let kind = match token {
                Ok(token) => SyntaxKind::from(token),
                Err(()) => {
                    let (line, column) = line_column(text, span.start);
                    diagnostics.push(
                        Diagnostic::warning(
                            "ASTRA_VN_LEX_RECOVERY",
                            "unrecognized source byte was retained as an error token",
                        )
                        .with_source(SourceRef {
                            source: path.to_string(),
                            line,
                            column,
                            length: (span.end - span.start) as u32,
                        }),
                    );
                    SyntaxKind::Error
                }
            };
            builder.token(AstraLanguage::kind_to_raw(kind), &text[span]);
        }
        offset += line.len();
    }
    while open_levels.pop().is_some() {
        builder.finish_node();
    }
    builder.finish_node();
    let green = builder.finish();
    (SyntaxNode::new_root(green), diagnostics)
}

fn validate_command_tokens(
    path: &str,
    source: &str,
    line_text: &str,
    offset: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tokens = LexToken::lexer(line_text).spanned().map(|(token, span)| {
        let kind = token.map(SyntaxKind::from).unwrap_or(SyntaxKind::Error);
        (kind, (offset + span.start..offset + span.end).into())
    });
    let stream = Stream::from_iter(tokens).map(
        (offset..offset + line_text.len()).into(),
        |(token, span): (_, _)| (token, span),
    );
    let result = command_parser().parse(stream);
    for error in result.errors() {
        let start = error.span().start;
        let (line, column) = line_column(source, start);
        diagnostics.push(
            Diagnostic::warning(
                "ASTRA_VN_PARSE_RECOVERY",
                "command parser recovered at the next line boundary",
            )
            .with_source(SourceRef {
                source: path.to_string(),
                line,
                column,
                length: error.span().end.saturating_sub(start) as u32,
            }),
        );
    }
}

fn command_parser<'a, I>() -> impl Parser<'a, I, (), extra::Err<Rich<'a, SyntaxKind>>>
where
    I: ValueInput<'a, Token = SyntaxKind, Span = SimpleSpan>,
{
    just(SyntaxKind::Whitespace)
        .repeated()
        .ignore_then(
            just(SyntaxKind::Word).recover_with(skip_then_retry_until(any().ignored(), end())),
        )
        .then(any().repeated())
        .then_ignore(end())
        .ignored()
}

fn command_node(line: &str) -> Option<(usize, SyntaxKind)> {
    let raw = line.trim_end_matches(['\r', '\n']);
    let body = raw.trim_start_matches(' ');
    if body.is_empty() || body.starts_with('#') {
        return None;
    }
    let indent = raw.len().saturating_sub(body.len());
    let keyword = body.split_ascii_whitespace().next()?;
    match keyword {
        "story" => Some((0, SyntaxKind::Story)),
        "state" => Some((1, SyntaxKind::State)),
        "scene" => Some((2, SyntaxKind::Scene)),
        _ => Some((indent / 2 + 1, SyntaxKind::Command)),
    }
}

fn line_column(text: &str, offset: usize) -> (u32, u32) {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.len(), |(_, tail)| tail.len()) as u32
        + 1;
    (line, column)
}
