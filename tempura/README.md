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

To install a new release of 🍤 bins:

```console
cargo build --release && \
    rm -f "$HOME"/.local/bin/catl && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/catl "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/idt && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/idt "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/oe && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/oe "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/yghfl && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/yghfl "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/yhfp && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/yhfp "$HOME"/.local/bin && \
```
