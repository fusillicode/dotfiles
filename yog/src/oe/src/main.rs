#![feature(exit_status_error)]

use std::str::FromStr;

use color_eyre::eyre::bail;
use utils::editor::Editor;
use utils::editor::FileToOpen;

/// Opens files in a running editor instance (Neovim or Helix) from Wezterm.
///
/// This tool integrates with Wezterm terminal multiplexer to open files in an existing
/// editor instance running in a sibling pane. It supports both Neovim and Helix editors
/// and can be used both as a command-line tool and as a Wezterm open-uri handler.
///
/// # Arguments
///
/// * `editor` - The editor to use ("nvim" for Neovim, "hx" for Helix)
/// * `file_path` - Path to the file to open
/// * `pane_id` - Optional Wezterm pane ID (auto-detected if not provided)
///
/// # How it Works
///
/// 1. Detects the current Wezterm pane
/// 2. Finds a sibling pane running the specified editor
/// 3. Sends commands to the editor pane to open the file
/// 4. Activates the editor pane for user interaction
///
/// # Examples
///
/// Open file in Neovim:
/// ```bash
/// oe nvim /path/to/file.rs
/// ```
///
/// Open file in Helix:
/// ```bash
/// oe hx /path/to/file.rs
/// ```
///
/// Open file in Neovim with specific pane:
/// ```bash
/// oe nvim /path/to/file.rs 5
/// ```
///
/// # Integration
///
/// This tool is designed to work with Wezterm's open-uri handler for seamless
/// file opening from various sources including terminals, file managers, and
/// other applications.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let enriched_path_env = get_enriched_path_env()?;
    let args = utils::system::get_args();

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        bail!("no editor specified {args:#?}");
    };

    let Some(file_to_open) = args.get(1) else {
        bail!("no input file specified {args:#?}");
    };

    let pane_id = match args.get(2) {
        Some(x) => x.parse()?,
        None => utils::wezterm::get_current_pane_id()?,
    };

    let panes = utils::wezterm::get_all_panes(&[enriched_path_env.by_ref()])?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        utils::wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles()).map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                "{} && {} && {} && {}",
                // `wezterm cli send-text $'\e'` sends the "ESC" to Wezterm to exit from insert mode
                // https://github.com/wez/wezterm/discussions/3945
                utils::wezterm::send_text_to_pane_cmd(r#"$'\e'"#, editor_pane_id),
                utils::wezterm::send_text_to_pane_cmd(&format!("'{open_file_cmd}'"), editor_pane_id),
                utils::wezterm::submit_pane_cmd(editor_pane_id),
                utils::wezterm::activate_pane_cmd(editor_pane_id),
            ),
        ])
        .envs([enriched_path_env.by_ref()].iter().copied())
        .spawn()?;

    Ok(())
}

/// Creates an enriched PATH environment variable for Wezterm integration.
///
/// When called from Wezterm's open-uri handler, the PATH environment variable
/// may not include all necessary directories. This function creates an enriched
/// PATH that includes:
/// - The existing PATH (if any)
/// - Homebrew's bin directory (/opt/homebrew/bin)
/// - The user's local bin directory (~/.local/bin)
///
/// This ensures that all necessary tools are available when opening files
/// through the Wezterm integration.
///
/// # Returns
///
/// Returns an [Env] containing the enriched PATH variable.
fn get_enriched_path_env() -> color_eyre::Result<Env> {
    let enriched_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &utils::system::build_home_path(&[".local", "bin"])?.to_string_lossy(),
    ]
    .join(":");

    Ok(Env {
        name: "PATH",
        value: enriched_path,
    })
}

/// A wrapper type for environment variables that provides convenient access methods.
///
/// This struct represents an environment variable with a static name and a dynamic value.
/// It's designed to work with APIs that require `(&str, &str)` tuples for environment
/// variables while allowing the value to be dynamically constructed.
struct Env {
    /// The name of the environment variable (static string).
    name: &'static str,
    /// The value of the environment variable (dynamically constructed string).
    value: String,
}

impl Env {
    /// Returns a reference to the environment variable as a tuple of name and value.
    ///
    /// This method provides a convenient way to get the environment variable in the
    /// format expected by APIs like `std::process::Command::envs()` which require
    /// `(&str, &str)` tuples.
    ///
    /// # Returns
    ///
    /// A tuple containing the environment variable name and value as string references.
    pub fn by_ref(&self) -> (&'static str, &str) {
        (self.name, &self.value)
    }
}
