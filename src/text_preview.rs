use encoding_rs::WINDOWS_1252;
use std::fs::File;
use std::io::{Read, Take};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, FillRect, GetTextExtentPoint32W, SelectObject, SetBkMode, SetTextColor, TextOutW,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DIB_RGB_COLORS,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_MODERN, FIXED_PITCH, FW_NORMAL,
    TRANSPARENT,
};

pub const TEXT_PREVIEW_WIDTH: u32 = 900;
pub const TEXT_PREVIEW_HEIGHT: u32 = 650;
const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_RENDERED_LINE_CHARS: usize = 400;

const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "log",
    "ini",
    "cfg",
    "conf",
    "rs",
    "py",
    "pyw",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hh",
    "hpp",
    "hxx",
    "cs",
    "java",
    "go",
    "rb",
    "php",
    "swift",
    "kt",
    "kts",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "xml",
    "json",
    "jsonc",
    "yaml",
    "yml",
    "toml",
    "sql",
    "sh",
    "bash",
    "zsh",
    "ps1",
    "psm1",
    "psd1",
    "bat",
    "cmd",
    "lua",
    "r",
    "dart",
    "vue",
    "svelte",
    "gradle",
    "properties",
    "env",
];
const TEXT_FILE_NAMES: &[&str] = &[
    "dockerfile",
    "makefile",
    "jenkinsfile",
    "readme",
    "license",
    "authors",
    "changelog",
    ".env",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
];

pub struct TextPreviewFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn is_text_preview_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let file_name = file_name.to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);

    extension
        .as_deref()
        .map(|extension| TEXT_EXTENSIONS.contains(&extension))
        .unwrap_or(false)
        || TEXT_FILE_NAMES.contains(&file_name.as_str())
}

pub fn load_text_preview(
    path: &Path,
    max_width: u32,
    max_height: u32,
    cancel: &AtomicBool,
) -> Option<TextPreviewFrame> {
    if cancel.load(Ordering::Acquire) {
        return None;
    }

    let file = File::open(path).ok()?;
    let file_size = file.metadata().ok().map(|metadata| metadata.len());
    let mut bytes = Vec::with_capacity(MAX_TEXT_BYTES.min(file_size.unwrap_or(0) as usize));
    let mut reader: Take<File> = file.take((MAX_TEXT_BYTES + 1) as u64);
    reader.read_to_end(&mut bytes).ok()?;
    if cancel.load(Ordering::Acquire) {
        return None;
    }

    let truncated = bytes.len() > MAX_TEXT_BYTES
        || file_size
            .map(|size| size > MAX_TEXT_BYTES as u64)
            .unwrap_or(false);
    bytes.truncate(MAX_TEXT_BYTES);

    let decoded = decode_text(&bytes);
    let (text, status) = match decoded {
        Some((text, encoding)) => {
            let status = if truncated {
                format!("{} · showing first 512 KB", encoding)
            } else {
                encoding.to_string()
            };
            (text, status)
        }
        None => (
            "Binary content is not displayed.".to_string(),
            "Not a text file".to_string(),
        ),
    };

    let width = TEXT_PREVIEW_WIDTH.min(max_width).max(1);
    let height = TEXT_PREVIEW_HEIGHT.min(max_height).max(1);
    unsafe { render_text(path, &text, &status, width, height) }
}

fn decode_text(bytes: &[u8]) -> Option<(String, &'static str)> {
    let (text, encoding) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (String::from_utf8_lossy(&bytes[3..]).into_owned(), "UTF-8")
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        (decode_utf16(&bytes[2..], true)?, "UTF-16 LE")
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (decode_utf16(&bytes[2..], false)?, "UTF-16 BE")
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        (text.to_string(), "UTF-8")
    } else {
        let (text, _, _) = WINDOWS_1252.decode(bytes);
        (text.into_owned(), "Windows-1252")
    };

    if looks_binary(&text) {
        None
    } else {
        Some((text, encoding))
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units = bytes.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    Some(
        char::decode_utf16(units)
            .map(|ch| ch.unwrap_or('\u{fffd}'))
            .collect(),
    )
}

fn looks_binary(text: &str) -> bool {
    if text.contains('\0') {
        return true;
    }
    let mut inspected = 0usize;
    let mut controls = 0usize;
    for ch in text.chars().take(4096) {
        inspected += 1;
        if ch.is_control() && !matches!(ch, '\r' | '\n' | '\t') {
            controls += 1;
        }
    }
    inspected > 0 && controls * 10 > inspected
}

fn expand_tabs(line: &str) -> String {
    let mut expanded = String::new();
    let mut column = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = 4 - column % 4;
            expanded.extend(std::iter::repeat(' ').take(spaces));
            column += spaces;
        } else if !ch.is_control() {
            expanded.push(ch);
            column += 1;
        }
        if column >= MAX_RENDERED_LINE_CHARS {
            break;
        }
    }
    expanded
}

unsafe fn render_text(
    path: &Path,
    text: &str,
    status: &str,
    width: u32,
    height: u32,
) -> Option<TextPreviewFrame> {
    let mem_dc = CreateCompatibleDC(None);
    if mem_dc.0.is_null() {
        return None;
    }
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [Default::default()],
    };
    let mut bits: *mut core::ffi::c_void = ptr::null_mut();
    let bitmap = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(bitmap) => bitmap,
        Err(_) => {
            let _ = DeleteDC(mem_dc);
            return None;
        }
    };
    if bits.is_null() {
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        return None;
    }
    let old_bitmap = SelectObject(mem_dc, bitmap);

    let background = CreateSolidBrush(COLORREF(0x00201e1e));
    let header_background = CreateSolidBrush(COLORREF(0x00302d2d));
    let gutter_background = CreateSolidBrush(COLORREF(0x00282424));
    let full_rect = RECT {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    FillRect(mem_dc, &full_rect, background);
    let header_height = 42.min(height as i32);
    let footer_height = 28.min((height as i32 - header_height).max(0));
    let header_rect = RECT {
        bottom: header_height,
        ..full_rect
    };
    FillRect(mem_dc, &header_rect, header_background);

    let font = CreateFontW(
        -16,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        0,
        0,
        CLEARTYPE_QUALITY.0 as u32,
        (FIXED_PITCH.0 | FF_MODERN.0) as u32,
        w!("Cascadia Mono"),
    );
    let old_font = SelectObject(mem_dc, font);
    SetBkMode(mem_dc, TRANSPARENT);

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Text preview".to_string());
    let mut file_name: Vec<u16> = file_name.encode_utf16().collect();
    let mut title_rect = RECT {
        left: 14,
        top: 0,
        right: width as i32 - 14,
        bottom: header_height,
    };
    SetTextColor(mem_dc, COLORREF(0x00f0f0f0));
    DrawTextW(
        mem_dc,
        &mut file_name,
        &mut title_rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );

    let line_height = 20;
    let content_top = header_height + 10;
    let content_bottom = height as i32 - footer_height - 8;
    let visible_lines = ((content_bottom - content_top) / line_height).max(0) as usize;
    let number_digits = visible_lines.max(1).to_string().len().max(3);
    let mut zero_size = windows::Win32::Foundation::SIZE::default();
    let zero: Vec<u16> = "0".encode_utf16().collect();
    let _ = GetTextExtentPoint32W(mem_dc, &zero, &mut zero_size);
    let gutter_width = (number_digits as i32 + 2) * zero_size.cx.max(8);
    let gutter_rect = RECT {
        left: 0,
        top: header_height,
        right: gutter_width,
        bottom: height as i32 - footer_height,
    };
    FillRect(mem_dc, &gutter_rect, gutter_background);

    for (index, line) in text.lines().take(visible_lines).enumerate() {
        let y = content_top + index as i32 * line_height;
        let number = format!("{:>width$}", index + 1, width = number_digits);
        let number: Vec<u16> = number.encode_utf16().collect();
        SetTextColor(mem_dc, COLORREF(0x00909090));
        let _ = TextOutW(mem_dc, 8, y, &number);

        let expanded = expand_tabs(line);
        let code: Vec<u16> = expanded.encode_utf16().collect();
        SetTextColor(mem_dc, COLORREF(0x00e6e6e6));
        let _ = TextOutW(mem_dc, gutter_width + 10, y, &code);
    }

    let mut status: Vec<u16> = status.encode_utf16().collect();
    let mut footer_rect = RECT {
        left: 14,
        top: height as i32 - footer_height,
        right: width as i32 - 14,
        bottom: height as i32,
    };
    SetTextColor(mem_dc, COLORREF(0x00a8a8a8));
    DrawTextW(
        mem_dc,
        &mut status,
        &mut footer_rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );

    let byte_len = width as usize * height as usize * 4;
    let mut pixels = vec![0u8; byte_len];
    ptr::copy_nonoverlapping(bits.cast::<u8>(), pixels.as_mut_ptr(), byte_len);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }

    let _ = SelectObject(mem_dc, old_font);
    let _ = SelectObject(mem_dc, old_bitmap);
    let _ = DeleteObject(font);
    let _ = DeleteObject(background);
    let _ = DeleteObject(header_background);
    let _ = DeleteObject(gutter_background);
    let _ = DeleteObject(bitmap);
    let _ = DeleteDC(mem_dc);

    Some(TextPreviewFrame {
        pixels,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_source_and_extensionless_build_files() {
        assert!(is_text_preview_file(Path::new("main.rs")));
        assert!(is_text_preview_file(Path::new("Dockerfile")));
        assert!(is_text_preview_file(Path::new(".gitignore")));
        assert!(!is_text_preview_file(Path::new("photo.png")));
    }

    #[test]
    fn decodes_utf8_and_utf16() {
        assert_eq!(decode_text(b"hello").unwrap().0, "hello");
        assert_eq!(
            decode_text(&[0xff, 0xfe, b'h', 0, b'i', 0]).unwrap().0,
            "hi"
        );
    }

    #[test]
    fn rejects_binary_content() {
        assert!(decode_text(b"hello\0world").is_none());
    }

    #[test]
    fn expands_tabs_to_four_column_stops() {
        assert_eq!(expand_tabs("a\tb"), "a   b");
        assert_eq!(expand_tabs("abcd\tb"), "abcd    b");
    }

    #[test]
    fn renders_text_to_a_bgra_frame() {
        let frame = unsafe {
            render_text(
                Path::new("sample.rs"),
                "fn main() {\n    println!(\"hello\");\n}",
                "UTF-8",
                480,
                320,
            )
        }
        .expect("GDI text rendering should succeed");

        assert_eq!(frame.pixels.len(), 480 * 320 * 4);
        assert!(frame.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let first_color = &frame.pixels[..3];
        assert!(frame
            .pixels
            .chunks_exact(4)
            .any(|pixel| &pixel[..3] != first_color));
    }
}
