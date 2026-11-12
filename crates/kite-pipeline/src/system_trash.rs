use crate::ports::Trash;
use std::path::Path;

/// The operating system's Trash (Recycle Bin on Windows). Files can be put back from there.
pub struct SystemTrash;

impl Trash for SystemTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        if std::fs::symlink_metadata(path).is_err() {
            return Err(format!("{} doesn't exist", path.display()));
        }
        context().delete(path).map_err(|e| e.to_string())
    }
}

/// How Kite Pack moves things to the Trash.
fn context() -> trash::TrashContext {
    trash::TrashContext::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn moves_a_folder_to_the_trash() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("eboot.bin"), b"boot").unwrap();
        assert_eq!(SystemTrash.trash(&game), Ok(()));
        assert!(!game.exists());
    }

    /// Asking Finder to delete makes macOS ask for permission to control Finder.
    #[cfg(target_os = "macos")]
    #[test]
    fn trashes_without_asking_to_control_finder() {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        assert!(matches!(context().delete_method(), DeleteMethod::NsFileManager));
    }

    #[test]
    fn missing_path_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(SystemTrash.trash(&tmp.path().join("nothing here")).is_err());
    }
}
