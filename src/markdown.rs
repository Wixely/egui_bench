use crate::prelude::*;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Label, Margin, RichText, Sense, Stroke, Ui};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

const BODY: f32 = 13.0;
const BRIGHT: Color32 = Color32::WHITE;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading(u8, Vec<Span>),
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Rule,
    Table { head: Vec<Vec<Span>>, rows: Vec<Vec<Vec<Span>>> },
}

pub fn parse(text: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<Event> = Parser::new_ext(text, options).collect();
    let mut at = 0;
    let mut out = Vec::new();
    while at < events.len() {
        out.extend(blocks(&events, &mut at));
        if at < events.len() {
            at += 1;
        }
    }
    out
}

fn level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn close(events: &[Event], at: &mut usize) {
    if matches!(events.get(*at), Some(Event::End(_))) {
        *at += 1;
    }
}

fn blocks(events: &[Event], at: &mut usize) -> Vec<Block> {
    let mut out = Vec::new();
    while *at < events.len() {
        let before = *at;
        match &events[*at] {
            Event::End(_) => return out,
            Event::Rule => {
                *at += 1;
                out.push(Block::Rule);
            }
            Event::Start(tag) => match tag {
                Tag::Paragraph | Tag::HtmlBlock => {
                    *at += 1;
                    let spans = inlines(events, at);
                    close(events, at);
                    if !spans.is_empty() {
                        out.push(Block::Paragraph(spans));
                    }
                }
                Tag::Heading { level: l, .. } => {
                    let n = level(*l);
                    *at += 1;
                    let spans = inlines(events, at);
                    close(events, at);
                    out.push(Block::Heading(n, spans));
                }
                Tag::BlockQuote(_) => {
                    *at += 1;
                    let inner = blocks(events, at);
                    close(events, at);
                    out.push(Block::Quote(inner));
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => {
                            l.split_whitespace().next().unwrap_or_default().to_string()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    *at += 1;
                    let mut text = String::new();
                    while let Some(Event::Text(t)) = events.get(*at) {
                        text.push_str(t);
                        *at += 1;
                    }
                    close(events, at);
                    out.push(Block::Code { lang, text });
                }
                Tag::List(start) => {
                    let start = *start;
                    *at += 1;
                    let mut items = Vec::new();
                    while matches!(events.get(*at), Some(Event::Start(Tag::Item))) {
                        *at += 1;
                        items.push(blocks(events, at));
                        close(events, at);
                    }
                    close(events, at);
                    out.push(Block::List { start, items });
                }
                Tag::Table(_) => {
                    *at += 1;
                    out.push(table(events, at));
                }
                Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::Image { .. } => {
                    let spans = inlines(events, at);
                    if !spans.is_empty() {
                        out.push(Block::Paragraph(spans));
                    }
                }
                _ => {
                    *at += 1;
                    out.extend(blocks(events, at));
                    close(events, at);
                }
            },
            _ => {
                let spans = inlines(events, at);
                if !spans.is_empty() {
                    out.push(Block::Paragraph(spans));
                }
            }
        }
        if *at == before {
            *at += 1;
        }
    }
    out
}

fn table(events: &[Event], at: &mut usize) -> Block {
    let mut head = Vec::new();
    let mut rows = Vec::new();
    while *at < events.len() {
        match &events[*at] {
            Event::Start(Tag::TableHead) => {
                *at += 1;
                head = cells(events, at);
                close(events, at);
            }
            Event::Start(Tag::TableRow) => {
                *at += 1;
                rows.push(cells(events, at));
                close(events, at);
            }
            Event::End(TagEnd::Table) => {
                *at += 1;
                break;
            }
            _ => *at += 1,
        }
    }
    Block::Table { head, rows }
}

fn cells(events: &[Event], at: &mut usize) -> Vec<Vec<Span>> {
    let mut out = Vec::new();
    while matches!(events.get(*at), Some(Event::Start(Tag::TableCell))) {
        *at += 1;
        out.push(inlines(events, at));
        close(events, at);
    }
    out
}

fn inlines(events: &[Event], at: &mut usize) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut italic, mut strike) = (0u32, 0u32, 0u32);
    let mut links: Vec<String> = Vec::new();
    let mut push =
        |text: &str, code: bool, bold: u32, italic: u32, strike: u32, link: Option<&String>| {
            let style = Style { bold: bold > 0, italic: italic > 0, strike: strike > 0, code };
            let link = link.cloned();
            if let Some(last) = out.last_mut()
                && last.style == style
                && last.link == link
            {
                last.text.push_str(text);
                return;
            }
            out.push(Span { text: text.to_string(), style, link });
        };
    while let Some(event) = events.get(*at) {
        match event {
            Event::Text(t) => push(t, false, bold, italic, strike, links.last()),
            Event::Html(t) | Event::InlineHtml(t) => {
                if is_line_break(t) {
                    push("\n", false, bold, italic, strike, links.last());
                }
            }
            Event::Code(t) => push(t, true, bold, italic, strike, links.last()),
            Event::SoftBreak => push(" ", false, bold, italic, strike, links.last()),
            Event::HardBreak => push("\n", false, bold, italic, strike, links.last()),
            Event::TaskListMarker(done) => {
                push(if *done { "[x] " } else { "[ ] " }, false, bold, italic, strike, None)
            }
            Event::FootnoteReference(t) => {
                push(&format!("[{t}]"), false, bold, italic, strike, None)
            }
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::Start(Tag::Strong) => bold += 1,
            Event::Start(Tag::Strikethrough) => strike += 1,
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::End(TagEnd::Strikethrough) => strike = strike.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. })
            | Event::Start(Tag::Image { dest_url, .. }) => links.push(dest_url.to_string()),
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                links.pop();
            }
            _ => break,
        }
        *at += 1;
    }
    autolinked(out)
}

/// Where a bare `http://` or `https://` address starts and ends in `text`. Agents rarely write
/// `[words](url)`, so addresses typed as plain text are made links too. The end skips the
/// punctuation that closes a sentence, and a `)` only counts when the address opened one.
pub fn urls_in(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while from < text.len() {
        let rest = &text[from..];
        let Some(at) = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min() else {
            break;
        };
        let start = from + at;
        let before = text[..start].chars().next_back();
        let tail = &text[start..];
        let mut end = start
            + tail
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
                .unwrap_or(tail.len());
        loop {
            let url = &text[start..end];
            let Some(last) = url.chars().next_back() else { break };
            let unmatched = url.matches(')').count() > url.matches('(').count();
            let trim = match last {
                '.' | ',' | ';' | ':' | '!' | '?' | '\'' | ']' | '}' | '*' => true,
                ')' => unmatched,
                _ => false,
            };
            if !trim {
                break;
            }
            end -= last.len_utf8();
        }
        let scheme = if text[start..].starts_with("https://") { 8 } else { 7 };
        let glued = before.is_some_and(|c| c.is_alphanumeric());
        if end > start + scheme && !glued {
            found.push(start..end);
        }
        from = end.max(start + scheme);
    }
    found
}

fn autolinked(spans: Vec<Span>) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if span.link.is_some() || span.style.code {
            out.push(span);
            continue;
        }
        let mut at = 0;
        for url in urls_in(&span.text) {
            if url.start > at {
                out.push(Span { text: span.text[at..url.start].to_string(), ..span.clone() });
            }
            let address = span.text[url.clone()].to_string();
            out.push(Span {
                text: address.clone(),
                style: span.style.clone(),
                link: Some(address),
            });
            at = url.end;
        }
        if at < span.text.len() {
            out.push(Span { text: span.text[at..].to_string(), ..span });
        }
    }
    out
}

fn is_line_break(tag: &str) -> bool {
    let t = tag.trim().to_ascii_lowercase();
    t.starts_with("<br") && t.ends_with('>')
}

pub fn resolve(url: &str, base: &str) -> String {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') || url.contains("://") || url.starts_with("mailto:") {
        return url.to_string();
    }
    let (root, path) = match base.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('/') {
            Some((host, path)) => (format!("{scheme}://{host}"), path.to_string()),
            None => (format!("{scheme}://{rest}"), String::new()),
        },
        None => return url.to_string(),
    };
    let mut parts: Vec<&str> = match url.starts_with('/') {
        true => Vec::new(),
        false => path.split('/').filter(|p| !p.is_empty()).collect(),
    };
    let relative = url.trim_start_matches('/');
    let (path_part, tail) = match relative.find(['?', '#']) {
        Some(i) => relative.split_at(i),
        None => (relative, ""),
    };
    for segment in path_part.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("{root}/{}{tail}", parts.join("/"))
}

fn resolved(blocks: Vec<Block>, base: &str) -> Vec<Block> {
    let fix = |spans: Vec<Span>| -> Vec<Span> {
        spans
            .into_iter()
            .map(|mut s| {
                s.link = s.link.map(|l| resolve(&l, base));
                s
            })
            .collect()
    };
    blocks
        .into_iter()
        .map(|b| match b {
            Block::Paragraph(s) => Block::Paragraph(fix(s)),
            Block::Heading(l, s) => Block::Heading(l, fix(s)),
            Block::Quote(inner) => Block::Quote(resolved(inner, base)),
            Block::List { start, items } => {
                Block::List { start, items: items.into_iter().map(|i| resolved(i, base)).collect() }
            }
            Block::Table { head, rows } => Block::Table {
                head: head.into_iter().map(&fix).collect(),
                rows: rows.into_iter().map(|r| r.into_iter().map(&fix).collect()).collect(),
            },
            other => other,
        })
        .collect()
}

#[derive(Default)]
pub struct Cache {
    parsed: HashMap<u64, Arc<Vec<Block>>>,
    used: HashSet<u64>,
}

impl Cache {
    pub fn get(&mut self, text: &str) -> Arc<Vec<Block>> {
        self.get_at(text, None)
    }

    pub fn get_at(&mut self, text: &str, base: Option<&str>) -> Arc<Vec<Block>> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        base.hash(&mut hasher);
        let key = hasher.finish();
        self.used.insert(key);
        self.parsed
            .entry(key)
            .or_insert_with(|| {
                let blocks = parse(text);
                Arc::new(match base {
                    Some(base) => resolved(blocks, base),
                    None => blocks,
                })
            })
            .clone()
    }

    pub fn sweep(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.parsed.retain(|k, _| used.contains(k));
    }

    pub fn len(&self) -> usize {
        self.parsed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parsed.is_empty()
    }
}

pub fn show(ui: &mut Ui, cache: &mut Cache, text: &str, salt: usize) {
    show_at(ui, cache, text, salt, None);
}

pub fn show_at(ui: &mut Ui, cache: &mut Cache, text: &str, salt: usize, base: Option<&str>) {
    let parsed = cache.get_at(text, base);
    ui.push_id(("markdown", salt), |ui| render(ui, &parsed));
}

fn render(ui: &mut Ui, blocks: &[Block]) {
    for (n, block) in blocks.iter().enumerate() {
        if n > 0 {
            ui.add_space(6.0);
        }
        draw(ui, block, n);
    }
}

fn draw(ui: &mut Ui, block: &Block, n: usize) {
    match block {
        Block::Paragraph(spans) => inline(ui, spans, BODY, false),
        Block::Heading(l, spans) => {
            let size = match l {
                1 => 21.0,
                2 => 17.5,
                3 => 15.0,
                _ => 13.5,
            };
            inline(ui, spans, size, true);
            if *l <= 2 {
                ui.add_space(2.0);
                rule(ui);
            }
        }
        Block::Code { lang, text } => code(ui, lang, text, n),
        Block::Quote(inner) => {
            let framed = egui::Frame::NONE
                .inner_margin(Margin { left: 12, right: 0, top: 0, bottom: 0 })
                .show(ui, |ui| render(ui, inner));
            let r = framed.response.rect;
            ui.painter().line_segment(
                [r.left_top() + egui::vec2(3.0, 1.0), r.left_bottom() + egui::vec2(3.0, -1.0)],
                Stroke::new(2.0, ETCH),
            );
        }
        Block::List { start, items } => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    ui.add_space(3.0);
                }
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0);
                    match start {
                        Some(first) => {
                            let mut job = LayoutJob::default();
                            job.append(
                                &format!("{}.", first + i as u64),
                                0.0,
                                TextFormat {
                                    font_id: figure(12.0),
                                    color: LEGEND,
                                    ..Default::default()
                                },
                            );
                            align_baselines(ui, &mut job, BODY);
                            ui.add_sized([22.0, BODY * 1.5], Label::new(job));
                        }
                        None => {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(18.0, BODY * 1.5), Sense::hover());
                            let base = ascent(ui, &FontId::proportional(BODY));
                            let centre =
                                egui::pos2(rect.center().x, rect.top() + base - BODY * 0.33);
                            ui.painter().circle_filled(centre, 2.5, LEGEND);
                        }
                    }
                    ui.vertical(|ui| render(ui, item));
                });
            }
        }
        Block::Rule => rule(ui),
        Block::Table { head, rows } => {
            // Cells do not wrap, so a wide table would widen everything after it. The scroll area
            // keeps it to the width it was given.
            egui::ScrollArea::horizontal()
                .id_salt(("md-table-scroll", n))
                .auto_shrink([true, true])
                .show(ui, |ui| {
                    egui::Grid::new(("md-table", n)).striped(true).spacing([16.0, 5.0]).show(
                        ui,
                        |ui| {
                            for c in head {
                                cell(ui, c, BODY, true, true);
                            }
                            ui.end_row();
                            for row in rows {
                                for c in row {
                                    cell(ui, c, BODY, false, true);
                                }
                                ui.end_row();
                            }
                        },
                    );
                });
        }
    }
}

fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, ETCH);
}

fn code(ui: &mut Ui, lang: &str, text: &str, n: usize) {
    let body = text.trim_end_matches('\n');
    egui::Frame::NONE
        .fill(WELL)
        .stroke(Stroke::new(1.0, ETCH))
        .corner_radius(2)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                Line::new().legend(if lang.is_empty() { "code" } else { lang }).size(10.0).show(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("copy").clicked() {
                        ui.ctx().copy_text(body.to_string());
                    }
                });
            });
            ui.add_space(2.0);
            egui::ScrollArea::horizontal().id_salt(("code", n)).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    ui.add(Label::new(RichText::new(body).font(mono(12.0)).color(VALUE)).extend());
                },
            );
        });
}

pub fn job_of(
    spans: &[Span],
    size: f32,
    bold_all: bool,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let mut job = LayoutJob::default();
    let mut links = Vec::new();
    for span in spans {
        let bold = bold_all || span.style.bold;
        let colour = if span.link.is_some() {
            TRACE
        } else if span.style.code {
            READOUT
        } else if bold {
            BRIGHT
        } else {
            VALUE
        };
        let format = TextFormat {
            font_id: if span.style.code { mono(size - 0.5) } else { FontId::proportional(size) },
            color: colour,
            italics: span.style.italic,
            strikethrough: if span.style.strike { Stroke::new(1.0, LEGEND) } else { Stroke::NONE },
            underline: if span.link.is_some() { Stroke::new(1.0, TRACE) } else { Stroke::NONE },
            background: if span.style.code { WELL } else { Color32::TRANSPARENT },
            line_height: Some(size * 1.5),
            ..Default::default()
        };
        let start = job.text.len();
        job.append(&span.text, 0.0, format);
        if let Some(url) = &span.link {
            links.push((start..job.text.len(), url.clone()));
        }
    }
    (job, links)
}

pub fn link_at<'a>(
    text: &str,
    links: &'a [(Range<usize>, String)],
    char_index: usize,
) -> Option<&'a str> {
    let byte = text.char_indices().nth(char_index).map(|(b, _)| b).unwrap_or(text.len());
    links.iter().find(|(range, _)| range.contains(&byte)).map(|(_, url)| url.as_str())
}

pub fn openable(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

/// The distance from the top of a line to the baseline in `font`.
fn ascent(ui: &Ui, font: &FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap("x".into(), font.clone(), Color32::WHITE))
        .rows
        .first()
        .and_then(|r| r.glyphs.first())
        .map_or(0.0, |g| g.font_face_ascent)
}

/// Put every section of `job` on the baseline of the body font at `size`.
///
/// A row places each glyph from the top of its own line box, and fonts disagree on how far down
/// the baseline sits, so code beside prose stood a little lower. Each section's line height is
/// moved by the difference in ascent, and with the sections aligned to the bottom of the row that
/// moves its baseline onto the body's.
pub fn align_baselines(ui: &Ui, job: &mut LayoutJob, size: f32) {
    let reference = ascent(ui, &FontId::proportional(size));
    let line = size * 1.5;
    for section in &mut job.sections {
        let own = ascent(ui, &section.format.font_id);
        section.format.line_height = Some((line + own - reference).max(1.0));
        section.format.valign = egui::Align::BOTTOM;
    }
}

fn inline(ui: &mut Ui, spans: &[Span], size: f32, bold_all: bool) {
    cell(ui, spans, size, bold_all, false);
}

fn cell(ui: &mut Ui, spans: &[Span], size: f32, bold_all: bool, extend: bool) {
    let (mut job, links) = job_of(spans, size, bold_all);
    if job.text.is_empty() {
        ui.label("");
        return;
    }
    align_baselines(ui, &mut job, size);
    let label = Label::new(job.clone());
    let mut label = if extend { label.extend() } else { label.wrap() };
    if !links.is_empty() {
        label = label.sense(Sense::click());
    }
    let resp = ui.add(label);
    if links.is_empty() {
        return;
    }
    let Some(pos) = resp.hover_pos().or(resp.interact_pointer_pos()) else { return };
    let mut laid = job.clone();
    laid.wrap.max_width = resp.rect.width();
    let galley = ui.fonts_mut(|f| f.layout_job(laid));
    let cursor = galley.cursor_from_pos(pos - resp.rect.min);
    let Some(url) = link_at(&job.text, &links, cursor.index.0) else { return };
    let url = url.to_string();
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    if resp.clicked() && openable(&url) {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
    }
    resp.on_hover_text(url);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;

    fn found(text: &str) -> Vec<&str> {
        urls_in(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn plain_addresses_are_found_without_the_punctuation_around_them() {
        assert_eq!(
            found("see https://a.io/x, and http://b.io."),
            ["https://a.io/x", "http://b.io"]
        );
        assert_eq!(found("(https://a.io/x)"), ["https://a.io/x"]);
        assert_eq!(
            found("https://en.wikipedia.org/wiki/Rust_(language)"),
            ["https://en.wikipedia.org/wiki/Rust_(language)"]
        );
        assert_eq!(found("<https://a.io>"), ["https://a.io"]);
        assert!(found("nothttps://a.io and https:// alone").is_empty());
    }

    #[test]
    fn a_plain_address_becomes_a_link_but_code_and_named_links_are_left() {
        let blocks = parse("go to https://a.io/x. `https://b.io` and [c](https://c.io)\n");
        let Block::Paragraph(p) = &blocks[0] else { panic!("{:?}", blocks[0]) };
        let links: Vec<_> = p.iter().filter_map(|s| s.link.as_deref()).collect();
        assert_eq!(links, ["https://a.io/x", "https://c.io"]);
        assert_eq!(
            p.iter().map(|s| s.text.as_str()).collect::<String>(),
            "go to https://a.io/x. https://b.io and c"
        );
    }

    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn headings_and_paragraphs_keep_their_inline_styles() {
        let blocks = parse("# Title\n\nSome **bold**, *soft* and `code` with ~~gone~~.\n");
        assert_eq!(blocks.len(), 2);
        let Block::Heading(1, h) = &blocks[0] else { panic!("{:?}", blocks[0]) };
        assert_eq!(plain(h), "Title");
        let Block::Paragraph(p) = &blocks[1] else { panic!("{:?}", blocks[1]) };
        assert_eq!(plain(p), "Some bold, soft and code with gone.");
        let styled = |t: &str| p.iter().find(|s| s.text == t).unwrap().style.clone();
        assert!(styled("bold").bold);
        assert!(styled("soft").italic);
        assert!(styled("code").code);
        assert!(styled("gone").strike);
        assert!(!styled("Some ").bold);
    }

    #[test]
    fn a_link_keeps_its_address_over_its_words() {
        let Block::Paragraph(p) = &parse("See [the docs](https://example.com/a) now.")[0] else {
            panic!()
        };
        let link = p.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(link.text, "the docs");
        assert_eq!(link.link.as_deref(), Some("https://example.com/a"));
        assert!(p.iter().filter(|s| s.link.is_none()).count() == 2);
        let (job, links) = job_of(p, 13.0, false);
        let at = job.text.find("docs").unwrap();
        assert_eq!(
            link_at(&job.text, &links, job.text[..at].chars().count()),
            Some("https://example.com/a")
        );
        assert_eq!(link_at(&job.text, &links, 0), None);
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(openable("https://a.b"));
        assert!(openable("HTTP://a.b"));
        assert!(openable("mailto:a@b"));
        assert!(!openable("file:///etc/passwd"));
        assert!(!openable("javascript:alert(1)"));
        assert!(!openable("/home/me"));
    }

    #[test]
    fn a_fenced_block_keeps_its_language_and_lines() {
        let blocks = parse("Before\n\n```gcad\nrect 40 30\nextrude 3\n```\n\nAfter");
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[1],
            Block::Code { lang: "gcad".into(), text: "rect 40 30\nextrude 3\n".into() }
        );
    }

    #[test]
    fn a_fence_still_open_while_streaming_is_a_code_block() {
        let blocks = parse("Here:\n\n```toml\n[board]\nname = \"x\"");
        let Block::Code { lang, text } = &blocks[1] else { panic!("{blocks:?}") };
        assert_eq!(lang, "toml");
        assert!(text.contains("name = \"x\""));
    }

    #[test]
    fn lists_nest_and_number() {
        let blocks = parse("1. one\n2. two\n   - inner a\n   - inner b\n3. three\n");
        let Block::List { start: Some(1), items } = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].len(), 2);
        let Block::List { start: None, items: inner } = &items[1][1] else {
            panic!("{:?}", items[1])
        };
        assert_eq!(inner.len(), 2);
        let Block::Paragraph(p) = &items[0][0] else { panic!() };
        assert_eq!(plain(p), "one");
    }

    #[test]
    fn a_loose_list_keeps_paragraphs_in_its_items() {
        let blocks = parse("- a\n\n  more\n\n- b\n");
        let Block::List { items, .. } = &blocks[0] else { panic!() };
        assert_eq!(items[0].len(), 2);
    }

    #[test]
    fn task_items_show_their_box() {
        let blocks = parse("- [x] done\n- [ ] todo\n");
        let Block::List { items, .. } = &blocks[0] else { panic!() };
        let Block::Paragraph(a) = &items[0][0] else { panic!() };
        let Block::Paragraph(b) = &items[1][0] else { panic!() };
        assert_eq!(plain(a), "[x] done");
        assert_eq!(plain(b), "[ ] todo");
    }

    #[test]
    fn a_table_has_a_head_and_rows() {
        let blocks =
            parse("| file | holds |\n|---|---|\n| `*.sch.toml` | schematic |\n| b | c |\n");
        let Block::Table { head, rows } = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(head.len(), 2);
        assert_eq!(plain(&head[0]), "file");
        assert_eq!(rows.len(), 2);
        assert_eq!(plain(&rows[0][0]), "*.sch.toml");
        assert!(rows[0][0][0].style.code);
    }

    #[test]
    fn html_tags_are_not_drawn_as_text_and_a_break_is_a_break() {
        let blocks =
            parse("<img src=\"a.svg\" width=\"64\">\n\n# Title\n\nline<br>next <b>bold</b>\n");
        let text: String = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(s) | Block::Heading(_, s) => Some(plain(s)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|");
        assert!(!text.contains('<') && !text.contains("img"), "{text}");
        assert!(text.contains("Title") && text.contains("line\nnext bold"), "{text}");
    }

    #[test]
    fn relative_links_are_made_whole_against_where_the_document_lives() {
        let base = "https://github.com/v0l/gcad/blob/HEAD/";
        let at = |u: &str| resolve(u, base);
        assert_eq!(
            at("docs/simulation.md"),
            "https://github.com/v0l/gcad/blob/HEAD/docs/simulation.md"
        );
        assert_eq!(at("./LICENSE"), "https://github.com/v0l/gcad/blob/HEAD/LICENSE");
        assert_eq!(at("/LICENSE"), "https://github.com/LICENSE");
        assert_eq!(at("../x.md"), "https://github.com/v0l/gcad/blob/x.md");
        assert_eq!(at("docs/a.md#part"), "https://github.com/v0l/gcad/blob/HEAD/docs/a.md#part");
        assert_eq!(at("https://example.com/a"), "https://example.com/a");
        assert_eq!(at("mailto:a@b"), "mailto:a@b");
        assert_eq!(at("#heading"), "#heading");
        let blocks = Cache::default()
            .get_at("See [the docs](docs/a.md) and [site](https://x.y).", Some(base));
        let Block::Paragraph(p) = &blocks[0] else { panic!("{blocks:?}") };
        let links: Vec<&str> = p.iter().filter_map(|s| s.link.as_deref()).collect();
        assert_eq!(links, ["https://github.com/v0l/gcad/blob/HEAD/docs/a.md", "https://x.y"]);
    }

    #[test]
    fn quotes_rules_and_html_do_not_get_lost() {
        let blocks = parse("> quoted\n> text\n\n---\n\n<b>raw</b>\n");
        let Block::Quote(inner) = &blocks[0] else { panic!("{blocks:?}") };
        assert_eq!(inner.len(), 1);
        assert_eq!(blocks[1], Block::Rule);
        assert!(blocks.len() >= 3);
    }

    #[test]
    fn odd_input_does_not_loop_or_panic() {
        for text in [
            "",
            "\n\n",
            "[x]: y\n",
            "* \n",
            "|a|\n|-|\n",
            "![alt](u)",
            "<div>\n\ntext\n</div>",
            "```",
            "- \n  - \n",
            "a  \nb",
        ] {
            let _ = parse(text);
        }
    }

    #[test]
    fn the_cache_reparses_only_what_changed_and_forgets_the_rest() {
        let mut cache = Cache::default();
        let a = cache.get("one");
        let again = cache.get("one");
        assert!(Arc::ptr_eq(&a, &again));
        cache.get("two");
        assert_eq!(cache.len(), 2);
        cache.sweep();
        cache.get("two");
        cache.sweep();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn everything_draws() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let mut cache = Cache::default();
        let text = "# H\n\npara **b** [l](https://x.y) `c`\n\n- a\n- b\n  1. n\n\n> q\n\n```rs\nfn x() {}\n```\n\n| a | b |\n|-|-|\n| 1 | 2 |\n\n---\n";
        let _ = ctx.run_frame(egui::RawInput::default(), |ui| {
            show(ui, &mut cache, text, 0);
        });
        let _ = ctx.run_frame(egui::RawInput::default(), |ui| {
            show(ui, &mut cache, text, 0);
        });
    }
}
