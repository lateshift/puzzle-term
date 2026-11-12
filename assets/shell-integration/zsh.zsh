# Puzzle OSC 133 integration: source after oh-my-zsh/Starship/powerlevel10k setup.
# Hook arrays preserve the framework's functions and registration order.
[[ -o interactive ]] || return
(( ${+_puzzle_integration_loaded} )) && return
typeset -g _puzzle_integration_loaded=1 _puzzle_running=0

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

# Encode the working directory as UTF-8 bytes without spawning a URL tool.
_puzzle_cwd() {
    emulate -L zsh
    local LC_ALL=C value=$PWD result= ch hex octet
    while [[ -n $value ]]; do
        ch=$value[1]; value=$value[2,-1]
        case $ch in
            [a-zA-Z0-9/._~-]) result+=$ch ;;
            * ) printf -v octet '%d' "'$ch";
               # Bash 3.2 sign-extends high bytes; URI escapes need an unsigned octet.
               printf -v hex '%%%02X' "$((octet & 255))"; result+=$hex ;;
        esac
    done
    printf '\033]7;file://localhost%s\007' "$result"
}

# precmd receives the previous command's status before a framework changes it.
_puzzle_prompt_start() {
    local code=$?
    if (( _puzzle_running )); then
        printf '\033]133;D;%s\007' "$code"
        _puzzle_running=0
    fi
    printf '\033]133;A\007'
    _puzzle_cwd
    return $code
}
# ZLE excludes the marker from width calculation; reapply after prompt updates.
_puzzle_prompt_end() {
    local suffix=$'%{\e]133;B\a%}'
    PROMPT=${PROMPT%"$suffix"}$suffix
}
# preexec runs once for the accepted interactive command, before any output.
_puzzle_preexec() {
    _puzzle_running=1
    _puzzle_command "$1"
}
typeset -ga precmd_functions preexec_functions
precmd_functions=(_puzzle_prompt_start $precmd_functions _puzzle_prompt_end)
preexec_functions=(_puzzle_preexec $preexec_functions)
