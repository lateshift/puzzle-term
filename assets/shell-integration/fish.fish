# Puzzle OSC 133 integration: source after defining the prompt (including Starship).
# Preserve the existing prompt function; no event handler replaces its appearance.
status is-interactive; or return
set -q _puzzle_integration_loaded; and return
set -g _puzzle_integration_loaded 1
set -g _puzzle_running 0
set -g _puzzle_exit 0
# Modern fish already emits all four OSC 133 boundaries, including exit status.
# Respect that source instead of emitting duplicate marks and splitting commands.
if status test-feature mark-prompt
    function _puzzle_cwd --on-event fish_prompt
        set -l directory (string escape --style=url -- $PWD | string replace --all '%2F' '/')
        printf '\033]7;file://localhost%s\a' $directory
    end
    return
end

functions -c fish_prompt _puzzle_original_prompt

# Returning the saved status lets the original prompt inspect the child result.
function _puzzle_restore_status --argument-names code
    return $code
end

# fish_preexec covers the whole accepted command, rather than individual processes.
function _puzzle_preexec --on-event fish_preexec
    set -g _puzzle_running 1
    # Cap before URL encoding; the shared parser enforces G1's UTF-8 byte cap.
    set -l command (string sub --length 1025 -- "$argv[1]" | string collect)
    printf '\033]133;C;cmdline_url=%s\a' (string escape --style=url -- "$command")
end
function _puzzle_postexec --on-event fish_postexec
    # Save before any prompt/event helper can overwrite fish's special $status.
    set -g _puzzle_exit $status
end

function fish_prompt
    set -l code $_puzzle_exit
    if test $_puzzle_running = 1
        printf '\033]133;D;%s\a' $code
        set -g _puzzle_running 0
    end
    printf '\033]133;A\a'
    # Fish's URL escape encodes UTF-8 bytes; slashes are legal in URI path segments.
    set -l directory (string escape --style=url -- $PWD | string replace --all '%2F' '/')
    printf '\033]7;file://localhost%s\a' $directory
    _puzzle_restore_status $code
    _puzzle_original_prompt
    printf '\033]133;B\a'
end
