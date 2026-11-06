//! Light and dark color palettes for the window chrome and the editor.

use super::gfx::Color;
use crate::syntax::Tok;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub dark: bool,
    pub window: Color,
    pub text: Color,
    pub text_dim: Color,
    pub border: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub hover: Color,
    pub pressed: Color,
    pub tab_bar: Color,
    pub tab: Color,
    pub tab_active: Color,
    pub field: Color,
    pub field_border: Color,
    pub status: Color,
    pub notice_info: Color,
    pub notice_warn: Color,
    pub popup: Color,
    pub editor: Color,
    pub gutter: Color,
    pub line_no: Color,
    pub line_no_current: Color,
    pub current_line: Color,
    pub selection: Color,
    pub match_bg: Color,
    pub match_border: Color,
    pub bracket: Color,
    pub whitespace: Color,
    pub ruler: Color,
    pub caret: Color,
    pub scrollbar: Color,
    pub fold: Color,
    pub error: Color,
    syntax: [Color; 24],
}

fn tok_index(t: Tok) -> usize {
    match t {
        Tok::Text => 0,
        Tok::Keyword => 1,
        Tok::Type => 2,
        Tok::Constant => 3,
        Tok::Comment => 4,
        Tok::String => 5,
        Tok::Number => 6,
        Tok::Preproc => 7,
        Tok::Punct => 8,
        Tok::Function => 9,
        Tok::Variable => 10,
        Tok::Meta => 11,
        Tok::Tag => 12,
        Tok::Attr => 13,
        Tok::Entity => 14,
        Tok::Heading => 15,
        Tok::Emphasis => 16,
        Tok::Strong => 17,
        Tok::Code => 18,
        Tok::Link => 19,
        Tok::Key => 20,
        Tok::Section => 21,
        Tok::Inserted => 22,
        Tok::Deleted => 23,
    }
}

impl Theme {
    pub fn tok(&self, t: Tok) -> Color {
        self.syntax[tok_index(t)]
    }

    pub fn new(dark: bool, accent: Color) -> Theme {
        let c = Color::rgb;
        if dark {
            let accent_fg = accent.mix(c(0xFFFFFF), 0.35);
            Theme {
                dark,
                window: c(0x202020),
                text: c(0xE4E4E4),
                text_dim: c(0x9A9A9A),
                border: c(0x3A3A3A),
                accent: accent_fg,
                accent_text: c(0x000000),
                hover: Color::rgba(0xFFFFFF, 0x14),
                pressed: Color::rgba(0xFFFFFF, 0x0A),
                tab_bar: c(0x202020),
                tab: c(0x2A2A2A),
                tab_active: c(0x1E1E1E),
                field: c(0x2B2B2B),
                field_border: c(0x4A4A4A),
                status: c(0x252526),
                notice_info: c(0x1F3A52),
                notice_warn: c(0x4D3A12),
                popup: c(0x2C2C2C),
                editor: c(0x1E1E1E),
                gutter: c(0x1E1E1E),
                line_no: c(0x6E7681),
                line_no_current: c(0xCCCCCC),
                current_line: c(0x2A2D2E),
                selection: c(0x264F78),
                match_bg: Color::rgba(0xEA5C00, 0x55),
                match_border: c(0xB0753A),
                bracket: c(0x888888),
                whitespace: c(0x4A4A4A),
                ruler: c(0x333333),
                caret: c(0xF0F0F0),
                scrollbar: Color::rgba(0xFFFFFF, 0x30),
                fold: c(0x8A8A8A),
                error: c(0xF48771),
                syntax: [
                    c(0xD4D4D4), // text
                    c(0x569CD6), // keyword
                    c(0x4EC9B0), // type
                    c(0x4FC1FF), // constant
                    c(0x6A9955), // comment
                    c(0xCE9178), // string
                    c(0xB5CEA8), // number
                    c(0xC586C0), // preproc
                    c(0xD4D4D4), // punct
                    c(0xDCDCAA), // function
                    c(0x9CDCFE), // variable
                    c(0xD7BA7D), // meta
                    c(0x569CD6), // tag
                    c(0x9CDCFE), // attr
                    c(0xD7BA7D), // entity
                    c(0x569CD6), // heading
                    c(0xC5A5E5), // emphasis
                    c(0xF0C674), // strong
                    c(0xCE9178), // code
                    c(0x4DAAFC), // link
                    c(0x9CDCFE), // key
                    c(0xC586C0), // section
                    c(0x89D185), // inserted
                    c(0xF48771), // deleted
                ],
            }
        } else {
            let accent_fg = accent.mix(c(0x000000), 0.1);
            Theme {
                dark,
                window: c(0xF3F3F3),
                text: c(0x1B1B1B),
                text_dim: c(0x616161),
                border: c(0xDADADA),
                accent: accent_fg,
                accent_text: c(0xFFFFFF),
                hover: Color::rgba(0x000000, 0x0C),
                pressed: Color::rgba(0x000000, 0x16),
                tab_bar: c(0xF3F3F3),
                tab: c(0xE9E9E9),
                tab_active: c(0xFFFFFF),
                field: c(0xFFFFFF),
                field_border: c(0xC8C8C8),
                status: c(0xEDEDED),
                notice_info: c(0xDCEBFA),
                notice_warn: c(0xFFF1C9),
                popup: c(0xFBFBFB),
                editor: c(0xFFFFFF),
                gutter: c(0xFFFFFF),
                line_no: c(0x8A8A8A),
                line_no_current: c(0x1B1B1B),
                current_line: c(0xF2F5FA),
                selection: c(0xADD6FF),
                match_bg: Color::rgba(0xF5A623, 0x66),
                match_border: c(0xE0A030),
                bracket: c(0xA0A0A0),
                whitespace: c(0xC4C4C4),
                ruler: c(0xE6E6E6),
                caret: c(0x000000),
                scrollbar: Color::rgba(0x000000, 0x30),
                fold: c(0x7A7A7A),
                error: c(0xC42B1C),
                syntax: [
                    c(0x1B1B1B), // text
                    c(0x0000FF), // keyword
                    c(0x267F99), // type
                    c(0x0070C1), // constant
                    c(0x008000), // comment
                    c(0xA31515), // string
                    c(0x098658), // number
                    c(0xAF00DB), // preproc
                    c(0x1B1B1B), // punct
                    c(0x795E26), // function
                    c(0x001080), // variable
                    c(0x795E26), // meta
                    c(0x800000), // tag
                    c(0xE50000), // attr
                    c(0xC5060B), // entity
                    c(0x0451A5), // heading
                    c(0x6F42C1), // emphasis
                    c(0x9A6700), // strong
                    c(0xA31515), // code
                    c(0x0068B8), // link
                    c(0x0451A5), // key
                    c(0xAF00DB), // section
                    c(0x116329), // inserted
                    c(0xC42B1C), // deleted
                ],
            }
        }
    }
}
