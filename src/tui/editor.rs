use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Default)]
pub struct Editor {
    str: String,
    cursor: Cursor,
    /// ensure to invalidate it when the cursor changed
    insert_index: Option<usize>,
}

/// |what | | | | | | |
/// |-----|-|-|-|-|-|-|
/// |str  |H|e|l|l|o| |
/// |index|0|1|2|3|4|5|
///
/// index value: 0 ~ str.real_len()
///
/// |char   |  (begin, end)  |
/// |-------|----------------|
/// |'H'    |     (0, 0)     |
/// |'He'   |     (0, 1)     |
/// |'Hello'|(0, 4) or (0, 5)|
///
#[derive(Debug)]
enum Cursor {
    /// we should ensure that `index` is always valid
    Normal { index: usize },
    Select {
        begin: usize,
        end: usize,
        direction: Direction,
    },
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Left,
    Right,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::Normal { index: 0 }
    }
}

trait RealLen {
    fn real_len(&self) -> usize;
    fn grapheme_index_str(&self, g_index: usize) -> (usize, &str);
    fn grapheme_index_end(&self, g_index: usize) -> (usize, bool);
    fn grapheme_delta(&self, before: usize, after: usize) -> isize;
}

pub trait StrStyle<'a> {
    fn selection_style(self) -> Span<'a>;
}

impl<'a, T> StrStyle<'a> for T
where
    T: Into<&'a str>,
{
    fn selection_style(self) -> Span<'a> {
        self.into().bg(Color::DarkGray).not_reversed()
    }
}

impl RealLen for str {
    fn real_len(&self) -> usize {
        self.graphemes(true).count()
    }
    /// -> (byte_index: usize, string_at_the_index: &str)
    fn grapheme_index_str(&self, g_index: usize) -> (usize, &str) {
        self.grapheme_indices(true)
            .nth(g_index)
            .unwrap_or_else(|| (self.len(), ""))
    }
    /// -> (byte_index: usize, is_over_end: bool)
    fn grapheme_index_end(&self, g_index: usize) -> (usize, bool) {
        if g_index == 0 {
            return (0, false);
        }
        let mut graphs = self.grapheme_indices(true);
        match graphs.nth(g_index - 1) {
            Some(_) => graphs
                .next()
                .map(|(index, _)| (index, false))
                .unwrap_or_else(|| (self.len(), false)),
            None => (self.len(), true),
        }
    }

    /// before: the grapheme count
    ///
    /// after: the byte index
    fn grapheme_delta(&self, before: usize, after: usize) -> isize {
        let after = &self[..after];
        let g_after = after.graphemes(true).count();

        g_after as isize - before as isize
    }
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn new_with_text(str: String) -> Self {
        Self {
            str,
            ..Default::default()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.str.is_empty()
    }
    pub fn content_len(&self) -> usize {
        self.str.real_len()
    }
    pub fn into_string(self) -> String {
        self.str
    }
    pub fn as_str(&self) -> &str {
        &self.str
    }

    pub fn right_arrow(&mut self) {
        match self.cursor {
            // move cursor normally
            Cursor::Normal { ref mut index } => {
                if *index + 1 <= self.str.real_len() {
                    self.insert_index.take();
                    *index += 1
                }
            }
            // cancel the selection, and move cursor to the end of the selection
            // if the selection is begin == end, move the cursor toward right
            Cursor::Select { begin, mut end, .. } => {
                if begin == end && end + 1 <= self.str.real_len() {
                    end += 1;
                }
                self.cursor = Cursor::Normal { index: end };
            }
        }
    }
    pub fn left_arrow(&mut self) {
        match self.cursor {
            Cursor::Normal { ref mut index } => {
                if *index > 0 {
                    self.insert_index.take();
                    *index -= 1;
                    log::info!("current index: {}", *index);
                }
            }
            Cursor::Select { mut begin, end, .. } => {
                if begin == end && begin > 0 {
                    begin -= 1;
                }
                self.cursor = Cursor::Normal { index: begin };
            }
        }
    }
    fn swap_direction(&mut self) {
        if let Cursor::Select {
            begin,
            end,
            direction,
        } = &mut self.cursor
        {
            std::mem::swap(begin, end);
            *direction = match direction {
                Direction::Left => Direction::Right,
                Direction::Right => Direction::Left,
            };
        }
    }
    pub fn right_arrow_shift(&mut self) {
        match self.cursor {
            // change to select mode, and move right
            Cursor::Normal { index } => {
                if index + 1 <= self.str.real_len() {
                    self.insert_index.take();
                    self.cursor = Cursor::Select {
                        begin: index,
                        end: index + 1,
                        direction: Direction::Right,
                    }
                }
            }
            Cursor::Select {
                ref mut begin,
                ref mut end,
                direction,
            } => {
                match direction {
                    // move begin toward right
                    Direction::Left => {
                        if *begin + 1 <= self.str.real_len() {
                            *begin += 1;
                        }
                    }
                    // move end toward right
                    Direction::Right => {
                        if *end + 1 <= self.str.real_len() {
                            *end += 1;
                        }
                    }
                }
                if begin > end {
                    self.swap_direction();
                }
            }
        }
    }
    pub fn left_arrow_shift(&mut self) {
        log::info!("{:?}", self.cursor);
        match self.cursor {
            // change to select mode, and move left
            Cursor::Normal { index } => {
                if index > 0 {
                    self.insert_index.take();
                    self.cursor = Cursor::Select {
                        begin: index - 1,
                        end: index,
                        direction: Direction::Left,
                    }
                }
            }
            Cursor::Select {
                ref mut begin,
                ref mut end,
                direction,
            } => {
                match direction {
                    // move begin toward left
                    Direction::Left => {
                        if *begin > 0 {
                            *begin -= 1;
                        }
                    }
                    // move end toward left
                    Direction::Right => {
                        if *end > 0 {
                            *end -= 1;
                        }
                    }
                }
                if begin > end {
                    self.swap_direction();
                }
            }
        }
    }
    pub fn select_all(&mut self) {
        if !self.str.is_empty() {
            self.insert_index.take();
            self.cursor = Cursor::Select {
                begin: 0,
                end: self.str.real_len(),
                direction: Direction::Right,
            }
        }
    }
    pub fn backspace(&mut self) {
        if self.str.is_empty() {
            return;
        }
        match self.cursor {
            Cursor::Normal {
                index: ref mut cursor_index,
            } => {
                if *cursor_index == 0 {
                    return;
                }
                self.insert_index.take();
                let mut graphs = self.str.grapheme_indices(true);
                let (index_before_cursor, _) = graphs
                    .nth(*cursor_index - 1)
                    .unwrap_or_else(|| (self.str.len(), ""));
                let (index_after_cursor, _) = graphs.next().unwrap_or_else(|| (self.str.len(), ""));
                self.str.drain(index_before_cursor..index_after_cursor);
                *cursor_index -= 1;
            }
            Cursor::Select { begin, end, .. } => {
                let (begin_byte_index, _) = self.str.grapheme_index_str(begin);
                let (end_byte_index, _) = self.str.grapheme_index_str(end);
                if end_byte_index == self.str.len() {
                    self.str.drain(begin_byte_index..end_byte_index);
                } else {
                    self.str.drain(begin_byte_index..=end_byte_index);
                }
                self.cursor = Cursor::Normal { index: begin };
            }
        }
    }
    pub fn insert(&mut self, ch: char) {
        log::info!("insert: {}", ch);
        log::info!("current str: {:?}", self.str);
        match self.cursor {
            Cursor::Normal {
                index: ref mut cursor_index,
            } => match self.insert_index {
                Some(ref mut last_index) => {
                    self.str.insert(*last_index, ch);
                    let ch_len = ch.len_utf8();
                    let after_byte_index = *last_index + ch_len;
                    let delta = self.str.grapheme_delta(*cursor_index, after_byte_index);
                    // we insert a char, record it
                    // the insert_index may be different from cursor_index
                    // only clear it when cursor is moved
                    *last_index = after_byte_index;
                    if delta > 0 {
                        *cursor_index += delta as usize;
                    } else if delta < 0 {
                        *cursor_index -= -delta as usize;
                    }
                }
                None => {
                    let (before_byte_index, _) = self.str.grapheme_index_str(*cursor_index);
                    self.str.insert(before_byte_index, ch);
                    let ch_len = ch.len_utf8();
                    let after_byte_index = before_byte_index + ch_len;
                    let delta = self.str.grapheme_delta(*cursor_index, after_byte_index);
                    // we insert a char, record it
                    // the insert_index may be different from cursor_index
                    // only clear it when cursor is moved
                    self.insert_index = Some(after_byte_index);
                    if delta > 0 {
                        *cursor_index += delta as usize;
                    } else if delta < 0 {
                        *cursor_index -= -delta as usize;
                    }
                }
            },
            Cursor::Select { begin, end, .. } => {
                let (begin_byte_index, _) = self.str.grapheme_index_str(begin);
                let (end_byte_index, _) = self.str.grapheme_index_str(end);
                let new_str = ch.to_string();
                self.str
                    .replace_range(begin_byte_index..end_byte_index, &new_str);
                let after_byte_index = begin_byte_index + new_str.len();
                let delta = self.str.grapheme_delta(begin, after_byte_index);
                // we insert a char but the grapheme doesn't change, record it
                if delta == 0 {
                    self.insert_index = Some(after_byte_index);
                } else {
                    let index = if delta > 0 {
                        begin + (delta as usize)
                    } else {
                        begin - (-delta as usize)
                    };
                    self.cursor = Cursor::Normal { index }
                }
            }
        }
    }
    pub fn to_reversed_line(&self) -> Line<'_> {
        self.to_line()
    }
    pub fn to_line(&self) -> Line<'_> {
        match self.cursor {
            Cursor::Normal { index } => {
                if index == 0 && self.str.is_empty() {
                    return Line::from(" ".reversed());
                    // match self.str.graphemes(true).next() {
                    //     Some(str) => {
                    //         // the cursor is at 0 and the str is not empty
                    //         // get the first graph's vec len
                    //         let first_len = str.len();
                    //         let (cursor, after) = self.str.split_at(first_len);
                    //         return Text::from(Line::from(vec![
                    //             cursor.reversed(),
                    //             after.reversed(),
                    //             " ".not_reversed(),
                    //         ]));
                    //     }
                    //     None => {
                    //         // the str is empty, just render the cursor itself
                    //         return Text::from(" ").reversed();
                    //     }
                    // }
                }
                let (cursor_byte_index, mut cursor_str) = self.str.grapheme_index_str(index);
                let (before_cursor, other) = self.str.split_at(cursor_byte_index);
                let (_, after_cursor) = other.split_at(cursor_str.len());
                if cursor_str.is_empty() {
                    cursor_str = " ";
                }
                Line::from(vec![
                    before_cursor.not_reversed(),
                    cursor_str.reversed(),
                    after_cursor.not_reversed(),
                ])
            }
            Cursor::Select {
                begin,
                end,
                direction,
            } => match direction {
                Direction::Left => {
                    // 番 ｜剧 ｜ 下｜载 ｜器  ｜  |
                    // 0  |1  ｜ 2 |3  ｜4   |5 ｜
                    // 0..|3..｜6..|9..｜12..|15｜
                    //     ↑        ⇑            // example 1
                    //     ↑                     // example 2
                    // ------ example 1 ------
                    // selected: "剧下载"
                    // begin: 1
                    // cursor_byte_index: 3
                    // cursor_str: '剧'
                    // ------ example 2 ------
                    // selected: "剧"
                    // begin: 1
                    // cursor_byte_index: 3
                    // cursor_str: '剧'
                    // ------ example 3 ------
                    // selected: " "
                    // begin: 5
                    // cursor_byte_index: 15
                    // cursor_str: ' '
                    let (cursor_byte_index, mut cursor_str) = self.str.grapheme_index_str(begin);
                    // ------ example 1 ------
                    // before_cursor: '番'
                    // other: '剧下载器'
                    // ------ example 2 ------
                    // before_cursor: '番'
                    // other: '剧下载器'
                    let (before_cursor, other) = self.str.split_at(cursor_byte_index);
                    // ------ example 1 ------
                    // after_cursor: "下载器"
                    // cursor_str len: 3
                    // ------ example 2 ------
                    // after_cursor: "下载器"
                    // cursor_str len: 3
                    let (_, after_cursor) = other.split_at(cursor_str.len());
                    // ------ example 1 ------
                    // end: 3
                    // end - begin: 2
                    // selection_end_index: 6
                    // selection: "下载"
                    // after_selection: "器"
                    // ------ example 2 ------
                    // end: 1
                    // end - begin: 0
                    // selection_end_index: 0
                    // selection: ""
                    // after_selection: "下载器"
                    // ------ example 3 ------
                    // after_cursor: "下载器"
                    // begin: 1
                    // end: 5
                    // end - begin: 4
                    // selection_end_index: 0
                    // selection: ""
                    // after_selection: "下载器"
                    let (selection_end_index, is_over_end) =
                        after_cursor.grapheme_index_end(end - begin);
                    let (selection, after_selection) = if is_over_end {
                        (after_cursor, " ".selection_style())
                    } else {
                        let (selection, after_selection) =
                            after_cursor.split_at(selection_end_index);
                        (selection, after_selection.not_reversed())
                    };
                    if cursor_str.is_empty() {
                        cursor_str = " ";
                    }

                    Line::from(vec![
                        before_cursor.not_reversed(),
                        cursor_str.reversed(),
                        selection.selection_style(),
                        after_selection,
                    ])
                }
                Direction::Right => {
                    // 番 ｜剧 ｜ 下｜载 ｜器  ｜  |
                    // 0  |1  ｜ 2 |3  ｜4   |5 ｜
                    // 0..|3..｜6..|9..｜12..|15｜
                    //     ⇑        ↑            // example 1
                    //     ↑                     // example 2
                    // ------ example 1 ------
                    // selected: "剧下载"
                    // begin: 1
                    // begin_byte_index: 3
                    // begin_str: '载'
                    // ------ example 2 ------
                    // selected: "剧"
                    // begin: 1
                    // cursor_byte_index: 3
                    // cursor_str: '剧'
                    let (begin_index, _) = self.str.grapheme_index_str(begin);
                    // ------ example 1 ------
                    // before_begin: '番'
                    // other: '剧下载器'
                    // ------ example 2 ------
                    // before_cursor: '番'
                    // other: '剧下载器'
                    let (before_begin, other) = self.str.split_at(begin_index);
                    // ------ example 1 ------
                    // cursor(end): 3
                    // end - begin: 2
                    // cursor_byte_index: 9
                    // cursor_str: '载'
                    // ------ example 2 ------
                    // after_cursor: "下载器"
                    // cursor_str len: 3
                    let (cursor_byte_index, mut cursor_str) = other.grapheme_index_str(end - begin);
                    let (selection, after_seletion) = other.split_at(cursor_byte_index);
                    let (_, after_cursor) = after_seletion.split_at(cursor_str.len());
                    // ------ example 1 ------
                    // end: 3
                    // end - begin: 2
                    // selection_end_index: 6
                    // selection: "下载"
                    // after_selection: "器"
                    // ------ example 2 ------
                    // end: 1
                    // end - begin: 0
                    // selection_end_index: 0
                    // selection: ""
                    // after_selection: "下载器"
                    // ------ example 3 ------
                    // after_cursor: "下载器"
                    // begin: 1
                    // end: 5
                    // end - begin: 4
                    // selection_end_index: 0
                    // selection: ""
                    // after_selection: "下载器"
                    // let (selection_end_index, is_over_end) =
                    //     after_cursor.grapheme_index_end(end - begin);
                    // let (selection, after_selection) = if is_over_end {
                    //     (after_cursor, " ".selection_style())
                    // } else {
                    //     let (selection, after_selection) =
                    //         after_cursor.split_at(selection_end_index);
                    //     (selection, after_selection.not_reversed())
                    // };
                    if cursor_str.is_empty() {
                        cursor_str = " ";
                    }
                    Line::from(vec![
                        before_begin.not_reversed(),
                        selection.selection_style(),
                        cursor_str.reversed(),
                        after_cursor.not_reversed(),
                    ])
                }
            },
        }
    }
}

// TODO: add more tests
#[test]
fn test_selection() {
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 1,
            end: 3,
            direction: Direction::Left,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    let expect_text = Line::from(vec![
        "番".not_reversed(),
        "剧".reversed(),
        "下载".bg(Color::DarkGray).not_reversed(),
        "器".not_reversed(),
    ]);
    assert_eq!(text, expect_text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 1,
            end: 4,
            direction: Direction::Left,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("1-4: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 1,
            end: 5,
            direction: Direction::Left,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("1-5: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 4,
            end: 5,
            direction: Direction::Left,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("4-5: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 4,
            end: 4,
            direction: Direction::Left,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("4-4: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 5,
            end: 5,
            direction: Direction::Left,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("5-5: {:?}", text);
}

#[test]
fn test_right_cursor() {
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 1,
            end: 3,
            direction: Direction::Right,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("Right cursor: 1-3: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 1,
            end: 4,
            direction: Direction::Right,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("Right cursor: 1-4: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 1,
            end: 5,
            direction: Direction::Right,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("Right cursor: 1-5: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 4,
            end: 4,
            direction: Direction::Right,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("Right cursor: 4-4: {:?}", text);
    let editor = Editor {
        str: "番剧下载器".to_string(),
        cursor: Cursor::Select {
            begin: 5,
            end: 5,
            direction: Direction::Right,
        },
        ..Default::default()
    };
    let text = editor.to_line();
    println!("Right cursor: 5-5: {:?}", text);
}
