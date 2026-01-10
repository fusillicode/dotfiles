#![feature(exit_status_error)]

use std::fmt::Display;
use std::fs::File;
use std::io::BufRead as _;
use std::io::BufReader;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::DateTime;
use chrono::Local;
use color_eyre::eyre::Context;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let history_path = std::env::var("HISTFILE").map(PathBuf::from).unwrap_or_else(|_| {
        let mut p = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        p.push(".zsh_history");
        p
    });

    let mut lines = BufReader::new(File::open(history_path)?).split(b'\n').peekable();

    while let Some(line) = lines.next() {
        let mut entry = String::from_utf8_lossy(&line?).to_string();

        if entry.starts_with(": ") {
            if entry.ends_with('\\') {
                while let Some(Ok(next_line)) = lines.peek() {
                    let next_line = String::from_utf8_lossy(next_line);
                    if !next_line.starts_with(": ") {
                        entry.push('\n');
                        entry.push_str(&next_line);
                        lines.next();
                    } else {
                        break;
                    }
                }
            }
            println!("{}", ParsedEntry::from_str(&entry)?);
        }
    }

    Ok(())
}

struct ParsedEntry {
    datetime: DateTime<Local>,
    duration: u64,
    cmd: String,
}

impl FromStr for ParsedEntry {
    type Err = color_eyre::eyre::Error;

    fn from_str(zsh_history_entry: &str) -> Result<Self, Self::Err> {
        let Some((metadata, cmd)) = zsh_history_entry.split_once(';') else {
            bail!("error missing ':' separator in zsh_history_entry={zsh_history_entry}");
        };
        let Some((unix_time, duration)) = metadata.trim_start_matches(": ").split_once(':') else {
            bail!("error missing ':' separator in metadata={metadata} zsh_history_entry={zsh_history_entry}");
        };
        Ok(Self {
            datetime: unix_time
                .parse()
                .with_context(|| format!("error parsing str as Unix time i64 | str={unix_time}"))
                .and_then(|secs| {
                    DateTime::from_timestamp(secs, 0)
                        .ok_or_else(|| eyre!("error parsing Unix time i64 as DateTime | secs={secs}"))
                })?
                .into(),
            duration: duration.parse()?,
            cmd: cmd.to_string(),
        })
    }
}

impl Display for ParsedEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}[{}] {}",
            self.datetime.format("%Y-%m-%d %H:%M:%S"),
            self.duration,
            self.cmd
        )
    }
}
