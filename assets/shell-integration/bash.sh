# Puzzle OSC 133 integration for interactive bash, including macOS bash 3.2.
# Source after the prompt framework. Existing PROMPT_COMMAND and DEBUG code stay
# in their original order; no command is re-executed and no external process runs.
case $- in *i*) ;; *) return ;; esac
[ "${_puzzle_integration_loaded-}" = 1 ] && return
_puzzle_integration_loaded=1
_puzzle_at_prompt=0
_puzzle_running=0

# Kitty's OSC 133 command metadata uses URL-escaped UTF-8, not executable quoting.
# One byte beyond G1's 1024-byte cap lets the parser add a UTF-8-safe ellipsis;
# even a huge accepted command cannot overflow the OSC buffer and lose its C mark.
_puzzle_command() {
    local LC_ALL=C value result= ch hex octet
    value=${1:0:1025}
    while [ -n "$value" ]; do
        ch=${value%"${value#?}"}; value=${value#?}
        case $ch in
            [a-zA-Z0-9/._~-]) result=$result$ch ;;
            *) printf -v octet '%d' "'$ch"
               printf -v hex '%%%02X' "$((octet & 255))"; result=$result$hex ;;
        esac
    done
    printf '\033]133;C;cmdline_url=%s\007' "$result"
}

# OSC 7 is a URI: encode pathname bytes, retaining only RFC 3986 safe characters.
_puzzle_cwd() {
    local LC_ALL=C value="$PWD" result= ch hex octet
    while [ -n "$value" ]; do
        ch=${value%"${value#?}"}; value=${value#?}
        case $ch in
            [a-zA-Z0-9/._~-]) result=$result$ch ;;
            * ) printf -v octet '%d' "'$ch";
               # Bash 3.2 sign-extends high bytes; URI escapes need an unsigned octet.
               printf -v hex '%%%02X' "$((octet & 255))"; result=$result$hex ;;
        esac
    done
    printf '\033]7;file://localhost%s\007' "$result"
}

# This is the first prompt hook, so $? is still the application's exit status.
_puzzle_prompt_start() {
    local status=$1
    _puzzle_at_prompt=0
    if [ "$_puzzle_running" = 1 ]; then
        printf '\033]133;D;%s\007' "$status"
        _puzzle_running=0
    fi
    printf '\033]133;A\007'
    _puzzle_cwd
    # Prompt frameworks that inspect $? must see the unmodified child status.
    return "$status"
}

# Readline must know B occupies no columns. Reapply after frameworks replace PS1.
_puzzle_prompt_end() {
    local suffix='\[\e]133;B\a\]'
    PS1=${PS1%"$suffix"}$suffix
    _puzzle_history_at_prompt=$(HISTTIMEFORMAT= builtin history 1)
    _puzzle_at_prompt=1
}

# DEBUG runs before a command, including builtins. Only the first command after
# the visible prompt starts output; subsequent pipeline/list commands stay in it.
_puzzle_debug() {
    local status=$1 command=$2
    case $command in _puzzle_*) return "$status" ;; esac
    if [ "$_puzzle_at_prompt" = 1 ]; then
        _puzzle_at_prompt=0
        _puzzle_running=1
        # A new history entry carries the whole accepted list/pipeline. If the
        # user's history policy hid it, BASH_COMMAND is safer than stale history.
        local latest
        latest=$(HISTTIMEFORMAT= builtin history 1)
        # HISTCMD is not a reliable counter inside older bash function hooks.
        # Compare the numbered entry instead; ignored input leaves it unchanged.
        if [ "$latest" != "${_puzzle_history_at_prompt-}" ]; then
            latest=${latest#"${latest%%[![:space:]]*}"}
            latest=${latest#*[[:space:]]}
            latest=${latest#"${latest%%[![:space:]]*}"}
            [ -z "$latest" ] || command=$latest
        fi
        _puzzle_command "$command"
    fi
    return "$status"
}

# Decode bash's own quoting of trusted existing DEBUG code, once at the prompt.
_puzzle_decode_debug() {
    _puzzle_old_debug=${_puzzle_old_debug#trap -- }
    _puzzle_old_debug=${_puzzle_old_debug% DEBUG}
    [ -z "$_puzzle_old_debug" ] || eval "_puzzle_old_debug=$_puzzle_old_debug"
}

# Bash 3.2 temporarily hides/restores DEBUG around `source`. Install at the first
# prompt in the caller's scope, where the framework's original trap is visible.
# Installation precedes prompt_end so setup commands cannot mark output as started.
PROMPT_COMMAND='_puzzle_prompt_start "$?"; '"${PROMPT_COMMAND-}"$'\n''
if [ "${_puzzle_debug_installed-}" != 1 ]; then
    _puzzle_old_debug=$(trap -p DEBUG)
    _puzzle_decode_debug
    trap '\''_puzzle_debug "$?" "$BASH_COMMAND"; '\''"$_puzzle_old_debug" DEBUG
    _puzzle_debug_installed=1
fi
_puzzle_prompt_end'
