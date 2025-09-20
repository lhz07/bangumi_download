use crate::tui::app::App;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Borders, Paragraph, StatefulWidget, Widget, Wrap};
use std::borrow::Cow;

pub struct ActionConfirm {
    question: Cow<'static, str>,
    content: Cow<'static, str>,
    pub action: Box<dyn FnOnce(&mut App)>,
}

impl ActionConfirm {
    pub fn new(
        question: Cow<'static, str>,
        content: Cow<'static, str>,
        action: Box<dyn FnOnce(&mut App)>,
    ) -> Self {
        Self {
            question,
            content,
            action,
        }
    }
}

pub struct ConfirmWidget;

impl StatefulWidget for ConfirmWidget {
    type State = ActionConfirm;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let block = Block::bordered().title(&*state.question);
        let layout = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(block.inner(area));
        block.render(area, buf);
        let content_area = layout[0];
        let line_area = layout[1];
        let option_area = layout[2];
        let content_para = Paragraph::new(&*state.content).wrap(Wrap { trim: true });
        content_para.render(content_area, buf);
        let line = Block::default().borders(Borders::BOTTOM);
        line.render(line_area, buf);
        let option_layout =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(option_area);
        let yes = "(Y)es";
        let no = "(N)o";
        yes.render(option_layout[0], buf);
        no.render(option_layout[1], buf);
    }
}
