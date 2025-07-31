use qrcode::{Color as QColor, QrCode};
use ratatui::{
    symbols::border,
    widgets::{Block, Paragraph, Widget},
};

pub struct QrcodeWidget {}

impl QrcodeWidget {
    pub fn new() -> Self {
        QrcodeWidget {}
    }
}

impl Widget for QrcodeWidget {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let block = Block::bordered()
            .title("login qrcode")
            .title_alignment(ratatui::layout::Alignment::Center)
            .border_set(border::THICK);
        let qrcode = "http://115.com/scan/dg-66129bc7909b60de61adfeb44a91a469a624cb4c";
        let code = QrCode::new(qrcode.as_bytes())
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
        let paragraph = Paragraph::new(output).centered().block(block);
        paragraph.render(area, buf);
    }
}
