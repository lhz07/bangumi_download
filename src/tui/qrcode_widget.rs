use qrcode::{Color as QColor, QrCode};
use ratatui::{
    symbols::border,
    widgets::{Block, Paragraph, Widget, Wrap},
};

pub struct QrcodeWidget<'a> {
    url: &'a Result<Box<str>, &'a str>,
}

impl<'a> QrcodeWidget<'a> {
    pub fn new(url: &'a Result<Box<str>, &'a str>) -> Self {
        QrcodeWidget { url }
    }
}

impl<'a> Widget for QrcodeWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let block = Block::bordered()
            .title("login qrcode")
            .title_alignment(ratatui::layout::Alignment::Center)
            .border_set(border::THICK);
        let para = match self.url {
            Ok(url) => {
                let code = QrCode::new(url.as_bytes())
                    .map_err(|e| format!("{e}"))
                    .unwrap();
                let width = code.width();
                let mut output = String::new();
                let quiet_zone = 1;
                let actual_size = width + 2 * quiet_zone;

                for y in (0..actual_size).step_by(2) {
                    for x in 0..actual_size {
                        let top = if x >= quiet_zone
                            && y >= quiet_zone
                            && x < width + quiet_zone
                            && y < width + quiet_zone
                        {
                            code[(x - quiet_zone, y - quiet_zone)] == QColor::Light
                        } else {
                            true
                        };
                        let bottom = if x >= quiet_zone
                            && y + 1 >= quiet_zone
                            && x < width + quiet_zone
                            && y + 1 < width + quiet_zone
                        {
                            code[(x - quiet_zone, y + 1 - quiet_zone)] == QColor::Light
                        }
                        // if y is already the last line, don't draw its bottom line
                        else if y == actual_size - 1 {
                            false
                        } else {
                            true
                        };
                        let ch = match (top, bottom) {
                            (true, true) => '█',
                            (true, false) => '▀',
                            (false, true) => '▄',
                            (false, false) => ' ',
                        };
                        output.push(ch);
                    }
                    output.push('\n');
                }
                Paragraph::new(output).centered().block(block)
            }
            Err(e) => Paragraph::new(*e).block(block).wrap(Wrap { trim: true }),
        };
        para.render(area, buf);
    }
}
