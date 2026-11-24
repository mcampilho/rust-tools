//! Printing: Windows' own Print dialog and a print job that takes finished pages as bitmaps. The apps draw
//! their pages on the CPU (as they draw their windows) at the printer's resolution, up to 300 dpi, and each
//! page is stretched onto the printer. Printing to PDF, to XPS or to a paper printer is the same to us.

use super::wide;
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::Xps::{AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
use windows_sys::Win32::UI::Controls::Dialogs::*;

/// Finest resolution the pages are drawn at: more would only cost memory (A4 at 300 dpi is 8.7 megapixels).
const MAX_RENDER_DPI: f32 = 300.0;

/// A printer chosen in the Print dialog, with what was asked for there.
pub struct Printer {
    dc: HDC,
    /// Printable area in device pixels.
    width: u32,
    height: u32,
    dpi_x: u32,
    dpi_y: u32,
    /// First and last page wanted (1-based, inclusive), or `None` for all of them.
    pub pages: Option<(u32, u32)>,
    /// A file to print into instead of the printer's port (printing to PDF or XPS without asking for a name).
    output: Option<Vec<u16>>,
}

/// A printer by name, without the dialog: all the pages, to its port or into `output` if it can print to a file.
pub fn named(name: &str, output: Option<&std::path::Path>) -> Result<Printer, String> {
    let (driver, device) = (wide("WINSPOOL"), wide(name));
    // SAFETY: NUL-terminated strings that outlive the call.
    let dc = unsafe { CreateDCW(driver.as_ptr(), device.as_ptr(), std::ptr::null(), std::ptr::null()) };
    if dc.is_null() {
        return Err(format!("{name}: {}", std::io::Error::last_os_error()));
    }
    Printer::from_dc(dc, None, output.map(|p| wide(&p.to_string_lossy())))
}

// The handle is only ever used by one thread at a time (`rt_ui::print::start` hands the printer over).
// SAFETY: a printer DC has no thread affinity as long as calls do not overlap.
unsafe impl Send for Printer {}

impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: a DC the dialog gave us, deleted once.
        unsafe { DeleteDC(self.dc) };
    }
}

/// Shows the Print dialog. `None` if it was cancelled or no printer could be had.
pub fn choose(hwnd: HWND) -> Option<Printer> {
    // SAFETY: zeroed is a valid initial state for this plain C struct; the dialog fills it.
    let mut pd: PRINTDLGW = unsafe { std::mem::zeroed() };
    pd.lStructSize = std::mem::size_of::<PRINTDLGW>() as u32;
    pd.hwndOwner = hwnd;
    // The driver counts the copies; the page range is free (the number of pages depends on the paper).
    pd.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOCURRENTPAGE | PD_USEDEVMODECOPIESANDCOLLATE;
    pd.nMinPage = 1;
    pd.nMaxPage = 9999;
    pd.nFromPage = 1;
    pd.nToPage = 1;
    // SAFETY: pd is live for the call; the handles it returns are ours to free.
    let ok = unsafe { PrintDlgW(&mut pd) } != 0;
    // SAFETY: handles from the dialog, freed once.
    unsafe {
        if !pd.hDevMode.is_null() {
            GlobalFree(pd.hDevMode);
        }
        if !pd.hDevNames.is_null() {
            GlobalFree(pd.hDevNames);
        }
    }
    if !ok || pd.hDC.is_null() {
        return None;
    }
    let pages = (pd.Flags & PD_PAGENUMS != 0).then(|| (u32::from(pd.nFromPage.max(1)), u32::from(pd.nToPage.max(pd.nFromPage).max(1))));
    Printer::from_dc(pd.hDC, pages, None).ok()
}

impl Printer {
    /// Takes over a printer DC (deleted when the `Printer` goes).
    fn from_dc(dc: HDC, pages: Option<(u32, u32)>, output: Option<Vec<u16>>) -> Result<Printer, String> {
        // SAFETY: querying a valid DC.
        let (width, height, dpi_x, dpi_y) = unsafe {
            (
                GetDeviceCaps(dc, HORZRES as i32),
                GetDeviceCaps(dc, VERTRES as i32),
                GetDeviceCaps(dc, LOGPIXELSX as i32),
                GetDeviceCaps(dc, LOGPIXELSY as i32),
            )
        };
        if width <= 0 || height <= 0 || dpi_x <= 0 || dpi_y <= 0 {
            // SAFETY: the DC is ours.
            unsafe { DeleteDC(dc) };
            return Err("the printer has no printable area".to_string());
        }
        Ok(Printer { dc, width: width as u32, height: height as u32, dpi_x: dpi_x as u32, dpi_y: dpi_y as u32, pages, output })
    }

    /// Resolution the pages are drawn at, in dots per inch (the same both ways).
    pub fn render_dpi(&self) -> f32 {
        (self.dpi_x.max(self.dpi_y) as f32).min(MAX_RENDER_DPI)
    }

    /// Size in pixels of the pages as they are drawn: the printable area at `render_dpi`.
    pub fn page_size(&self) -> (u32, u32) {
        let k = self.render_dpi();
        (
            ((self.width as f32 * k / self.dpi_x as f32).round() as u32).max(1),
            ((self.height as f32 * k / self.dpi_y as f32).round() as u32).max(1),
        )
    }

    /// Prints pages `1..=total` (or those asked for in the dialog). `draw(n, pixels)` fills page `n` (0-based)
    /// as `page_size()` pixels, rows from the top, each `0x00RRGGBB`. Returns how many pages were printed.
    pub fn print(&self, title: &str, total: u32, draw: &mut dyn FnMut(u32, &mut Vec<u32>)) -> Result<u32, String> {
        let (from, to) = match self.pages {
            Some((a, b)) => (a, b.min(total)),
            None => (1, total),
        };
        if total == 0 || from > to {
            return Ok(0);
        }
        let (w, h) = self.page_size();
        let name = wide(title);
        let doc = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: name.as_ptr(),
            lpszOutput: self.output.as_ref().map_or(std::ptr::null(), |o| o.as_ptr()),
            lpszDatatype: std::ptr::null(),
            fwType: 0,
        };
        // SAFETY: the DC is valid; `doc` and the name outlive the call.
        if unsafe { StartDocW(self.dc, &doc) } <= 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: a valid DC.
        unsafe { SetStretchBltMode(self.dc, HALFTONE) };
        // A top-down 32-bit DIB: a `0x00RRGGBB` pixel is B, G, R, 0 in memory.
        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w as i32;
        bmi.bmiHeader.biHeight = -(h as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut pixels: Vec<u32> = Vec::new();
        let mut printed = 0;
        for page in from..=to {
            pixels.clear();
            pixels.resize(w as usize * h as usize, 0x00FF_FFFF);
            draw(page - 1, &mut pixels);
            pixels.resize(w as usize * h as usize, 0x00FF_FFFF);
            // SAFETY: the DC is valid; the pixels are `w * h` u32s as `bmi` says.
            let ok = unsafe {
                StartPage(self.dc) > 0
                    && StretchDIBits(
                        self.dc,
                        0,
                        0,
                        self.width as i32,
                        self.height as i32,
                        0,
                        0,
                        w as i32,
                        h as i32,
                        pixels.as_ptr().cast(),
                        &bmi,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    ) != GDI_ERROR
                    && EndPage(self.dc) > 0
            };
            if !ok {
                let e = std::io::Error::last_os_error().to_string();
                // SAFETY: ends the job we started.
                unsafe { AbortDoc(self.dc) };
                return Err(e);
            }
            printed += 1;
        }
        // SAFETY: ends the job we started.
        if unsafe { EndDoc(self.dc) } <= 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(printed)
    }
}
