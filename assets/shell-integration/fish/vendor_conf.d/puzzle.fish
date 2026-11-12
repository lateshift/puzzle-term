# Puzzle's original vendor configuration entry point (M18.8).
# User config.fish follows vendor files; defer until its prompt is fully defined.
status is-interactive; or return
set -q PUZZLE_SHELL_INTEGRATION; or return
set -g _puzzle_auto_root $PUZZLE_SHELL_INTEGRATION
set -e PUZZLE_SHELL_INTEGRATION
# Restore nested-child discovery after fish has collected its startup paths.
if set -q PUZZLE_SAVED_XDG_DATA_DIRS
    set -gx XDG_DATA_DIRS $PUZZLE_SAVED_XDG_DATA_DIRS
else
    set -e XDG_DATA_DIRS
end
set -e PUZZLE_SAVED_XDG_DATA_DIRS
function _puzzle_auto_install --on-event fish_prompt
    source "$_puzzle_auto_root/fish.fish"
    set -e _puzzle_auto_root
    functions -e _puzzle_auto_install
    # The newly attached OSC 7 event hook may not be in this event's iteration.
    # Emit the initial directory here only on the native-mark-prompt path.
    if status test-feature mark-prompt
        _puzzle_cwd
    end
end
