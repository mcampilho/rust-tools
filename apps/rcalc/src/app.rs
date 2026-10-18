//! Application state and the actions that change it. Everything here is
//! independent of the windowing system so it can be unit tested.

use crate::calc::programmer::{Radix, ShiftMode, WordSize};
use crate::calc::{Engine, HistoryEntry, Key, Kind};
use crate::convert::{self, Category};
use crate::date::{self, Date};
use crate::expr::{BinOp, Constant, Unary};
use crate::num::func::{Angle, Trig};
use crate::num::Dec;
use crate::platform;
use crate::storage::{Location, Store, CURRENCY_FILE};

pub const MAX_HISTORY: usize = 100;
pub const MAX_MEMORY: usize = 100;
pub const CONV_MAX_DIGITS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Standard,
    Scientific,
    Programmer,
    Date,
    /// Index into `App::categories` (0 = currency).
    Converter(u8),
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelTab {
    History,
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemOp {
    Clear,
    Recall,
    Add,
    Sub,
    Store,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComboId {
    DateMode,
    Years,
    Months,
    Days,
    ConvFrom,
    ConvTo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    None,
    Nav,
    /// History/memory flyout used when the side panel is not docked.
    Panel(PanelTab),
    Trig,
    Func,
    Bitwise,
    Shift,
    Combo(ComboId),
    /// Date picker for field 0 (from) or 1 (to).
    Calendar(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollId {
    Panel,
    Nav,
    Combo,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Keypad key for the active calculator or converter.
    Key(Key),
    Mem(MemOp),
    MemItem(usize, MemOp),
    HistoryItem(usize),
    ClearHistory,
    ClearMemory,
    OpenNav,
    CloseOverlay,
    Navigate(Mode),
    Open(Overlay),
    PanelTab(PanelTab),
    CycleAngle,
    ToggleFE,
    Toggle2nd,
    TrigInv,
    TrigHyp,
    Radix(Radix),
    CycleWord,
    BitPad(bool),
    SetShift(ShiftMode),
    ToggleBit(u32),
    ComboPick(ComboId, usize),
    CalMonth(i32),
    CalPick(u8, Date),
    DateSubtract(bool),
    ConvField(u8),
    Theme(ThemePref),
    KeepOnTop(bool),
    Copy,
    Paste,
    /// Inert element (labels, modal scrim without action).
    None,
}

pub struct DateState {
    pub add_mode: bool,
    pub from: Date,
    pub to: Date,
    pub subtract: bool,
    pub years: u32,
    pub months: u32,
    pub days: u32,
}

pub struct App {
    pub mode: Mode,
    /// Last calculator mode, used for the history/memory engine when in other modes.
    pub std: Engine,
    pub sci: Engine,
    pub prog: Engine,
    pub history: Vec<HistoryEntry>,
    pub memory: Vec<Dec>,
    pub sci_2nd: bool,
    pub trig_inv: bool,
    pub trig_hyp: bool,
    pub bitpad: bool,
    pub date: DateState,
    pub categories: Vec<Category>,
    pub currency_source: String,
    pub conv_sel: Vec<(usize, usize)>,
    pub conv_active: u8,
    pub conv_entry: String,
    pub theme_pref: ThemePref,
    pub keep_on_top: bool,
    pub overlay: Overlay,
    pub panel_tab: PanelTab,
    /// Month shown by the date picker (year, month).
    pub cal_view: (i32, u32),
    pub scroll: [f32; 4],
    pub location: Location,
    /// Short status message (e.g. "Copied"), cleared by the next action.
    pub toast: Option<String>,
    pub window_size: Option<(f64, f64)>,
}

impl App {
    pub fn new(location: Location) -> App {
        let today = platform::today();
        let mut categories = Vec::new();
        let currency = convert::currency(location.read_text(CURRENCY_FILE).as_deref());
        categories.push(currency.category);
        categories.extend(convert::unit_categories());
        let conv_sel = categories.iter().map(|c| c.defaults).collect();
        App {
            mode: Mode::Standard,
            std: Engine::new(Kind::Standard),
            sci: Engine::new(Kind::Scientific),
            prog: Engine::new(Kind::Programmer),
            history: Vec::new(),
            memory: Vec::new(),
            sci_2nd: false,
            trig_inv: false,
            trig_hyp: false,
            bitpad: false,
            date: DateState {
                add_mode: false,
                from: today,
                to: today,
                subtract: false,
                years: 0,
                months: 0,
                days: 0,
            },
            categories,
            currency_source: currency.source,
            conv_sel,
            conv_active: 0,
            conv_entry: "0".into(),
            theme_pref: ThemePref::System,
            keep_on_top: false,
            overlay: Overlay::None,
            panel_tab: PanelTab::History,
            cal_view: (today.y, today.m),
            scroll: [0.0; 4],
            location,
            toast: None,
            window_size: None,
        }
    }

    pub fn scroll(&self, id: ScrollId) -> f32 {
        self.scroll[id as usize]
    }

    pub fn set_scroll(&mut self, id: ScrollId, v: f32) {
        self.scroll[id as usize] = v.max(0.0);
    }

    pub fn is_calculator(&self) -> bool {
        matches!(self.mode, Mode::Standard | Mode::Scientific | Mode::Programmer)
    }

    pub fn engine(&self) -> Option<&Engine> {
        match self.mode {
            Mode::Standard => Some(&self.std),
            Mode::Scientific => Some(&self.sci),
            Mode::Programmer => Some(&self.prog),
            _ => None,
        }
    }

    pub fn engine_mut(&mut self) -> Option<&mut Engine> {
        match self.mode {
            Mode::Standard => Some(&mut self.std),
            Mode::Scientific => Some(&mut self.sci),
            Mode::Programmer => Some(&mut self.prog),
            _ => None,
        }
    }

    pub fn mode_title(&self) -> &str {
        match self.mode {
            Mode::Standard => "Standard",
            Mode::Scientific => "Scientific",
            Mode::Programmer => "Programmer",
            Mode::Date => "Date calculation",
            Mode::Converter(i) => self.categories[i as usize].name,
            Mode::Settings => "Settings",
        }
    }

    /// History is shared by Standard and Scientific; Programmer has memory only.
    pub fn has_history(&self) -> bool {
        matches!(self.mode, Mode::Standard | Mode::Scientific)
    }

    // ------------------------------------------------------------ actions

    pub fn apply(&mut self, a: &Action) {
        self.toast = None;
        match a {
            Action::Key(k) => self.key(*k),
            Action::Mem(op) => self.memory_op(*op),
            Action::MemItem(i, op) => self.memory_item(*i, *op),
            Action::HistoryItem(i) => {
                if let Some(h) = self.history.get(*i).cloned() {
                    if let Some(e) = self.engine_mut() {
                        e.restore(&h);
                    }
                    self.close_flyout_panel();
                }
            }
            Action::ClearHistory => {
                self.history.clear();
                self.set_scroll(ScrollId::Panel, 0.0);
            }
            Action::ClearMemory => {
                self.memory.clear();
                self.set_scroll(ScrollId::Panel, 0.0);
            }
            Action::OpenNav => {
                self.overlay = if self.overlay == Overlay::Nav {
                    Overlay::None
                } else {
                    Overlay::Nav
                };
            }
            Action::CloseOverlay => self.overlay = Overlay::None,
            Action::Navigate(m) => self.navigate(*m),
            Action::Open(o) => {
                self.overlay = if self.overlay == *o { Overlay::None } else { *o };
                if let Overlay::Panel(t) = o {
                    self.panel_tab = *t;
                }
                if let Overlay::Calendar(t) = o {
                    let d = if *t == 0 { self.date.from } else { self.date.to };
                    self.cal_view = (d.y, d.m);
                }
                if let Overlay::Combo(id) = o {
                    // Scroll so the selected entry is visible.
                    let sel = self.combo_selected(*id) as f32;
                    self.set_scroll(ScrollId::Combo, (sel - 3.0).max(0.0) * 40.0);
                }
            }
            Action::PanelTab(t) => {
                self.panel_tab = *t;
                self.set_scroll(ScrollId::Panel, 0.0);
            }
            Action::CycleAngle => {
                self.sci.angle = match self.sci.angle {
                    Angle::Deg => Angle::Rad,
                    Angle::Rad => Angle::Grad,
                    Angle::Grad => Angle::Deg,
                }
            }
            Action::ToggleFE => self.sci.fe = !self.sci.fe,
            Action::Toggle2nd => self.sci_2nd = !self.sci_2nd,
            Action::TrigInv => self.trig_inv = !self.trig_inv,
            Action::TrigHyp => self.trig_hyp = !self.trig_hyp,
            Action::Radix(r) => self.prog.set_radix(*r),
            Action::CycleWord => {
                let w = self.prog.word.next();
                self.prog.set_word(w);
            }
            Action::BitPad(b) => self.bitpad = *b,
            Action::SetShift(m) => {
                self.prog.shift = *m;
                self.overlay = Overlay::None;
            }
            Action::ToggleBit(b) => self.prog.toggle_bit(*b),
            Action::ComboPick(id, i) => {
                self.combo_pick(*id, *i);
                self.overlay = Overlay::None;
            }
            Action::CalMonth(delta) => {
                let (y, m) = self.cal_view;
                let total = y as i64 * 12 + (m as i64 - 1) + *delta as i64;
                let ny = total.div_euclid(12) as i32;
                if (date::MIN_YEAR..=date::MAX_YEAR).contains(&ny) {
                    self.cal_view = (ny, (total.rem_euclid(12) + 1) as u32);
                }
            }
            Action::CalPick(t, d) => {
                if *t == 0 {
                    self.date.from = *d;
                } else {
                    self.date.to = *d;
                }
                self.overlay = Overlay::None;
            }
            Action::DateSubtract(s) => self.date.subtract = *s,
            Action::ConvField(f) => self.set_conv_field(*f),
            Action::Theme(t) => self.theme_pref = *t,
            Action::KeepOnTop(on) => {
                self.keep_on_top = *on;
                self.overlay = Overlay::None;
            }
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::None => {}
        }
    }

    fn close_flyout_panel(&mut self) {
        if matches!(self.overlay, Overlay::Panel(_)) {
            self.overlay = Overlay::None;
        }
    }

    pub fn navigate(&mut self, m: Mode) {
        if self.mode != m {
            self.mode = m;
            if !matches!(m, Mode::Standard) {
                self.keep_on_top = false;
            }
            if let Mode::Converter(_) = m {
                self.conv_active = 0;
                self.conv_entry = "0".into();
            }
            if m == Mode::Programmer && self.panel_tab == PanelTab::History {
                self.panel_tab = PanelTab::Memory;
            }
            self.set_scroll(ScrollId::Panel, 0.0);
        }
        self.overlay = Overlay::None;
    }

    fn key(&mut self, k: Key) {
        match self.mode {
            Mode::Converter(_) => self.conv_key(k),
            _ => {
                // Trig/function flyouts apply a function then close.
                if matches!(k, Key::Unary(_) | Key::Rand) && matches!(self.overlay, Overlay::Trig | Overlay::Func | Overlay::Bitwise) {
                    self.overlay = Overlay::None;
                }
                if matches!(k, Key::Bin(_)) && self.overlay == Overlay::Bitwise {
                    self.overlay = Overlay::None;
                }
                let Some(e) = self.engine_mut() else { return };
                if let Some(h) = e.press(k) {
                    self.history.insert(0, h);
                    self.history.truncate(MAX_HISTORY);
                }
            }
        }
    }

    fn memory_op(&mut self, op: MemOp) {
        let Some(e) = self.engine() else { return };
        if e.error().is_some() && op != MemOp::Clear && op != MemOp::Recall {
            return;
        }
        let v = e.value();
        if matches!(op, MemOp::Store | MemOp::Add | MemOp::Sub) {
            self.engine_mut().expect("calculator").commit();
        }
        match op {
            MemOp::Store => {
                self.memory.insert(0, v);
                self.memory.truncate(MAX_MEMORY);
            }
            MemOp::Clear => self.memory.clear(),
            MemOp::Recall => {
                if let Some(m) = self.memory.first().cloned() {
                    self.engine_mut().expect("calculator").set_value(m);
                }
            }
            MemOp::Add | MemOp::Sub => {
                if self.memory.is_empty() {
                    self.memory.push(Dec::zero());
                }
                self.memory_item(0, op);
            }
        }
    }

    fn memory_item(&mut self, i: usize, op: MemOp) {
        let Some(e) = self.engine() else { return };
        let v = e.value();
        let prog = self.mode == Mode::Programmer;
        let word = self.prog.word;
        let Some(m) = self.memory.get_mut(i) else { return };
        match op {
            MemOp::Add | MemOp::Sub => {
                let r = if op == MemOp::Add { m.add(&v) } else { m.sub(&v) };
                *m = if prog {
                    Dec::from_i64(word.wrap(r.to_i128().unwrap_or(0)).0)
                } else {
                    r.check().unwrap_or_else(|_| m.clone())
                };
            }
            MemOp::Clear => {
                self.memory.remove(i);
            }
            MemOp::Recall | MemOp::Store => {
                let m = m.clone();
                self.engine_mut().expect("calculator").set_value(m);
                self.close_flyout_panel();
            }
        }
    }

    pub fn format_memory(&self, v: &Dec) -> String {
        match self.engine() {
            Some(e) => e.format_value(v),
            None => crate::format::display(v, 32, false),
        }
    }

    // ------------------------------------------------------------ combos

    pub fn combo_items(&self, id: ComboId) -> Vec<String> {
        match id {
            ComboId::DateMode => vec!["Difference between dates".into(), "Add or subtract days".into()],
            ComboId::Years | ComboId::Months | ComboId::Days => (0..=999).map(|n| n.to_string()).collect(),
            ComboId::ConvFrom | ComboId::ConvTo => self.conv_category().units.iter().map(|u| u.name.clone()).collect(),
        }
    }

    pub fn combo_selected(&self, id: ComboId) -> usize {
        match id {
            ComboId::DateMode => self.date.add_mode as usize,
            ComboId::Years => self.date.years as usize,
            ComboId::Months => self.date.months as usize,
            ComboId::Days => self.date.days as usize,
            ComboId::ConvFrom => self.conv_units().0,
            ComboId::ConvTo => self.conv_units().1,
        }
    }

    pub fn combo_label(&self, id: ComboId) -> String {
        let i = self.combo_selected(id);
        match id {
            ComboId::ConvFrom | ComboId::ConvTo => self.conv_category().units[i].name.clone(),
            _ => self.combo_items(id).swap_remove(i),
        }
    }

    pub fn combo_pick(&mut self, id: ComboId, i: usize) {
        let n = self.combo_items(id).len();
        if n == 0 {
            return;
        }
        let i = i.min(n - 1);
        match id {
            ComboId::DateMode => self.date.add_mode = i == 1,
            ComboId::Years => self.date.years = i as u32,
            ComboId::Months => self.date.months = i as u32,
            ComboId::Days => self.date.days = i as u32,
            ComboId::ConvFrom | ComboId::ConvTo => {
                if let Mode::Converter(c) = self.mode {
                    let sel = &mut self.conv_sel[c as usize];
                    if id == ComboId::ConvFrom {
                        sel.0 = i;
                    } else {
                        sel.1 = i;
                    }
                }
            }
        }
    }

    /// Up/Down on a focused combo box.
    pub fn combo_step(&mut self, id: ComboId, delta: i64) {
        let n = self.combo_items(id).len() as i64;
        let cur = self.combo_selected(id) as i64;
        self.combo_pick(id, (cur + delta).clamp(0, n - 1) as usize);
    }

    // ------------------------------------------------------------ date

    pub fn date_difference(&self) -> date::Difference {
        date::difference(self.date.from, self.date.to)
    }

    pub fn date_result(&self) -> Option<Date> {
        let d = &self.date;
        d.from.add(d.years as i64, d.months as i64, d.days as i64, d.subtract)
    }

    // ------------------------------------------------------------ converter

    pub fn conv_category(&self) -> &Category {
        let i = match self.mode {
            Mode::Converter(i) => i as usize,
            _ => 1,
        };
        &self.categories[i]
    }

    pub fn conv_units(&self) -> (usize, usize) {
        match self.mode {
            Mode::Converter(i) => self.conv_sel[i as usize],
            _ => (0, 0),
        }
    }

    fn conv_entry_value(&self) -> Dec {
        self.conv_entry.trim_end_matches('.').parse().unwrap_or_else(|_| Dec::zero())
    }

    /// Display strings for the two converter fields.
    pub fn conv_texts(&self) -> (String, String) {
        let (from, to) = self.conv_units();
        let cat = self.conv_category();
        let typed = crate::format::group_decimal(&self.conv_entry);
        let v = self.conv_entry_value();
        let (src, dst) = if self.conv_active == 0 { (from, to) } else { (to, from) };
        let other = match cat.convert(&v, src, dst) {
            Ok(r) => crate::format::display(&r, CONV_MAX_DIGITS, false),
            Err(e) => e.message().to_string(),
        };
        if self.conv_active == 0 {
            (typed, other)
        } else {
            (other, typed)
        }
    }

    fn set_conv_field(&mut self, f: u8) {
        if f == self.conv_active {
            return;
        }
        let (a, b) = self.conv_texts();
        let shown = if f == 0 { a } else { b };
        let plain: String = shown.chars().filter(|c| *c != ',').collect();
        self.conv_active = f;
        self.conv_entry = if plain.contains('e') || plain.parse::<Dec>().is_err() {
            "0".into()
        } else {
            plain
        };
    }

    fn conv_key(&mut self, k: Key) {
        let s = &mut self.conv_entry;
        match k {
            Key::Digit(d) if d < 10 => {
                let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
                if s == "0" || s == "-0" {
                    s.pop();
                    s.push((b'0' + d) as char);
                } else if digits < CONV_MAX_DIGITS {
                    s.push((b'0' + d) as char);
                }
            }
            Key::Point => {
                if !s.contains('.') {
                    s.push('.');
                }
            }
            Key::Back => {
                s.pop();
                if s.is_empty() || s == "-" {
                    *s = "0".into();
                }
            }
            Key::ClearEntry | Key::Clear => *s = "0".into(),
            Key::Negate => {
                if let Some(r) = s.strip_prefix('-') {
                    *s = r.to_string();
                } else if s != "0" {
                    s.insert(0, '-');
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ clipboard

    /// Text placed on the clipboard by Ctrl+C.
    pub fn copy_text(&self) -> Option<String> {
        match self.mode {
            Mode::Standard | Mode::Scientific | Mode::Programmer => {
                let e = self.engine()?;
                if e.error().is_some() {
                    return None;
                }
                let t = e.display_text();
                Some(t.chars().filter(|c| *c != ',' && *c != ' ').collect())
            }
            Mode::Converter(_) => {
                let (a, b) = self.conv_texts();
                let t = if self.conv_active == 0 { b } else { a };
                Some(t.replace(',', ""))
            }
            Mode::Date => Some(if self.date.add_mode {
                self.date_result().map(|d| d.long_text()).unwrap_or_default()
            } else {
                self.date_difference().text()
            }),
            Mode::Settings => None,
        }
    }

    fn copy(&mut self) {
        if let Some(t) = self.copy_text() {
            if platform::set_clipboard(&t) {
                self.toast = Some("Copied".into());
            }
        }
    }

    fn paste(&mut self) {
        let Some(text) = platform::get_clipboard() else { return };
        self.paste_text(&text);
    }

    pub fn paste_text(&mut self, text: &str) {
        match self.mode {
            Mode::Converter(_) => {
                let t: String = text.trim().chars().filter(|c| *c != ',').collect();
                if let Ok(v) = t.parse::<Dec>() {
                    self.conv_entry = v.format(CONV_MAX_DIGITS, false);
                    if self.conv_entry.contains('e') {
                        self.conv_entry = "0".into();
                        self.toast = Some("Number too large to paste".into());
                    }
                } else {
                    self.toast = Some("Invalid input".into());
                }
            }
            _ => {
                if let Some(e) = self.engine_mut() {
                    if !e.paste(text) {
                        self.toast = Some("Invalid input".into());
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ persistence

    pub fn load(&mut self, s: &Store) {
        let mode = match s.get("mode") {
            Some("Scientific") => Mode::Scientific,
            Some("Programmer") => Mode::Programmer,
            Some("Date") => Mode::Date,
            Some(c) if c.starts_with("Converter:") => {
                let name = &c["Converter:".len()..];
                match self.categories.iter().position(|cat| cat.name == name) {
                    Some(i) => Mode::Converter(i as u8),
                    None => Mode::Standard,
                }
            }
            _ => Mode::Standard,
        };
        self.mode = mode;
        self.theme_pref = match s.get("theme") {
            Some("light") => ThemePref::Light,
            Some("dark") => ThemePref::Dark,
            _ => ThemePref::System,
        };
        self.sci.angle = match s.get("angle") {
            Some("rad") => Angle::Rad,
            Some("grad") => Angle::Grad,
            _ => Angle::Deg,
        };
        if let Some(w) = s.get("word").and_then(WordSize::from_name) {
            self.prog.set_word(w);
        }
        if let Some(r) = s.get("radix").and_then(Radix::from_name) {
            self.prog.set_radix(r);
        }
        if let Some(m) = s.get("shift").and_then(ShiftMode::from_name) {
            self.prog.shift = m;
        }
        self.date.add_mode = s.get("date_mode") == Some("add");
        for line in s.get_all("conv") {
            // conv=<category>\t<from abbr>\t<to abbr>
            let mut p = line.split('\t');
            let (Some(cat), Some(f), Some(t)) = (p.next(), p.next(), p.next()) else {
                continue;
            };
            if let Some(ci) = self.categories.iter().position(|c| c.name == cat) {
                let c = &self.categories[ci];
                if let (Some(fi), Some(ti)) = (c.find(f), c.find(t)) {
                    self.conv_sel[ci] = (fi, ti);
                }
            }
        }
        self.history = s
            .get_all("history")
            .filter_map(|l| {
                let mut p = l.split('\t');
                let (Some(e), Some(r), Some(v)) = (p.next(), p.next(), p.next()) else {
                    return None;
                };
                Some(HistoryEntry {
                    expression: e.into(),
                    result: r.into(),
                    value: v.parse().ok()?,
                })
            })
            .take(MAX_HISTORY)
            .collect();
        self.memory = s.get_all("memory").filter_map(|v| v.parse().ok()).take(MAX_MEMORY).collect();
        if let (Some(w), Some(h)) = (
            s.get("width").and_then(|v| v.parse().ok()),
            s.get("height").and_then(|v| v.parse().ok()),
        ) {
            self.window_size = Some((w, h));
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        let mode = match self.mode {
            Mode::Standard | Mode::Settings => "Standard".to_string(),
            Mode::Scientific => "Scientific".into(),
            Mode::Programmer => "Programmer".into(),
            Mode::Date => "Date".into(),
            Mode::Converter(i) => format!("Converter:{}", self.categories[i as usize].name),
        };
        s.set("mode", mode);
        s.set(
            "theme",
            match self.theme_pref {
                ThemePref::System => "system",
                ThemePref::Light => "light",
                ThemePref::Dark => "dark",
            },
        );
        s.set(
            "angle",
            match self.sci.angle {
                Angle::Deg => "deg",
                Angle::Rad => "rad",
                Angle::Grad => "grad",
            },
        );
        s.set("word", self.prog.word.label());
        s.set("radix", self.prog.radix.label());
        s.set("shift", self.prog.shift.name());
        s.set("date_mode", if self.date.add_mode { "add" } else { "difference" });
        if let Some((w, h)) = self.window_size {
            s.set("width", format!("{w:.0}"));
            s.set("height", format!("{h:.0}"));
        }
        for (ci, c) in self.categories.iter().enumerate() {
            let (f, t) = self.conv_sel[ci];
            if (f, t) != c.defaults {
                s.push("conv", format!("{}\t{}\t{}", c.name, c.units[f].abbr, c.units[t].abbr));
            }
        }
        for h in &self.history {
            s.push("history", format!("{}\t{}\t{}", h.expression, h.result, h.value.to_exact_string()));
        }
        for m in &self.memory {
            s.push("memory", m.to_exact_string());
        }
        s
    }

    pub fn save(&mut self) {
        let store = self.to_store();
        self.location.save(&store);
    }
}

// ---------------------------------------------------------------- keyboard

/// A key press, independent of the windowing library.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyIn {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

fn trig(f: Trig, inv: bool, hyp: bool) -> Key {
    Key::Unary(Unary::Trig { f, inv, hyp })
}

impl App {
    /// Maps a shortcut to an action for the current mode (keyboard focus
    /// handling such as Tab/Space/Enter is done by the UI layer).
    pub fn shortcut(&self, k: &KeyIn, m: Mods) -> Option<Action> {
        use KeyIn::*;
        // Global shortcuts.
        if m.alt && !m.ctrl {
            if let Char(c) = k {
                return match c {
                    '1' => Some(Action::Navigate(Mode::Standard)),
                    '2' => Some(Action::Navigate(Mode::Scientific)),
                    '4' => Some(Action::Navigate(Mode::Programmer)),
                    '5' => Some(Action::Navigate(Mode::Date)),
                    _ => None,
                };
            }
            return None;
        }
        if m.ctrl {
            let Char(c) = k else { return None };
            let c = c.to_ascii_lowercase();
            let calc = self.is_calculator();
            let sci = self.mode == Mode::Scientific;
            return match c {
                'c' => Some(Action::Copy),
                'v' => Some(Action::Paste),
                'm' if calc => Some(Action::Mem(MemOp::Store)),
                'p' if calc => Some(Action::Mem(MemOp::Add)),
                'q' if calc => Some(Action::Mem(MemOp::Sub)),
                'r' if calc => Some(Action::Mem(MemOp::Recall)),
                'l' if calc => Some(Action::Mem(MemOp::Clear)),
                'h' if self.has_history() => Some(Action::Open(Overlay::Panel(PanelTab::History))),
                'd' if m.shift && self.has_history() => Some(Action::ClearHistory),
                's' if sci => Some(Action::Key(trig(Trig::Sin, m.shift, true))),
                'o' if sci => Some(Action::Key(trig(Trig::Cos, m.shift, true))),
                't' if sci => Some(Action::Key(trig(Trig::Tan, m.shift, true))),
                'g' if sci => Some(Action::Key(Key::Unary(Unary::Pow10))),
                'n' if sci => Some(Action::Key(Key::Unary(Unary::Exp))),
                'y' if sci => Some(Action::Key(Key::Bin(BinOp::Root))),
                'b' if sci => Some(Action::Key(Key::Unary(Unary::Cbrt))),
                _ => None,
            };
        }
        match self.mode {
            Mode::Standard | Mode::Scientific | Mode::Programmer => self.calc_shortcut(k, m),
            Mode::Converter(_) => match k {
                Char(c @ '0'..='9') => Some(Action::Key(Key::Digit(*c as u8 - b'0'))),
                Char('.') | Char(',') => Some(Action::Key(Key::Point)),
                Backspace => Some(Action::Key(Key::Back)),
                Delete | Escape => Some(Action::Key(Key::ClearEntry)),
                F(9) | Char('_') => Some(Action::Key(Key::Negate)),
                _ => None,
            },
            _ => None,
        }
    }

    fn calc_shortcut(&self, k: &KeyIn, m: Mods) -> Option<Action> {
        use KeyIn::*;
        let key = |k: Key| Some(Action::Key(k));
        let sci = self.mode == Mode::Scientific;
        let prog = self.mode == Mode::Programmer;
        let std = self.mode == Mode::Standard;
        match k {
            Enter => key(Key::Equals),
            Backspace => key(Key::Back),
            Delete => key(Key::ClearEntry),
            Escape => key(Key::Clear),
            F(9) => key(Key::Negate),
            F(n) if prog => match n {
                5 => Some(Action::Radix(Radix::Hex)),
                6 => Some(Action::Radix(Radix::Dec)),
                7 => Some(Action::Radix(Radix::Oct)),
                8 => Some(Action::Radix(Radix::Bin)),
                // F12/F2/F3/F4 select the word size directly in Windows; we cycle to it.
                12 | 2 | 3 | 4 => {
                    let target = match n {
                        12 => WordSize::Qword,
                        2 => WordSize::Dword,
                        3 => WordSize::Word,
                        _ => WordSize::Byte,
                    };
                    (self.prog.word != target).then_some(Action::CycleWord)
                }
                _ => None,
            },
            F(3) if sci => Some(Action::CycleAngle),
            Char(c) => {
                let c = *c;
                if let Some(d) = c.to_digit(10) {
                    return key(Key::Digit(d as u8));
                }
                if prog && self.prog.radix == Radix::Hex {
                    if let Some(d) = c.to_digit(16) {
                        if !m.ctrl {
                            return key(Key::Digit(d as u8));
                        }
                    }
                }
                match c {
                    '.' | ',' if !prog => key(Key::Point),
                    '+' => key(Key::Bin(BinOp::Add)),
                    '-' => key(Key::Bin(BinOp::Sub)),
                    '*' => key(Key::Bin(BinOp::Mul)),
                    '/' => key(Key::Bin(BinOp::Div)),
                    '=' => key(Key::Equals),
                    '%' if std => key(Key::Percent),
                    '%' => key(Key::Bin(BinOp::Mod)),
                    '(' if !std => key(Key::LParen),
                    ')' if !std => key(Key::RParen),
                    '^' if prog => key(Key::Bin(BinOp::Xor)),
                    '&' if prog => key(Key::Bin(BinOp::And)),
                    '|' if prog => key(Key::Bin(BinOp::Or)),
                    '~' if prog => key(Key::Unary(Unary::Not)),
                    '<' if prog => key(if self.prog.shift == ShiftMode::Rotate {
                        Key::Unary(Unary::Rol)
                    } else {
                        Key::Bin(BinOp::Shl)
                    }),
                    '>' if prog => key(if self.prog.shift == ShiftMode::Rotate {
                        Key::Unary(Unary::Ror)
                    } else {
                        Key::Bin(BinOp::Shr)
                    }),
                    _ if prog => None,
                    'r' | 'R' => key(Key::Unary(Unary::Recip)),
                    '@' => key(Key::Unary(Unary::Sqrt)),
                    'q' | 'Q' => key(Key::Unary(Unary::Square)),
                    _ if !sci => None,
                    '^' | 'y' | 'Y' => key(Key::Bin(BinOp::Pow)),
                    '#' => key(Key::Unary(Unary::Cube)),
                    '!' => key(Key::Unary(Unary::Fact)),
                    'p' | 'P' => key(Key::Const(Constant::Pi)),
                    'e' | 'E' | 'x' | 'X' => key(Key::Exp),
                    's' => key(trig(Trig::Sin, false, false)),
                    'o' => key(trig(Trig::Cos, false, false)),
                    't' => key(trig(Trig::Tan, false, false)),
                    'S' => key(trig(Trig::Sin, true, false)),
                    'O' => key(trig(Trig::Cos, true, false)),
                    'T' => key(trig(Trig::Tan, true, false)),
                    'l' | 'L' => key(Key::Unary(Unary::Log)),
                    'n' | 'N' => key(Key::Unary(Unary::Ln)),
                    'm' | 'M' => key(Key::Bin(BinOp::Mod)),
                    'v' | 'V' => Some(Action::ToggleFE),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(Location {
            dir: None,
            portable: false,
            error: None,
        })
    }

    fn press(a: &mut App, s: &str) {
        for c in s.chars() {
            let k = match c {
                '\n' => KeyIn::Enter,
                c => KeyIn::Char(c),
            };
            if let Some(act) = a.shortcut(&k, Mods::default()) {
                a.apply(&act);
            }
        }
    }

    #[test]
    fn keyboard_drives_calculator_and_history() {
        let mut a = app();
        press(&mut a, "12+30=");
        assert_eq!(a.std.display_text(), "42");
        assert_eq!(a.history.len(), 1);
        assert_eq!(a.history[0].expression, "12 + 30 =");
        a.navigate(Mode::Scientific);
        press(&mut a, "2+3*4\n");
        assert_eq!(a.sci.display_text(), "14");
        assert_eq!(a.history.len(), 2);
        a.apply(&Action::HistoryItem(1));
        assert_eq!(a.sci.display_text(), "42");
    }

    #[test]
    fn memory_operations() {
        let mut a = app();
        press(&mut a, "5");
        a.apply(&Action::Mem(MemOp::Store));
        a.apply(&Action::Mem(MemOp::Add));
        assert_eq!(a.memory, vec![Dec::from_i64(10)]);
        press(&mut a, "3");
        a.apply(&Action::Mem(MemOp::Sub));
        assert_eq!(a.memory, vec![Dec::from_i64(7)]);
        a.apply(&Action::Key(Key::Clear));
        a.apply(&Action::Mem(MemOp::Recall));
        assert_eq!(a.std.display_text(), "7");
        a.apply(&Action::MemItem(0, MemOp::Clear));
        assert!(a.memory.is_empty());
        // M+ with empty memory stores the value.
        a.apply(&Action::Mem(MemOp::Add));
        assert_eq!(a.memory.len(), 1);
    }

    #[test]
    fn converter_typing_and_swapping() {
        let mut a = app();
        let len = a.categories.iter().position(|c| c.name == "Length").unwrap();
        a.navigate(Mode::Converter(len as u8));
        let c = a.conv_category();
        let (cm, inch) = (c.find("cm").unwrap(), c.find("in").unwrap());
        assert_eq!(a.conv_units(), (cm, inch));
        press(&mut a, "254");
        assert_eq!(a.conv_texts(), ("254".to_string(), "100".to_string()));
        a.apply(&Action::ConvField(1));
        a.apply(&Action::Key(Key::Back));
        assert_eq!(a.conv_texts().1, "10");
        assert_eq!(a.conv_texts().0, "25.4");
    }

    #[test]
    fn date_calculation() {
        let mut a = app();
        a.date.from = Date::new(2026, 1, 31).unwrap();
        a.date.to = Date::new(2026, 3, 1).unwrap();
        assert_eq!(a.date_difference().text(), "1 month, 1 day");
        a.combo_pick(ComboId::Months, 1);
        assert_eq!(a.date_result(), Date::new(2026, 2, 28));
        a.apply(&Action::DateSubtract(true));
        assert_eq!(a.date_result(), Date::new(2025, 12, 31));
    }

    #[test]
    fn persistence_round_trip() {
        let mut a = app();
        press(&mut a, "2*21=");
        a.apply(&Action::Mem(MemOp::Store));
        a.navigate(Mode::Programmer);
        a.apply(&Action::Radix(Radix::Hex));
        a.apply(&Action::CycleWord);
        a.apply(&Action::Theme(ThemePref::Dark));
        let store = a.to_store();
        let mut b = app();
        b.load(&crate::storage::Store::parse(&store.serialize()));
        assert_eq!(b.mode, Mode::Programmer);
        assert_eq!(b.prog.radix, Radix::Hex);
        assert_eq!(b.prog.word, WordSize::Dword);
        assert_eq!(b.theme_pref, ThemePref::Dark);
        assert_eq!(b.history, a.history);
        assert_eq!(b.memory, vec![Dec::from_i64(42)]);
    }

    #[test]
    fn programmer_hex_keys() {
        let mut a = app();
        a.navigate(Mode::Programmer);
        a.apply(&Action::Radix(Radix::Hex));
        press(&mut a, "ff+1=");
        assert_eq!(a.prog.display_text(), "100");
        a.apply(&Action::Radix(Radix::Dec));
        assert_eq!(a.prog.display_text(), "256");
    }
}
