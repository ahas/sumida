require("telescope").setup({
	defaults = {
		file_ignore_patterns = {
			"target",
			".nvim",
			".git",
			"Cargo.lock",
			".vscode",
			".cargo",
		},
	},
})

vim.lsp.config("rust_analyzer", {
	settings = {
		["rust-analyzer"] = {
			cargo = {
				features = {
					"full-aws",
				},
			},
		},
	},
})
