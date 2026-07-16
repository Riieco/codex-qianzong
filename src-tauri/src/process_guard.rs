use crate::error::{AppError, AppResult};
use std::process::Command;

pub fn ensure_codex_not_running() -> AppResult<()> {
    if codex_is_running()? {
        return Err(AppError::Process(
            "检测到 Codex 仍在运行。请完全退出 Codex 后再迁移已有会话，避免会话文件继续写入旧版本。"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn codex_is_running() -> AppResult<bool> {
    use std::os::windows::process::CommandExt;

    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Codex.exe", "/FO", "CSV", "/NH"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|err| AppError::Process(format!("无法检查 Codex 进程: {err}")))?;
    if !output.status.success() {
        return Err(AppError::Process(format!(
            "检查 Codex 进程失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(tasklist_contains_codex(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(windows)]
fn tasklist_contains_codex(output: &str) -> bool {
    output.lines().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("\"codex.exe\"")
    })
}

#[cfg(not(windows))]
fn codex_is_running() -> AppResult<bool> {
    for name in ["Codex", "codex"] {
        let output = Command::new("pgrep")
            .args(["-x", name])
            .output()
            .map_err(|err| AppError::Process(format!("无法检查 Codex 进程: {err}")))?;
        match output.status.code() {
            Some(0) => return Ok(true),
            Some(1) => {}
            _ => {
                return Err(AppError::Process(format!(
                    "检查 Codex 进程失败: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )))
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn parses_tasklist_output_without_matching_similar_names() {
        assert!(super::tasklist_contains_codex(
            "\"Codex.exe\",\"100\",\"Console\",\"1\",\"10,000 K\""
        ));
        assert!(!super::tasklist_contains_codex(
            "\"codex-qianzong.exe\",\"100\",\"Console\",\"1\",\"10,000 K\""
        ));
    }
}
