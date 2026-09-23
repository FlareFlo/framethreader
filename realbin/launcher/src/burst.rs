use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use image::{DynamicImage, ImageFormat, RgbImage};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;
use time::PlainDateTime;
use getset::Getters;
use time::format_description::StaticFormatDescription;
use time::macros::format_description;

#[derive(Debug, Getters)]
pub struct BurstFile {
    #[getset(get = "pub")]
    path: PathBuf,
    #[getset(get = "pub")]
    created: PlainDateTime,
}

impl BurstFile {
    pub fn new(path: impl Into<PathBuf>, create_date: &str, subsec: u16) -> Self {
        static DATEFMT: StaticFormatDescription = format_description!("[year]:[month]:[day] [hour]:[minute]:[second]");


        Self  {
            path: path.into(),
            created: PlainDateTime::parse(&create_date, &DATEFMT)
                .unwrap()
                .replace_microsecond(subsec as u32 * 1000)
                .unwrap(),
        }
    }

    pub fn extract_embedded_image(path: &PathBuf, imgtype: EmbeddedImageType) -> Option<RgbImage> {
        let mut file = File::open(path).ok()?;
        let mut buf = vec![0u8; 2usize.pow(16)];
        file.read_exact(buf.as_mut_slice()).ok()?;

        let mut exiftool = Command::new("exiftool")
            .args(imgtype.to_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let stdin = exiftool.stdin.as_mut()?;
        stdin.write_all(&buf).ok()?;

        let header_res = exiftool.wait_with_output().ok()?;
        let offsets = String::from_utf8(header_res.stdout).ok()?;
        let (start, len) = offsets.split_once("\n")?;
        let start = usize::from_str(start.trim()).ok()?;
        let len = usize::from_str(len.trim()).ok()?;

        let mut preview_buf = vec![0_u8; len];
        file.seek(SeekFrom::Start(start as _)).ok()?;
        file.read_exact(&mut preview_buf).ok()?;
        Some(image::load_from_memory_with_format(&preview_buf, ImageFormat::Jpeg).ok()?.into_rgb8())
    }
}

pub enum EmbeddedImageType {
    Thumbnail,
    Preview,
    Full,
}

impl EmbeddedImageType {
    fn to_args(&self) -> &'static[&'static str] {
        match self {
            EmbeddedImageType::Thumbnail => {
                &["-s3", "-ThumbnailOffset", "-ThumbnailLength", "-"]
            }
            EmbeddedImageType::Preview => {
                &["-s3", "-PreviewImageStart", "-PreviewImageLength", "-"]
            }
            EmbeddedImageType::Full => {
                &["-s3", "-JpgFromRawStart", "-JpgFromRawLength", "-"]
            }
        }
    }
}