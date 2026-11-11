//! 화면 읽기(BitBlt, 작은 영역만), 테스트용 이미지 파일 읽기, Windows 내장 OCR.
//! 게임 프로세스에는 손대지 않고, 화면에 보이는 픽셀만 읽는다.

use crate::detect::Image;
use crate::layout::Rect;
use windows::core::{Result, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode,
    SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, SRCCOPY,
};

pub enum Source {
    /// 게임 창 클라이언트 영역의 화면 좌표 원점.
    Screen { x: i32, y: i32 },
    /// 오프라인 테스트: 클라이언트 영역 전체를 찍은 이미지.
    File(Image),
}

impl Source {
    pub fn grab(&self, r: &Rect) -> Option<Image> {
        let (x, y) = (r.l as i32, r.t as i32);
        let (w, h) = (((r.r - r.l) as i32).max(1), ((r.b - r.t) as i32).max(1));
        match self {
            Source::Screen { x: ox, y: oy } => grab_screen(ox + x, oy + y, w, h),
            Source::File(img) => {
                let (x, y, w, h) = (x.max(0) as usize, y.max(0) as usize, w as usize, h as usize);
                if x + w > img.w || y + h > img.h {
                    return None;
                }
                let px = (y..y + h).flat_map(|row| &img.px[(row * img.w + x) * 4..(row * img.w + x + w) * 4]).copied().collect();
                Some(Image { w, h, px })
            }
        }
    }
}

fn grab_screen(x: i32, y: i32, w: i32, h: i32) -> Option<Image> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let px = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok().and_then(|bmp| {
            let old = SelectObject(dc, bmp.into());
            let px = BitBlt(dc, 0, 0, w, h, Some(screen), x, y, SRCCOPY)
                .is_ok()
                .then(|| std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec());
            SelectObject(dc, old);
            let _ = DeleteObject(bmp.into());
            px
        });
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        px.map(|px| Image { w: w as usize, h: h as usize, px })
    }
}

pub fn load(path: &str) -> Result<Image> {
    let bytes = std::fs::read(path)?;
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(&bytes)?;
    writer.StoreAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let data = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Ignore,
            &BitmapTransform::new()?,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?;
    Ok(Image { w: decoder.PixelWidth()? as usize, h: decoder.PixelHeight()? as usize, px: data.DetachPixelData()?.to_vec() })
}

/// Windows 내장 OCR(한국어). 한국어 언어 팩이 없으면 new() 가 실패한다.
pub struct Ocr(OcrEngine);

impl Ocr {
    pub fn new() -> Result<Self> {
        Ok(Ocr(OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from("ko"))?)?))
    }

    pub fn lines(&self, img: &Image) -> Result<Vec<String>> {
        let writer = DataWriter::new()?;
        writer.WriteBytes(&img.px)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&writer.DetachBuffer()?, BitmapPixelFormat::Bgra8, img.w as i32, img.h as i32)?;
        let result = self.0.RecognizeAsync(&bitmap)?.join()?;
        result.Lines()?.into_iter().map(|line| Ok(line.Text()?.to_string())).collect()
    }
}
