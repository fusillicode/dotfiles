use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct GraphQlLsp {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for GraphQlLsp {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn download(&self) -> color_eyre::Result<NeedSymlink> {
        let bin_src_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
        )?;

        Ok(NeedSymlink::Yes {
            src: format!("{bin_src_dir}/{}", self.bin_name()).into(),
            dest: self.bin_dest_dir.clone().into(),
        })
    }
}
