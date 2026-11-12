//! Deterministic ANSI output for M5.5b's `script` capture fixtures.
//!
//! These commands are small real processes with no host-specific values.
//! macOS `script -q` records their terminal byte stream, including SGR
//! controls, exactly as a PTY-facing application would emit it. The chart
//! covers every indexed color plus a truecolor gradient; the box sample
//! stresses builtin glyphs across adjacent cells and mixed line weights.
//! The emitted bytes contain no machine names or user paths, so a fresh
//! capture can be reviewed without carrying a personal shell prompt.

/// Emits indexed and RGB spans with an explicit neutral prompt.
pub(crate) fn chart() -> Result<(), String> {
    println!("puzzle$ color chart: ANSI 0..255 and truecolor");
    for base in (0..256).step_by(16) {
        print!("{base:03}: ");
        for index in base..(base + 16).min(256) {
            // Two spaces give a visible background swatch per palette slot.
            print!("\x1b[48;5;{index}m  ");
        }
        println!("\x1b[0m");
    }
    for step in 0..16 {
        let red = step * 17;
        let blue = 255 - red;
        // Adjacent RGB swatches reveal channel-order and quantization bugs.
        print!("\x1b[48;2;{red};64;{blue}m  ");
    }
    println!("\x1b[0m\npuzzle$ done");
    Ok(())
}

/// Emits a dense box-drawing and block specimen under SGR changes.
pub(crate) fn box_grid() -> Result<(), String> {
    println!("puzzle$ box drawing preview");
    println!("\x1b[1;36m╭─────┬─────╮  ┌──┬──┐  ╔══╦══╗\x1b[0m");
    println!("\x1b[1;36m│ one │ two │  │a │b │  ║A ║B ║\x1b[0m");
    println!("\x1b[1;36m├─────┼─────┤  ├──┼──┤  ╠══╬══╣\x1b[0m");
    println!("\x1b[1;36m│ 123 │ 456 │  │c │d │  ║C ║D ║\x1b[0m");
    println!("\x1b[1;36m╰─────┴─────╯  └──┴──┘  ╚══╩══╝\x1b[0m");
    println!("shades: ░▒▓█  braille: ⠿⣷⣿  powerline: ");
    // Italic and inverse exercise style runs that have different shaping
    // faces and color resolution while retaining the same cell origins.
    println!("styles: \x1b[3mitalic\x1b[0m  \x1b[7minverse\x1b[0m  \x1b[1mbold\x1b[0m");
    println!("puzzle$ done");
    Ok(())
}
