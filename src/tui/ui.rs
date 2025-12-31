use crate::recovery_signal::WaiterKind;
use crate::tui::app::App;
use crate::tui::confirm_widget::{ActionConfirm, ConfirmWidget};
use crate::tui::editor::Editor;
use crate::tui::input_widget::InputWidget;
use crate::tui::notification_widget::NotificationWidget;
use crate::tui::progress_bar::{BasicBar, SpeedSum};
use crate::tui::qrcode_widget;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    Table, Tabs, Wrap,
};
use std::borrow::Cow;
use std::io;
use strum::{EnumCount, VariantArray};

#[derive(Clone, Copy, PartialEq)]
pub enum CurrentScreen {
    Main,
    Downloading,
    Finished,
    Filter,
    State,
    Log,
}

pub enum Popup {
    DownloadFolder,
    Login,
    AddRSSLink,
    Confirm(ActionConfirm),
}

pub enum InputState {
    NotInput,
    Text(Editor),
}

impl<'a> From<&'a InputState> for Line<'a> {
    fn from(value: &'a InputState) -> Self {
        match value {
            InputState::NotInput => Line::from(""),
            InputState::Text(editor) => editor.to_line(),
        }
    }
}
// impl<'a> From<&'a InputState> for Line<'a> {
//     fn from(value: &'a InputState) -> Self {
//         match value {
//             InputState::NotInput => Line::from(""),
//             InputState::Text(inner) => Line::from(Span::raw(inner.str.as_str()).not_reversed()),
//             InputState::SelectedAll(str) => Line::from(Span::raw(str.as_str()).reversed()),
//         }
//     }
// }

impl InputState {
    pub fn empty_text() -> Self {
        Self::Text(Editor::new())
    }
    pub fn text(str: String) -> Self {
        Self::Text(Editor::new_with_text(str))
    }
    pub fn to_selected(&mut self) {}
    pub fn to_unselected(&mut self) {}
    pub fn is_typing(&self) -> bool {
        !matches!(self, InputState::NotInput)
    }
    pub fn reverse(&self) -> Line<'_> {
        match self {
            InputState::NotInput => Line::from(""),
            InputState::Text(editor) => Line::from(editor.to_reversed_line()),
        }
    }
    pub fn take(&mut self) -> InputState {
        match self {
            InputState::Text(_) => std::mem::replace(self, InputState::Text(Editor::default())),
            InputState::NotInput => InputState::NotInput,
        }
    }
}

pub fn render(app: &mut App) -> io::Result<()> {
    app.terminal.draw(|f| {
        let downloading_tasks = app.downloading_state.progress_suit.len();
        let downloading_tab = if downloading_tasks > 0 {
            format!("Downloading ({})", downloading_tasks)
        } else {
            "Downloading".to_string()
        };
        // render tabs
        let tabs = Tabs::new([
            "Main".to_string(),
            downloading_tab,
            "Finished".to_string(),
            "Filter Rules".to_string(),
            "Running State".to_string(),
            "Log".to_string(),
        ])
        .select(app.current_screen as usize);
        let main_layout =
            Layout::vertical([Constraint::Min(2), Constraint::Percentage(100)]).split(f.area());
        let tab_horizontal_layout =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(3)]).split(main_layout[0]);
        let tab_title_area = tab_horizontal_layout[0];
        let tab_status_area = tab_horizontal_layout[1];
        let tab_content_area = main_layout[1];
        f.render_widget(tabs, tab_title_area);
        let status = if app.waiting_state.waiting_count > 0 {
            Span::raw("X").red()
        } else {
            Span::raw("✓").green()
        };
        f.render_widget(status, tab_status_area);

        if app.current_screen != CurrentScreen::Downloading {
            app.downloading_state
                .progress_suit
                .retain(|p| !p.is_finished());
        }

        match app.current_screen {
            CurrentScreen::Main => {
                // render main screen
                let horizontal_layuout =
                    Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                        .split(tab_content_area);
                let anime_list_area = horizontal_layuout[0];
                let anime_detail_area = horizontal_layuout[1];
                let list_items = app
                    .rss_data
                    .iter()
                    .map(|anime| {
                        let mut lines = Vec::with_capacity(3);
                        lines.push(Line::from(anime.name.as_str()));
                        lines.push(Line::from(format!("Last Update: {}", anime.last_update)));
                        lines.push(Line::default());
                        ListItem::new(lines)
                    })
                    .collect::<Vec<_>>();
                let list_block_title = match &mut app.loading_state {
                    Some(state) => {
                        let str = state.next_state();
                        Cow::from(format!("Subscribed Bangumi {str}"))
                    }
                    None => Cow::from("Subscribed Bangumi"),
                };
                let list = List::new(list_items)
                    .block(
                        Block::default()
                            .title(list_block_title)
                            .borders(Borders::ALL),
                    )
                    .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
                    .highlight_style(
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::REVERSED),
                    )
                    .highlight_symbol("› ");
                f.render_stateful_widget(list, anime_list_area, &mut app.rss_state);
                if let Some(index) = app.rss_state.selected() {
                    let detail_block = Block::default()
                        .title("Bangumi Detail")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::LightBlue));
                    let mut lines = Vec::with_capacity(6);
                    let anime = &app.rss_data[index];
                    lines.push(Line::from(Span::from(anime.name.as_str()).bold()));
                    lines.push(Line::default());
                    lines.push(Line::from(format!("Last Update: {}", anime.last_update)));
                    lines.push(Line::default());
                    lines.push(Line::from("Latest Episode: "));
                    lines.push(Line::from(anime.latest_episode.as_str()));
                    let detail_paragraph = Paragraph::new(lines)
                        .block(detail_block)
                        .wrap(Wrap { trim: true });
                    f.render_widget(detail_paragraph, anime_detail_area);
                }
            }
            CurrentScreen::Downloading => {
                let state = &mut app.downloading_state;
                let horizontal_layout = Layout::horizontal([
                    Constraint::Length(tab_content_area.width - 3),
                    Constraint::Length(3),
                ])
                .split(tab_content_area);
                let vertical_layout =
                    Layout::vertical([Constraint::Length(2), Constraint::Fill(1)])
                        .split(horizontal_layout[0]);
                let download_status_area = vertical_layout[0];
                let line = Line::raw(format!(
                    "Downloading task(s): {}       Speed: {}/s",
                    state.progress_suit.len(),
                    state.progress_suit.speed()
                ));
                f.render_widget(line, download_status_area);
                let progresses_area = vertical_layout[1];
                let scroll_bar_area = horizontal_layout[1];
                let height = vertical_layout[1].height as usize;
                // 每个进度条占用 3 行：上下边框 + 内容
                let per_item_height = 3;
                // 可视的进度条个数
                let visible_count = height / per_item_height;
                // 计算可见区间
                state.offset = state
                    .offset
                    .min(state.progress_suit.len().saturating_sub(visible_count));
                let end = (state.offset + visible_count).min(state.progress_suit.len());
                state.scroll_state = state
                    .scroll_state
                    .content_length(state.progress_suit.len().saturating_sub(visible_count));
                // 为每个进度条生成一个长度为 per_item_height 的约束
                let constraints =
                    vec![Constraint::Length(per_item_height as u16); end - state.offset];
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(constraints)
                    .split(progresses_area);
                let mut chunks_iter = chunks.iter();
                let mut j = 0;
                state.progress_suit.retain(|p| {
                    let percent = p.pos();
                    if j >= state.offset
                        && j < end
                        && let Some(chunk) = chunks_iter.next()
                    {
                        let gauge = Gauge::default()
                            .block(Block::default().borders(Borders::ALL).title(format!(
                                "{} {} / {}   {}/s",
                                p.name(),
                                p.current_size_format(),
                                p.size_format(),
                                p.current_speed()
                            )))
                            .gauge_style(
                                Style::default()
                                    .fg(Color::Rgb(0, 212, 241))
                                    .bg(Color::Rgb(37, 50, 56)),
                            )
                            .percent(percent);
                        f.render_widget(gauge, *chunk);
                    }
                    j += 1;
                    // we should use accurate data here, instead of using percent.
                    // percent is not accurate, when its true percent is almost 100%, it will show as 100%,
                    // but removing the bar at that time is too early
                    !p.is_finished()
                });
                f.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(Some("↑"))
                        .end_symbol(Some("↓")),
                    scroll_bar_area,
                    &mut state.scroll_state,
                );
            }
            CurrentScreen::Finished => {}
            CurrentScreen::Filter => {
                let horizontal_layuout =
                    Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                        .split(tab_content_area);
                let id_area = horizontal_layuout[0];
                let filter_area = horizontal_layuout[1];
                let is_editing_id =
                    app.filter_rule_state.selected().is_none() && app.input_state.is_typing();
                let list_items = app
                    .filters
                    .iter()
                    .enumerate()
                    .map(|(index, filter)| {
                        let mut lines = Vec::with_capacity(2);
                        let is_selected = if let Some(i) = app.filter_id_state.selected()
                            && index == i
                        {
                            true
                        } else {
                            false
                        };
                        if is_selected && is_editing_id {
                            lines.push(app.input_state.reverse());
                        } else {
                            lines.push(Line::from(format!(
                                "{} {}",
                                filter.id, filter.subgroup.name
                            )));
                        }
                        lines.push(Line::default());
                        if is_selected {
                            ListItem::new(lines).bold().reversed()
                        } else {
                            ListItem::new(lines)
                        }
                    })
                    .collect::<Vec<_>>();
                let list_block_title = "Filters";
                let symbol = if is_editing_id { "- " } else { "> " };
                let list = List::new(list_items)
                    .block(
                        Block::default()
                            .title(list_block_title)
                            .borders(Borders::ALL),
                    )
                    .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
                    .highlight_symbol(symbol);
                f.render_stateful_widget(list, id_area, &mut app.filter_id_state);
                if let Some(index) = app.filter_id_state.selected() {
                    let is_editing_rule = app.input_state.is_typing();
                    let detail_block = Block::default()
                        .title("Rules")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::LightBlue));
                    let list_items = app.filters[index]
                        .subgroup
                        .filter_list
                        .iter()
                        .enumerate()
                        .map(|(index, rule)| {
                            let mut lines = Vec::with_capacity(2);
                            let is_selected = if let Some(i) = app.filter_rule_state.selected()
                                && index == i
                            {
                                true
                            } else {
                                false
                            };
                            if is_selected && is_editing_rule {
                                lines.push(app.input_state.reverse());
                            } else {
                                lines.push(Line::from(rule.as_str()));
                            }
                            lines.push(Line::default());
                            if is_selected {
                                ListItem::new(lines).bold().reversed()
                            } else {
                                ListItem::new(lines)
                            }
                        })
                        .collect::<Vec<_>>();
                    let symbol = if is_editing_rule { "- " } else { "> " };
                    let list = List::new(list_items)
                        .block(detail_block)
                        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
                        .highlight_symbol(symbol);
                    f.render_stateful_widget(list, filter_area, &mut app.filter_rule_state);
                }
            }
            CurrentScreen::State => {
                let mut rows = Vec::with_capacity(WaiterKind::COUNT);
                for (i, state) in app.waiting_state.states.iter().enumerate() {
                    let status = if *state {
                        Text::raw("\nStopped").red()
                    } else {
                        Text::raw("\nWorking").green()
                    };
                    rows.push(
                        Row::new([Text::from(format!("\n{}", WaiterKind::VARIANTS[i])), status])
                            .height(2),
                    );
                }
                let header = Row::new(["Name", "Status"])
                    .style(Style::default().bold())
                    .height(1);
                let table = Table::default()
                    .rows(rows)
                    .header(header)
                    .block(Block::default().borders(Borders::ALL).title("Services"))
                    .widths([Constraint::Percentage(40), Constraint::Fill(1)]);
                f.render_widget(table, tab_content_area);
            }
            CurrentScreen::Log => {
                let logs = tui_logger::TuiLoggerWidget::default()
                    .block(Block::default().title("Logs").borders(Borders::ALL))
                    .state(&app.log_widget_state);
                f.render_widget(logs, tab_content_area);
            }
        }
        if let Some(popup) = &mut app.current_popup {
            let vertical_layout = Layout::vertical([
                Constraint::Percentage(10),
                Constraint::Percentage(70),
                Constraint::Percentage(20),
            ])
            .split(tab_content_area);
            let horizontal_layout = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .split(vertical_layout[1]);
            let popup_area = horizontal_layout[1];
            match popup {
                Popup::DownloadFolder => {
                    let input_widget = InputWidget::new(
                        "Download a Folder",
                        "Please enter the cid of the folder",
                        &app.input_state,
                        2,
                    );
                    f.render_widget(input_widget, popup_area);
                }
                Popup::AddRSSLink => {
                    let input_widget = InputWidget::new(
                        "Add a RSS Link",
                        "Please enter the RSS link",
                        &app.input_state,
                        2,
                    );
                    f.render_widget(input_widget, popup_area);
                }
                Popup::Login => {
                    let vertical_layout = Layout::vertical([
                        Constraint::Fill(1),
                        Constraint::Min(22),
                        Constraint::Fill(1),
                    ])
                    .split(tab_content_area);
                    let horizontal_layout = Layout::horizontal([
                        Constraint::Fill(1),
                        Constraint::Min(41),
                        Constraint::Fill(1),
                    ])
                    .split(vertical_layout[1]);
                    let popup_area = horizontal_layout[1];
                    let qrcode = qrcode_widget::QrcodeWidget::new(&app.qrcode_url);
                    // clear the screen first
                    f.render_widget(Clear, popup_area.outer(Margin::new(1, 0)));
                    f.render_widget(qrcode, popup_area);
                }
                Popup::Confirm(confirm) => {
                    // clear the screen first
                    f.render_widget(Clear, popup_area.outer(Margin::new(1, 0)));
                    f.render_stateful_widget(ConfirmWidget, popup_area, confirm);
                }
            }
        }
        if let Some(noti) = app.notifications_queue.front_mut() {
            f.render_stateful_widget(NotificationWidget, tab_content_area, noti);
            if noti.should_disappear() {
                app.notifications_queue.pop_front();
            }
        }
    })?;
    Ok(())
}

pub trait OutterRect {
    fn outer(&self, margin: Margin) -> Rect;
}

impl OutterRect for Rect {
    fn outer(&self, margin: Margin) -> Rect {
        Rect {
            x: self.x.saturating_sub(margin.horizontal),
            y: self.y.saturating_sub(margin.vertical),
            width: self.width.saturating_add(margin.horizontal * 2),
            height: self.height.saturating_add(margin.vertical * 2),
        }
    }
}
