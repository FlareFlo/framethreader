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
    #[getset(get = "pub")]
    thumbnail: Option<RgbImage>,
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
            thumbnail: None,
        }
    }

    pub fn gen_thumbnail(&mut self) {
        let mut file = File::open(&self.path).unwrap();
        let mut buf = vec![0u8; 2usize.pow(16)];
        file.read_exact(buf.as_mut_slice()).unwrap();

        // Get offsets
        let mut exiftool = Command::new("exiftool")
            .args(["-s3", "-ThumbnailOffset", "-ThumbnailLength", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = exiftool.stdin.as_mut().unwrap();
        stdin.write_all(&buf).unwrap();

        let header_res = exiftool.wait_with_output().unwrap();
        let offsets = String::from_utf8(header_res.stdout).unwrap();
        let (start, len) = offsets.split_once("\n").map(|(l,r)|(usize::from_str(l).unwrap(), usize::from_str(r.trim()).unwrap())).unwrap();
        let mut preview_buf = vec![0_u8; len];
        file.seek(SeekFrom::Start(start as _)).unwrap();
        file.read_exact(&mut preview_buf).unwrap();
        self.thumbnail = Some(image::load_from_memory_with_format(&preview_buf, ImageFormat::Jpeg).unwrap().into_rgb8())
    }
}