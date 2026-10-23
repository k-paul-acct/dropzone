use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use tokio::{fs, io::AsyncWriteExt};

pub async fn save_file(directory: &Path, name: &str, data: &[u8]) -> io::Result<PathBuf> {
    let name = Path::new(name)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid upload filename"))?;
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or(name);
    let mut number = 1usize;

    loop {
        let candidate = if number == 1 {
            name.to_os_string()
        } else {
            let mut candidate = OsString::from(stem);
            candidate.push(format!(" ({number})"));
            if let Some(extension) = path.extension() {
                candidate.push(".");
                candidate.push(extension);
            }
            candidate
        };
        let destination = directory.join(candidate);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .await
        {
            Ok(mut file) => {
                file.write_all(data).await?;
                return Ok(destination);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => number += 1,
            Err(err) => return Err(err),
        }
    }
}
