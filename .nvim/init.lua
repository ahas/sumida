require('telescope').setup {
  defaults = {
    file_ignore_patterns = {
      "target", ".nvim", ".git", "Cargo.lock", ".vscode", ".cargo"
    }
  }
}
