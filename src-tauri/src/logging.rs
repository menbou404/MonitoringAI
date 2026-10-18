use std::{
    fs, io,
    path::Path,
    time::{Duration, SystemTime},
};
use tracing_subscriber::EnvFilter;
#[cfg(debug_assertions)]
use tracing_subscriber::fmt::writer::MakeWriterExt;

pub fn init(dir: &Path) -> io::Result<tracing_appender::non_blocking::WorkerGuard> {
    fs::create_dir_all(dir)?;
    prune(dir)?;
    let file = tracing_appender::rolling::daily(dir, "monitoringai.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    #[cfg(debug_assertions)]
    let writer = writer.and(std::io::stderr);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .try_init()
        .map_err(io::Error::other)?;
    Ok(guard)
}

fn prune(dir: &Path) -> io::Result<()> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(7 * 24 * 3600))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("monitoringai.log.")
        {
            continue;
        }
        if entry.metadata()?.modified().is_ok_and(|time| time < cutoff) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
