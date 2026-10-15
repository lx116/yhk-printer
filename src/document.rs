use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    ListItem(Option<u32>, u8, Vec<Span>),
    Quote(Vec<Span>),
    Rule,
    Code(String),
    PageBreakGap(u16),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
}

fn normalise(input: &str) -> &str {
    input.strip_prefix('\u{feff}').unwrap_or(input)
}

pub fn parse_text(input: &str) -> Document {
    let input = normalise(input).replace("\r\n", "\n").replace('\r', "\n");
    Document {
        blocks: input
            .split("\n\n")
            .filter_map(|paragraph| {
                let text = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
                (!text.is_empty()).then_some(Block::Paragraph(vec![Span {
                    text,
                    ..Span::default()
                }]))
            })
            .collect(),
    }
}

pub fn parse_markdown(input: &str) -> Document {
    let input = normalise(input).replace("\r\n", "\n").replace('\r', "\n");
    let mut document = Document::default();
    let mut spans = Vec::new();
    let mut bold = 0usize;
    let mut italic = 0usize;
    let mut mono = 0usize;
    let mut heading = None;
    let mut quote = 0usize;
    let mut list = Vec::new();
    let mut code = None;
    for event in Parser::new(&input) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(heading_level(level)),
            Event::End(TagEnd::Heading(_)) => {
                document.blocks.push(Block::Heading(
                    heading.take().unwrap_or(3),
                    std::mem::take(&mut spans),
                ));
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                if quote > 0 {
                    document
                        .blocks
                        .push(Block::Quote(std::mem::take(&mut spans)));
                } else if list.is_empty() && !spans.is_empty() {
                    document
                        .blocks
                        .push(Block::Paragraph(std::mem::take(&mut spans)));
                }
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::Start(Tag::Link { .. }) => {}
            Event::End(TagEnd::Link) => {}
            Event::Start(Tag::Image { .. }) => mono += 1000,
            Event::End(TagEnd::Image) => mono = mono.saturating_sub(1000),
            Event::Start(Tag::CodeBlock(kind)) => {
                code = Some((String::new(), matches!(kind, CodeBlockKind::Indented)))
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((text, _)) = code.take() {
                    document.blocks.push(Block::Code(text));
                }
            }
            Event::Start(Tag::List(start)) => list.push((start, 0u32)),
            Event::End(TagEnd::List(_)) => {
                list.pop();
            }
            Event::Start(Tag::Item) => spans.clear(),
            Event::End(TagEnd::Item) => {
                if let Some((ordered, count)) = list.last_mut() {
                    let number = ordered.map(|start| (start as u32).saturating_add(*count));
                    *count += 1;
                    document.blocks.push(Block::ListItem(
                        number,
                        list.len().min(3) as u8,
                        std::mem::take(&mut spans),
                    ));
                }
            }
            Event::Start(Tag::BlockQuote(_)) => quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote = quote.saturating_sub(1),
            Event::Rule => document.blocks.push(Block::Rule),
            Event::Text(text) => {
                if let Some((code, _)) = code.as_mut() {
                    code.push_str(&text);
                } else if mono < 1000 {
                    spans.push(Span {
                        text: text.into_string(),
                        bold: bold > 0,
                        italic: italic > 0,
                        mono: mono > 0,
                    });
                }
            }
            Event::Code(text) => spans.push(Span {
                text: text.into_string(),
                bold: bold > 0,
                italic: italic > 0,
                mono: true,
            }),
            Event::SoftBreak | Event::HardBreak => {
                if let Some((code, _)) = code.as_mut() {
                    code.push('\n');
                } else {
                    spans.push(Span {
                        text: " ".into(),
                        bold: bold > 0,
                        italic: italic > 0,
                        mono: mono > 0,
                    });
                }
            }
            _ => {}
        }
    }
    document
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_normalises_bom_crlf_and_paragraphs() {
        assert_eq!(
            parse_text("\u{feff}one\r\ntwo\r\n\r\nthree").blocks,
            vec![
                Block::Paragraph(vec![Span {
                    text: "one two".into(),
                    ..Span::default()
                }]),
                Block::Paragraph(vec![Span {
                    text: "three".into(),
                    ..Span::default()
                }]),
            ]
        );
    }

    #[test]
    fn markdown_covers_blocks() {
        let document = parse_markdown(
            "# H\n\n**bold** *italic* `code`\n\n- one\n  1. two\n\n> quote\n\n---\n\n```\nraw\n```",
        );
        assert!(matches!(document.blocks[0], Block::Heading(1, _)));
        assert!(document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::ListItem(None, _, _))));
        assert!(document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Quote(_))));
        assert!(document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Rule)));
        assert!(document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Code(_))));
    }
}
