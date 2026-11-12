# FinalTerm shell integration

Reference: [FinalTerm sequence documentation maintained by iTerm2](https://iterm2.com/documentation-escape-codes.html#shell-integrationfinalterm).

OSC 133 A/B/C/D records prompt/input/output/completion boundaries with optional
exit status. VS Code OSC 633 A–D are accepted as aliases of these boundaries;
its other dialect keys remain ignored. Shared zones/tracked positions own the
10,000-entry bound and eviction lifetime; Session supplies time.

Puzzle's supported A–D subset is unchanged. Code: `puzzle-vt/src/protocols/finalterm.rs`.


Native login-shell startup now injects the existing snippets through shared
puzzle-embed construction (M18.8), selected by `shellIntegration`. Zsh/fish and
ENV-capable GNU bash are automatic; unsupported Apple bash 3.2 keeps normal
login behavior and manual sourcing. Unknown
shells, disabled injection and host transports stay untouched. See
[shell setup and verification](../shell-integration.md). The wire grammar and
bounded zone model are unchanged.

M12.tabs G1 forwards output-start command metadata through the host event seam.
The FinalTerm C mark still owns the boundary; kitty's optional `cmdline_url`
interpretation lives in `protocols/kitty/shell.rs` (D-58). Missing or malformed
metadata never removes the mark. See [host tab facts](../embedding/tabs.md).
