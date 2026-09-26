use zed_extension_api as zed;

struct PlsqllangExtension;

impl zed::Extension for PlsqllangExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let path = worktree
            .which("plsqllang-server")
            .ok_or_else(|| {
                "plsqllang-server not found on PATH. Check C:\\tools\\bin\\plsqllang-server.bat exists and PATH includes C:\\tools\\bin.".to_string()
            })?;

        Ok(zed::Command {
            command: path,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(PlsqllangExtension);