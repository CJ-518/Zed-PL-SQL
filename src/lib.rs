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
        // The proxy sits between Zed and the real plsqllang-server: it forwards
        // every LSP message through unchanged, except it filters out known
        // false-positive diagnostics (SQL*Plus / SQLcl client-side directives
        // that plsqllang-server incorrectly flags as PL/SQL syntax errors).
        let proxy_path = worktree.which("plsqllang-proxy").ok_or_else(|| {
            "plsqllang-proxy not found on PATH. Build it from the proxy/ directory of this repo (cargo build --release) and make sure the resulting binary is on PATH.".to_string()
        })?;

        let real_server_path = worktree.which("plsqllang-server").ok_or_else(|| {
            "plsqllang-server not found on PATH. Check C:\\tools\\bin\\plsqllang-server.bat exists and PATH includes C:\\tools\\bin.".to_string()
        })?;

        Ok(zed::Command {
            command: proxy_path,
            args: vec![real_server_path],
            env: Default::default(),
        })
    }
}

zed::register_extension!(PlsqllangExtension);
