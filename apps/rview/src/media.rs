//! Background work on PDF pages and images. Windows has the engines built in
//! (nothing to ship): `Windows.Data.Pdf` and its imaging codecs (PNG, JPEG, GIF,
//! BMP, TIFF, ICO, WebP/HEIF when installed). Linux uses the hayro and image
//! crates. One worker thread owns the open PDF; page requests replace older
//! pending ones so scrolling never queues stale work.

use crate::doc::pdf::PageText;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};

/// Decoded pixels, premultiplied RGBA.
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub enum Job {
    OpenPdf { path: PathBuf, gen: u64 },
    RenderPage { gen: u64, page: u32, width: u32 },
    Image { key: String, bytes: Vec<u8> },
}

pub enum Done {
    PdfOpened { gen: u64, sizes: Vec<(f32, f32)> },
    PdfFailed { gen: u64, message: String },
    Page { gen: u64, page: u32, width: u32, bmp: Bitmap },
    Image { key: String, bmp: Option<Bitmap> },
    /// The text of the PDF's pages (empty when it cannot be read).
    PdfText { gen: u64, pages: Vec<PageText> },
}

type Queue = Arc<(Mutex<VecDeque<Job>>, Condvar)>;
type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Media {
    queue: Queue,
    pub rx: mpsc::Receiver<Done>,
    tx: mpsc::Sender<Done>,
    wake: Wake,
    /// Document whose text is wanted; an older read stops when this changes.
    text_gen: Arc<AtomicU64>,
}

/// Largest bitmap edge produced (larger pages are drawn scaled up).
pub const MAX_EDGE: u32 = 4096;

impl Media {
    pub fn start(wake: Wake) -> Media {
        let queue: Queue = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        let q = queue.clone();
        let (worker_tx, worker_wake) = (tx.clone(), wake.clone());
        std::thread::Builder::new()
            .name("media".into())
            .spawn(move || worker(q, worker_tx, worker_wake))
            .expect("spawn media thread");
        Media { queue, rx, tx, wake, text_gen: Arc::new(AtomicU64::new(0)) }
    }

    /// Reads the text of a PDF on its own thread, so page rendering never
    /// waits for it. A file the reader cannot handle (even by panicking)
    /// only means that its text cannot be selected.
    pub fn read_pdf_text(&self, path: PathBuf, gen: u64, sizes: Vec<(f32, f32)>) {
        self.text_gen.store(gen, Ordering::Relaxed);
        let (tx, wake, current) = (self.tx.clone(), self.wake.clone(), self.text_gen.clone());
        let work = move || {
            let keep_going = || current.load(Ordering::Relaxed) == gen;
            let pages = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::doc::pdf::extract(&path, &sizes, &keep_going)));
            let pages = pages.ok().flatten().unwrap_or_default();
            if tx.send(Done::PdfText { gen, pages }).is_ok() {
                wake();
            }
        };
        let _ = std::thread::Builder::new().name("pdf-text".into()).stack_size(8 << 20).spawn(work);
    }

    pub fn send(&self, job: Job) {
        let (m, cv) = &*self.queue;
        if let Ok(mut q) = m.lock() {
            if matches!(job, Job::OpenPdf { .. }) {
                q.retain(|j| !matches!(j, Job::RenderPage { .. } | Job::OpenPdf { .. }));
            }
            q.push_back(job);
            cv.notify_one();
        }
    }

    /// Replaces all pending page renders with `pages` (page, width), in order.
    pub fn request_pages(&self, gen: u64, pages: &[(u32, u32)]) {
        let (m, cv) = &*self.queue;
        if let Ok(mut q) = m.lock() {
            q.retain(|j| !matches!(j, Job::RenderPage { .. }));
            for &(page, width) in pages {
                q.push_back(Job::RenderPage { gen, page, width });
            }
            cv.notify_one();
        }
    }
}

#[cfg(windows)]
mod winrt {
    use super::Bitmap;
    use windows::core::{Interface, HSTRING};
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};

    pub fn init() {
        // SAFETY: initializes COM for this worker thread (multithreaded apartment).
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
    }

    pub fn open_pdf(path: &std::path::Path) -> windows::core::Result<(PdfDocument, Vec<(f32, f32)>)> {
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(abs.as_path()))?.join()?;
        let doc = PdfDocument::LoadFromFileAsync(&file)?.join()?;
        let n = doc.PageCount()?;
        let mut sizes = Vec::with_capacity(n as usize);
        for i in 0..n {
            let s = doc.GetPage(i)?.Size()?;
            sizes.push((s.Width.max(1.0), s.Height.max(1.0)));
        }
        Ok((doc, sizes))
    }

    pub fn render_page(doc: &PdfDocument, page: u32, width: u32) -> windows::core::Result<Bitmap> {
        let p = doc.GetPage(page)?;
        let stream = InMemoryRandomAccessStream::new()?;
        let opts = PdfPageRenderOptions::new()?;
        opts.SetDestinationWidth(width)?;
        opts.SetBitmapEncoderId(BitmapEncoder::BmpEncoderId()?)?;
        p.RenderWithOptionsToStreamAsync(&stream, &opts)?.join()?;
        decode(&stream.cast()?, u32::MAX)
    }

    pub fn decode_bytes(bytes: &[u8], max: u32) -> windows::core::Result<Bitmap> {
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
        writer.WriteBytes(bytes)?;
        writer.StoreAsync()?.join()?;
        writer.FlushAsync()?.join()?;
        let _ = writer.DetachStream();
        stream.Seek(0)?;
        decode(&stream.cast()?, max)
    }

    fn decode(stream: &IRandomAccessStream, max: u32) -> windows::core::Result<Bitmap> {
        stream.Seek(0)?;
        let dec = BitmapDecoder::CreateAsync(stream)?.join()?;
        let (mut w, mut h) = (dec.OrientedPixelWidth()?, dec.OrientedPixelHeight()?);
        let t = BitmapTransform::new()?;
        if w > max || h > max {
            let k = max as f64 / w.max(h) as f64;
            // The transform scales before orientation; use the raw size.
            let (rw, rh) = (dec.PixelWidth()?, dec.PixelHeight()?);
            t.SetScaledWidth(((rw as f64 * k).round() as u32).max(1))?;
            t.SetScaledHeight(((rh as f64 * k).round() as u32).max(1))?;
            t.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
            w = ((w as f64 * k).round() as u32).max(1);
            h = ((h as f64 * k).round() as u32).max(1);
        }
        let pd = dec
            .GetPixelDataTransformedAsync(BitmapPixelFormat::Rgba8, BitmapAlphaMode::Premultiplied, &t, ExifOrientationMode::RespectExifOrientation, ColorManagementMode::DoNotColorManage)?
            .join()?;
        let data = pd.DetachPixelData()?;
        let rgba = data.as_slice().to_vec();
        if rgba.len() as u64 != w as u64 * h as u64 * 4 {
            return Err(windows::core::Error::from(windows::core::HRESULT(0x8007000Du32 as i32)));
        }
        Ok(Bitmap { w, h, rgba })
    }
}

#[cfg(not(windows))]
mod native {
    use super::{Bitmap, MAX_EDGE};
    use hayro::hayro_interpret::font::FontQuery;
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
    use std::sync::Arc;

    /// Page sizes are in DIPs (1/96 in), as `Windows.Data.Pdf` gives them.
    const DIPS_PER_POINT: f32 = 96.0 / 72.0;

    /// Runs code from a library that may panic on a malformed file.
    pub fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
    }

    pub fn open_pdf(path: &std::path::Path) -> Result<(Pdf, Vec<(f32, f32)>), String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let damaged = || crate::i18n::tr("The file is damaged or protected.");
        let pdf = Pdf::new(data).map_err(|_| damaged())?;
        let sizes: Vec<(f32, f32)> = pdf.pages().iter().map(|p| p.render_dimensions()).map(|(w, h)| ((w * DIPS_PER_POINT).max(1.0), (h * DIPS_PER_POINT).max(1.0))).collect();
        if sizes.is_empty() {
            return Err(damaged());
        }
        Ok((pdf, sizes))
    }

    pub fn render_page(pdf: &Pdf, page: u32, width: u32) -> Option<Bitmap> {
        let pages = pdf.pages();
        let p = pages.get(page as usize)?;
        let (w, h) = p.render_dimensions();
        let scale = (width as f32 / w).min(MAX_EDGE as f32 / h);
        let settings = InterpreterSettings {
            // Fonts that the PDF does not embed use the built-in standard ones.
            font_resolver: Arc::new(|query| match query {
                FontQuery::Standard(s) => Some(s.get_font_data()),
                FontQuery::Fallback(f) => Some(f.pick_standard_font().get_font_data()),
            }),
            ..Default::default()
        };
        let pixmap_settings = PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
        let px = render(p, &RenderCache::new(), &settings, &RenderSettings::default(), &pixmap_settings);
        Some(Bitmap { w: px.width() as u32, h: px.height() as u32, rgba: px.data_as_u8_slice().to_vec() })
    }

    pub fn decode_image(bytes: &[u8], max: u32) -> Option<Bitmap> {
        use image::{DynamicImage, ImageDecoder, ImageReader};
        let mut decoder = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_decoder().ok()?;
        let orientation = decoder.orientation().ok();
        let mut img = DynamicImage::from_decoder(decoder).ok()?;
        if let Some(o) = orientation {
            img.apply_orientation(o);
        }
        if img.width() > max || img.height() > max {
            img = img.thumbnail(max, max);
        }
        let mut rgba = img.into_rgba8();
        let (w, h) = rgba.dimensions();
        for p in rgba.pixels_mut() {
            let a = p[3] as u32;
            for c in &mut p.0[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
        Some(Bitmap { w, h, rgba: rgba.into_raw() })
    }
}

fn worker(queue: Queue, tx: mpsc::Sender<Done>, wake: Wake) {
    #[cfg(windows)]
    winrt::init();
    #[cfg(windows)]
    let mut pdf: Option<(u64, windows::Data::Pdf::PdfDocument)> = None;
    #[cfg(not(windows))]
    let mut pdf: Option<(u64, hayro::hayro_syntax::Pdf)> = None;
    loop {
        let job = {
            let (m, cv) = &*queue;
            let Ok(mut q) = m.lock() else { return };
            loop {
                if let Some(j) = q.pop_front() {
                    break j;
                }
                q = match cv.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };
        let done = match job {
            #[cfg(windows)]
            Job::OpenPdf { path, gen } => match winrt::open_pdf(&path) {
                Ok((doc, sizes)) => {
                    pdf = Some((gen, doc));
                    Done::PdfOpened { gen, sizes }
                }
                Err(e) => {
                    pdf = None;
                    Done::PdfFailed { gen, message: e.message().to_string() }
                }
            },
            #[cfg(windows)]
            Job::RenderPage { gen, page, width } => match &pdf {
                Some((g, doc)) if *g == gen => match winrt::render_page(doc, page, width.clamp(16, MAX_EDGE)) {
                    Ok(bmp) => Done::Page { gen, page, width, bmp },
                    Err(_) => continue,
                },
                _ => continue,
            },
            #[cfg(windows)]
            Job::Image { key, bytes } => Done::Image { key, bmp: winrt::decode_bytes(&bytes, MAX_EDGE).ok() },
            #[cfg(not(windows))]
            Job::OpenPdf { path, gen } => {
                let opened = native::guard(|| native::open_pdf(&path)).unwrap_or_else(|| Err(crate::i18n::tr("The file is damaged or protected.")));
                match opened {
                    Ok((doc, sizes)) => {
                        pdf = Some((gen, doc));
                        Done::PdfOpened { gen, sizes }
                    }
                    Err(message) => {
                        pdf = None;
                        Done::PdfFailed { gen, message }
                    }
                }
            }
            #[cfg(not(windows))]
            Job::RenderPage { gen, page, width } => match &pdf {
                Some((g, doc)) if *g == gen => match native::guard(|| native::render_page(doc, page, width.clamp(16, MAX_EDGE))) {
                    Some(Some(bmp)) => Done::Page { gen, page, width, bmp },
                    _ => continue,
                },
                _ => continue,
            },
            #[cfg(not(windows))]
            Job::Image { key, bytes } => Done::Image { key, bmp: native::guard(|| native::decode_image(&bytes, MAX_EDGE)).flatten() },
        };
        if tx.send(done).is_err() {
            return;
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A minimal one-page PDF (200 × 100 pt) with the word "Hello".
    fn tiny_pdf() -> Vec<u8> {
        let content = "BT /F1 36 Tf 20 35 Td (Hello) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
        out
    }

    fn next(m: &Media) -> Done {
        m.rx.recv_timeout(Duration::from_secs(20)).expect("media result")
    }

    #[test]
    fn renders_pdf_pages() {
        let path = std::env::temp_dir().join(format!("rview-test-{}.pdf", std::process::id()));
        std::fs::write(&path, tiny_pdf()).unwrap();
        let m = Media::start(Arc::new(|| {}));
        m.send(Job::OpenPdf { path: path.clone(), gen: 7 });
        match next(&m) {
            Done::PdfOpened { gen, sizes } => {
                assert_eq!(gen, 7);
                assert_eq!(sizes.len(), 1);
                // Points (1/72 in) become DIPs (1/96 in).
                assert!((sizes[0].0 - 266.67).abs() < 1.0 && (sizes[0].1 - 133.33).abs() < 1.0, "{sizes:?}");
            }
            _ => panic!("PDF not opened"),
        }
        m.request_pages(7, &[(0, 400)]);
        match next(&m) {
            Done::Page { page, width, bmp, .. } => {
                assert_eq!((page, width), (0, 400));
                // Scaling may round a pixel down.
                assert!(bmp.w.abs_diff(400) <= 1 && bmp.h.abs_diff(200) <= 1, "{}x{}", bmp.w, bmp.h);
                // White page with dark text pixels.
                let dark = bmp.rgba.chunks(4).filter(|p| p[0] < 100 && p[3] > 200).count();
                assert!(dark > 100, "text drawn ({dark} dark pixels)");
            }
            _ => panic!("page not rendered"),
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn broken_pdf_reports_an_error() {
        let path = std::env::temp_dir().join(format!("rview-bad-{}.pdf", std::process::id()));
        std::fs::write(&path, b"not a pdf").unwrap();
        let m = Media::start(Arc::new(|| {}));
        m.send(Job::OpenPdf { path: path.clone(), gen: 1 });
        assert!(matches!(next(&m), Done::PdfFailed { .. }));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn decodes_images() {
        let mut pm = tiny_skia::Pixmap::new(30, 20).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let png = pm.encode_png().unwrap();
        let m = Media::start(Arc::new(|| {}));
        m.send(Job::Image { key: "k".into(), bytes: png });
        match next(&m) {
            Done::Image { key, bmp: Some(b) } => {
                assert_eq!((key.as_str(), b.w, b.h), ("k", 30, 20));
                assert_eq!(&b.rgba[..4], &[255, 0, 0, 255]);
            }
            _ => panic!("image not decoded"),
        }
        m.send(Job::Image { key: "bad".into(), bytes: b"<svg/>".to_vec() });
        assert!(matches!(next(&m), Done::Image { bmp: None, .. }));
    }
}
