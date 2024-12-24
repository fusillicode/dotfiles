# [yog](https://en.wikipedia.org/wiki/Cthulhu_Mythos_deities#Yog-Sothoth) 🍝 👀

> It was an All-in-One and One-in-All of limitless being and self—not merely a thing of one Space-Time continuum, but
> allied to the ultimate animating essence of existence's whole unbounded sweep—the last, utter sweep which has no
> confines and which outreaches fancy and mathematics alike. (cit. Wikipedia)

Bunch of _personal_ command-line utilities built in [Rust](https://www.rust-lang.org/) because I want to be happy.

Requirements:

- `curl`
- `gh`
- `git`
- `hx`
- `nvim`
- `tar`
- `vault`
- `wezterm`
- `zcat`

To install a new release of bins:

```console
cargo build --release && \
    rm -f "$HOME"/.local/bin/idt && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/idt "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/yghfl && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/yghfl "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/yhfp && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/yhfp "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/oe && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/oe "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/catl && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/catl "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/gcu && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/gcu "$HOME"/.local/bin
    rm -f "$HOME"/.local/bin/vpg && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/vpg "$HOME"/.local/bin && \
    rm -f "$HOME"/.local/bin/try && \
    ln -s "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/try "$HOME"/.local/bin && \
    mv "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/librua.dylib "$HOME"/data/dev/dotfiles/dotfiles/yog/target/release/rua.so
```
