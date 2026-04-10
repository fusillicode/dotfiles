`third_party/nvim-oxi` is intended to be managed as a git subtree copy of `nvim-oxi`.

Current import base:

- upstream repo: `https://github.com/noib3/nvim-oxi`

Workflow:

- local patches live directly under `third_party/nvim-oxi` and are committed in `dotfiles`
- fresh `dotfiles` clones get the full source without submodule setup

Important:

- `git subtree pull` / `git subtree split` require an actual `git subtree add --prefix=third_party/nvim-oxi ... --squash` merge commit
- on a clean checkout, initialize the subtree before building

Initialize subtree:

- run:
  `git subtree add --prefix=third_party/nvim-oxi https://github.com/noib3/nvim-oxi master --squash`

Update from upstream:

- run:
  `git subtree pull --prefix=third_party/nvim-oxi https://github.com/noib3/nvim-oxi master --squash`
