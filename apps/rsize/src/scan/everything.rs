//! Reading a folder from the index of Everything (voidtools), when it is
//! running: one query returns every file and folder below the folder, with
//! sizes and dates, in a fraction of the time a scan takes.
//!
//! Everything is asked through its window-message interface, the same one
//! its SDK uses, without the SDK's DLL: a `WM_COPYDATA` with a "query 2"
//! request goes to Everything's window, and the answer comes back as a
//! `WM_COPYDATA` to a hidden window of rsize.
//!
//! The answer is used only when it looks complete: Everything must know the
//! sizes, and it must list (nearly) everything the folder holds at its top
//! level. A folder outside Everything's index, or an index without sizes,
//! is read another way.

#[cfg(windows)]
use super::Progress;
use crate::model::{Entry, Kind, Tree};
use std::collections::HashMap;

/// Request flags: what each result carries, in this order in its data.
pub const REQUEST_NAME: u32 = 0x01;
pub const REQUEST_PATH: u32 = 0x02;
pub const REQUEST_SIZE: u32 = 0x10;
pub const REQUEST_DATE_MODIFIED: u32 = 0x40;
const REQUEST: u32 = REQUEST_NAME | REQUEST_PATH | REQUEST_SIZE | REQUEST_DATE_MODIFIED;
/// Result flags.
const ITEM_FOLDER: u32 = 0x01;
const ITEM_DRIVE: u32 = 0x02;

/// One result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub folder: bool,
    pub name: String,
    /// The folder it is in, as Everything writes it (`C:` for a drive root).
    pub path: String,
    /// None when Everything does not know it.
    pub size: Option<u64>,
    pub mtime: i64,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// A length-prefixed UTF-16 string followed by a terminator.
fn text_at(b: &[u8], o: &mut usize) -> Option<String> {
    let len = u32_at(b, *o)? as usize;
    let start = *o + 4;
    let bytes = b.get(start..start + len * 2)?;
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    *o = start + (len + 1) * 2;
    Some(String::from_utf16_lossy(&units))
}

/// Reads an answer ("list 2"): five counters, then a (flags, data offset)
/// pair per result, the offsets counted from the start.
pub fn parse_reply(b: &[u8]) -> Option<Vec<Item>> {
    let n = u32_at(b, 4)? as usize;
    let flags = u32_at(b, 12)?;
    if flags & REQUEST != REQUEST {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let base = 20 + i * 8;
        let item_flags = u32_at(b, base)?;
        let mut o = u32_at(b, base + 4)? as usize;
        let name = text_at(b, &mut o)?;
        let path = text_at(b, &mut o)?;
        let size = u64_at(b, o)?;
        let ft = u64_at(b, o + 8)? as i64;
        if item_flags & ITEM_DRIVE != 0 {
            continue;
        }
        out.push(Item {
            folder: item_flags & ITEM_FOLDER != 0,
            name,
            path,
            size: (size != u64::MAX).then_some(size),
            mtime: if ft > 0 { ft / 10_000_000 - 11_644_473_600 } else { 0 },
        });
    }
    Some(out)
}

/// The search for everything below `root`: its path with a backslash at the
/// end, in quotes (a search with a backslash matches whole paths).
pub fn search_for(root: &str) -> String {
    format!("\"{}\\\"", root.trim_end_matches('\\'))
}

fn key(path: &str) -> String {
    path.trim_end_matches('\\').to_lowercase()
}

/// Why the index was not used.
#[derive(Debug, PartialEq, Eq)]
pub enum Skip {
    /// Everything is not running, or does not answer.
    NotRunning,
    /// The folder is not (fully) in the index, or the index has no sizes.
    NotIndexed,
    Stopped,
}

/// Builds the tree of `root` from the results. `top` holds the names the
/// folder really contains at its top level, to check the index covers it;
/// `cluster` rounds sizes up to estimate the space on the disk.
pub fn build(root: &str, items: &[Item], top: &[String], cluster: u64, mtime: i64) -> Result<Tree, Skip> {
    let mut by_parent: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        by_parent.entry(key(&it.path)).or_default().push(i);
    }
    let root_key = key(root);
    let found = by_parent.get(&root_key).map_or(0, Vec::len);
    // Allow a few changes since Everything last looked.
    if top.is_empty() && found == 0 && items.is_empty() {
        let mut t = Tree::new(root, mtime);
        t.finish();
        return Ok(t);
    }
    if (found as f64) < top.len() as f64 * 0.95 || found == 0 {
        return Err(Skip::NotIndexed);
    }
    let files = items.iter().filter(|i| !i.folder).count();
    let known = items.iter().filter(|i| !i.folder && i.size.is_some()).count();
    if files > 0 && known * 10 < files * 9 {
        return Err(Skip::NotIndexed);
    }
    let round = |s: u64| if cluster > 1 { s.div_ceil(cluster) * cluster } else { s };
    let mut tree = Tree::new(root, mtime);
    let mut queue = std::collections::VecDeque::from([(root_key, 0u32)]);
    while let Some((k, id)) = queue.pop_front() {
        let Some(kids) = by_parent.get(&k) else {
            tree.add_children(id, &[]);
            continue;
        };
        let entries: Vec<Entry> = kids
            .iter()
            .map(|&i| {
                let it = &items[i];
                let size = if it.folder { 0 } else { it.size.unwrap_or(0) };
                let kind = if it.folder { Kind::Dir } else { Kind::File };
                Entry { name: it.name.clone(), kind, size, alloc: round(size), mtime: it.mtime, flags: 0 }
            })
            .collect();
        let first = tree.add_children(id, &entries);
        for (n, &i) in kids.iter().enumerate() {
            if items[i].folder {
                queue.push_back((format!("{k}\\{}", items[i].name.to_lowercase()), first + n as u32));
            }
        }
    }
    tree.finish();
    Ok(tree)
}

/// Asks the running Everything for everything below `root`.
#[cfg(windows)]
pub fn query(root: &str, cancel: &std::sync::atomic::AtomicBool) -> Result<Vec<Item>, Skip> {
    win::query(&search_for(root), cancel)
}

/// Reads `root` from the index of Everything, when it is running and
/// covers the folder.
#[cfg(windows)]
pub fn scan_index(root: &std::path::Path, cancel: &std::sync::atomic::AtomicBool, progress: &dyn Fn(&Progress)) -> Result<Tree, Skip> {
    if !win::running() {
        return Err(Skip::NotRunning);
    }
    let text = root.to_string_lossy().into_owned();
    progress(&Progress { current: crate::i18n::tr("Asking Everything…"), ..Progress::default() });
    let items = query(&text, cancel)?;
    let top: Vec<String> = std::fs::read_dir(root).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    let mtime = std::fs::metadata(root).map_or(0, |m| super::mtime_of(&m));
    build(&text, &items, &top, win::cluster_size(&text), mtime)
}

#[cfg(windows)]
mod win {
    use super::{parse_reply, Item, Skip, REQUEST};
    use rt_sys::win::wide;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceW, GetVolumePathNameW};
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    /// The window of a running Everything (1.4, 1.5).
    const CLASSES: [&str; 2] = ["EVERYTHING_TASKBAR_NOTIFICATION", "EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)"];
    /// "query 2", UTF-16.
    const COPYDATA_QUERY2W: usize = 18;
    /// The tag of the answer to rsize.
    const REPLY: usize = 0x5253_495A;
    /// Everything's own messages: is the database loaded?
    const WM_EVERYTHING_IPC: u32 = WM_USER;
    const IPC_IS_DB_LOADED: usize = 401;
    /// How long to wait for the answer.
    const TIMEOUT: Duration = Duration::from_secs(120);

    thread_local! {
        static ANSWER: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    }

    fn everything() -> Option<HWND> {
        CLASSES.iter().find_map(|c| {
            let w = wide(c);
            // SAFETY: NUL-terminated class name; no window title.
            let h = unsafe { FindWindowW(w.as_ptr(), std::ptr::null()) };
            (!h.is_null()).then_some(h)
        })
    }

    /// Whether Everything is running with its database loaded.
    pub fn running() -> bool {
        // SAFETY: a query message with no pointers; a dead window just fails.
        everything().is_some_and(|h| unsafe { SendMessageW(h, WM_EVERYTHING_IPC, IPC_IS_DB_LOADED, 0) } != 0)
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_COPYDATA && lp != 0 {
            // SAFETY: for WM_COPYDATA, lParam is a COPYDATASTRUCT valid during the call.
            let cds = unsafe { &*(lp as *const COPYDATASTRUCT) };
            if cds.dwData == REPLY && !cds.lpData.is_null() {
                // SAFETY: lpData holds cbData bytes for the duration of the call.
                let bytes = unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) }.to_vec();
                ANSWER.with(|a| *a.borrow_mut() = Some(bytes));
                return 1;
            }
        }
        // SAFETY: default handling of everything else.
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    /// A hidden window on this thread to receive the answer.
    fn reply_window() -> Option<HWND> {
        let class = wide("rsize.everything.reply");
        // SAFETY: the module handle of this program; registering twice only fails.
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), 0, 0, 0, 0, 0, HWND_MESSAGE, std::ptr::null_mut(), inst, std::ptr::null());
            (!h.is_null()).then_some(h)
        }
    }

    pub fn query(search: &str, cancel: &AtomicBool) -> Result<Vec<Item>, Skip> {
        let target = everything().ok_or(Skip::NotRunning)?;
        let me = reply_window().ok_or(Skip::NotRunning)?;
        ANSWER.with(|a| *a.borrow_mut() = None);
        // The request: seven counters, then the search text.
        let mut req: Vec<u8> = Vec::new();
        for v in [me as usize as u32, REPLY as u32, 0, 0, u32::MAX, REQUEST, 1] {
            req.extend_from_slice(&v.to_le_bytes());
        }
        for u in search.encode_utf16().chain(std::iter::once(0)) {
            req.extend_from_slice(&u.to_le_bytes());
        }
        let cds = COPYDATASTRUCT { dwData: COPYDATA_QUERY2W, cbData: req.len() as u32, lpData: req.as_ptr() as *mut _ };
        // SAFETY: `cds` and `req` outlive the call; the answer arrives at `me`.
        let sent = unsafe { SendMessageW(target, WM_COPYDATA, me as WPARAM, &cds as *const _ as LPARAM) };
        let result = if sent == 0 { Err(Skip::NotRunning) } else { wait(cancel) };
        // SAFETY: our own window, created on this thread.
        unsafe { DestroyWindow(me) };
        result
    }

    /// Runs this thread's messages until the answer is in.
    fn wait(cancel: &AtomicBool) -> Result<Vec<Item>, Skip> {
        let start = Instant::now();
        loop {
            if let Some(bytes) = ANSWER.with(|a| a.borrow_mut().take()) {
                return parse_reply(&bytes).ok_or(Skip::NotIndexed);
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(Skip::Stopped);
            }
            if start.elapsed() > TIMEOUT {
                return Err(Skip::NotRunning);
            }
            // SAFETY: waits for and dispatches this thread's own messages.
            unsafe {
                MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 100, QS_ALLINPUT);
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    /// The volume's cluster size, to estimate the space files take.
    pub fn cluster_size(root: &str) -> u64 {
        let w = wide(root);
        let mut vol = vec![0u16; 512];
        let (mut spc, mut bps, mut free, mut total) = (0u32, 0u32, 0u32, 0u32);
        // SAFETY: NUL-terminated input; the outputs are valid.
        let ok = unsafe {
            GetVolumePathNameW(w.as_ptr(), vol.as_mut_ptr(), vol.len() as u32) != 0 && GetDiskFreeSpaceW(vol.as_ptr(), &mut spc, &mut bps, &mut free, &mut total) != 0
        };
        if ok {
            spc as u64 * bps as u64
        } else {
            4096
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer with the given (flags, name, path, size, filetime) items.
    fn reply(items: &[(u32, &str, &str, u64, u64)]) -> Vec<u8> {
        let mut head = Vec::new();
        for v in [items.len() as u32, items.len() as u32, 0, REQUEST, 1] {
            head.extend_from_slice(&v.to_le_bytes());
        }
        let data_start = 20 + items.len() * 8;
        let mut data = Vec::new();
        for (flags, name, path, size, ft) in items {
            head.extend_from_slice(&flags.to_le_bytes());
            head.extend_from_slice(&((data_start + data.len()) as u32).to_le_bytes());
            for s in [name, path] {
                let u: Vec<u16> = s.encode_utf16().collect();
                data.extend_from_slice(&(u.len() as u32).to_le_bytes());
                for c in u.iter().chain(&[0]) {
                    data.extend_from_slice(&c.to_le_bytes());
                }
            }
            data.extend_from_slice(&size.to_le_bytes());
            data.extend_from_slice(&ft.to_le_bytes());
        }
        head.extend(data);
        head
    }

    const FT_2020: u64 = 132_223_104_000_000_000;

    #[test]
    fn answers_are_read() {
        let b = reply(&[(1, "docs", "C:\\data", u64::MAX, FT_2020), (0, "ação.txt", "C:\\data\\docs", 1234, FT_2020), (2, "C:", "", 0, 0)]);
        let items = parse_reply(&b).unwrap();
        assert_eq!(items.len(), 2, "drives are left out");
        assert!(items[0].folder && items[0].size.is_none());
        assert_eq!(items[1].name, "ação.txt");
        assert_eq!(items[1].size, Some(1234));
        assert_eq!(items[1].mtime, 1_577_836_800);
        assert!(parse_reply(&b[..30]).is_none());
        assert_eq!(search_for("C:\\"), "\"C:\\\"");
        assert_eq!(search_for("E:\\a b"), "\"E:\\a b\\\"");
    }

    #[test]
    fn the_tree_comes_from_the_paths() {
        let items = parse_reply(&reply(&[
            (1, "Docs", "C:\\Data", 0, FT_2020),
            (0, "a.txt", "C:\\Data", 10, FT_2020),
            (0, "b.bin", "C:\\Data\\Docs", 5000, FT_2020),
            (0, "c.bin", "c:\\data\\docs", 1, FT_2020),
        ]))
        .unwrap();
        let top = vec!["Docs".to_string(), "a.txt".to_string()];
        let t = build("C:\\Data", &items, &top, 4096, 0).unwrap();
        assert_eq!(t.node(0).size, 5011);
        assert_eq!(t.node(0).alloc, 4 * 4096, "1 + 2 + 1 clusters");
        let docs = t.children(0)[0];
        assert_eq!((t.name(docs), t.children(docs).len()), ("Docs", 2), "paths match without case");
        // A folder the index does not cover.
        assert_eq!(build("C:\\Other", &items, &top, 4096, 0).err(), Some(Skip::NotIndexed));
        // An index that misses much of the top level.
        let more: Vec<String> = (0..10).map(|i| format!("f{i}")).collect();
        assert_eq!(build("C:\\Data", &items, &more, 4096, 0).err(), Some(Skip::NotIndexed));
        // Without sizes.
        let blind = parse_reply(&reply(&[(0, "a.txt", "C:\\D", u64::MAX, 0)])).unwrap();
        assert_eq!(build("C:\\D", &blind, &["a.txt".into()], 4096, 0).err(), Some(Skip::NotIndexed));
    }

    /// What the running Everything answers for `RSIZE_BENCH_DIR`:
    /// `cargo test -p rsize --lib probe -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn probe() {
        use std::sync::atomic::AtomicBool;
        let Some(dir) = std::env::var_os("RSIZE_BENCH_DIR") else { return };
        let dir = crate::scan::normalize(std::path::Path::new(&dir));
        let text = dir.to_string_lossy().into_owned();
        println!("running: {}, search: {}", win::running(), search_for(&text));
        let start = std::time::Instant::now();
        match query(&text, &AtomicBool::new(false)) {
            Ok(items) => {
                println!("{} items in {:.2?}", items.len(), start.elapsed());
                for it in items.iter().take(5) {
                    println!("  {it:?}");
                }
                let top: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                println!("build: {:?}", build(&text, &items, &top, 4096, 0).map(|t| t.node(0).files));
            }
            Err(e) => println!("query: {e:?}"),
        }
    }

    /// Compares the index with a scan of the repository, when Everything is
    /// running here.
    #[cfg(windows)]
    #[test]
    fn the_running_everything_agrees_with_a_scan() {
        use std::sync::atomic::AtomicBool;
        if !win::running() {
            return;
        }
        let dir = crate::scan::normalize(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "\\src")));
        let t = scan_index(&dir, &AtomicBool::new(false), &|_| {}).expect("Everything is running but does not cover the sources");
        let s = crate::scan::tests::sys(&dir, &AtomicBool::new(false), &|_| {}).unwrap();
        // Files written in the last moments may not be in the index yet.
        assert!(t.node(0).files.abs_diff(s.tree.node(0).files) <= 2, "{} vs {}", t.node(0).files, s.tree.node(0).files);
    }
}
