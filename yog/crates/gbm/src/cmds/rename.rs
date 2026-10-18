use owo_colors::OwoColorize;
use ytil_git::branch;

pub fn run(branch_name: &str) -> rootcause::Result<()> {
    branch::rename_current(branch_name, None)?;
    println!("{} {}", ">".magenta().bold(), branch_name.bold());
    Ok(())
}
