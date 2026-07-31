use crate::config::PdfPreviewHandler;
use std::path::Path;
use windows::core::{Error, IUnknown, Interface, Result, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, HWND, RECT};
use windows::Win32::System::Com::{
    CoCreateInstance, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, STGM_READ,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ, RRF_NOEXPAND,
    RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithFile, IInitializeWithStream};
use windows::Win32::UI::Shell::{
    IInitializeWithItem, IPreviewHandler, IShellItem, SHCreateItemFromParsingName,
    SHCreateStreamOnFileEx,
};

const PDF_PREVIEW_HANDLER_KEY: &str = r#".pdf\shellex\{8895b1c6-b41f-4c1c-a562-0d564250836f}"#;
const PDF_XCHANGE_PREVIEW_HANDLER: GUID = GUID::from_u128(0x9b68bdf7_95f9_4a1f_851c_27d822f8e3e9);
const PDF_XCHANGE_LEGACY_PREVIEW_HANDLER: GUID =
    GUID::from_u128(0xdc6efb56_9cfa_464d_8880_44885d7dc193);
const POWERTOYS_PDF_PREVIEW_HANDLER: GUID = GUID::from_u128(0xa5a41cc7_02cb_41d4_8c9b_9087040d6098);

const PDF_XCHANGE_REGISTRATION_KEY: &str = r#"CLSID\{9B68BDF7-95F9-4A1F-851C-27D822F8E3E9}"#;
const PDF_XCHANGE_LEGACY_REGISTRATION_KEY: &str = r#"CLSID\{DC6EFB56-9CFA-464D-8880-44885D7DC193}"#;
const POWERTOYS_REGISTRATION_KEY: &str = r#"CLSID\{A5A41CC7-02CB-41D4-8C9B-9087040D6098}"#;

pub struct PdfPreviewHost {
    handler: Option<IPreviewHandler>,
}

impl PdfPreviewHost {
    pub fn new() -> Self {
        Self { handler: None }
    }

    pub unsafe fn open(
        &mut self,
        path: &Path,
        parent: HWND,
        rect: RECT,
        selection: PdfPreviewHandler,
    ) -> Result<()> {
        self.close();

        let clsid = resolve_pdf_handler_clsid(selection);
        let object: IUnknown = CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)
            .or_else(|_| CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER))?;

        initialize_handler(&object, path)?;

        let handler: IPreviewHandler = object.cast()?;
        if let Err(error) = handler
            .SetWindow(parent, &rect)
            .and_then(|_| handler.DoPreview())
        {
            let _ = handler.Unload();
            return Err(error);
        }

        self.handler = Some(handler);
        Ok(())
    }

    pub unsafe fn resize(&self, rect: RECT) -> Result<()> {
        match &self.handler {
            Some(handler) => handler.SetRect(&rect),
            None => Ok(()),
        }
    }

    pub unsafe fn close(&mut self) {
        if let Some(handler) = self.handler.take() {
            let _ = handler.Unload();
        }
    }

    pub fn is_open(&self) -> bool {
        self.handler.is_some()
    }
}

impl Drop for PdfPreviewHost {
    fn drop(&mut self) {
        unsafe { self.close() };
    }
}

unsafe fn initialize_handler(object: &IUnknown, path: &Path) -> Result<()> {
    let path = HSTRING::from(path.as_os_str());

    if let Ok(initializer) = object.cast::<IInitializeWithStream>() {
        if let Ok(stream) = SHCreateStreamOnFileEx(&path, STGM_READ.0, 0, false, None) {
            if initializer.Initialize(&stream, STGM_READ.0).is_ok() {
                return Ok(());
            }
        }
    }

    if let Ok(initializer) = object.cast::<IInitializeWithFile>() {
        if initializer.Initialize(&path, STGM_READ.0).is_ok() {
            return Ok(());
        }
    }

    if let Ok(initializer) = object.cast::<IInitializeWithItem>() {
        let item: Result<IShellItem> = SHCreateItemFromParsingName(&path, None);
        if let Ok(item) = item {
            if initializer.Initialize(&item, STGM_READ.0).is_ok() {
                return Ok(());
            }
        }
    }

    Err(Error::from_hresult(E_FAIL))
}

fn resolve_pdf_handler_clsid(selection: PdfPreviewHandler) -> GUID {
    match selection {
        PdfPreviewHandler::WindowsDefault => {
            unsafe { read_registered_pdf_handler_clsid() }.unwrap_or(PDF_XCHANGE_PREVIEW_HANDLER)
        }
        PdfPreviewHandler::PdfXChange => PDF_XCHANGE_PREVIEW_HANDLER,
        PdfPreviewHandler::PdfXChangeLegacy => PDF_XCHANGE_LEGACY_PREVIEW_HANDLER,
        PdfPreviewHandler::PowerToys => POWERTOYS_PDF_PREVIEW_HANDLER,
    }
}

pub fn is_pdf_handler_available(selection: PdfPreviewHandler) -> bool {
    let registration_key = match selection {
        PdfPreviewHandler::WindowsDefault => return true,
        PdfPreviewHandler::PdfXChange => PDF_XCHANGE_REGISTRATION_KEY,
        PdfPreviewHandler::PdfXChangeLegacy => PDF_XCHANGE_LEGACY_REGISTRATION_KEY,
        PdfPreviewHandler::PowerToys => POWERTOYS_REGISTRATION_KEY,
    };

    unsafe { registry_key_exists(registration_key) }
}

unsafe fn registry_key_exists(path: &str) -> bool {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();
    if RegOpenKeyExW(HKEY_CLASSES_ROOT, &path, 0, KEY_READ, &mut key).is_err() {
        return false;
    }
    let _ = RegCloseKey(key);
    true
}

unsafe fn read_registered_pdf_handler_clsid() -> Option<GUID> {
    let key = HSTRING::from(PDF_PREVIEW_HANDLER_KEY);
    let mut value = [0u16; 64];
    let mut bytes = std::mem::size_of_val(&value) as u32;
    let status = RegGetValueW(
        HKEY_CLASSES_ROOT,
        &key,
        PCWSTR::null(),
        RRF_RT_REG_SZ | RRF_NOEXPAND,
        None,
        Some(value.as_mut_ptr().cast()),
        Some(&mut bytes),
    );
    if status.is_err() {
        return None;
    }

    let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    if len == 0 {
        return None;
    }

    let clsid = HSTRING::from(String::from_utf16_lossy(&value[..len]).trim());
    windows::Win32::System::Com::CLSIDFromString(&clsid).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_xchange_fallback_clsid_matches_documented_value() {
        assert_eq!(
            PDF_XCHANGE_PREVIEW_HANDLER,
            GUID::from_u128(0x9b68bdf7_95f9_4a1f_851c_27d822f8e3e9)
        );
    }

    #[test]
    fn explicit_handler_choices_resolve_to_their_clsids() {
        assert_eq!(
            resolve_pdf_handler_clsid(PdfPreviewHandler::PdfXChangeLegacy),
            PDF_XCHANGE_LEGACY_PREVIEW_HANDLER
        );
        assert_eq!(
            resolve_pdf_handler_clsid(PdfPreviewHandler::PowerToys),
            POWERTOYS_PDF_PREVIEW_HANDLER
        );
    }
}
