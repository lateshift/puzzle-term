# Puzzle's original temporary ZDOTDIR entry point (M18.8).
# Restore the user's directory before any later zsh startup file is selected.
typeset -g _puzzle_auto_root=$PUZZLE_SHELL_INTEGRATION
unset PUZZLE_SHELL_INTEGRATION
if (( ${+PUZZLE_SAVED_ZDOTDIR} )); then
    export ZDOTDIR=$PUZZLE_SAVED_ZDOTDIR
else
    unset ZDOTDIR
fi
unset PUZZLE_SAVED_ZDOTDIR
# .zshenv may itself change ZDOTDIR; zsh then uses that result for .zprofile/.zshrc.
if [[ -r ${ZDOTDIR-$HOME}/.zshenv ]]; then
    builtin source -- "${ZDOTDIR-$HOME}/.zshenv"
fi
if [[ -o interactive ]]; then
    # The first precmd follows all login/interactive startup files. Install then,
    # so prompt frameworks exist before our existing snippet attaches its hooks.
    _puzzle_auto_install() {
        local code=$?
        precmd_functions=(${precmd_functions:#_puzzle_auto_install})
        if (( ! ${+_puzzle_integration_loaded} )); then
            builtin source -- "$_puzzle_auto_root/zsh.zsh"
            # Newly registered hooks are not in this iteration's precmd snapshot.
            # Produce its initial prompt boundary once, preserving the old status.
            _puzzle_auto_status $code
            _puzzle_prompt_start
            # Startup-installed framework hooks still run later in this first
            # precmd iteration and may replace PROMPT. Emit B at ZLE entry,
            # after that prompt is actually drawn; later prompts use the suffix.
            autoload -Uz add-zle-hook-widget
            add-zle-hook-widget line-init _puzzle_auto_first_input
        fi
        unset _puzzle_auto_root
        unfunction _puzzle_auto_install _puzzle_auto_status
        return $code
    }
    _puzzle_auto_status() { return $1; }
    typeset -ga precmd_functions
    precmd_functions+=(_puzzle_auto_install)
else
    unset _puzzle_auto_root
fi

# The standard hook helper preserves a framework's own ZLE line-init widget.
_puzzle_auto_first_input() {
    printf '\033]133;B\007'
    add-zle-hook-widget -d line-init _puzzle_auto_first_input
    zle -D _puzzle_auto_first_input
    unfunction _puzzle_auto_first_input
}
