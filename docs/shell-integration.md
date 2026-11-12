# Shell integration

Native PTY surfaces in both the harness and macOS app automatically load the
existing snippets by default. `shellIntegration` applies to the next session:
`detect` chooses by the configured executable's basename; `none` disables
injection; `bash`, `zsh` and `fish` force a route for wrapper executables. Unknown
shells and host-supplied byte transports receive no injection. Startup files in
your home are never modified. Missing resources leave normal startup intact.

Zsh temporarily redirects `ZDOTDIR`, restores its original unset/empty/custom
value, runs your startup files and installs hooks after prompt initialization.
Fish prepends a vendor configuration directory to `XDG_DATA_DIRS`, preserves
other vendors and restores the original value before child commands run.
Supported GNU bash uses POSIX `ENV`, then restores ordinary login profiles,
`ENV`, bash options and history defaults before installing the snippet.

**Apple bash 3.2 exception (reviewer, 2026-10-12):** Apple's bash disables the
`ENV` mechanism. Puzzle detects the executable's version, preserves its true
login behavior and logs one debug skip at startup. Use manual sourcing below;
this is the expected behaviour. Modern ENV-capable GNU bash (e.g. Homebrew bash 5) gets
automatic injection. No alternate shell is installed or substituted.

Manual sourcing remains safe together with automatic injection. Source after
your prompt framework has initialized. Replace `/path/to/puzzle` below with
this checkout or the installed assets path.

Bash (including macOS `/bin/bash` 3.2): put this in the startup file you actually
load. For a login shell, use `~/.bash_profile` (or have it source `~/.bashrc`);
non-login interactive shells normally use `~/.bashrc`.

```sh
source /path/to/puzzle/assets/shell-integration/bash.sh
```

Zsh (`~/.zshrc`, after oh-my-zsh, powerlevel10k or Starship initialization):

```sh
source /path/to/puzzle/assets/shell-integration/zsh.zsh
```

Fish (`~/.config/fish/config.fish`, after defining `fish_prompt` or initializing Starship):

```fish
source /path/to/puzzle/assets/shell-integration/fish.fish
```

The snippets are interactive-only and safe to source twice. They emit semantic
prompt/input/output/finish marks (OSC 133) and a percent-encoded working-directory
URI (OSC 7). They preserve the existing prompt appearance and its command status.
Bash composes the existing `PROMPT_COMMAND` and `DEBUG` trap; zsh retains the
`precmd_functions`/`preexec_functions` hooks; fish retains the existing prompt.
Modern fish already emits OSC 133 through `mark-prompt`, so its snippet adds only
OSC 7. With that feature disabled, the snippet supplies the four boundaries.

The marks enable prompt navigation, command-output selection/copy, exit-status
markers and command-block duration badges. They do not change command execution.
Remote shells need their own snippet; the local shell cannot mark remote output.

Verification uses real interactive PTYs with a success, `false`, and multiline
output, sources each snippet twice, and checks exact parsed output ranges and exit
codes. Framework-style prompt hooks are exercised without installing third-party
frameworks. The user approved fresh-zsh integration in the app and harness on
2026-10-13, with no manual sourcing or duplicate marks. For future regression checks after sourcing,
run `printf 'hello\n'`, `false`, and `printf 'first\nsecond\n'`; confirm the prompt
still looks correct, a failed command is marked, and Copy output contains only
its output. For transient/instant prompt modes, also check the first prompt after
starting a fresh shell.


Automatic-startup verification uses private empty homes and separate fixtures
with existing profiles, custom ZDOTDIR, fish vendor/user configuration and prompt
hooks. Both automatic-only and double manual-sourcing cases check exact OSC 133
zones, exit status and OSC 7; Apple bash tests separately check login_shell and
unchanged ENV. Run `cargo xtask accept M18 --task shell`. These are functional
checks, with a deadlock guard rather than a debug timing budget. Fresh-zsh user review is approved in both hosts (2026-10-13); other supported
shells retain this automated coverage and Apple bash retains manual integration.
