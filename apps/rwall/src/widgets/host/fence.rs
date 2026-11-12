//! Fences and the Explorer: files dropped into a fence are moved or copied
//! there (`SHFileOperationW` in a thread, with Explorer's own progress and
//! undo), and files dragged out of one carry the shell's own data object.

use super::*;

/// The files named by a dragged data object.
pub(super) fn dropped_files(d: &IDataObject) -> Vec<PathBuf> {
    let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    let mut out = Vec::new();
    // SAFETY: the medium is released after its list is read; the buffers
    // are as long as said.
    unsafe {
        let Ok(mut med) = d.GetData(&fmt) else { return out };
        let drop = HDROP(med.u.hGlobal.0);
        let n = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..n.min(10_000) {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(std::ffi::OsString::from_wide(&buf[..got.min(len)])));
        }
        ReleaseStgMedium(&mut med);
    }
    out
}

pub(super) fn same_volume(a: &Path, b: &Path) -> bool {
    match (a.components().next(), b.components().next()) {
        (Some(x), Some(y)) => x.as_os_str().to_string_lossy().eq_ignore_ascii_case(&y.as_os_str().to_string_lossy()),
        _ => false,
    }
}

/// What dropping `files` into `folder` does: move on the same drive, copy
/// from another (Ctrl copies, Shift moves), nothing for what is there already.
pub(super) fn drop_effect(files: &[PathBuf], folder: &Path, keys: MODIFIERKEYS_FLAGS) -> DROPEFFECT {
    if folder.as_os_str().is_empty() || files.is_empty() || files.iter().all(|f| f.parent() == Some(folder)) || files.iter().any(|f| folder.starts_with(f)) {
        return DROPEFFECT_NONE;
    }
    if keys.0 & MK_CONTROL.0 != 0 {
        DROPEFFECT_COPY
    } else if keys.0 & MK_SHIFT.0 != 0 || files.iter().all(|f| same_volume(f, folder)) {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_COPY
    }
}

/// Copies or moves `files` into `folder` with Explorer's own dialogs (and
/// undo), on a thread of its own.
pub(super) fn file_operation(files: Vec<PathBuf>, folder: PathBuf, effect: DROPEFFECT, waker: Waker) {
    std::thread::spawn(move || {
        let mut from: Vec<u16> = Vec::new();
        for f in &files {
            from.extend(f.as_os_str().encode_wide());
            from.push(0);
        }
        from.push(0);
        let mut to: Vec<u16> = folder.as_os_str().encode_wide().collect();
        to.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: if effect == DROPEFFECT_MOVE { FO_MOVE } else { FO_COPY },
            pFrom: PCWSTR(from.as_ptr()),
            pTo: PCWSTR(to.as_ptr()),
            fFlags: FOF_ALLOWUNDO.0 as u16,
            ..Default::default()
        };
        // SAFETY: both lists end with two zeros and outlive the call.
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            SHFileOperationW(&mut op);
        }
        waker.wake();
    });
}


/// A fence takes files dropped on it.
#[implement(IDropTarget)]
pub(super) struct FenceDrop {
    /// Shared with the widget, which changes it.
    pub(super) folder: Rc<RefCell<PathBuf>>,
    pub(super) files: RefCell<Vec<PathBuf>>,
    pub(super) waker: Waker,
}

impl IDropTarget_Impl for FenceDrop_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let files = data.as_ref().map(dropped_files).unwrap_or_default();
        let e = drop_effect(&files, &self.folder.borrow(), keys);
        *self.files.borrow_mut() = files;
        // SAFETY: OLE passes a valid pointer.
        unsafe { *effect = e };
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let e = drop_effect(&self.files.borrow(), &self.folder.borrow(), keys);
        // SAFETY: OLE passes a valid pointer.
        unsafe { *effect = e };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.files.borrow_mut().clear();
        Ok(())
    }

    fn Drop(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let files = data.as_ref().map(dropped_files).unwrap_or_default();
        let folder = self.folder.borrow().clone();
        let e = drop_effect(&files, &folder, keys);
        if e != DROPEFFECT_NONE {
            file_operation(files, folder, e, self.waker);
        }
        // A move done here is not for the source to finish (it would
        // delete what it moved): it is told nothing happened.
        // SAFETY: OLE passes a valid pointer.
        unsafe { *effect = if e == DROPEFFECT_MOVE { DROPEFFECT_NONE } else { e } };
        self.files.borrow_mut().clear();
        Ok(())
    }
}

/// Dragging a file out of a fence.
#[implement(IDropSource)]
pub(super) struct FenceSource;

impl IDropSource_Impl for FenceSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

pub(super) fn drag_out(path: &Path) {
    // SAFETY: COM calls on objects made here, on this OLE thread.
    unsafe {
        let _ = ReleaseCapture();
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path.as_os_str()), None) else { return };
        let Ok(data) = item.BindToHandler::<_, IDataObject>(None::<&IBindCtx>, &BHID_DataObject) else { return };
        let source: IDropSource = FenceSource.into();
        let mut effect = DROPEFFECT_NONE;
        let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK, &mut effect);
    }
}
