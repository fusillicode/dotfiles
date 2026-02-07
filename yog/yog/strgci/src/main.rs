//! Print compact HEAD commit info for shell prompts.
//!
//! # Errors
//! - Repository discovery or HEAD resolution fails.

use std::io::Write;
use std::path::Path;

use chrono::Utc;
use color_eyre::eyre::WrapErr as _;
use color_eyre::eyre::eyre;
use ytil_sys::cli::Args as _;

/// Maximum number of characters shown for the commit subject before truncating with `…`.
const MAX_SUBJECT_LEN: usize = 33;

/// Short hash length (standard Git abbreviation).
const SHORT_HASH_LEN: usize = 7;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let repo = ytil_git::repo::discover(Path::new("."))?;
    let head = repo.head().wrap_err_with(|| eyre!("error resolving HEAD"))?;
    let commit = head
        .peel_to_commit()
        .wrap_err_with(|| eyre!("error peeling HEAD to commit"))?;

    let hash = commit.id().to_string();
    let short_hash = hash.get(..SHORT_HASH_LEN).unwrap_or(&hash);

    let commit_epoch = commit.time().seconds();
    let commit_seconds_delta = u64::try_from(Utc::now().timestamp().saturating_sub(commit_epoch).max(0))
        .wrap_err_with(|| eyre!("negative time delta after clamp"))?;

    let out = std::io::stdout();
    let mut out = out.lock();
    write!(out, "{short_hash} ")?;
    write_commit_relative_time(&mut out, commit_seconds_delta)?;
    write!(out, " | ")?;
    write_commit_truncated_msg(&mut out, commit.summary().unwrap_or(""), MAX_SUBJECT_LEN)?;
    writeln!(out)?;

    Ok(())
}

/// Writes a duration in seconds as the shortest human-readable unit directly to `out`.
fn write_commit_relative_time(out: &mut impl Write, secs: u64) -> std::io::Result<()> {
    let (value, suffix) = if secs < 60 {
        (secs, "s")
    } else if secs < 3_600 {
        (secs.saturating_div(60), "m")
    } else if secs < 86_400 {
        (secs.saturating_div(3_600), "h")
    } else if secs < 604_800 {
        (secs.saturating_div(86_400), "d")
    } else if secs < 2_592_000 {
        (secs.saturating_div(604_800), "w")
    } else if secs < 31_536_000 {
        (secs.saturating_div(2_592_000), "mo")
    } else {
        (secs.saturating_div(31_536_000), "y")
    };

    write!(out, "{value}{suffix}")
}

/// Writes up to `max` characters of `s`, appending `…` when the string is longer.
fn write_commit_truncated_msg(out: &mut impl Write, commit_msg: &str, max: usize) -> std::io::Result<()> {
    let boundary = commit_msg.char_indices().nth(max).map(|(i, _)| i);
    match boundary {
        Some(i) => write!(out, "{}…", commit_msg.get(..i).unwrap_or(commit_msg)),
        None => out.write_all(commit_msg.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    /// Helper: write to a `Vec<u8>` and return the resulting UTF-8 string.
    fn collect(f: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
        let mut buf = Vec::new();
        f(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[rstest]
    #[case::zero_seconds(0, "0s")]
    #[case::thirty_seconds(30, "30s")]
    #[case::fifty_nine_seconds(59, "59s")]
    #[case::one_minute(60, "1m")]
    #[case::ninety_seconds(90, "1m")]
    #[case::thirty_minutes(1_800, "30m")]
    #[case::fifty_nine_minutes(3_599, "59m")]
    #[case::one_hour(3_600, "1h")]
    #[case::twelve_hours(43_200, "12h")]
    #[case::twenty_three_hours(86_399, "23h")]
    #[case::one_day(86_400, "1d")]
    #[case::six_days(518_400, "6d")]
    #[case::one_week(604_800, "1w")]
    #[case::three_weeks(1_814_400, "3w")]
    #[case::one_month(2_592_000, "1mo")]
    #[case::six_months(15_552_000, "6mo")]
    #[case::one_year(31_536_000, "1y")]
    #[case::three_years(94_608_000, "3y")]
    fn write_commit_relative_time_formats_correctly(#[case] secs: u64, #[case] expected: &str) {
        let result = collect(|buf| write_commit_relative_time(buf, secs));
        pretty_assertions::assert_eq!(result, expected);
    }

    #[rstest]
    #[case::empty("", 33, "")]
    #[case::short("hello", 33, "hello")]
    #[case::exact_limit("abc", 3, "abc")]
    #[case::one_over("abcd", 3, "abc…")]
    #[case::long_subject(
        "feat: add user authentication and session management",
        33,
        "feat: add user authentication and…"
    )]
    #[case::unicode_safe("áéíóú_abcdef", 5, "áéíóú…")]
    fn write_commit_truncated_msg_formats_correctly(#[case] input: &str, #[case] max: usize, #[case] expected: &str) {
        let result = collect(|buf| write_commit_truncated_msg(buf, input, max));
        pretty_assertions::assert_eq!(result, expected);
    }
}
