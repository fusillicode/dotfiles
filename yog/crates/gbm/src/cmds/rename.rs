use owo_colors::OwoColorize;

pub fn run(branch_name: &str) -> rootcause::Result<()> {
    ytil_git::branch::rename_current(branch_name, None)?;
    println!("{} {}", ">".magenta().bold(), branch_name.bold());
    Ok(())
}
