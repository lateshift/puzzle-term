# Puzzle's original login bootstrap for ENV-capable GNU bash (M18.8).
# Apple's bash 3.2 skips injection and keeps its normal login startup instead.
# ENV is loaded in POSIX mode; restore normal semantics before startup files run.
case $- in *i*) ;; *) return ;; esac
_puzzle_boot_root=$PUZZLE_SHELL_INTEGRATION
unset PUZZLE_SHELL_INTEGRATION ENV
if [ "${PUZZLE_SAVED_ENV+x}" = x ]; then
    export ENV="$PUZZLE_SAVED_ENV"
fi
unset PUZZLE_SAVED_ENV
set +o posix
# Modern bash carries this POSIX-derived option separately; 3.2 has no such option.
shopt -u inherit_errexit 2>/dev/null
if [ "${PUZZLE_UNEXPORT_HISTFILE-}" = 1 ]; then
    export -n HISTFILE
fi
unset PUZZLE_UNEXPORT_HISTFILE

# Recreate bash(1)'s login ordering, including the first-readable user-file rule.
# Sourcing happens at top level so startup declarations keep their normal scope.
if [ -r /etc/profile ]; then source /etc/profile; fi
for _puzzle_boot_file in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
    if [ -r "$_puzzle_boot_file" ]; then
        source "$_puzzle_boot_file"
        break
    fi
done
# Install after prompt frameworks; manual sourcing in the profile is idempotent.
source "$_puzzle_boot_root/bash.sh"
unset _puzzle_boot_root _puzzle_boot_file
