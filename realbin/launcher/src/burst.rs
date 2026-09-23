use std::path::PathBuf;
use time::PlainDateTime;
use getset::Getters;
use time::format_description::StaticFormatDescription;
use time::macros::format_description;

#[derive(Debug, Getters)]
pub struct BurstFile {
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
}