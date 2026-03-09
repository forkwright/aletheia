# Shell Completions

Aletheia provides tab-completion for all subcommands, flags, and options via [clap_complete](https://docs.rs/clap_complete).

## Generating Completions

```bash
aletheia completions <SHELL>
```

Where `<SHELL>` is one of: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

---

## Bash

```bash
# Generate and install
aletheia completions bash > ~/.local/share/bash-completion/completions/aletheia

# Or system-wide (requires root)
aletheia completions bash | sudo tee /etc/bash_completion.d/aletheia > /dev/null
```

Reload your shell or source the file:

```bash
source ~/.local/share/bash-completion/completions/aletheia
```

---

## Zsh

```bash
# Generate to a directory in your $fpath
aletheia completions zsh > ~/.zfunc/_aletheia
```

Ensure `~/.zfunc` is in your `fpath` (add to `~/.zshrc` before `compinit`):

```zsh
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

Restart your shell or run `compinit` to pick up the new completions.

---

## Fish

```bash
aletheia completions fish > ~/.config/fish/completions/aletheia.fish
```

Fish picks up completions from this directory automatically — no restart needed.

---

## Verifying

After installation, type `aletheia <TAB>` to see available subcommands, or `aletheia health --<TAB>` to see flags.
