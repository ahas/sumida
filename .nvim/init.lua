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

require("lspconfig").rust_analyzer.setup({
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
