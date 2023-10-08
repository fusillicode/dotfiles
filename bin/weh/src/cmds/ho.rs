use std::process::Command;

pub fn run<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<(), anyhow::Error> {
    let Some(file_to_open) = args.next() else {
        anyhow::bail!("no input file supplied {:?}", args.collect::<Vec<_>>())
    };

    let hx_pane_id = crate::utils::get_current_pane_sibling_with_title("hx")?.pane_id;

    Command::new("sh")
        .args([
            "-c",
            &format!(
                r#"
                    wezterm cli send-text --pane-id '{hx_pane_id}' ':o {file_to_open}' --no-paste && \
                        printf "\r" | wezterm cli send-text --pane-id '{hx_pane_id}' --no-paste && \
                        wezterm cli activate-pane --pane-id '{hx_pane_id}'
                "#,
            ),
        ])
        .spawn()?;

    Ok(())
}
