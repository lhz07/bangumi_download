use crate::tui::ui::OutterRect;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

pub struct InputWidget<'a> {
    title: Line<'a>,
    prompt: Text<'a>,
    input: Text<'a>,
    input_lines: u16,
}

impl<'a> InputWidget<'a> {
    pub fn new<T1, T2, L>(title: L, prompt: T1, input: T2, input_lines: u16) -> Self
    where
        T1: Into<Text<'a>>,
        T2: Into<Text<'a>>,
        L: Into<Line<'a>>,
    {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            input: input.into(),
            input_lines,
        }
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        // clear the screen first
        Clear.render(area.outer(Margin::new(1, 0)), buf);
        let popup_block = Block::bordered()
            .title(self.title)
            .title_alignment(ratatui::layout::Alignment::Center);
        popup_block.render(area, buf);
        let popup_layout = Layout::vertical([
            Constraint::Percentage(10),
            Constraint::Length(self.input_lines + 2),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(area);
        self.prompt.render(popup_layout[0], buf);
        let input_block = Block::bordered();
        let input_para = Paragraph::new(self.input)
            .block(input_block)
            .wrap(Wrap { trim: true });
        input_para.render(popup_layout[1], buf);
        let line = Block::default().borders(Borders::BOTTOM);
        line.render(popup_layout[3], buf);
        let hint = Line::from(vec![
            Span::raw("Press "),
            Span::raw("Enter").bold(),
            Span::raw(" to confirm, "),
            Span::raw("Esc").bold(),
            Span::raw(" to cancel"),
        ])
        .centered();
        hint.render(popup_layout[4], buf);
    }
}
