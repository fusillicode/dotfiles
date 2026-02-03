//! Rust test runner helpers integrating with Nvim.
//!
//! Exposes a dictionary enabling cursor-aware test execution (`run_test`) by parsing the current buffer
//! with Tree‑sitter to locate the nearest test function and spawning it inside a WezTerm pane.
//! All Nvim API failures are reported via [`ytil_noxi::notify::error`].

use std::path::Path;

use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;
use ytil_noxi::buffer::BufferExt;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "run_test": fn_from!(run_test),
    }
}

#[derive(Clone, Copy, Deserialize)]
enum TargetTerminal {
    WezTerm,
    Nvim,
}

impl FromObject for TargetTerminal {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

impl Poppable for TargetTerminal {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        // SAFETY: The caller (nvim-oxi framework) guarantees that:
        // 1. `lstate` is a valid pointer to an initialized Lua state
        // 2. The Lua stack has at least one value to pop
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

fn run_test(target_terminal: TargetTerminal) -> Option<()> {
    let file_path = ytil_noxi::buffer::get_absolute_path(Some(&nvim_oxi::api::get_current_buf()))?;

    let test_name = ytil_noxi::tree_sitter::get_enclosing_fn_name_of_position(&file_path)?;

    let test_runner = get_test_runner_for_path(&file_path)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "error getting test runner | path={} error={err:#?}",
                file_path.display()
            ));
        })
        .ok()?;

    match target_terminal {
        TargetTerminal::WezTerm => run_test_in_wezterm(test_runner, &test_name),
        TargetTerminal::Nvim => run_test_in_nvim_term(test_runner, &test_name),
    }
}

fn run_test_in_wezterm(test_runner: &str, test_name: &str) -> Option<()> {
    let cur_pane_id = ytil_wezterm::get_current_pane_id()
        .inspect_err(|err| ytil_noxi::notify::error(format!("error getting current WezTerm pane id | error={err:#?}")))
        .ok()?;

    let wez_panes = ytil_wezterm::get_all_panes(&[])
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!("error getting WezTerm panes | error={err:#?}"));
        })
        .ok()?;

    let Some(cur_pane) = wez_panes.iter().find(|p| p.pane_id == cur_pane_id) else {
        ytil_noxi::notify::error(format!(
            "error WezTerm pane not found | pane_id={cur_pane_id:#?} panes={wez_panes:#?}"
        ));
        return None;
    };

    let Some(test_runner_pane) = wez_panes.iter().find(|p| p.is_sibling_terminal_pane_of(cur_pane)) else {
        ytil_noxi::notify::error(format!(
            "error finding sibling pane to run test | current_pane={cur_pane:#?} panes={wez_panes:#?} test={test_name}"
        ));
        return None;
    };

    let test_run_cmd = format!("'{test_runner} {test_name}'");

    let send_text_to_pane_cmd = ytil_wezterm::send_text_to_pane_cmd(&test_run_cmd, test_runner_pane.pane_id);
    let submit_pane_cmd = ytil_wezterm::submit_pane_cmd(test_runner_pane.pane_id);

    ytil_cmd::silent_cmd("sh")
        .args(["-c", &format!("{send_text_to_pane_cmd} && {submit_pane_cmd}")])
        .spawn()
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "error executing test run cmd | cmd={test_run_cmd:#?} pane={test_runner_pane:#?} error={err:#?}"
            ));
        })
        .ok()?;

    Some(())
}

fn run_test_in_nvim_term(test_runner: &str, test_name: &str) -> Option<()> {
    let Some(terminal_buffer) = nvim_oxi::api::list_bufs().find(BufferExt::is_terminal) else {
        ytil_noxi::notify::error(format!(
            "error no terminal buffer found | test_runner={test_runner:?} test_name={test_name:?}",
        ));
        return None;
    };

    terminal_buffer.send_command(&format!("{test_runner} {test_name}\n"));

    Some(())
}

/// Get the application to use to run the tests based on the presence of a `Makefile.toml`
/// in the root of a git repository where the supplied [Path] resides.
///
/// If the file is found "cargo make test" is used to run the tests.
/// "cargo test" is used otherwise.
///
/// Assumptions:
/// 1. We're always working in a git repository
/// 2. no custom config file for cargo-make
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - The path is not inside a Git repository.
fn get_test_runner_for_path(path: &Path) -> color_eyre::Result<&'static str> {
    let git_repo_root = ytil_git::repo::get_root(&ytil_git::repo::discover(path)?);

    if std::fs::read_dir(git_repo_root)?.any(|res| res.as_ref().is_ok_and(|de| de.file_name() == "Makefile.toml")) {
        return Ok("cargo make test");
    }

    Ok("cargo test")
}
