# 🍤

> Tempura is a typical Japanese dish that usually consists of seafood and vegetables that have been coated in a thin
> batter and deep fried. (cit. Wikipedia)

Bunch of _personal_ command-line utilities built in [Rust](https://www.rust-lang.org/) because I love like it.

Requirements:

- `curl`
- `gh`
- `git`
- `hx`
- `nvim`
- `tar`
- `wezterm`
- `zcat`

Useful cmds:

```console
# To install a new release
cargo build --release && \
    rm "$HOME"/.local/bin/ebi && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/ebi "$HOME"/.local/bin
```
