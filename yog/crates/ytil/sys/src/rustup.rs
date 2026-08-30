use std::fmt::Display;
use std::fmt::Formatter;
use std::process::Command;
use std::process::Output;
use std::str::FromStr;

use jiff::civil::Date;
use rootcause::prelude::ResultExt;
use rootcause::report;
use strum::EnumString;
use ytil_cmd::CmdExt;

/// A requested Rust toolchain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestedRustToolchain {
    /// Select the newest installed stable channel, optionally starting at a date.
    Stable(Option<RustToolchainDate>),
    /// Select the newest installed beta channel, optionally starting at a date.
    Beta(Option<RustToolchainDate>),
    /// Select the newest installed nightly channel, optionally starting at a date.
    Nightly(Option<RustToolchainDate>),
    /// Use an exact Rustup toolchain name after checking that it is installed.
    Exact(RustToolchainName),
}

/// A validated date embedded in a Rustup toolchain name.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustToolchainDate(Date);

impl RustToolchainDate {
    /// Returns the underlying calendar date.
    pub const fn as_date(self) -> Date {
        self.0
    }
}

impl TryFrom<(&str, &str, &str)> for RustToolchainDate {
    type Error = rootcause::Report;

    fn try_from((year, month, day): (&str, &str, &str)) -> Result<Self, Self::Error> {
        if year.len() != 4 || !year.chars().all(|character| character.is_ascii_digit()) {
            return Err(report!("Rust toolchain date year has an invalid format").attach(format!("year={year:?}")));
        }
        if month.len() != 2 || !month.chars().all(|character| character.is_ascii_digit()) {
            return Err(report!("Rust toolchain date month has an invalid format").attach(format!("month={month:?}")));
        }
        if day.len() != 2 || !day.chars().all(|character| character.is_ascii_digit()) {
            return Err(report!("Rust toolchain date day has an invalid format").attach(format!("day={day:?}")));
        }

        let year = year
            .parse::<i16>()
            .context("failed to parse Rust toolchain date year")
            .attach(format!("year={year:?}"))?;
        let month = month
            .parse::<i8>()
            .context("failed to parse Rust toolchain date month")
            .attach(format!("month={month:?}"))?;
        let day = day
            .parse::<i8>()
            .context("failed to parse Rust toolchain date day")
            .attach(format!("day={day:?}"))?;

        Ok(Date::new(year, month, day)
            .context("Rust toolchain date is not a valid calendar date")
            .attach(format!("year={year}"))
            .attach(format!("month={month}"))
            .attach(format!("day={day}"))
            .map(Self)?)
    }
}

impl From<Date> for RustToolchainDate {
    fn from(value: Date) -> Self {
        Self(value)
    }
}

impl Display for RustToolchainDate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RustToolchainDate {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let input = value;
        let mut components = value.split('-');
        let Some(year) = components.next() else {
            return Err(report!("Rust toolchain date is missing a year").attach(format!("input={input:?}")));
        };
        if year.is_empty() {
            return Err(report!("Rust toolchain date is missing a year").attach(format!("input={input:?}")));
        }
        let Some(month) = components.next() else {
            return Err(report!("Rust toolchain date is missing a month").attach(format!("input={input:?}")));
        };
        if month.is_empty() {
            return Err(report!("Rust toolchain date is missing a month").attach(format!("input={input:?}")));
        }
        let Some(day) = components.next() else {
            return Err(report!("Rust toolchain date is missing a day").attach(format!("input={input:?}")));
        };
        if day.is_empty() {
            return Err(report!("Rust toolchain date is missing a day").attach(format!("input={input:?}")));
        }
        if components.next().is_some() {
            return Err(report!("Rust toolchain date has extra components").attach(format!("input={input:?}")));
        }

        Self::try_from((year, month, day)).attach(format!("input={input:?}"))
    }
}

/// A validated Rust compiler commit date.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustcCommitDate(Date);

impl RustcCommitDate {
    /// Returns the underlying calendar date.
    pub const fn as_date(self) -> Date {
        self.0
    }
}

impl From<Date> for RustcCommitDate {
    fn from(value: Date) -> Self {
        Self(value)
    }
}

impl Display for RustcCommitDate {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RustcCommitDate {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let input = value;
        let value = value.strip_prefix("commit-date: ").ok_or_else(|| {
            report!("Rust compiler commit date is missing its prefix").attach(format!("input={input:?}"))
        })?;
        let date = value
            .parse::<RustToolchainDate>()
            .context("failed to parse Rust compiler commit date")
            .attach(format!("input={input:?}"))?;
        Ok(Self(date.as_date()))
    }
}

/// A validated Rustup toolchain name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustToolchainName(String);

impl Display for RustToolchainName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for RustToolchainName {
    type Error = rootcause::Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(report!("Rust toolchain name is empty").attach(format!("input={value:?}")));
        }
        if let Some(character) = value
            .chars()
            .find(|character| !character.is_ascii_alphanumeric() && !matches!(*character, '-' | '_' | '.'))
        {
            return Err(report!("Rust toolchain name contains an invalid character")
                .attach(format!("input={value:?}"))
                .attach(format!("character={character:?}")));
        }

        Ok(Self(value.to_owned()))
    }
}

impl FromStr for RustToolchainName {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}

#[derive(Clone, Copy, Debug, EnumString, Eq, PartialEq)]
#[strum(serialize_all = "snake_case")]
enum RustToolchainChannel {
    Stable,
    Beta,
    Nightly,
}

impl RequestedRustToolchain {
    const fn channel_request(&self) -> Option<(RustToolchainChannel, Option<RustToolchainDate>)> {
        match self {
            Self::Stable(date) => Some((RustToolchainChannel::Stable, *date)),
            Self::Beta(date) => Some((RustToolchainChannel::Beta, *date)),
            Self::Nightly(date) => Some((RustToolchainChannel::Nightly, *date)),
            Self::Exact(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstalledChannelToolchain {
    channel: RustToolchainChannel,
    date: Option<RustToolchainDate>,
    name: RustToolchainName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum InstalledRustToolchain {
    Channel(InstalledChannelToolchain),
    Exact(RustToolchainName),
}

impl FromStr for InstalledRustToolchain {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let name = value.parse::<RustToolchainName>()?;
        let (channel_name, channel_suffix) = value.split_once('-').unwrap_or((value, ""));
        let Ok(channel) = channel_name.parse::<RustToolchainChannel>() else {
            return Ok(Self::Exact(name));
        };

        let mut components = channel_suffix.split('-');
        let date = match components.next() {
            None => None,
            Some(year) if year.len() != 4 || !year.chars().all(|character| character.is_ascii_digit()) => None,
            Some(year) => {
                let month = components.next().ok_or_else(|| {
                    report!("installed Rust toolchain date is missing a month")
                        .attach(format!("toolchain_name={value:?}"))
                        .attach(format!("year={year:?}"))
                })?;
                let day = components.next().ok_or_else(|| {
                    report!("installed Rust toolchain date is missing a day")
                        .attach(format!("toolchain_name={value:?}"))
                        .attach(format!("year={year:?}"))
                })?;
                Some(
                    RustToolchainDate::try_from((year, month, day))
                        .context("failed to parse installed Rust toolchain date")
                        .attach(format!("toolchain_name={value:?}"))?,
                )
            }
        };
        Ok(Self::Channel(InstalledChannelToolchain { channel, date, name }))
    }
}

/// Find the latest installed Rust toolchain matching the request.
///
/// This does not update or install any Rust toolchain.
///
/// An exact request returns its name after checking that the toolchain is installed.
///
/// # Errors
///
/// Returns an error if Rustup cannot list the installed toolchains or if no matching toolchain is
/// installed.
pub fn find_latest_installed_rust_toolchain(
    requested_rust_toolchain: &RequestedRustToolchain,
) -> rootcause::Result<RustToolchainName> {
    let requested_channel = match requested_rust_toolchain {
        RequestedRustToolchain::Stable(_) => RustToolchainChannel::Stable,
        RequestedRustToolchain::Beta(_) => RustToolchainChannel::Beta,
        RequestedRustToolchain::Nightly(_) => RustToolchainChannel::Nightly,
        RequestedRustToolchain::Exact(name) => {
            let rustc_output = inspect_rustc_toolchain(name)?;
            ytil_cmd::extract_success_output(&rustc_output)
                .context("failed to read exact Rust toolchain information")
                .attach(format!("toolchain_name={name:?}"))?;
            return Ok(name.clone());
        }
    };

    let rustup_output = list_rustup_toolchains()?;
    let installed_rust_toolchains = parse_installed_rust_toolchains(
        &ytil_cmd::extract_success_output(&rustup_output).context("failed to read installed Rust toolchain list")?,
    );

    let unqualified_toolchain = installed_rust_toolchains.iter().find_map(|installed| match installed {
        InstalledRustToolchain::Channel(candidate)
            if candidate.channel == requested_channel && candidate.date.is_none() =>
        {
            Some(candidate)
        }
        InstalledRustToolchain::Channel(_) | InstalledRustToolchain::Exact(_) => None,
    });
    let unqualified_rustc_commit_date = match unqualified_toolchain {
        Some(candidate) => {
            let rustc_output = inspect_rustc_toolchain(&candidate.name)?;
            let output = ytil_cmd::extract_success_output(&rustc_output)
                .context("failed to read Rust compiler toolchain information")
                .attach(format!("toolchain_name={:?}", candidate.name))?;
            output
                .lines()
                .find_map(|line| line.strip_prefix("commit-date: ").map(|_| line))
                .map(|line| {
                    line.parse::<RustcCommitDate>()
                        .context("failed to parse Rust compiler commit date")
                        .attach(format!("line={line:?}"))
                })
                .transpose()?
        }
        None => None,
    };

    let Some(name) = select_latest_installed_rust_toolchain(
        requested_rust_toolchain,
        &installed_rust_toolchains,
        unqualified_rustc_commit_date,
    ) else {
        return Err(report!("no matching installed Rust toolchain found")
            .attach(format!("requested_rust_toolchain={requested_rust_toolchain:?}")));
    };

    Ok(name)
}

fn list_rustup_toolchains() -> rootcause::Result<Output> {
    let mut command = Command::new("rustup");
    command.args(["toolchain", "list"]);
    Ok(command.exec().context("failed to list installed Rust toolchains")?)
}

fn inspect_rustc_toolchain(toolchain: &RustToolchainName) -> rootcause::Result<Output> {
    let mut command = Command::new("rustc");
    command.arg(format!("+{toolchain}")).arg("-Vv");
    Ok(command
        .exec()
        .context("failed to inspect Rust toolchain")
        .attach(format!("toolchain_name={toolchain:?}"))?)
}

fn parse_installed_rust_toolchains(output: &str) -> Vec<InstalledRustToolchain> {
    let mut installed_rust_toolchains = Vec::new();
    for line in output.lines() {
        let Some(name) = line.split_ascii_whitespace().next() else {
            continue;
        };
        let Ok(toolchain) = name.parse::<InstalledRustToolchain>() else {
            continue;
        };
        installed_rust_toolchains.push(toolchain);
    }
    installed_rust_toolchains
}

fn select_latest_installed_rust_toolchain(
    requested_rust_toolchain: &RequestedRustToolchain,
    installed_rust_toolchains: &[InstalledRustToolchain],
    unqualified_rustc_commit_date: Option<RustcCommitDate>,
) -> Option<RustToolchainName> {
    let (channel, minimum_date) = requested_rust_toolchain.channel_request()?;
    let mut unqualified_toolchain = None;
    let mut latest_dated_toolchain = None;

    for installed in installed_rust_toolchains {
        let InstalledRustToolchain::Channel(candidate) = installed else {
            continue;
        };
        if candidate.channel != channel {
            continue;
        }

        let Some(date) = candidate.date else {
            if unqualified_toolchain.is_none() {
                unqualified_toolchain = Some(candidate);
            }
            continue;
        };
        if minimum_date.is_some_and(|minimum| date < minimum) {
            continue;
        }

        let is_newer = latest_dated_toolchain.is_none_or(|(_, latest_date)| date > latest_date);
        if is_newer {
            latest_dated_toolchain = Some((candidate, date));
        }
    }

    let unqualified_rustc_date = unqualified_rustc_commit_date.map(RustcCommitDate::as_date);
    let unqualified_toolchain_is_eligible = unqualified_toolchain.is_some()
        && minimum_date.is_none_or(|minimum| unqualified_rustc_date.is_some_and(|date| date >= minimum.as_date()));

    match (unqualified_toolchain, latest_dated_toolchain) {
        (Some(unqualified), Some((dated, dated_date))) if unqualified_toolchain_is_eligible => {
            if unqualified_rustc_date.is_some_and(|date| date > dated_date.as_date()) {
                Some(unqualified.name.clone())
            } else {
                Some(dated.name.clone())
            }
        }
        (Some(unqualified), None) if unqualified_toolchain_is_eligible => Some(unqualified.name.clone()),
        (None | Some(_), Some((dated, _))) => Some(dated.name.clone()),
        (Some(_) | None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[rstest::rstest]
    #[case("1.95.0", RustToolchainName("1.95.0".to_owned()))]
    #[case("1.99.0-beta.1", RustToolchainName("1.99.0-beta.1".to_owned()))]
    fn test_rust_toolchain_name_when_name_is_parsed_returns_typed_value(
        #[case] value: &str,
        #[case] expected: RustToolchainName,
    ) {
        assert_that!(value.parse::<RustToolchainName>(), ok(eq(expected)));
    }

    #[rstest::rstest]
    #[case("1.95.0")]
    #[case("1.99.0-beta.1")]
    fn test_rust_toolchain_name_when_formatted_returns_original_name(#[case] value: &str) {
        let actual = RustToolchainName(value.to_owned()).to_string();

        assert_that!(actual, eq(value));
    }

    #[rstest::rstest]
    #[case("")]
    #[case("nightly 2026")]
    #[case("nightly/2026")]
    fn test_rust_toolchain_name_when_invalid_name_is_parsed_returns_error(#[case] value: &str) {
        let actual = value.parse::<RustToolchainName>();

        assert_that!(actual, err(anything()));
    }

    #[test]
    fn test_rust_toolchain_name_when_name_contains_invalid_character_returns_static_error_with_input_context() {
        let actual = "nightly/2026".parse::<RustToolchainName>().unwrap_err();

        assert_eq!(
            actual.format_current_context().to_string(),
            "Rust toolchain name contains an invalid character"
        );
        assert!(
            actual
                .attachments()
                .iter()
                .any(|attachment| attachment.to_string() == "input=\"nightly/2026\"")
        );
        assert!(
            actual
                .attachments()
                .iter()
                .any(|attachment| attachment.to_string() == "character='/'")
        );
    }

    #[test]
    fn test_parse_installed_rust_toolchains_when_output_contains_status_marker_returns_toolchains() {
        let output = "nightly-2026-07-12-aarch64-apple-darwin\nnightly-2026-08-30-aarch64-apple-darwin (active)\n";
        let actual = parse_installed_rust_toolchains(output);
        let expected = vec![
            InstalledRustToolchain::Channel(InstalledChannelToolchain {
                channel: RustToolchainChannel::Nightly,
                date: Some(RustToolchainDate(jiff::civil::Date::new(2026, 7, 12).unwrap())),
                name: RustToolchainName("nightly-2026-07-12-aarch64-apple-darwin".to_owned()),
            }),
            InstalledRustToolchain::Channel(InstalledChannelToolchain {
                channel: RustToolchainChannel::Nightly,
                date: Some(RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap())),
                name: RustToolchainName("nightly-2026-08-30-aarch64-apple-darwin".to_owned()),
            }),
        ];

        assert_that!(actual, eq(expected));
    }

    #[rstest::rstest]
    #[case(
        "stable-aarch64-apple-darwin",
        InstalledRustToolchain::Channel(InstalledChannelToolchain {
            channel: RustToolchainChannel::Stable,
            date: None,
            name: RustToolchainName("stable-aarch64-apple-darwin".to_owned()),
        })
    )]
    #[case(
        "stable",
        InstalledRustToolchain::Channel(InstalledChannelToolchain {
            channel: RustToolchainChannel::Stable,
            date: None,
            name: RustToolchainName("stable".to_owned()),
        })
    )]
    #[case(
        "nightly-2026-08-30-aarch64-apple-darwin",
        InstalledRustToolchain::Channel(InstalledChannelToolchain {
            channel: RustToolchainChannel::Nightly,
            date: Some(RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap())),
            name: RustToolchainName("nightly-2026-08-30-aarch64-apple-darwin".to_owned()),
        })
    )]
    #[case(
        "1.99.0-beta.1-aarch64-apple-darwin",
        InstalledRustToolchain::Exact(RustToolchainName("1.99.0-beta.1-aarch64-apple-darwin".to_owned()))
    )]
    fn test_parse_installed_rust_toolchains_when_input_contains_channel_and_exact_names_classifies_toolchains(
        #[case] name: &str,
        #[case] expected: InstalledRustToolchain,
    ) {
        let actual = parse_installed_rust_toolchains(name);

        assert_that!(actual, eq(vec![expected]));
    }

    #[rstest::rstest]
    #[case(RequestedRustToolchain::Stable(None), "stable-aarch64-apple-darwin")]
    #[case(RequestedRustToolchain::Beta(None), "beta-aarch64-apple-darwin")]
    #[case(RequestedRustToolchain::Nightly(None), "nightly-aarch64-apple-darwin")]
    fn test_select_latest_installed_rust_toolchain_when_only_unqualified_channel_is_installed_returns_it(
        #[case] toolchain: RequestedRustToolchain,
        #[case] expected: &str,
    ) {
        let installed = parse_installed_rust_toolchains(
            "stable-aarch64-apple-darwin\nbeta-aarch64-apple-darwin\nnightly-aarch64-apple-darwin\n",
        );
        let actual = select_latest_installed_rust_toolchain(&toolchain, &installed, None);

        assert_that!(actual, some(eq(RustToolchainName(expected.to_owned()))));
    }

    #[test]
    fn test_select_latest_installed_rust_toolchain_when_multiple_dated_channels_exist_returns_newest() {
        let installed = parse_installed_rust_toolchains(
            "nightly-2026-07-12-aarch64-apple-darwin\nnightly-2026-08-31-aarch64-apple-darwin\n",
        );
        let actual = select_latest_installed_rust_toolchain(&RequestedRustToolchain::Nightly(None), &installed, None);

        assert_that!(
            actual,
            some(eq(RustToolchainName(
                "nightly-2026-08-31-aarch64-apple-darwin".to_owned(),
            )))
        );
    }

    #[rstest::rstest]
    #[case(
        Some(RustcCommitDate(jiff::civil::Date::new(2026, 8, 31).unwrap())),
        "nightly-aarch64-apple-darwin"
    )]
    #[case(
        Some(RustcCommitDate(jiff::civil::Date::new(2026, 8, 30).unwrap())),
        "nightly-2026-08-30-aarch64-apple-darwin"
    )]
    #[case(
        Some(RustcCommitDate(jiff::civil::Date::new(2026, 8, 29).unwrap())),
        "nightly-2026-08-30-aarch64-apple-darwin"
    )]
    #[case(None, "nightly-2026-08-30-aarch64-apple-darwin")]
    fn test_select_latest_installed_rust_toolchain_when_unqualified_and_dated_channels_exist_selects_newest_by_compiler_date(
        #[case] rustc_commit_date: Option<RustcCommitDate>,
        #[case] expected: &str,
    ) {
        let installed =
            parse_installed_rust_toolchains("nightly-2026-08-30-aarch64-apple-darwin\nnightly-aarch64-apple-darwin\n");
        let actual = select_latest_installed_rust_toolchain(
            &RequestedRustToolchain::Nightly(None),
            &installed,
            rustc_commit_date,
        );

        assert_that!(actual, some(eq(RustToolchainName(expected.to_owned()))));
    }

    #[test]
    fn test_select_latest_installed_rust_toolchain_when_only_unqualified_channel_exists_returns_toolchain() {
        let installed = parse_installed_rust_toolchains("nightly-aarch64-apple-darwin\n");
        let actual = select_latest_installed_rust_toolchain(&RequestedRustToolchain::Nightly(None), &installed, None);

        assert_that!(
            actual,
            some(eq(RustToolchainName("nightly-aarch64-apple-darwin".to_owned())))
        );
    }

    #[test]
    fn test_select_latest_installed_rust_toolchain_when_requested_channel_is_not_installed_returns_none() {
        let installed = parse_installed_rust_toolchains("stable-aarch64-apple-darwin\n");
        let actual = select_latest_installed_rust_toolchain(&RequestedRustToolchain::Nightly(None), &installed, None);

        assert_that!(actual, none());
    }

    #[rstest::rstest]
    #[case(
        RequestedRustToolchain::Stable(Some(RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap()))),
        "stable-2026-08-31-aarch64-apple-darwin"
    )]
    #[case(
        RequestedRustToolchain::Beta(Some(RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap()))),
        "beta-2026-08-31-aarch64-apple-darwin"
    )]
    #[case(
        RequestedRustToolchain::Nightly(Some(RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap()))),
        "nightly-2026-08-31-aarch64-apple-darwin"
    )]
    fn test_select_latest_installed_rust_toolchain_when_minimum_date_is_requested_returns_newest_matching_toolchain(
        #[case] toolchain: RequestedRustToolchain,
        #[case] expected: &str,
    ) {
        let installed = parse_installed_rust_toolchains(
            "stable-2026-08-31-aarch64-apple-darwin\nbeta-2026-08-31-aarch64-apple-darwin\nnightly-2026-08-30-aarch64-apple-darwin\nnightly-2026-08-31-aarch64-apple-darwin\n",
        );
        let actual = select_latest_installed_rust_toolchain(&toolchain, &installed, None);

        assert_that!(actual, some(eq(RustToolchainName(expected.to_owned()))));
    }

    #[test]
    fn test_select_latest_installed_rust_toolchain_when_channel_is_missing_returns_none() {
        let installed = parse_installed_rust_toolchains("stable-2026-08-30-aarch64-apple-darwin\n");
        let toolchain =
            RequestedRustToolchain::Nightly(Some(RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap())));
        let actual = select_latest_installed_rust_toolchain(&toolchain, &installed, None);

        assert_that!(actual, none());
    }

    #[rstest::rstest]
    #[case(
        "commit-date: 2026-08-30",
        RustcCommitDate(jiff::civil::Date::new(2026, 8, 30).unwrap())
    )]
    fn test_rustc_commit_date_when_valid_value_is_parsed_returns_date(
        #[case] value: &str,
        #[case] expected: RustcCommitDate,
    ) {
        assert_that!(value.parse::<RustcCommitDate>(), ok(eq(expected)));
    }

    #[rstest::rstest]
    #[case("")]
    #[case("2026-08-30")]
    #[case("2026-8-30")]
    #[case("commit-date: 2026-02-29")]
    #[case("2026-08-30-extra")]
    #[case("aarch64-apple-darwin")]
    fn test_rustc_commit_date_when_invalid_value_is_parsed_returns_error(#[case] value: &str) {
        let actual = value.parse::<RustcCommitDate>();

        assert_that!(actual, err(anything()));
    }

    #[rstest::rstest]
    #[case(
        "2026-08-30",
        RustToolchainDate(jiff::civil::Date::new(2026, 8, 30).unwrap())
    )]
    fn test_rust_toolchain_date_when_valid_value_is_parsed_returns_date(
        #[case] value: &str,
        #[case] expected: RustToolchainDate,
    ) {
        assert_that!(value.parse::<RustToolchainDate>(), ok(eq(expected)));
    }

    #[rstest::rstest]
    #[case("")]
    #[case("2026-02-29")]
    #[case("2026-8-30")]
    #[case("2026-08-30-extra")]
    #[case("commit-date: 2026-08-30")]
    fn test_rust_toolchain_date_when_invalid_value_is_parsed_returns_error(#[case] value: &str) {
        let actual = value.parse::<RustToolchainDate>();

        assert_that!(actual, err(anything()));
    }

    #[test]
    fn test_rust_toolchain_date_when_calendar_date_is_invalid_returns_static_error_with_input_context() {
        let actual = "2026-02-29".parse::<RustToolchainDate>().unwrap_err();

        assert_eq!(
            actual.format_current_context().to_string(),
            "Rust toolchain date is not a valid calendar date"
        );
        assert!(
            actual
                .attachments()
                .iter()
                .any(|attachment| attachment.to_string() == "input=\"2026-02-29\"")
        );
    }
}
