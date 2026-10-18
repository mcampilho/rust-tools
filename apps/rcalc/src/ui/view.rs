//! Window layout for every mode. Coordinates are computed from the window
//! size each frame, so all views resize fluidly.

use super::gfx::{Align, Color, FontId, Rect};
use super::icons::Icon;
use super::{Button, Content, Role, Style, Ui, Widget};
use crate::app::{Action, App, ComboId, MemOp, Mode, Overlay, PanelTab, ScrollId, ThemePref};
use crate::calc::programmer::{Radix, ShiftMode};
use crate::calc::{Engine, Key};
use crate::date::{self, Date};
use crate::expr::{BinOp, Constant, Unary};
use crate::i18n::{self, dates, num, tf, tr};
use crate::num::func::{Angle, Trig};

/// Positions of controls that overlays attach to.
#[derive(Default)]
struct Anchors {
    trig: Rect,
    func: Rect,
    bitwise: Rect,
    shift: Rect,
    combo: [Rect; 7],
    cal: [Rect; 2],
    /// Area covered by the history/memory flyout.
    flyout_area: Rect,
}

/// Width (logical px) at which the history/memory panel docks on the right.
pub const DOCK_WIDTH: f32 = 560.0;
pub const MIN_WIDTH: f64 = 320.0;
pub const MIN_HEIGHT: f64 = 500.0;

pub fn build(app: &App, ui: &mut Ui) {
    let t = ui.t;
    ui.g.clear(t.bg);
    let full = Rect::new(0.0, 0.0, ui.width(), ui.height());
    let logical_w = full.w / ui.s;
    let mut a = Anchors::default();
    let docked = app.is_calculator() && !app.keep_on_top && logical_w >= DOCK_WIDTH;
    let mut main = full;
    let panel = docked.then(|| main.take_right(ui.px((logical_w * 0.42).min(360.0))));
    match app.mode {
        Mode::Standard => standard(app, ui, main, &mut a, docked),
        Mode::Scientific => scientific(app, ui, main, &mut a, docked),
        Mode::Programmer => programmer(app, ui, main, &mut a, docked),
        Mode::Date => date_view(app, ui, main, &mut a),
        Mode::Converter(_) => converter(app, ui, main, &mut a),
        Mode::Settings => settings(app, ui, main, &mut a),
    }
    if let Some(p) = panel {
        side_panel(app, ui, p);
    }
    ui.layer = 1;
    overlays(app, ui, &a, full);
    if let Some(msg) = &app.toast {
        let tw = ui.g.measure(msg, ui.fs(14.0), FontId::Regular) + ui.px(32.0);
        let r = Rect::new(((full.w - tw) / 2.0).round(), full.h - ui.px(64.0), tw, ui.px(36.0));
        ui.flyout(r);
        ui.text(msg, r, 14.0, FontId::Regular, t.text, Align::Center);
        ui.label(r, msg, Role::Label);
    }
    ui.layer = 0;
}

// ---------------------------------------------------------------- shared parts

fn header(app: &App, ui: &mut Ui, r: Rect, docked: bool) {
    let t = ui.t;
    let btn = Rect::new(r.x + ui.px(4.0), r.y + ui.px(4.0), ui.px(40.0), ui.px(36.0));
    ui.button(Button::new(
        Action::OpenNav,
        btn,
        "Open navigation",
        Content::Icon(Icon::Menu, 16.0),
        Style::Subtle,
    ));
    let title = app.mode_title();
    let tr = Rect::new(btn.right() + ui.px(8.0), btn.y, r.w, btn.h);
    let tw = ui.text(title, tr, 20.0, FontId::Semibold, t.text, Align::Left);
    ui.label(Rect::new(tr.x, tr.y, tw, tr.h), title, Role::Label);
    if app.mode == Mode::Standard {
        let kr = Rect::new(tr.x + tw + ui.px(8.0), btn.y, ui.px(40.0), btn.h);
        ui.button(Button::new(
            Action::KeepOnTop(true),
            kr,
            "Keep on top",
            Content::Icon(Icon::KeepOnTop, 14.0),
            Style::Subtle,
        ));
    }
    if app.has_history() && !docked {
        let hr = Rect::new(r.right() - ui.px(44.0), btn.y, ui.px(40.0), btn.h);
        ui.button(Button::new(
            Action::Open(Overlay::Panel(PanelTab::History)),
            hr,
            "Open history flyout",
            Content::Icon(Icon::History, 16.0),
            Style::Subtle,
        ));
    }
}

fn calc_display(ui: &mut Ui, e: &Engine, r: Rect, notice: Option<&str>) {
    let t = ui.t;
    let pad = ui.px(12.0);
    let mut rr = Rect::new(r.x + pad, r.y, r.w - 2.0 * pad, r.h);
    let er = rr.take_top(ui.px(26.0));
    let expr = num(&e.expression_text());
    if let Some(n) = notice {
        let nr = Rect::new(er.x, er.y, er.w, er.h);
        ui.text(n, nr, 12.0, FontId::Semibold, t.accent_fg, Align::Left);
        ui.label(nr, n, Role::Label);
    }
    ui.fit_text(&expr, er, 14.0, 14.0, FontId::Regular, t.text_secondary, Align::Right);
    if !expr.is_empty() {
        ui.label(er, &tf("Expression is {}", &[&expr]), Role::Label);
    }
    let result = if e.error().is_some() {
        tr(&e.display_text()).to_string()
    } else {
        num(&e.display_text())
    };
    let max = (rr.h / ui.s * 0.62).clamp(28.0, 72.0);
    let max = if e.error().is_some() { max.min(32.0) } else { max };
    ui.fit_text(&result, rr, max, 12.0, FontId::Semibold, t.text, Align::Right);
    ui.label(rr, &tf("Display is {}", &[&result]), Role::Display);
}

fn memory_row(app: &App, ui: &mut Ui, r: Rect, e: &Engine, list_button: bool, only_store: bool) {
    let has = !app.memory.is_empty();
    let ok = e.error().is_none();
    let mut items: Vec<(&str, &str, Action, bool)> = Vec::new();
    if !only_store {
        items.push(("MC", "Clear all memory", Action::Mem(MemOp::Clear), has));
        items.push(("MR", "Memory recall", Action::Mem(MemOp::Recall), has));
        items.push(("M+", "Memory add", Action::Mem(MemOp::Add), ok));
        items.push(("M-", "Memory subtract", Action::Mem(MemOp::Sub), ok));
    }
    items.push(("MS", "Memory store", Action::Mem(MemOp::Store), ok));
    if list_button {
        items.push(("M▾", "Open memory flyout", Action::Open(Overlay::Panel(PanelTab::Memory)), has));
    }
    let n = items.len();
    for (i, (txt, label, action, enabled)) in items.into_iter().enumerate() {
        let cell = r.cell(n, 1, i, 0, ui.px(2.0));
        ui.button(Button::new(action, cell, label, Content::Text(txt, 12.0, FontId::Semibold), Style::Subtle).enabled(enabled));
    }
}

/// One keypad key.
struct K {
    text: String,
    label: &'static str,
    action: Action,
    style: Style,
    enabled: bool,
    /// `logᵧx`-style rendering: (base, subscript, tail).
    sub: Option<(String, String, String)>,
    icon: Option<Icon>,
}

fn k(text: &str, label: &'static str, key: Key, style: Style) -> K {
    K {
        text: text.to_string(),
        label,
        action: Action::Key(key),
        style,
        enabled: true,
        sub: None,
        icon: None,
    }
}

fn ka(text: &str, label: &'static str, action: Action, style: Style) -> K {
    K {
        text: text.to_string(),
        label,
        action,
        style,
        enabled: true,
        sub: None,
        icon: None,
    }
}

fn digit(d: u8) -> K {
    const NAMES: [&str; 16] = [
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "A", "B", "C", "D", "E", "F",
    ];
    let text = std::char::from_digit(d as u32, 16).unwrap().to_ascii_uppercase().to_string();
    let style = if d < 10 { Style::Num } else { Style::Op };
    k(&text, NAMES[d as usize], Key::Digit(d), style)
}

fn grid(ui: &mut Ui, r: Rect, cols: usize, keys: Vec<K>) {
    let rows = keys.len().div_ceil(cols);
    let gap = ui.px(2.0);
    let cell_h = (r.h - gap * (rows as f32 - 1.0)) / rows as f32;
    let cell_w = (r.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let size = (cell_h.min(cell_w * 0.8) / ui.s * 0.36).clamp(12.0, 24.0);
    for (i, key) in keys.into_iter().enumerate() {
        let cell = r.cell(cols, rows, i % cols, i / cols, gap);
        if key.label.is_empty() {
            continue;
        }
        let font = if key.style == Style::Num {
            FontId::Semibold
        } else {
            FontId::Regular
        };
        let content = if let Some(icon) = key.icon {
            Content::Icon(icon, size * 0.8)
        } else if let Some((a, b, c)) = &key.sub {
            Content::Sub(a, b, c, size * 0.9)
        } else {
            let n = key.text.chars().count();
            // Single operator glyphs (÷ × − + =) read small in Segoe UI; enlarge them.
            let sz = if n > 3 {
                size * 0.85
            } else if n == 1 && key.style != Style::Num && "÷×−+=".contains(key.text.as_str()) {
                size * 1.25
            } else {
                size
            };
            Content::Text(&key.text, sz, font)
        };
        ui.button(Button::new(key.action, cell, key.label, content, key.style).enabled(key.enabled));
    }
}

fn op(sym: &str, label: &'static str, b: BinOp) -> K {
    k(sym, label, Key::Bin(b), Style::Op)
}

fn un(sym: &str, label: &'static str, u: Unary) -> K {
    k(sym, label, Key::Unary(u), Style::Op)
}

fn backspace() -> K {
    let mut key = k("⌫", "Backspace", Key::Back, Style::Op);
    key.icon = Some(Icon::Backspace);
    key
}

fn clear_key(e: &Engine) -> K {
    if e.is_typing() {
        k("CE", "Clear entry", Key::ClearEntry, Style::Op)
    } else {
        k("C", "Clear", Key::Clear, Style::Op)
    }
}

/// While an error is shown, only keys that start over stay enabled.
fn apply_error_state(keys: &mut [K], e: &Engine) {
    if e.error().is_none() {
        return;
    }
    for key in keys {
        let keep = matches!(
            key.action,
            Action::Key(Key::Digit(_) | Key::Point | Key::Clear | Key::ClearEntry | Key::Back | Key::Equals)
        );
        if !keep {
            key.enabled = false;
        }
    }
}

// ---------------------------------------------------------------- standard

fn standard(app: &App, ui: &mut Ui, r: Rect, a: &mut Anchors, docked: bool) {
    let e = &app.std;
    let mut r = r;
    if app.keep_on_top {
        let top = r.take_top(ui.px(40.0));
        let br = Rect::new(top.right() - ui.px(44.0), top.y + ui.px(2.0), ui.px(40.0), ui.px(36.0));
        ui.button(Button::new(
            Action::KeepOnTop(false),
            br,
            "Back to full view",
            Content::Icon(Icon::BackToFull, 14.0),
            Style::Subtle,
        ));
        let disp_h = (r.h * 0.3).max(ui.px(72.0));
        calc_display(ui, e, r.take_top(disp_h), None);
    } else {
        header(app, ui, r.take_top(ui.px(44.0)), docked);
        let disp_h = (r.h * 0.27).clamp(ui.px(96.0), ui.px(220.0));
        calc_display(ui, e, r.take_top(disp_h), None);
        a.flyout_area = r;
        let mr = r.take_top(ui.px(34.0));
        memory_row(
            app,
            ui,
            Rect::new(mr.x + ui.px(4.0), mr.y, mr.w - ui.px(8.0), mr.h),
            e,
            !docked,
            false,
        );
    }
    let pad = ui.px(4.0);
    let pad_r = Rect::new(r.x + pad, r.y + ui.px(2.0), r.w - 2.0 * pad, r.h - pad - ui.px(2.0));
    let mut keys = vec![
        k("%", "Percent", Key::Percent, Style::Op),
        k("CE", "Clear entry", Key::ClearEntry, Style::Op),
        k("C", "Clear", Key::Clear, Style::Op),
        backspace(),
        un("¹/x", "Reciprocal", Unary::Recip),
        un("x²", "Square", Unary::Square),
        un("²√x", "Square root", Unary::Sqrt),
        op("÷", "Divide by", BinOp::Div),
        digit(7),
        digit(8),
        digit(9),
        op("×", "Multiply by", BinOp::Mul),
        digit(4),
        digit(5),
        digit(6),
        op("−", "Minus", BinOp::Sub),
        digit(1),
        digit(2),
        digit(3),
        op("+", "Plus", BinOp::Add),
        k("+/−", "Positive negative", Key::Negate, Style::Num),
        digit(0),
        k(i18n::decimal_point(), "Decimal separator", Key::Point, Style::Num),
        k("=", "Equals", Key::Equals, Style::Accent),
    ];
    apply_error_state(&mut keys, e);
    grid(ui, pad_r, 4, keys);
}

// ---------------------------------------------------------------- scientific

fn scientific(app: &App, ui: &mut Ui, r: Rect, a: &mut Anchors, docked: bool) {
    let e = &app.sci;
    let t = ui.t;
    let mut r = r;
    header(app, ui, r.take_top(ui.px(44.0)), docked);
    let disp_h = (r.h * 0.22).clamp(ui.px(88.0), ui.px(200.0));
    calc_display(ui, e, r.take_top(disp_h), None);
    a.flyout_area = r;
    let pad = ui.px(4.0);
    // DEG / F-E
    let row = r.take_top(ui.px(32.0));
    let angle = match e.angle {
        Angle::Deg => "DEG",
        Angle::Rad => "RAD",
        Angle::Grad => "GRAD",
    };
    let angle_label = match e.angle {
        Angle::Deg => "Degrees, switch to radians",
        Angle::Rad => "Radians, switch to gradians",
        Angle::Grad => "Gradians, switch to degrees",
    };
    let c0 = Rect::new(row.x + pad, row.y, ui.px(64.0), row.h);
    ui.button(Button::new(
        Action::CycleAngle,
        c0,
        angle_label,
        Content::Text(angle, 12.0, FontId::Semibold),
        Style::Subtle,
    ));
    let c1 = Rect::new(c0.right() + ui.px(2.0), row.y, ui.px(56.0), row.h);
    ui.button(
        Button::new(
            Action::ToggleFE,
            c1,
            "Scientific notation (F-E)",
            Content::Text("F-E", 12.0, FontId::Semibold),
            Style::Subtle,
        )
        .role(Role::Toggle)
        .checked(e.fe),
    );
    if e.fe {
        ui.g.fill_rect(
            Rect::new(c1.x + ui.px(14.0), c1.bottom() - ui.px(4.0), c1.w - ui.px(28.0), ui.px(2.0)),
            t.accent_fg,
        );
    }
    let mr = r.take_top(ui.px(32.0));
    memory_row(app, ui, Rect::new(mr.x + pad, mr.y, mr.w - 2.0 * pad, mr.h), e, !docked, false);
    // Trigonometry / Function openers
    let row = r.take_top(ui.px(40.0));
    let half = ((row.w - 2.0 * pad) / 2.0).min(ui.px(200.0));
    a.trig = Rect::new(row.x + pad, row.y + ui.px(2.0), half, row.h - ui.px(4.0));
    a.func = Rect::new(a.trig.right() + ui.px(2.0), a.trig.y, half, a.trig.h);
    let err = e.error().is_some();
    ui.button(
        Button::new(
            Action::Open(Overlay::Trig),
            a.trig,
            "Trigonometry",
            Content::Dropdown(Some(Icon::Triangle), "Trigonometry", 14.0),
            Style::Subtle,
        )
        .enabled(!err),
    );
    ui.button(
        Button::new(
            Action::Open(Overlay::Func),
            a.func,
            "Function",
            Content::Dropdown(Some(Icon::Function), "Function", 14.0),
            Style::Subtle,
        )
        .enabled(!err),
    );
    let pad_r = Rect::new(r.x + pad, r.y + ui.px(2.0), r.w - 2.0 * pad, r.h - pad - ui.px(2.0));
    let s2 = app.sci_2nd;
    let mut second = ka(
        "2ⁿᵈ",
        "Second functions",
        Action::Toggle2nd,
        if s2 { Style::Accent } else { Style::Op },
    );
    second.text = "2nd".into();
    let paren_count = e.open_parens().to_string();
    let mut logy = un("logᵧx", "Log base y", Unary::Log);
    if s2 {
        logy = op("logᵧx", "Log base y", BinOp::LogBase);
        logy.sub = Some(("log".into(), "y".into(), "x".into()));
    }
    let mut keys = vec![
        second,
        k("π", "Pi", Key::Const(Constant::Pi), Style::Op),
        k("e", "Euler's number", Key::Const(Constant::E), Style::Op),
        clear_key(e),
        backspace(),
        if s2 {
            un("x³", "Cube", Unary::Cube)
        } else {
            un("x²", "Square", Unary::Square)
        },
        un("¹/x", "Reciprocal", Unary::Recip),
        un("|x|", "Absolute value", Unary::Abs),
        k("exp", "Exponential notation", Key::Exp, Style::Op),
        op("mod", "Modulo", BinOp::Mod),
        if s2 {
            un("∛x", "Cube root", Unary::Cbrt)
        } else {
            un("²√x", "Square root", Unary::Sqrt)
        },
        {
            let mut lp = k("(", "Left parenthesis", Key::LParen, Style::Op);
            if e.open_parens() > 0 {
                // Open-parenthesis count shown as a subscript, like Windows.
                lp.sub = Some(("(".into(), paren_count.clone(), String::new()));
            }
            lp
        },
        k(")", "Right parenthesis", Key::RParen, Style::Op),
        un("n!", "Factorial", Unary::Fact),
        op("÷", "Divide by", BinOp::Div),
        if s2 {
            op("ʸ√x", "Y root of x", BinOp::Root)
        } else {
            op("xʸ", "X to the exponent", BinOp::Pow)
        },
        digit(7),
        digit(8),
        digit(9),
        op("×", "Multiply by", BinOp::Mul),
        if s2 {
            un("2ˣ", "Two to the exponent", Unary::Pow2)
        } else {
            un("10ˣ", "Ten to the exponent", Unary::Pow10)
        },
        digit(4),
        digit(5),
        digit(6),
        op("−", "Minus", BinOp::Sub),
        if s2 { logy } else { un("log", "Log", Unary::Log) },
        digit(1),
        digit(2),
        digit(3),
        op("+", "Plus", BinOp::Add),
        if s2 {
            un("eˣ", "E to the exponent", Unary::Exp)
        } else {
            un("ln", "Natural log", Unary::Ln)
        },
        k("+/−", "Positive negative", Key::Negate, Style::Num),
        digit(0),
        k(i18n::decimal_point(), "Decimal separator", Key::Point, Style::Num),
        k("=", "Equals", Key::Equals, Style::Accent),
    ];
    apply_error_state(&mut keys, e);
    if let Some(rp) = keys.iter_mut().find(|k| k.action == Action::Key(Key::RParen)) {
        rp.enabled &= e.open_parens() > 0;
    }
    grid(ui, pad_r, 5, keys);
}

// ---------------------------------------------------------------- programmer

fn programmer(app: &App, ui: &mut Ui, r: Rect, a: &mut Anchors, docked: bool) {
    let e = &app.prog;
    let t = ui.t;
    let mut r = r;
    header(app, ui, r.take_top(ui.px(44.0)), docked);
    let notice = e.overflowed.then(|| tf("Overflow: wrapped to {}", &[e.word.label()]));
    let disp_h = (r.h * 0.17).clamp(ui.px(80.0), ui.px(150.0));
    calc_display(ui, e, r.take_top(disp_h), notice.as_deref());
    // Radix rows
    let v = e.int_value();
    let err = e.error().is_some();
    for radix in Radix::all() {
        let row = r.take_top(ui.px(26.0));
        let selected = e.radix == radix;
        let value = if err {
            String::new()
        } else {
            num(&crate::calc::programmer::format(v, radix, e.word))
        };
        let label = format!("{} {}", radix.label(), value);
        ui.button(
            Button::new(Action::Radix(radix), row, &label, Content::None, Style::Subtle)
                .role(Role::Radio)
                .checked(selected),
        );
        if selected {
            let bar = Rect::new(row.x + ui.px(4.0), row.y + ui.px(6.0), ui.px(3.0), row.h - ui.px(12.0));
            ui.g.fill_rrect(bar, ui.px(1.5), t.accent_fg);
        }
        let lr = Rect::new(row.x + ui.px(16.0), row.y, ui.px(48.0), row.h);
        ui.text(
            radix.label(),
            lr,
            13.0,
            if selected { FontId::Semibold } else { FontId::Regular },
            t.text,
            Align::Left,
        );
        let vr = Rect::new(lr.right(), row.y, row.w - lr.w - ui.px(28.0), row.h);
        ui.fit_text(&value, vr, 13.0, 8.0, FontId::Regular, t.text, Align::Left);
    }
    a.flyout_area = r;
    let pad = ui.px(4.0);
    // Keypad type, word size, memory
    let row = r.take_top(ui.px(38.0));
    let row = Rect::new(row.x + pad, row.y + ui.px(2.0), row.w - 2.0 * pad, row.h - ui.px(2.0));
    let n = if docked { 5 } else { 6 };
    let gap2 = ui.px(2.0);
    let cell = |i: usize| row.cell(n, 1, i, 0, gap2);
    ui.button(
        Button::new(
            Action::BitPad(false),
            cell(0),
            "Full keypad",
            Content::Icon(Icon::FullKeypad, 14.0),
            Style::Subtle,
        )
        .role(Role::Radio)
        .checked(!app.bitpad),
    );
    ui.button(
        Button::new(
            Action::BitPad(true),
            cell(1),
            "Bit toggling keypad",
            Content::Icon(Icon::BitKeypad, 14.0),
            Style::Subtle,
        )
        .role(Role::Radio)
        .checked(app.bitpad),
    );
    let sel = if app.bitpad { cell(1) } else { cell(0) };
    ui.g.fill_rect(
        Rect::new(sel.x + sel.w * 0.3, sel.bottom() - ui.px(3.0), sel.w * 0.4, ui.px(2.0)),
        t.accent_fg,
    );
    let word_label = tf("Word size {}, change", &[e.word.label()]);
    let wr = Rect::new(cell(2).x, row.y, cell(3).right() - cell(2).x, row.h);
    ui.button(Button::new(
        Action::CycleWord,
        wr,
        &word_label,
        Content::Text(e.word.label(), 13.0, FontId::Semibold),
        Style::Subtle,
    ));
    let ms = cell(4);
    ui.button(
        Button::new(
            Action::Mem(MemOp::Store),
            ms,
            "Memory store",
            Content::Text("MS", 12.0, FontId::Semibold),
            Style::Subtle,
        )
        .enabled(!err),
    );
    if !docked {
        ui.button(
            Button::new(
                Action::Open(Overlay::Panel(PanelTab::Memory)),
                cell(5),
                "Open memory flyout",
                Content::Text("M▾", 12.0, FontId::Semibold),
                Style::Subtle,
            )
            .enabled(!app.memory.is_empty()),
        );
    }
    // Bitwise / Bit shift
    let row = r.take_top(ui.px(40.0));
    let half = ((row.w - 2.0 * pad) / 2.0).min(ui.px(200.0));
    a.bitwise = Rect::new(row.x + pad, row.y + ui.px(2.0), half, row.h - ui.px(4.0));
    a.shift = Rect::new(a.bitwise.right() + ui.px(2.0), a.bitwise.y, half, a.bitwise.h);
    ui.button(
        Button::new(
            Action::Open(Overlay::Bitwise),
            a.bitwise,
            "Bitwise",
            Content::Dropdown(None, "Bitwise", 14.0),
            Style::Subtle,
        )
        .enabled(!err),
    );
    ui.button(
        Button::new(
            Action::Open(Overlay::Shift),
            a.shift,
            "Bit shift",
            Content::Dropdown(None, "Bit shift", 14.0),
            Style::Subtle,
        )
        .enabled(!err),
    );
    let pad_r = Rect::new(r.x + pad, r.y + ui.px(2.0), r.w - 2.0 * pad, r.h - pad - ui.px(2.0));
    if app.bitpad {
        bit_pad(app, ui, pad_r);
        return;
    }
    let base = e.radix.base();
    let mut keys = Vec::new();
    let rot = e.shift == ShiftMode::Rotate;
    let (lsh, rsh) = if rot {
        (un("RoL", "Rotate left", Unary::Rol), un("RoR", "Rotate right", Unary::Ror))
    } else {
        (op("<<", "Left shift", BinOp::Shl), op(">>", "Right shift", BinOp::Shr))
    };
    keys.push(digit(10));
    keys.push(lsh);
    keys.push(rsh);
    keys.push(clear_key(e));
    keys.push(backspace());
    keys.push(digit(11));
    keys.push(k("(", "Left parenthesis", Key::LParen, Style::Op));
    keys.push(k(")", "Right parenthesis", Key::RParen, Style::Op));
    keys.push(op("%", "Modulo", BinOp::Mod));
    keys.push(op("÷", "Divide by", BinOp::Div));
    type Row<'a> = (u8, [u8; 3], (&'a str, &'static str, BinOp));
    let rows: [Row; 3] = [
        (12, [7, 8, 9], ("×", "Multiply by", BinOp::Mul)),
        (13, [4, 5, 6], ("−", "Minus", BinOp::Sub)),
        (14, [1, 2, 3], ("+", "Plus", BinOp::Add)),
    ];
    for (hex, ds, (sym, label, b)) in rows {
        keys.push(digit(hex));
        for d in ds {
            keys.push(digit(d));
        }
        keys.push(op(sym, label, b));
    }
    keys.push(digit(15));
    keys.push(k("+/−", "Positive negative", Key::Negate, Style::Num));
    keys.push(digit(0));
    let mut point = k(i18n::decimal_point(), "Decimal separator", Key::Point, Style::Num);
    point.enabled = false;
    keys.push(point);
    keys.push(k("=", "Equals", Key::Equals, Style::Accent));
    apply_error_state(&mut keys, e);
    for key in keys.iter_mut() {
        if let Action::Key(Key::Digit(d)) = key.action {
            key.enabled &= (d as u32) < base;
        }
        if key.action == Action::Key(Key::RParen) {
            key.enabled &= e.open_parens() > 0;
        }
    }
    grid(ui, pad_r, 5, keys);
}

fn bit_pad(app: &App, ui: &mut Ui, r: Rect) {
    let e = &app.prog;
    let t = ui.t;
    let pattern = e.word.pattern(e.int_value());
    let bits = e.word.bits();
    let rows = 4;
    let row_h = r.h / rows as f32;
    let group_gap = ui.px(10.0);
    let cell_w = (r.w - 3.0 * group_gap) / 16.0;
    for row in 0..rows {
        let top_bit = 63 - row * 16;
        let y = r.y + row as f32 * row_h;
        for i in 0..16u32 {
            let bit = top_bit - i;
            let group = i / 4;
            let x = r.x + i as f32 * cell_w + group as f32 * group_gap;
            let cr = Rect::new(x.round(), (y + row_h * 0.1).round(), cell_w.round(), (row_h * 0.55).round());
            let on = pattern >> bit & 1 == 1;
            let label = tf("Bit {}, {}", &[&bit.to_string(), if on { "1" } else { "0" }]);
            let size = (cell_w / ui.s * 0.6).clamp(10.0, 18.0);
            ui.button(
                Button::new(
                    Action::ToggleBit(bit),
                    cr,
                    &label,
                    Content::Text(if on { "1" } else { "0" }, size, FontId::Semibold),
                    Style::Subtle,
                )
                .enabled(bit < bits)
                .role(Role::Toggle)
                .checked(on),
            );
            if i % 4 == 3 {
                let lr = Rect::new(cr.x - cell_w * 3.0, cr.bottom(), cell_w * 4.0, row_h * 0.3);
                ui.text(
                    &bit.to_string(),
                    lr,
                    11.0,
                    FontId::Regular,
                    if bit < bits { t.text_secondary } else { t.text_disabled },
                    Align::Center,
                );
            }
        }
    }
}

// ---------------------------------------------------------------- history / memory panel

fn side_panel(app: &App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    let mut r = r;
    let tabs = r.take_top(ui.px(44.0));
    let list: Vec<PanelTab> = if app.has_history() {
        vec![PanelTab::History, PanelTab::Memory]
    } else {
        vec![PanelTab::Memory]
    };
    let tab = if app.has_history() { app.panel_tab } else { PanelTab::Memory };
    let mut x = tabs.x + ui.px(12.0);
    for tb in list {
        let name = if tb == PanelTab::History { "History" } else { "Memory" };
        let w = ui.g.measure(name, ui.fs(14.0), FontId::Semibold) + ui.px(20.0);
        let tr = Rect::new(x, tabs.y + ui.px(6.0), w, ui.px(34.0));
        ui.button(
            Button::new(
                Action::PanelTab(tb),
                tr,
                name,
                Content::Text(name, 14.0, FontId::Semibold),
                Style::Subtle,
            )
            .role(Role::Tab)
            .checked(tab == tb),
        );
        if tab == tb {
            ui.g.fill_rrect(
                Rect::new(tr.x + w * 0.3, tr.bottom() - ui.px(3.0), w * 0.4, ui.px(3.0)),
                ui.px(1.5),
                t.accent_fg,
            );
        }
        x += w + ui.px(4.0);
    }
    panel_list(app, ui, r, tab);
}

fn panel_list(app: &App, ui: &mut Ui, r: Rect, tab: PanelTab) {
    let t = ui.t;
    let mut r = r;
    let bottom = r.take_bottom(ui.px(48.0));
    let list = Rect::new(r.x, r.y, r.w, r.h);
    let pad = ui.px(16.0);
    let empty = match tab {
        PanelTab::History => app.history.is_empty(),
        PanelTab::Memory => app.memory.is_empty(),
    };
    if empty {
        let msg = if tab == PanelTab::History {
            "There's no history yet"
        } else {
            "There's nothing saved in memory"
        };
        let mr = Rect::new(list.x + pad, list.y + ui.px(8.0), list.w - 2.0 * pad, ui.px(24.0));
        ui.text(msg, mr, 14.0, FontId::Regular, t.text, Align::Left);
        ui.label(mr, msg, Role::Label);
        return;
    }
    let item_h = ui.px(if tab == PanelTab::History { 68.0 } else { 78.0 });
    let count = if tab == PanelTab::History {
        app.history.len()
    } else {
        app.memory.len()
    };
    let content_h = count as f32 * item_h;
    let offset = app.scroll(ScrollId::Panel).min((content_h - list.h).max(0.0));
    ui.scroll_area(ScrollId::Panel, list, content_h, offset);
    ui.set_clip(Some(list));
    let first = (offset / item_h) as usize;
    for i in first..count {
        let y = list.y + i as f32 * item_h - offset;
        if y > list.bottom() {
            break;
        }
        let ir = Rect::new(list.x + ui.px(4.0), y, list.w - ui.px(8.0), item_h - ui.px(2.0));
        let inner = Rect::new(ir.x + ui.px(12.0), ir.y, ir.w - ui.px(24.0), ir.h);
        match tab {
            PanelTab::History => {
                let h = &app.history[i];
                let (hx, hr) = (num(&h.expression), num(&h.result));
                let label = format!("{hx} {hr}");
                ui.button(Button::new(Action::HistoryItem(i), ir, &label, Content::None, Style::Subtle).role(Role::ListItem));
                ui.fit_text(
                    &hx,
                    Rect::new(inner.x, inner.y + ui.px(6.0), inner.w, ui.px(22.0)),
                    14.0,
                    9.0,
                    FontId::Regular,
                    t.text_secondary,
                    Align::Right,
                );
                ui.fit_text(
                    &hr,
                    Rect::new(inner.x, inner.y + ui.px(28.0), inner.w, ui.px(34.0)),
                    22.0,
                    10.0,
                    FontId::Semibold,
                    t.text,
                    Align::Right,
                );
            }
            PanelTab::Memory => {
                let v = num(&app.format_memory(&app.memory[i]));
                ui.button(
                    Button::new(
                        Action::MemItem(i, MemOp::Recall),
                        ir,
                        &tf("Memory item {}", &[&v]),
                        Content::None,
                        Style::Subtle,
                    )
                    .role(Role::ListItem),
                );
                ui.fit_text(
                    &v,
                    Rect::new(inner.x, inner.y + ui.px(6.0), inner.w, ui.px(34.0)),
                    22.0,
                    10.0,
                    FontId::Semibold,
                    t.text,
                    Align::Right,
                );
                let mut br = Rect::new(inner.right() - ui.px(3.0 * 40.0), inner.y + ui.px(44.0), ui.px(40.0), ui.px(26.0));
                for (txt, label, op) in [
                    ("MC", "Clear memory item", MemOp::Clear),
                    ("M+", "Add to memory item", MemOp::Add),
                    ("M-", "Subtract from memory item", MemOp::Sub),
                ] {
                    ui.button(Button::new(
                        Action::MemItem(i, op),
                        br,
                        label,
                        Content::Text(txt, 11.0, FontId::Semibold),
                        Style::Op,
                    ));
                    br.x += ui.px(40.0);
                }
            }
        }
    }
    ui.set_clip(None);
    let tr = Rect::new(bottom.right() - ui.px(48.0), bottom.y + ui.px(4.0), ui.px(40.0), ui.px(40.0));
    let (action, label) = if tab == PanelTab::History {
        (Action::ClearHistory, "Clear all history")
    } else {
        (Action::ClearMemory, "Clear all memory")
    };
    ui.button(Button::new(action, tr, label, Content::Icon(Icon::Trash, 15.0), Style::Subtle));
}

// ---------------------------------------------------------------- date calculation

fn combo(app: &App, ui: &mut Ui, a: &mut Anchors, id: ComboId, r: Rect, label: &str) {
    let value = tr(&app.combo_label(id)).to_string();
    a.combo[id as usize] = r;
    let full_label = format!("{}, {value}", tr(label));
    let b = Button::new(
        Action::Open(Overlay::Combo(id)),
        r,
        &full_label,
        Content::Dropdown(None, &value, 14.0),
        Style::Num,
    )
    .role(Role::ComboBox);
    ui.button(b);
    if let Some(w) = ui.widgets.last_mut() {
        w.value = Some(value);
    }
}

fn caption(ui: &mut Ui, r: &mut Rect, text: &str) {
    let t = ui.t;
    let lr = r.take_top(ui.px(28.0));
    ui.text(
        text,
        Rect::new(lr.x, lr.y + ui.px(4.0), lr.w, lr.h - ui.px(4.0)),
        14.0,
        FontId::Regular,
        t.text_secondary,
        Align::Left,
    );
    ui.label(lr, text, Role::Label);
}

fn radio(ui: &mut Ui, action: Action, r: Rect, text: &str, on: bool) {
    let t = ui.t;
    ui.button(
        Button::new(action, r, text, Content::None, Style::Subtle)
            .role(Role::Radio)
            .checked(on),
    );
    let cx = r.x + ui.px(14.0);
    let cy = r.center_y();
    let rad = ui.px(10.0);
    let outline = Rect::new(cx - rad, cy - rad, 2.0 * rad, 2.0 * rad);
    if on {
        ui.g.fill_circle(cx, cy, rad, t.accent_fg);
        ui.g.fill_circle(cx, cy, rad * 0.45, if t.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) });
    } else {
        ui.g.stroke_rrect(outline, rad, ui.px(1.0).max(1.0), t.text_secondary);
    }
    ui.text(
        text,
        Rect::new(r.x + ui.px(32.0), r.y, r.w - ui.px(32.0), r.h),
        14.0,
        FontId::Regular,
        t.text,
        Align::Left,
    );
}

fn date_field(ui: &mut Ui, a: &mut Anchors, idx: u8, r: Rect, d: Date, label: &str) {
    a.cal[idx as usize] = r;
    let text = dates::short_date(d);
    let full = format!("{}, {}", tr(label), dates::long_date(d));
    ui.button(
        Button::new(
            Action::Open(Overlay::Calendar(idx)),
            r,
            &full,
            Content::Dropdown(Some(Icon::Calendar), &text, 14.0),
            Style::Num,
        )
        .role(Role::ComboBox),
    );
}

fn date_view(app: &App, ui: &mut Ui, r: Rect, a: &mut Anchors) {
    let t = ui.t;
    let mut r = r;
    header(app, ui, r.take_top(ui.px(44.0)), false);
    let pad = ui.px(16.0);
    let mut c = Rect::new(r.x + pad, r.y + ui.px(4.0), r.w - 2.0 * pad, r.h - pad);
    let field_w = c.w.min(ui.px(320.0));
    let cr = c.take_top(ui.px(36.0));
    combo(
        app,
        ui,
        a,
        ComboId::DateMode,
        Rect::new(cr.x, cr.y, field_w, cr.h),
        "Calculation mode",
    );
    c.take_top(ui.px(16.0));
    let dw = c.w.min(ui.px(260.0));
    if !app.date.add_mode {
        caption(ui, &mut c, "From");
        let fr = c.take_top(ui.px(36.0));
        date_field(ui, a, 0, Rect::new(fr.x, fr.y, dw, fr.h), app.date.from, "From");
        c.take_top(ui.px(12.0));
        caption(ui, &mut c, "To");
        let tr = c.take_top(ui.px(36.0));
        date_field(ui, a, 1, Rect::new(tr.x, tr.y, dw, tr.h), app.date.to, "To");
        c.take_top(ui.px(20.0));
        caption(ui, &mut c, "Difference");
        let diff = app.date_difference();
        let text = dates::difference(&diff);
        let br = c.take_top(ui.px(40.0));
        ui.fit_text(&text, br, 24.0, 14.0, FontId::Semibold, t.text, Align::Left);
        ui.label(br, &tf("Difference {}", &[&text]), Role::Display);
        if diff.total_days != 0 && (diff.years, diff.months, diff.weeks) != (0, 0, 0) {
            let sr = c.take_top(ui.px(28.0));
            let total = dates::total_days(&diff);
            ui.text(&total, sr, 14.0, FontId::Regular, t.text_secondary, Align::Left);
            ui.label(sr, &total, Role::Label);
        }
    } else {
        caption(ui, &mut c, "From");
        let fr = c.take_top(ui.px(36.0));
        date_field(ui, a, 0, Rect::new(fr.x, fr.y, dw, fr.h), app.date.from, "From");
        c.take_top(ui.px(12.0));
        let rr = c.take_top(ui.px(36.0));
        radio(
            ui,
            Action::DateSubtract(false),
            Rect::new(rr.x, rr.y, ui.px(110.0), rr.h),
            "Add",
            !app.date.subtract,
        );
        radio(
            ui,
            Action::DateSubtract(true),
            Rect::new(rr.x + ui.px(120.0), rr.y, ui.px(130.0), rr.h),
            "Subtract",
            app.date.subtract,
        );
        c.take_top(ui.px(8.0));
        let colw = ((c.w - ui.px(16.0)) / 3.0).min(ui.px(100.0));
        let lr = c.take_top(ui.px(28.0));
        let cr = c.take_top(ui.px(36.0));
        for (i, (id, name)) in [(ComboId::Years, "Years"), (ComboId::Months, "Months"), (ComboId::Days, "Days")]
            .into_iter()
            .enumerate()
        {
            let x = lr.x + i as f32 * (colw + ui.px(8.0));
            let l = Rect::new(x, lr.y + ui.px(4.0), colw, lr.h - ui.px(4.0));
            ui.text(name, l, 14.0, FontId::Regular, t.text_secondary, Align::Left);
            combo(app, ui, a, id, Rect::new(x, cr.y, colw, cr.h), name);
        }
        c.take_top(ui.px(20.0));
        caption(ui, &mut c, "Date");
        let br = c.take_top(ui.px(40.0));
        let text = match app.date_result() {
            Some(d) => dates::long_date(d),
            None => tf(
                "Outside the supported range ({}–{})",
                &[&date::MIN_YEAR.to_string(), &date::MAX_YEAR.to_string()],
            ),
        };
        ui.fit_text(&text, br, 24.0, 12.0, FontId::Semibold, t.text, Align::Left);
        ui.label(br, &tf("Resulting date {}", &[&text]), Role::Display);
    }
}

// ---------------------------------------------------------------- converter

fn converter(app: &App, ui: &mut Ui, r: Rect, a: &mut Anchors) {
    let t = ui.t;
    let mut r = r;
    header(app, ui, r.take_top(ui.px(44.0)), false);
    let keypad_h = (r.h * 0.48).max(ui.px(220.0));
    let kp = r.take_bottom(keypad_h);
    let pad = ui.px(16.0);
    let mut c = Rect::new(r.x + pad, r.y, r.w - 2.0 * pad, r.h);
    let (v0, v1) = app.conv_texts();
    let (v0, v1) = (num(&v0), num(&v1));
    let cat = app.conv_category();
    let (from, to) = app.conv_units();
    let is_currency = cat.name == "Currency";
    let field_h = (c.h * 0.24).clamp(ui.px(44.0), ui.px(72.0));
    let combo_w = c.w.min(ui.px(300.0));
    for (idx, (text, unit, combo_id)) in [(v0, from, ComboId::ConvFrom), (v1, to, ComboId::ConvTo)].into_iter().enumerate() {
        let active = app.conv_active as usize == idx;
        let fr = c.take_top(field_h);
        let unit_name = tr(&cat.units[unit].name);
        let label = if active {
            tf("Input value {} {}", &[&text, unit_name])
        } else {
            tf("Converts into {} {}", &[&text, unit_name])
        };
        ui.button(
            Button::new(Action::ConvField(idx as u8), fr, &label, Content::None, Style::Subtle)
                .role(Role::Field)
                .checked(active),
        );
        let font = if active { FontId::Semibold } else { FontId::Regular };
        let max = (field_h / ui.s * 0.68).clamp(20.0, 46.0);
        let tr = Rect::new(fr.x + ui.px(4.0), fr.y, fr.w - ui.px(8.0), fr.h);
        ui.fit_text(&text, tr, max, 12.0, font, t.text, Align::Left);
        let cr = c.take_top(ui.px(34.0));
        combo(
            app,
            ui,
            a,
            combo_id,
            Rect::new(cr.x, cr.y, combo_w, cr.h),
            if idx == 0 { "Input unit" } else { "Output unit" },
        );
        c.take_top(ui.px(8.0));
    }
    if is_currency {
        let note = tf(
            "{}. Rates may be out of date; place a currency.txt next to the settings file to update them.",
            &[&app.currency_source.text()],
        );
        ui.set_clip(Some(c));
        ui.paragraph(&note, Rect::new(c.x, c.y, c.w, c.h), 12.0, FontId::Regular, t.text_secondary);
        ui.set_clip(None);
    }
    let kpad = ui.px(4.0);
    let kr = Rect::new(kp.x + kpad, kp.y, kp.w - 2.0 * kpad, kp.h - kpad);
    let mut blank = k("", "", Key::Clear, Style::Op);
    blank.label = "";
    let keys = vec![
        blank,
        k("CE", "Clear entry", Key::ClearEntry, Style::Op),
        backspace(),
        digit(7),
        digit(8),
        digit(9),
        digit(4),
        digit(5),
        digit(6),
        digit(1),
        digit(2),
        digit(3),
        k("+/−", "Positive negative", Key::Negate, Style::Num),
        digit(0),
        k(i18n::decimal_point(), "Decimal separator", Key::Point, Style::Num),
    ];
    grid(ui, kr, 3, keys);
}

// ---------------------------------------------------------------- settings

fn settings(app: &App, ui: &mut Ui, r: Rect, a: &mut Anchors) {
    let t = ui.t;
    let mut r = r;
    header(app, ui, r.take_top(ui.px(44.0)), false);
    let pad = ui.px(20.0);
    let view = Rect::new(r.x, r.y, r.w, r.h);
    let offset = app.scroll(ScrollId::Settings);
    ui.set_clip(Some(view));
    let start_y = view.y + ui.px(8.0) - offset;
    let mut y = start_y;
    let w = view.w - 2.0 * pad;
    let x = view.x + pad;
    let heading = |ui: &mut Ui, y: &mut f32, s: &str| {
        let hr = Rect::new(x, *y, w, ui.px(36.0));
        ui.text(s, hr, 18.0, FontId::Semibold, t.text, Align::Left);
        ui.label(hr, s, Role::Label);
        *y += ui.px(40.0);
    };
    let sub = |ui: &mut Ui, y: &mut f32, s: &str| {
        let tr = Rect::new(x, *y, w, ui.px(24.0));
        ui.text(s, tr, 14.0, FontId::Regular, t.text_secondary, Align::Left);
        *y += ui.px(28.0);
    };
    heading(ui, &mut y, "Appearance");
    sub(ui, &mut y, "App theme");
    for (pref, name) in [
        (ThemePref::Light, "Light"),
        (ThemePref::Dark, "Dark"),
        (ThemePref::System, "Use system setting"),
    ] {
        radio(
            ui,
            Action::Theme(pref),
            Rect::new(x, y, w.min(ui.px(280.0)), ui.px(36.0)),
            name,
            app.theme_pref == pref,
        );
        y += ui.px(38.0);
    }
    y += ui.px(8.0);
    sub(ui, &mut y, "Language");
    combo(
        app,
        ui,
        a,
        ComboId::Language,
        Rect::new(x, y, w.min(ui.px(300.0)), ui.px(36.0)),
        "Language",
    );
    y += ui.px(52.0);

    heading(ui, &mut y, "Calculator key");
    if crate::platform::CALC_KEY_SUPPORTED {
        let on = app.calc_key;
        ui.button(
            Button::new(
                Action::CalcKey(!on),
                Rect::new(x, y, w, ui.px(40.0)),
                "Open rcalc with the keyboard Calculator key",
                Content::None,
                Style::Subtle,
            )
            .role(Role::Toggle)
            .checked(on),
        );
        toggle_switch(ui, Rect::new(x + ui.px(4.0), y + ui.px(10.0), ui.px(40.0), ui.px(20.0)), on);
        let lr = Rect::new(x + ui.px(56.0), y, w - ui.px(56.0), ui.px(40.0));
        ui.fit_text(
            "Open rcalc with the keyboard Calculator key",
            lr,
            14.0,
            10.0,
            FontId::Regular,
            t.text,
            Align::Left,
        );
        y += ui.px(46.0);
    }
    y += ui.paragraph(
        "When enabled, the Calculator key on your keyboard opens rcalc instead of Windows Calculator. If rcalc is already running in the notification area, the key shows that window instantly.",
        Rect::new(x, y, w, 0.0),
        13.0,
        FontId::Regular,
        t.text_secondary,
    ) + ui.px(16.0);

    heading(ui, &mut y, "Background mode");
    let bg = tf(
        "Start rcalc with {} to keep it running in the notification area for instant access. Closing the window then hides it; use the tray icon menu to exit.",
        &["rcalc.exe --tray"],
    );
    y += ui.paragraph(&bg, Rect::new(x, y, w, 0.0), 14.0, FontId::Regular, t.text) + ui.px(16.0);

    heading(ui, &mut y, "Saved data");
    let loc = &app.location;
    let mut storage = match &loc.dir {
        Some(d) => {
            let path = d.join(crate::storage::FILE_NAME).display().to_string();
            let text = tf("History, memory and settings are stored in {}.", &[&path]);
            if loc.portable {
                format!("{} {text}", tr("Portable mode."))
            } else {
                text
            }
        }
        None => tr("Settings and history are not saved on this system.").to_string(),
    };
    if let Some(err) = &loc.error {
        storage.push(' ');
        storage.push_str(&tf("Warning: {}", &[err]));
    }
    storage.push(' ');
    storage.push_str(&tf(
        "To run fully portable, create an empty file named {} next to rcalc.exe.",
        &[crate::storage::PORTABLE_MARKER],
    ));
    y += ui.paragraph(&storage, Rect::new(x, y, w, 0.0), 14.0, FontId::Regular, t.text) + ui.px(16.0);
    heading(ui, &mut y, "Currency rates");
    let cur = tf(
        "{}. Currency conversion works offline from bundled reference rates that are not live market data. To use your own rates, create {} in the data folder with lines like \"EUR = 0.92\" (units per US dollar), optionally starting with \"# date: 2026-10-01\".",
        &[&app.currency_source.text(), crate::storage::CURRENCY_FILE],
    );
    y += ui.paragraph(&cur, Rect::new(x, y, w, 0.0), 14.0, FontId::Regular, t.text) + ui.px(16.0);
    heading(ui, &mut y, "Keyboard");
    let keys = "Digits, + - * / and Enter work as expected. Esc clears, Delete clears the entry, Backspace deletes a digit, F9 negates. Ctrl+C/Ctrl+V copy and paste, Ctrl+M/P/Q/R/L are MS, M+, M-, MR and MC, Ctrl+H opens history. Alt+1, Alt+2, Alt+4 and Alt+5 switch to Standard, Scientific, Programmer and Date calculation. Tab moves between controls and Space activates the focused control. See README for the full list.";
    y += ui.paragraph(keys, Rect::new(x, y, w, 0.0), 14.0, FontId::Regular, t.text) + ui.px(16.0);
    heading(ui, &mut y, "About");
    let about = tf(
        "rcalc {} — a portable calculator written in Rust. Its design is inspired by Windows Calculator; it is an independent project and is not affiliated with or endorsed by Microsoft.",
        &[env!("CARGO_PKG_VERSION")],
    );
    y += ui.paragraph(&about, Rect::new(x, y, w, 0.0), 14.0, FontId::Regular, t.text) + ui.px(24.0);
    ui.set_clip(None);
    ui.scroll_area(ScrollId::Settings, view, y - start_y, offset);
}

/// Fluent-style on/off switch.
fn toggle_switch(ui: &mut Ui, r: Rect, on: bool) {
    let t = ui.t;
    let rad = r.h / 2.0;
    if on {
        ui.g.fill_rrect(r, rad, t.accent);
        ui.g.fill_circle(r.right() - rad, r.center_y(), rad * 0.55, t.accent_text);
    } else {
        ui.g.stroke_rrect(r, rad, ui.px(1.0).max(1.0), t.text_secondary);
        ui.g.fill_circle(r.x + rad, r.center_y(), rad * 0.5, t.text_secondary);
    }
}

// ---------------------------------------------------------------- overlays

fn overlays(app: &App, ui: &mut Ui, a: &Anchors, full: Rect) {
    match app.overlay {
        Overlay::None => {}
        Overlay::Nav => nav(app, ui, full),
        Overlay::Panel(tab) => {
            let area = a.flyout_area;
            ui.scrim(full, Action::CloseOverlay, false);
            ui.flyout(area);
            let tab = if app.has_history() { tab } else { PanelTab::Memory };
            panel_list(app, ui, Rect::new(area.x, area.y + ui.px(8.0), area.w, area.h - ui.px(8.0)), tab);
        }
        Overlay::Trig => {
            let inv = app.trig_inv;
            let hyp = app.trig_hyp;
            let mut keys = vec![ka(
                "2nd",
                "Inverse functions",
                Action::TrigInv,
                if inv { Style::Accent } else { Style::Op },
            )];
            let fns = [(Trig::Sin, "sin"), (Trig::Cos, "cos"), (Trig::Tan, "tan")];
            let fns2 = [(Trig::Sec, "sec"), (Trig::Csc, "csc"), (Trig::Cot, "cot")];
            let push = |keys: &mut Vec<K>, f: Trig| {
                let name = crate::expr::ast::trig_name(f, inv, hyp);
                keys.push(K {
                    text: name.to_string(),
                    label: trig_label(f, inv, hyp),
                    action: Action::Key(Key::Unary(Unary::Trig { f, inv, hyp })),
                    style: Style::Op,
                    enabled: true,
                    sub: None,
                    icon: None,
                });
            };
            for (f, _) in fns {
                push(&mut keys, f);
            }
            keys.push(ka(
                "hyp",
                "Hyperbolic functions",
                Action::TrigHyp,
                if hyp { Style::Accent } else { Style::Op },
            ));
            for (f, _) in fns2 {
                push(&mut keys, f);
            }
            flyout_grid(ui, a.trig, full, 4, keys);
        }
        Overlay::Func => {
            let keys = vec![
                un("|x|", "Absolute value", Unary::Abs),
                un("⌊x⌋", "Floor", Unary::Floor),
                un("⌈x⌉", "Ceiling", Unary::Ceil),
                k("rand", "Random", Key::Rand, Style::Op),
                un("→dms", "Degrees minutes seconds", Unary::ToDms),
                un("→deg", "Decimal degrees", Unary::FromDms),
            ];
            flyout_grid(ui, a.func, full, 3, keys);
        }
        Overlay::Bitwise => {
            let keys = vec![
                op("AND", "And", BinOp::And),
                op("OR", "Or", BinOp::Or),
                un("NOT", "Not", Unary::Not),
                op("NAND", "Nand", BinOp::Nand),
                op("NOR", "Nor", BinOp::Nor),
                op("XOR", "Exclusive or", BinOp::Xor),
            ];
            flyout_grid(ui, a.bitwise, full, 3, keys);
        }
        Overlay::Shift => {
            ui.scrim(full, Action::CloseOverlay, false);
            let w = ui.px(260.0).min(full.w - ui.px(8.0));
            let h = ui.px(3.0 * 40.0 + 12.0);
            let fr = place(Rect::new(a.shift.x, a.shift.bottom() + ui.px(4.0), w, h), full, ui.px(4.0));
            ui.flyout(fr);
            for (i, m) in [ShiftMode::Arithmetic, ShiftMode::Logical, ShiftMode::Rotate]
                .into_iter()
                .enumerate()
            {
                let rr = Rect::new(
                    fr.x + ui.px(8.0),
                    fr.y + ui.px(6.0) + i as f32 * ui.px(40.0),
                    fr.w - ui.px(16.0),
                    ui.px(40.0),
                );
                radio(ui, Action::SetShift(m), rr, m.label(), app.prog.shift == m);
            }
        }
        Overlay::Combo(id) => combo_list(app, ui, a.combo[id as usize], id, full),
        Overlay::Calendar(target) => calendar(app, ui, a.cal[target as usize], target, full),
    }
}

fn trig_label(f: Trig, inv: bool, hyp: bool) -> &'static str {
    use Trig::*;
    match (f, inv, hyp) {
        (Sin, false, false) => "Sine",
        (Cos, false, false) => "Cosine",
        (Tan, false, false) => "Tangent",
        (Sec, false, false) => "Secant",
        (Csc, false, false) => "Cosecant",
        (Cot, false, false) => "Cotangent",
        (Sin, true, false) => "Arc sine",
        (Cos, true, false) => "Arc cosine",
        (Tan, true, false) => "Arc tangent",
        (Sec, true, false) => "Arc secant",
        (Csc, true, false) => "Arc cosecant",
        (Cot, true, false) => "Arc cotangent",
        (Sin, false, true) => "Hyperbolic sine",
        (Cos, false, true) => "Hyperbolic cosine",
        (Tan, false, true) => "Hyperbolic tangent",
        (Sec, false, true) => "Hyperbolic secant",
        (Csc, false, true) => "Hyperbolic cosecant",
        (Cot, false, true) => "Hyperbolic cotangent",
        (Sin, true, true) => "Inverse hyperbolic sine",
        (Cos, true, true) => "Inverse hyperbolic cosine",
        (Tan, true, true) => "Inverse hyperbolic tangent",
        (Sec, true, true) => "Inverse hyperbolic secant",
        (Csc, true, true) => "Inverse hyperbolic cosecant",
        (Cot, true, true) => "Inverse hyperbolic cotangent",
    }
}

/// Keeps a popup inside the window.
fn place(mut r: Rect, full: Rect, margin: f32) -> Rect {
    if r.right() > full.right() - margin {
        r.x = (full.right() - margin - r.w).max(full.x + margin);
    }
    if r.bottom() > full.bottom() - margin {
        r.y = (full.bottom() - margin - r.h).max(full.y + margin);
    }
    r
}

fn flyout_grid(ui: &mut Ui, anchor: Rect, full: Rect, cols: usize, keys: Vec<K>) {
    ui.scrim(full, Action::CloseOverlay, false);
    let rows = keys.len().div_ceil(cols);
    let w = (ui.px(78.0) * cols as f32).min(full.w - ui.px(8.0));
    let h = ui.px(50.0) * rows as f32 + ui.px(12.0);
    let fr = place(Rect::new(anchor.x, anchor.bottom() + ui.px(4.0), w, h), full, ui.px(4.0));
    ui.flyout(fr);
    grid(ui, fr.inset(ui.px(6.0)), cols, keys);
}

fn nav(app: &App, ui: &mut Ui, full: Rect) {
    let t = ui.t;
    ui.scrim(full, Action::CloseOverlay, true);
    let w = ui.px(280.0).min(full.w);
    let pane = Rect::new(0.0, 0.0, w, full.h);
    ui.g.fill_rect(pane, t.flyout);
    ui.g.fill_rect(Rect::new(pane.right() - 1.0, 0.0, 1.0, full.h), t.flyout_border);
    // Pane background absorbs clicks so they don't close the overlay.
    ui.register(Widget {
        id: super::wid(&Action::None) ^ 0xAB,
        rect: pane,
        action: Action::None,
        role: Role::Scrim,
        label: String::new(),
        value: None,
        enabled: true,
        checked: None,
        layer: 1,
    });
    let btn = Rect::new(ui.px(4.0), ui.px(4.0), ui.px(40.0), ui.px(36.0));
    ui.button(Button::new(
        Action::CloseOverlay,
        btn,
        "Close navigation",
        Content::Icon(Icon::Menu, 16.0),
        Style::Subtle,
    ));
    let mut r = Rect::new(0.0, ui.px(48.0), w, full.h - ui.px(48.0));
    let settings_r = r.take_bottom(ui.px(48.0));
    enum Row {
        Header(&'static str),
        Item(Mode, String),
    }
    let mut rows = vec![
        Row::Header("Calculator"),
        Row::Item(Mode::Standard, "Standard".into()),
        Row::Item(Mode::Scientific, "Scientific".into()),
        Row::Item(Mode::Programmer, "Programmer".into()),
        Row::Item(Mode::Date, "Date calculation".into()),
        Row::Header("Converter"),
    ];
    for (i, c) in app.categories.iter().enumerate() {
        rows.push(Row::Item(Mode::Converter(i as u8), c.name.to_string()));
    }
    let header_h = ui.px(36.0);
    let item_h = ui.px(40.0);
    let content_h: f32 = rows
        .iter()
        .map(|r| if matches!(r, Row::Header(_)) { header_h } else { item_h })
        .sum();
    let offset = app.scroll(ScrollId::Nav).min((content_h - r.h).max(0.0));
    ui.scroll_area(ScrollId::Nav, r, content_h, offset);
    ui.set_clip(Some(r));
    let mut y = r.y - offset;
    for row in &rows {
        match row {
            Row::Header(h) => {
                let hr = Rect::new(r.x + ui.px(16.0), y, r.w - ui.px(32.0), header_h);
                ui.text(h, hr, 14.0, FontId::Semibold, t.text, Align::Left);
                ui.label(hr, h, Role::Label);
                y += header_h;
            }
            Row::Item(m, name) => {
                nav_item(
                    app,
                    ui,
                    Rect::new(r.x + ui.px(4.0), y, r.w - ui.px(8.0), item_h - ui.px(4.0)),
                    *m,
                    name,
                );
                y += item_h;
            }
        }
    }
    ui.set_clip(None);
    ui.g.fill_rect(Rect::new(settings_r.x, settings_r.y, settings_r.w - 1.0, 1.0), t.divider);
    nav_item(
        app,
        ui,
        Rect::new(
            settings_r.x + ui.px(4.0),
            settings_r.y + ui.px(4.0),
            settings_r.w - ui.px(8.0),
            item_h - ui.px(4.0),
        ),
        Mode::Settings,
        "Settings",
    );
}

fn nav_item(app: &App, ui: &mut Ui, r: Rect, m: Mode, name: &str) {
    let t = ui.t;
    let selected = app.mode == m;
    if selected {
        ui.g.fill_rrect(r, ui.px(4.0), t.selected);
    }
    ui.button(
        Button::new(Action::Navigate(m), r, name, Content::None, Style::Subtle)
            .role(Role::MenuItem)
            .checked(selected),
    );
    if selected {
        ui.g.fill_rrect(Rect::new(r.x, r.y + r.h * 0.3, ui.px(3.0), r.h * 0.4), ui.px(1.5), t.accent_fg);
    }
    let mut tr = Rect::new(r.x + ui.px(12.0), r.y, r.w - ui.px(16.0), r.h);
    if m == Mode::Settings {
        let ir = tr.take_left(ui.px(20.0));
        super::icons::draw(ui.g, Icon::Settings, ir, ui.fs(15.0), t.text);
        tr.take_left(ui.px(10.0));
    }
    ui.text(name, tr, 14.0, FontId::Regular, t.text, Align::Left);
}

fn combo_list(app: &App, ui: &mut Ui, anchor: Rect, id: ComboId, full: Rect) {
    let t = ui.t;
    ui.scrim(full, Action::CloseOverlay, false);
    let items = app.combo_items(id);
    let sel = app.combo_selected(id);
    let item_h = ui.px(36.0);
    let visible = items.len().min(9);
    let h = visible as f32 * item_h + ui.px(8.0);
    let w = anchor.w.max(ui.px(140.0));
    let fr = place(Rect::new(anchor.x, anchor.y, w, h), full, ui.px(4.0));
    ui.flyout(fr);
    let list = fr.inset(ui.px(4.0));
    let content_h = items.len() as f32 * item_h;
    let offset = app.scroll(ScrollId::Combo).min((content_h - list.h).max(0.0));
    ui.scroll_area(ScrollId::Combo, list, content_h, offset);
    ui.set_clip(Some(list));
    let first = (offset / item_h) as usize;
    for (i, item) in items.iter().enumerate().skip(first) {
        let y = list.y + i as f32 * item_h - offset;
        if y > list.bottom() {
            break;
        }
        let ir = Rect::new(list.x, y, list.w, item_h - ui.px(2.0));
        if i == sel {
            ui.g.fill_rrect(ir, ui.px(4.0), t.selected);
        }
        ui.button(
            Button::new(Action::ComboPick(id, i), ir, item, Content::None, Style::Subtle)
                .role(Role::ListItem)
                .checked(i == sel),
        );
        if i == sel {
            ui.g.fill_rrect(Rect::new(ir.x, ir.y + ir.h * 0.3, ui.px(3.0), ir.h * 0.4), ui.px(1.5), t.accent_fg);
        }
        ui.fit_text(
            item,
            Rect::new(ir.x + ui.px(12.0), ir.y, ir.w - ui.px(16.0), ir.h),
            14.0,
            10.0,
            FontId::Regular,
            t.text,
            Align::Left,
        );
    }
    ui.set_clip(None);
}

fn calendar(app: &App, ui: &mut Ui, anchor: Rect, target: u8, full: Rect) {
    let t = ui.t;
    ui.scrim(full, Action::CloseOverlay, false);
    let w = ui.px(300.0).min(full.w - ui.px(8.0));
    let h = ui.px(340.0);
    let fr = place(Rect::new(anchor.x, anchor.bottom() + ui.px(4.0), w, h), full, ui.px(4.0));
    ui.flyout(fr);
    let mut r = fr.inset(ui.px(8.0));
    let (vy, vm) = app.cal_view;
    let head = r.take_top(ui.px(40.0));
    let title = dates::month_year(vy, vm);
    ui.text(
        &title,
        Rect::new(head.x + ui.px(8.0), head.y, head.w, head.h),
        15.0,
        FontId::Semibold,
        t.text,
        Align::Left,
    );
    ui.label(head, &title, Role::Label);
    let down = Rect::new(head.right() - ui.px(36.0), head.y + ui.px(4.0), ui.px(32.0), ui.px(32.0));
    let up = Rect::new(down.x - ui.px(34.0), down.y, down.w, down.h);
    ui.button(Button::new(
        Action::CalMonth(-1),
        up,
        "Previous month",
        Content::Icon(Icon::ChevronUp, 12.0),
        Style::Subtle,
    ));
    ui.button(Button::new(
        Action::CalMonth(1),
        down,
        "Next month",
        Content::Icon(Icon::ChevronDown, 12.0),
        Style::Subtle,
    ));
    let wd = r.take_top(ui.px(28.0));
    for (i, n) in dates::weekday_headers().iter().enumerate() {
        let c = wd.cell(7, 1, i, 0, 0.0);
        ui.text(n, c, 12.0, FontId::Semibold, t.text_secondary, Align::Center);
    }
    let Some(first) = Date::new(vy, vm, 1) else { return };
    let lead = (first.weekday() + 7 - dates::first_weekday()) % 7;
    let start = first.to_days() - lead as i64;
    let selected = if target == 0 { app.date.from } else { app.date.to };
    let today = crate::platform::today();
    for i in 0..42 {
        let Some(d) = Date::from_days(start + i) else { continue };
        let c = r.cell(7, 6, (i % 7) as usize, (i / 7) as usize, ui.px(2.0));
        let in_month = d.m == vm;
        let is_sel = d == selected;
        ui.button(
            Button::new(Action::CalPick(target, d), c, &dates::long_date(d), Content::None, Style::Subtle)
                .role(Role::ListItem)
                .checked(is_sel),
        );
        let rad = (c.w.min(c.h) / 2.0 - ui.px(2.0)).max(4.0);
        let (cx, cy) = (c.x + c.w / 2.0, c.center_y());
        if is_sel {
            ui.g.fill_circle(cx, cy, rad, t.accent);
        } else if d == today {
            let ring = Rect::new(cx - rad, cy - rad, 2.0 * rad, 2.0 * rad);
            ui.g.stroke_rrect(ring, rad, ui.px(1.5), t.accent_fg);
        }
        let fg = if is_sel {
            t.accent_text
        } else if in_month {
            t.text
        } else {
            t.text_disabled
        };
        ui.text(&d.d.to_string(), c, 14.0, FontId::Regular, fg, Align::Center);
    }
}
