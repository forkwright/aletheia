# Shell completions

Tab-completion for all subcommands, flags, and options via [clap_complete](https://docs.rs/clap_complete).

Generate and install for your shell:

```bash
# Bash
aletheia completions bash > ~/.local/share/bash-completion/completions/aletheia

# Zsh (add ~/.zfunc to $fpath before compinit)
aletheia completions zsh > ~/.zfunc/_aletheia

# Fish (picked up automatically)
aletheia completions fish > ~/.config/fish/completions/aletheia.fish
```

After install, type `aletheia <TAB>` to see subcommands or `aletheia health --<TAB>` for flags.
