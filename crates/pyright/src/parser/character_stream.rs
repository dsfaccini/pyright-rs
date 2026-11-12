//! Port of `packages/pyright-internal/src/parser/characterStream.ts`.

use super::characters::{is_line_break, is_white_space};
use crate::common::char_codes::Char;

// parser/characterStream.ts:16
pub struct CharacterStream<'a> {
    _text: &'a [u16],
    _position: i32,
    _current_char: u16,
    _is_end_of_stream: bool,
}

impl<'a> CharacterStream<'a> {
    // parser/characterStream.ts:22
    pub fn new(text: &'a [u16]) -> CharacterStream<'a> {
        CharacterStream {
            _text: text,
            _position: 0,
            _current_char: if !text.is_empty() { text[0] } else { 0 },
            _is_end_of_stream: text.is_empty(),
        }
    }

    // parser/characterStream.ts:29
    pub fn position(&self) -> i32 {
        self._position
    }

    // parser/characterStream.ts:33
    pub fn set_position(&mut self, value: i32) {
        self._position = value;
        self._check_bounds();
    }

    // parser/characterStream.ts:38
    pub fn current_char(&self) -> u16 {
        self._current_char
    }

    // parser/characterStream.ts:42
    pub fn next_char(&self) -> u16 {
        if self.position() + 1 < self._text.len() as i32 {
            self._text[(self.position() + 1) as usize]
        } else {
            0
        }
    }

    // parser/characterStream.ts:46
    #[allow(clippy::int_plus_one)]
    pub fn prev_char(&self) -> u16 {
        if self.position() - 1 >= 0 {
            self._text[(self.position() - 1) as usize]
        } else {
            0
        }
    }

    // parser/characterStream.ts:50
    pub fn length(&self) -> i32 {
        self._text.len() as i32
    }

    // parser/characterStream.ts:54
    pub fn get_text(&self) -> &'a [u16] {
        self._text
    }

    // parser/characterStream.ts:63
    pub fn get_current_char(&self) -> u16 {
        self._current_char
    }

    // parser/characterStream.ts:67
    pub fn is_end_of_stream(&self) -> bool {
        self._is_end_of_stream
    }

    // parser/characterStream.ts:71
    pub fn look_ahead(&self, offset: i32) -> u16 {
        let pos = self._position + offset;
        if pos < 0 || pos >= self._text.len() as i32 {
            0
        } else {
            self._text[pos as usize]
        }
    }

    // parser/characterStream.ts:76
    pub fn advance(&mut self, offset: i32) {
        self.set_position(self.position() + offset);
    }

    // parser/characterStream.ts:80
    pub fn move_next(&mut self) -> bool {
        if self._position < self._text.len() as i32 - 1 {
            // Most common case, no need to check bounds extensively
            self._position += 1;
            self._current_char = self._text[self._position as usize];
            return true;
        }
        self.advance(1);
        !self.is_end_of_stream()
    }

    // parser/characterStream.ts:91
    pub fn is_at_white_space(&self) -> bool {
        is_white_space(self.current_char())
    }

    // parser/characterStream.ts:95
    pub fn is_at_line_break(&self) -> bool {
        is_line_break(self.current_char())
    }

    // parser/characterStream.ts:99
    pub fn skip_line_break(&mut self) {
        if self._current_char == Char::CarriageReturn {
            self.move_next();
            if self.current_char() == Char::LineFeed {
                self.move_next();
            }
        } else if self._current_char == Char::LineFeed {
            self.move_next();
        }
    }

    // parser/characterStream.ts:110
    pub fn skip_whitespace(&mut self) {
        let text = self._text;
        let len = text.len() as i32;
        let mut pos = self._position;
        while pos < len {
            let ch = text[pos as usize];
            if ch == Char::Space || ch == Char::Tab || ch == Char::FormFeed {
                pos += 1;
            } else {
                break;
            }
        }
        if pos != self._position {
            self._position = pos;
            if pos >= len {
                self._is_end_of_stream = true;
                self._position = len;
                self._current_char = 0;
            } else {
                self._current_char = text[pos as usize];
            }
        }
    }

    // parser/characterStream.ts:138
    pub fn skip_to_eol(&mut self) {
        // `move_next` until a line break or the end, in one step.
        let run = self._text[self._position as usize..].iter().take_while(|&&c| !is_line_break(c)).count();
        if run > 0 {
            self.advance(run as i32);
        }
    }

    // parser/characterStream.ts:144
    pub fn skip_to_whitespace(&mut self) {
        while !self.is_end_of_stream() && !self.is_at_white_space() {
            self.move_next();
        }
    }

    /// `None` is the `NaN` that `charCodeAt` returns past the end.
    // parser/characterStream.ts:150
    pub fn char_code_at(&self, index: i32) -> Option<u16> {
        if index >= 0 && (index as usize) < self._text.len() {
            Some(self._text[index as usize])
        } else {
            None
        }
    }

    // parser/characterStream.ts:154
    fn _check_bounds(&mut self) {
        if self._position < 0 {
            self._position = 0;
        }

        self._is_end_of_stream = self._position >= self._text.len() as i32;
        if self._is_end_of_stream {
            self._position = self._text.len() as i32;
        }

        self._current_char = if self._is_end_of_stream {
            0
        } else {
            self._text[self._position as usize]
        };
    }
}
